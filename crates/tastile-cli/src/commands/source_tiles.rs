//! `tastile source-tiles` — list / create / get / update / cancel /
//! completion / placements / reflow source tiles.
//!
//! All HTTP work goes through `crate::app::*`. Every mutating command
//! sends a request through the shared guarded `app::*` service layer, and
//! every mutating command either previews its scope (`--dry-run`) or reads
//! the resulting state back from the server.

use std::fs;
use std::process::ExitCode;

use anyhow::{Context, Result, anyhow, bail};
use chrono::{DateTime, Duration, NaiveDate, Utc};
use serde_json::Value;
use uuid::Uuid;

use crate::app::{self, AppContext, CreateSourceTileDraft};
use crate::cli::{SourceTilesArgs, SourceTilesCommand};
use crate::output;
use crate::simple_input::{self, ScheduleInput};

pub async fn run(cfg: tastile_config::Config, args: SourceTilesArgs) -> Result<ExitCode> {
    let ctx = AppContext::load(cfg)?;
    match args.sub {
        SourceTilesCommand::List { json } => list(&ctx, json).await,
        SourceTilesCommand::Create {
            from_json,
            title,
            once,
            daily,
            weekly,
            days,
            start_date,
            utc_offset,
            exclude,
            duration,
            window_minutes,
            horizon_days,
            priority,
            monthly,
        } => {
            create(
                &ctx,
                from_json,
                title,
                once,
                daily,
                weekly,
                days,
                start_date,
                utc_offset,
                exclude,
                duration,
                window_minutes,
                horizon_days,
                priority,
                monthly,
            )
            .await
        }
        SourceTilesCommand::Get { id, json } => get(&ctx, id, json).await,
        SourceTilesCommand::Update {
            id,
            from_json,
            dry_run,
            expect_revision,
        } => update(&ctx, id, from_json, dry_run, expect_revision).await,
        SourceTilesCommand::Cancel {
            id,
            reason_code,
            dry_run,
            expect_revision,
        } => cancel(&ctx, id, reason_code, dry_run, expect_revision).await,
        SourceTilesCommand::Completion { id, json } => completion(&ctx, id, json).await,
        SourceTilesCommand::Placements { id, json } => placements(&ctx, id, json).await,
        SourceTilesCommand::Reflow {
            id,
            from,
            to,
            dry_run,
            expect_revision,
        } => reflow(&ctx, id, from, to, dry_run, expect_revision).await,
    }
}

async fn list(ctx: &AppContext, json: bool) -> Result<ExitCode> {
    let items = app::list_source_tiles(ctx, None).await?;
    if json {
        println!("{}", serde_json::to_string_pretty(&items)?);
    } else if items.is_empty() {
        println!("(no source tiles)");
    } else {
        output::print_source_tiles_table(&items);
        println!("{} source tile(s)", items.len());
    }
    Ok(ExitCode::SUCCESS)
}

#[allow(clippy::too_many_arguments)]
async fn create(
    ctx: &AppContext,
    from_json: Option<String>,
    title: Option<String>,
    once: Option<String>,
    daily: Option<String>,
    weekly: Option<String>,
    days: Option<String>,
    start_date: Option<String>,
    utc_offset: Option<i32>,
    exclude: Vec<String>,
    duration: i64,
    window_minutes: i64,
    horizon_days: i64,
    priority: i32,
    monthly: bool,
) -> Result<ExitCode> {
    if monthly {
        bail!(
            "monthly recurrence is not described by the pinned Tastile OpenAPI \
             (SourceGenerationSchema.kind documents 0 = one-time and 1 = recurring). \
             Refusing to approximate it as a daily recurrence — wait for the \
             monthly contract to ship, or model the month with explicit dates."
        );
    }
    if duration <= 0 {
        bail!("--duration must be a positive number of minutes");
    }
    if horizon_days <= 0 {
        bail!("--horizon-days must be a positive number of days");
    }

    let draft = match from_json {
        Some(path) => load_draft(&path).context("could not load --from-json draft")?,
        None => {
            let title = title.ok_or_else(|| {
                anyhow!(
                    "provide --title together with a schedule (--once, --daily or --weekly), \
                     or use --from-json"
                )
            })?;
            if once.is_some()
                && (start_date.is_some() || utc_offset.is_some() || !exclude.is_empty())
            {
                bail!(
                    "--start-date, --utc-offset, and --exclude apply only to recurring --daily or --weekly schedules; --once already contains an exact instant"
                );
            }
            let schedule = build_schedule_input(once, daily, weekly, days, start_date, utc_offset)?;
            for date in &exclude {
                parse_excluded_date(date)?;
            }
            // `starts_at` is computed with this same fixed offset. Pass it
            // explicitly as well so Core does not substitute an owner
            // profile timezone when the operator omits `--utc-offset`.
            let generation_offset = generation_offset(&schedule, utc_offset);
            let generation_plan =
                simple_input::build_generation(&schedule, &exclude, generation_offset);
            println!("schedule: {}", generation_plan.summary);
            if !exclude.is_empty() {
                println!("excluded dates: {}", exclude.join(", "));
            }
            let mut draft = CreateSourceTileDraft::minimal(title);
            draft.generation = Some(generation_plan.generation);
            draft.window = Some(simple_input::window_from_minutes(window_minutes)?);
            draft.priority = priority;
            draft.required_duration_ms = duration
                .checked_mul(simple_input::MINUTE_MS)
                .ok_or_else(|| anyhow!("--duration is too large"))?;
            let horizon = Duration::try_days(horizon_days)
                .ok_or_else(|| anyhow!("--horizon-days is too large"))?;
            draft.horizon_end = draft
                .horizon_start
                .checked_add_signed(horizon)
                .ok_or_else(|| anyhow!("--horizon-days exceeds the representable date range"))?;
            draft
        }
    };

    let resp = app::create_source_tile_draft(ctx, &draft).await?;
    println!("✓ Created source tile.");
    output::print_command_response(&resp);
    // The create response carries the new source id in `aggregate_meta`;
    // read its detail once so the credential's owner pin is established
    // even when the guard probe saw an empty tile list.
    if let Some(source_tile_id) = resp
        .aggregate_meta
        .as_ref()
        .and_then(|meta| meta.get("source_tile_id"))
        .and_then(serde_json::Value::as_str)
        .and_then(|raw| Uuid::parse_str(raw).ok())
    {
        app::confirm_owner_from_source_tile(ctx, source_tile_id).await?;
    }
    Ok(ExitCode::SUCCESS)
}

/// Turn the convenience flags into a recurrence the contract can express.
fn build_schedule_input(
    once: Option<String>,
    daily: Option<String>,
    weekly: Option<String>,
    days: Option<String>,
    start_date: Option<String>,
    utc_offset: Option<i32>,
) -> Result<ScheduleInput> {
    if [once.is_some(), daily.is_some(), weekly.is_some()]
        .into_iter()
        .filter(|set| *set)
        .count()
        > 1
    {
        bail!("choose exactly one of --once, --daily or --weekly");
    }
    if once.is_some() && (days.is_some() || start_date.is_some() || utc_offset.is_some()) {
        bail!("--days, --start-date and --utc-offset apply only to --daily or --weekly");
    }
    if daily.is_some() && days.is_some() {
        bail!("--days is only valid with --weekly");
    }
    let offset = utc_offset.unwrap_or(0);
    if let Some(raw) = once {
        let at = DateTime::parse_from_rfc3339(raw.trim())
            .map_err(|_| anyhow!("invalid --once timestamp `{raw}`; expected RFC 3339"))?
            .with_timezone(&Utc);
        return Ok(ScheduleInput::OneTime { at });
    }
    let date = match start_date {
        Some(raw) => NaiveDate::parse_from_str(raw.trim(), "%Y-%m-%d")
            .map_err(|_| anyhow!("invalid --start-date `{raw}`; expected YYYY-MM-DD"))?,
        None => (Utc::now() + Duration::minutes(i64::from(offset))).date_naive(),
    };
    if let Some(raw) = daily {
        let time = simple_input::parse_clock(&raw)?;
        let anchor = simple_input::local_instant(date, time, offset)?;
        return Ok(ScheduleInput::Daily { anchor });
    }
    if let Some(raw) = weekly {
        let time = simple_input::parse_clock(&raw)?;
        let days_raw =
            days.ok_or_else(|| anyhow!("--weekly needs --days, for example --days mon,wed,fri"))?;
        let weekdays = simple_input::parse_weekdays(&days_raw)?;
        let anchor = simple_input::local_instant(date, time, offset)?;
        return Ok(ScheduleInput::Weekly { anchor, weekdays });
    }
    bail!("specify a schedule: --once RFC3339, --daily HH:MM or --weekly HH:MM --days mon,wed")
}

fn generation_offset(schedule: &ScheduleInput, requested_offset: Option<i32>) -> Option<i32> {
    match schedule {
        ScheduleInput::OneTime { .. } => None,
        ScheduleInput::Daily { .. } | ScheduleInput::Weekly { .. } => {
            Some(requested_offset.unwrap_or(0))
        }
    }
}

fn parse_excluded_date(raw: &str) -> Result<()> {
    NaiveDate::parse_from_str(raw.trim(), "%Y-%m-%d")
        .map_err(|_| anyhow!("invalid --exclude date `{raw}`; expected YYYY-MM-DD"))?;
    Ok(())
}

async fn get(ctx: &AppContext, id: Uuid, json: bool) -> Result<ExitCode> {
    let detail = app::show_source_tile(ctx, id).await?;
    if json {
        println!("{}", serde_json::to_string_pretty(&detail)?);
    } else {
        output::print_source_tile_detail(&detail);
    }
    Ok(ExitCode::SUCCESS)
}

async fn update(
    ctx: &AppContext,
    id: Uuid,
    from_json: Option<String>,
    dry_run: bool,
    expect_revision: Option<i64>,
) -> Result<ExitCode> {
    let Some(path) = from_json else {
        bail!("update requires --from-json PATH; see `docs/architecture.md` for the schema");
    };
    let draft =
        load_update_draft(&path).context("could not load full replacement --from-json draft")?;
    let permit = if !dry_run {
        // Refuse ambiguous/operational targets before the authenticated
        // pre-read used to build the diff. The permit is then reused for the
        // mutation so the expensive identity probe runs only once.
        Some(app::guard_write(ctx).await?)
    } else {
        None
    };
    let current = app::show_source_tile(ctx, id).await?;
    let diffs = app::diff_source_tile(&draft, &current.source);
    output::print_diff(&diffs);
    println!(
        "The API read model omits plan details, flows, horizon, request-shaped relations, and \
         source_client_local_id; dry-run cannot compare these fields. The full replacement will send the supplied values."
    );
    if dry_run {
        println!("Dry run: no write sent.");
        return Ok(ExitCode::SUCCESS);
    }
    let permit = permit.ok_or_else(|| anyhow!("update apply requires write preflight"))?;
    let revision = expect_revision.unwrap_or(current.source.revision);
    let resp =
        app::update_source_tile_draft_with_permit(ctx, id, &draft, Some(revision), &permit).await?;
    println!("✓ Updated source tile.");
    output::print_command_response(&resp);
    output::print_readback(&app::read_back(ctx, id).await?);
    Ok(ExitCode::SUCCESS)
}

async fn cancel(
    ctx: &AppContext,
    id: Uuid,
    reason_code: i32,
    dry_run: bool,
    expect_revision: Option<i64>,
) -> Result<ExitCode> {
    let permit = if !dry_run {
        // Gate the target before reading the impact preview on an apply path.
        Some(app::guard_write(ctx).await?)
    } else {
        None
    };
    let impact = app::placement_impact(ctx, id, None).await?;
    output::print_impact("cancel", &impact);
    if dry_run {
        println!("Dry run: no write sent.");
        return Ok(ExitCode::SUCCESS);
    }
    let permit = permit.ok_or_else(|| anyhow!("cancel apply requires write preflight"))?;
    let resp =
        app::cancel_source_tile_with_permit(ctx, id, reason_code, expect_revision, &permit).await?;
    println!("✓ Cancelled source tile.");
    output::print_command_response(&resp);
    output::print_readback(&app::read_back(ctx, id).await?);
    Ok(ExitCode::SUCCESS)
}

async fn completion(ctx: &AppContext, id: Uuid, json: bool) -> Result<ExitCode> {
    let v = app::source_tile_completion(ctx, id).await?;
    // Completion is opaque nested JSON (tasks + time_requirements tree);
    // keep the machine-readable mode compact for piping.
    if json {
        println!("{}", serde_json::to_string(&v)?);
    } else {
        println!("{}", serde_json::to_string_pretty(&v)?);
    }
    Ok(ExitCode::SUCCESS)
}

async fn placements(ctx: &AppContext, id: Uuid, json: bool) -> Result<ExitCode> {
    let items = tastile_api::list_all_source_tile_placements(ctx, ctx.require_token()?, id)
        .await
        .context("list_source_tile_placements failed")?;
    if json {
        println!("{}", serde_json::to_string_pretty(&items)?);
    } else if items.is_empty() {
        println!("(no placements)");
    } else {
        output::print_placements_table(&items);
        println!("{} placement(s)", items.len());
    }
    Ok(ExitCode::SUCCESS)
}

async fn reflow(
    ctx: &AppContext,
    id: Uuid,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
    dry_run: bool,
    expect_revision: Option<i64>,
) -> Result<ExitCode> {
    if from >= to {
        bail!("--from must be earlier than --to");
    }
    let permit = if !dry_run {
        // Gate the target before reading the impact preview on an apply path.
        Some(app::guard_write(ctx).await?)
    } else {
        None
    };
    let impact = app::placement_impact(ctx, id, Some((from, to))).await?;
    output::print_impact("reflow", &impact);
    if dry_run {
        println!("Dry run: no write sent.");
        return Ok(ExitCode::SUCCESS);
    }
    let permit = permit.ok_or_else(|| anyhow!("reflow apply requires write preflight"))?;
    let resp = app::reflow_source_tile_with_permit(
        ctx,
        id,
        tastile_api::Span {
            start: from,
            end: to,
        },
        expect_revision,
        &permit,
    )
    .await?;
    println!("✓ Reflow dispatched.");
    output::print_command_response(&resp);
    output::print_readback(&app::read_back(ctx, id).await?);
    Ok(ExitCode::SUCCESS)
}

fn load_draft(path: &str) -> Result<CreateSourceTileDraft> {
    Ok(serde_json::from_value(read_draft_value(path)?)?)
}

fn load_update_draft(path: &str) -> Result<CreateSourceTileDraft> {
    let value = read_draft_value(path)?;
    validate_full_update_draft(&value)?;
    let draft = serde_json::from_value(value)?;
    app::build_create_request(&draft)?;
    Ok(draft)
}

fn read_draft_value(path: &str) -> Result<Value> {
    let text = if path == "-" {
        use std::io::Read as _;
        let mut buf = String::new();
        std::io::stdin().read_to_string(&mut buf)?;
        buf
    } else {
        fs::read_to_string(path).with_context(|| format!("reading draft from `{path}`"))?
    };
    serde_json::from_str(&text).context("draft must be valid JSON")
}

fn validate_full_update_draft(value: &Value) -> Result<()> {
    let object = value
        .as_object()
        .ok_or_else(|| anyhow!("update draft must be a JSON object"))?;
    for field in [
        "title",
        "description",
        "color",
        "icon",
        "external_id",
        "priority",
        "required_duration_ms",
        "horizon_start",
        "horizon_end",
        "plan_role",
        "plan",
        "flows",
        "generation",
        "window",
        "split_policy",
        "relations",
        "source_client_local_id",
    ] {
        if !object.contains_key(field) {
            bail!(
                "update is a full replacement under the pinned OpenAPI contract; draft must explicitly include `{field}` (use null for optional tile metadata or source_client_local_id, and [] for empty collections)"
            );
        }
    }
    for field in ["description", "color", "icon", "external_id"] {
        let field_value = &object[field];
        if !field_value.is_null() && !field_value.is_string() {
            bail!("update draft field `{field}` must be a string or null");
        }
    }
    if !object["source_client_local_id"].is_null() && !object["source_client_local_id"].is_string()
    {
        bail!("update draft field `source_client_local_id` must be a UUID string or null");
    }
    for field in ["flows", "relations"] {
        if !object[field].is_array() {
            bail!("update draft field `{field}` must be an array; use [] when empty");
        }
    }
    for field in ["generation", "window", "split_policy"] {
        if !object[field].is_object() {
            bail!("update draft field `{field}` must be an object");
        }
    }
    let plan = object["plan"]
        .as_object()
        .ok_or_else(|| anyhow!("update draft field `plan` must be an object"))?;
    for (field, array) in [
        ("completion", false),
        ("planning", false),
        ("metrics", true),
        ("decisions", true),
        ("references", true),
    ] {
        let valid = plan
            .get(field)
            .is_some_and(|v| if array { v.is_array() } else { v.is_object() });
        if !valid {
            let kind = if array { "array" } else { "object" };
            bail!("full update plan must include `{field}` as an {kind}");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn monthly_is_refused_instead_of_approximated() {
        let draft = build_schedule_input(None, None, None, None, None, None);
        assert!(draft.is_err());
    }

    #[test]
    fn once_parses_an_exact_instant() {
        let input = build_schedule_input(
            Some("2026-11-20T09:00:00Z".to_string()),
            None,
            None,
            None,
            None,
            None,
        )
        .unwrap();
        assert!(matches!(input, ScheduleInput::OneTime { .. }));
    }

    #[test]
    fn conflicting_recurrence_flags_are_rejected() {
        let err = build_schedule_input(
            None,
            Some("09:00".to_string()),
            Some("10:00".to_string()),
            Some("mon".to_string()),
            None,
            None,
        )
        .unwrap_err();
        assert!(err.to_string().contains("choose exactly one"));
    }

    #[test]
    fn recurring_input_defaults_to_an_explicit_utc_offset() {
        let schedule = build_schedule_input(
            None,
            Some("09:00".to_string()),
            None,
            None,
            Some("2026-11-20".to_string()),
            None,
        )
        .unwrap();
        assert_eq!(generation_offset(&schedule, None), Some(0));
    }

    #[test]
    fn weekly_requires_days() {
        let err = build_schedule_input(
            None,
            None,
            Some("09:00".to_string()),
            None,
            Some("2026-11-20".to_string()),
            Some(540),
        )
        .unwrap_err();
        assert!(err.to_string().contains("--days"));
    }

    #[test]
    fn weekly_with_days_builds_the_mask() {
        let input = build_schedule_input(
            None,
            None,
            Some("09:00".to_string()),
            Some("mon,wed,fri".to_string()),
            Some("2026-11-20".to_string()),
            Some(540),
        )
        .unwrap();
        match input {
            ScheduleInput::Weekly { weekdays, .. } => {
                assert_eq!(weekdays.len(), 3);
            }
            other => panic!("unexpected schedule input: {other:?}"),
        }
    }

    #[test]
    fn excluded_dates_are_validated() {
        assert!(parse_excluded_date("2026-12-25").is_ok());
        assert!(parse_excluded_date("25/12/2026").is_err());
    }

    #[test]
    fn full_update_requires_every_contract_replacement_field() {
        let mut draft = serde_json::json!({
            "title": "complete",
            "description": null,
            "color": null,
            "icon": null,
            "external_id": null,
            "priority": 0,
            "required_duration_ms": 1800000,
            "horizon_start": "2026-11-15T00:00:00Z",
            "horizon_end": "2026-11-29T00:00:00Z",
            "plan_role": 0,
            "plan": {
                "completion": {"root": {"All": []}, "tasks": [], "time_requirements": []},
                "planning": {"nesting_rules": [], "placement_rules": []},
                "metrics": [], "decisions": [], "references": []
            },
            "flows": [],
            "generation": {"kind": 0, "at": "2026-11-15T00:00:00Z"},
            "window": {"start_offset_ms": 0, "end_offset_ms": 14400000},
            "split_policy": {"kind": 0},
            "relations": [],
            "source_client_local_id": null
        });
        assert!(validate_full_update_draft(&draft).is_ok());
        let mut missing_client_id = draft.clone();
        missing_client_id
            .as_object_mut()
            .unwrap()
            .remove("source_client_local_id");
        assert!(
            validate_full_update_draft(&missing_client_id)
                .unwrap_err()
                .to_string()
                .contains("`source_client_local_id`")
        );
        draft.as_object_mut().unwrap().remove("plan");
        assert!(
            validate_full_update_draft(&draft)
                .unwrap_err()
                .to_string()
                .contains("`plan`")
        );
    }
}
