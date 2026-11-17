//! `tastile timeline` — placements intersecting a required time range.
//!
//! Read-only: no write guard. LABEL (`role = 1`) items are non-blocking
//! label periods (v1/14 §4); they are marked in a separate lane and never
//! count as busy time.

use std::process::ExitCode;

use anyhow::{Context, Result, anyhow, bail};
use chrono::{DateTime, Duration, Utc};

use crate::app::{self, AppContext};
use crate::cli::TimelineArgs;
use crate::output;
use tastile_api::TimelineQuery;

pub async fn run(cfg: tastile_config::Config, args: TimelineArgs) -> Result<ExitCode> {
    let ctx = AppContext::load(cfg)?;
    timeline(&ctx, args).await
}

async fn timeline(ctx: &AppContext, args: TimelineArgs) -> Result<ExitCode> {
    let start = parse_instant(&args.start, "--start")?;
    let end = parse_instant(&args.end, "--end")?;
    if start >= end {
        bail!("--start must be before --end");
    }
    if end - start > Duration::days(31) {
        bail!("the timeline window may span at most 31 days");
    }
    if args.parent_placement_id.is_some() && !args.include_nested {
        bail!("--parent-placement-id requires --include-nested");
    }
    let exclude_source_kinds = match args.exclude_source_kinds.as_deref() {
        Some(raw) => raw
            .split(',')
            .map(|part| {
                part.trim()
                    .parse::<i32>()
                    .map_err(|_| anyhow!("invalid --exclude-source-kinds value `{part}`; expected numeric PlacementSource constants"))
            })
            .collect::<Result<Vec<_>>>()?,
        None => Vec::new(),
    };
    let query = TimelineQuery {
        start,
        end,
        parent_placement_id: args.parent_placement_id,
        include_labels: args.include_labels,
        include_closed: args.include_closed,
        include_blocked: args.include_blocked,
        include_nested: args.include_nested,
        owner_ids: args.owners,
        limit: args.limit,
        offset: args.offset,
        summary_month: args.month,
        min_duration_ms: args.min_duration_ms,
        min_recurring_step_ms: args.min_recurring_step_ms,
        exclude_source_kinds,
    };
    let items = app::timeline(ctx, &query).await?;
    if args.json {
        println!("{}", serde_json::to_string_pretty(&items)?);
    } else {
        output::print_timeline(&items);
    }
    Ok(ExitCode::SUCCESS)
}

fn parse_instant(raw: &str, flag: &str) -> Result<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(raw.trim())
        .map(|dt| dt.with_timezone(&Utc))
        .with_context(|| format!("invalid {flag} `{raw}`; expected RFC 3339"))
}
