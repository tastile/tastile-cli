//! `tastile schedule` — schedule operations.
//!
//! MVP: only `regenerate`. The CLI does not yet drive the
//! `POST /v1/schedule-definitions` write directly because the published
//! v1.0.0 spec does not include the corresponding `ScheduleDefinition`
//! payload schema in the typed surface. Until the next spec revision
//! lands, the regenerate subcommand prints the action the operator would
//! take.

use std::process::ExitCode;

use anyhow::Result;

use crate::cli::{ScheduleArgs, ScheduleCommand};

pub async fn run(cfg: tastile_config::Config, args: ScheduleArgs) -> Result<ExitCode> {
    match args.sub {
        ScheduleCommand::Regenerate { dry_run } => regenerate(cfg, dry_run).await,
    }
}

async fn regenerate(cfg: tastile_config::Config, dry_run: bool) -> Result<ExitCode> {
    println!("Scheduling regenerator is not yet wired to the CLI typed surface.");
    println!();
    println!(
        "Current API endpoint: POST {}/v1/schedule-definitions",
        cfg.api_url
    );
    println!("Operation ID: publish_schedule_definition");
    println!("Schema: PublishScheduleDefinitionRequest (components.schemas)");
    println!();
    if dry_run {
        println!("Dry run: no request was sent.");
    } else {
        println!("Re-run with --dry-run to skip the actual call once wired.");
    }
    Ok(ExitCode::SUCCESS)
}
