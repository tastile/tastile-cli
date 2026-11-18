//! `tastile schedule` — local-first schedule publication.
//!
//! A preview is completely offline; `--apply` makes one authenticated API
//! request and is limited to a loopback Core to protect the Muse production
//! source of truth until an explicit migration gate is met.

use std::fs;
use std::io::Read as _;
use std::process::ExitCode;

use anyhow::{Context, Result, bail};
use serde_json::Value;
use tastile_api::{PublishScheduleDefinitionRequest, publish_schedule_definition};
use tastile_config::{Config, with_env_overrides};
use uuid::Uuid;

use crate::app::AppContext;
use crate::cli::{ScheduleArgs, ScheduleCommand};
use crate::output;

pub async fn run(cfg: Config, args: ScheduleArgs) -> Result<ExitCode> {
    match args.sub {
        ScheduleCommand::Regenerate { dry_run: _ } => {
            bail!(
                "`schedule regenerate` has never been implemented and did not send an API request. \
                 Use `tastile schedule publish --from-json PATH` to preview a real payload, \
                 then publish with `--apply --idempotency-key <UUID>` (reuse the key shown \
                 by the preview) and TASTILE_API_URL pointing to a loopback Core."
            )
        }
        ScheduleCommand::Publish {
            from_json,
            apply,
            idempotency_key,
            expect_revision,
        } => publish(cfg, &from_json, apply, idempotency_key, expect_revision).await,
    }
}

async fn publish(
    cfg: Config,
    from_json: &str,
    apply: bool,
    idempotency_key: Option<Uuid>,
    expect_revision: Option<i64>,
) -> Result<ExitCode> {
    let idempotency_key = resolve_idempotency_key(apply, idempotency_key)?;
    let payload = load_payload(from_json)?;
    let mut request = PublishScheduleDefinitionRequest::new(payload, idempotency_key)
        .map_err(anyhow::Error::msg)
        .context("invalid schedule payload")?;
    if let Some(revision) = expect_revision {
        request = request.with_expected_revision(revision);
    }
    if !apply {
        // Preview without auth, credential store, network, or database.
        println!("{}", serde_json::to_string_pretty(&request)?);
        eprintln!(
            "Preview only: no API request sent. \
             To publish, use --apply with TASTILE_API_URL pointing to a loopback Core."
        );
        return Ok(ExitCode::SUCCESS);
    }

    let cfg = with_env_overrides(cfg);
    let ctx = AppContext::load(cfg)?;
    // The write gate classifies the target and proves it answers as a
    // Tastile v1 API before the publish leaves the process.
    crate::app::guard_schedule_publish_write(&ctx).await?;
    let response = publish_schedule_definition(&ctx.api, ctx.require_token()?, &request)
        .await
        .context("schedule publish failed; retry with the same --idempotency-key")?;
    output::print_command_response(&response);
    Ok(ExitCode::SUCCESS)
}

fn resolve_idempotency_key(apply: bool, idempotency_key: Option<Uuid>) -> Result<Uuid> {
    match (apply, idempotency_key) {
        (true, None) => bail!(
            "schedule publish --apply requires --idempotency-key UUID so an uncertain result can be retried safely"
        ),
        (_, Some(key)) => Ok(key),
        (false, None) => Ok(Uuid::new_v4()),
    }
}

fn load_payload(path: &str) -> Result<Value> {
    let text = if path == "-" {
        let mut buf = String::new();
        std::io::stdin()
            .read_to_string(&mut buf)
            .context("reading schedule payload from standard input")?;
        buf
    } else {
        fs::read_to_string(path)
            .with_context(|| format!("reading schedule payload from `{path}`"))?
    };
    serde_json::from_str(&text).context("schedule payload must be valid JSON")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_input_stops_before_network() {
        let result = PublishScheduleDefinitionRequest::new(
            serde_json::json!({"tile": {"title": "x"}, "plan": {}}),
            Uuid::new_v4(),
        );
        assert!(result.is_err());
    }

    #[test]
    fn publish_never_targets_the_operational_port() {
        // The write gate is shared with every other mutating command and
        // refuses the reserved operational port outright.
        let mut cfg = Config::default();
        cfg.api_url = "http://127.0.0.1:31400".into();
        assert!(
            crate::safety::classify_write_target(&cfg, true).is_err(),
            "the operational port must never accept a write"
        );
    }

    #[test]
    fn apply_requires_a_reusable_idempotency_key() {
        assert!(resolve_idempotency_key(true, None).is_err());
        let key = Uuid::new_v4();
        assert_eq!(resolve_idempotency_key(true, Some(key)).unwrap(), key);
        assert!(resolve_idempotency_key(false, None).is_ok());
    }
}
