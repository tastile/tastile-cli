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
use url::Url;
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
                 then add `--apply` to publish to a loopback Core."
            )
        }
        ScheduleCommand::Publish {
            from_json,
            apply,
            idempotency_key,
        } => publish(cfg, &from_json, apply, idempotency_key).await,
    }
}

async fn publish(
    cfg: Config,
    from_json: &str,
    apply: bool,
    idempotency_key: Option<Uuid>,
) -> Result<ExitCode> {
    let payload = load_payload(from_json)?;
    let request = PublishScheduleDefinitionRequest::new(
        payload,
        idempotency_key.unwrap_or_else(Uuid::new_v4),
    )
    .map_err(anyhow::Error::msg)
    .context("invalid schedule payload")?;
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
    ensure_local_target(&cfg)?;
    let ctx = AppContext::load(cfg)?;
    let response = publish_schedule_definition(&ctx.api, ctx.require_token()?, &request)
        .await
        .context("schedule publish failed; retry with the same --idempotency-key")?;
    output::print_command_response(&response);
    Ok(ExitCode::SUCCESS)
}

fn load_payload(path: &str) -> Result<Value> {
    let text = if path == "-" {
        let mut buf = String::new();
        std::io::stdin()
            .read_to_string(&mut buf)
            .context("reading schedule payload from standard input")?;
        buf
    } else {
        fs::read_to_string(path).with_context(|| format!("reading schedule payload from `{path}`"))?
    };
    serde_json::from_str(&text).context("schedule payload must be valid JSON")
}

fn ensure_local_target(cfg: &Config) -> Result<()> {
    let url = Url::parse(&cfg.api_url).context("invalid configured API URL")?;
    let is_local = matches!(
        url.host_str(),
        Some("localhost" | "127.0.0.1" | "::1" | "[::1]")
    );
    if !is_local {
        bail!(
            "refusing to publish to non-loopback API {}; this local-first CLI \
             cannot mutate a remote or operational schedule",
            cfg.api_url
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_target_gate_is_local_only() {
        let mut cfg = Config::default();
        assert!(ensure_local_target(&cfg).is_err());
        cfg.api_url = "http://127.0.0.1:31400".into();
        assert!(ensure_local_target(&cfg).is_ok());
        cfg.api_url = "http://localhost:31400".into();
        assert!(ensure_local_target(&cfg).is_ok());
        cfg.api_url = "http://192.168.1.2:31400".into();
        assert!(ensure_local_target(&cfg).is_err());
    }

    #[test]
    fn invalid_input_stops_before_network() {
        let result = PublishScheduleDefinitionRequest::new(
            serde_json::json!({"tile": {"title": "x"}, "plan": {}}),
            Uuid::new_v4(),
        );
        assert!(result.is_err());
    }
}
