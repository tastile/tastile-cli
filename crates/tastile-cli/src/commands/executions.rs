//! `tastile executions start | pause | resume | finish`.

use std::process::ExitCode;

use anyhow::Result;
use uuid::Uuid;

use crate::app::{self, AppContext};
use crate::cli::{ExecutionsArgs, ExecutionsCommand};
use crate::output;

pub async fn run(cfg: tastile_config::Config, args: ExecutionsArgs) -> Result<ExitCode> {
    let ctx = AppContext::load(cfg)?;
    match args.sub {
        ExecutionsCommand::Start { placement_id, json } => start(&ctx, placement_id, json).await,
        ExecutionsCommand::Pause { execution_id, json } => pause(&ctx, execution_id, json).await,
        ExecutionsCommand::Resume { execution_id, json } => resume(&ctx, execution_id, json).await,
        ExecutionsCommand::Finish {
            execution_id,
            kind,
            note,
            json,
        } => finish(&ctx, execution_id, kind, note, json).await,
    }
}

async fn start(ctx: &AppContext, placement_id: Uuid, json: bool) -> Result<ExitCode> {
    let resp = app::start_execution(ctx, placement_id).await?;
    print_or_json("✓ Started execution.", &resp, json)?;
    Ok(ExitCode::SUCCESS)
}

async fn pause(ctx: &AppContext, execution_id: Uuid, json: bool) -> Result<ExitCode> {
    let resp = app::pause_execution(ctx, execution_id).await?;
    print_or_json("✓ Paused execution.", &resp, json)?;
    Ok(ExitCode::SUCCESS)
}

async fn resume(ctx: &AppContext, execution_id: Uuid, json: bool) -> Result<ExitCode> {
    let resp = app::resume_execution(ctx, execution_id).await?;
    print_or_json("✓ Resumed execution.", &resp, json)?;
    Ok(ExitCode::SUCCESS)
}

async fn finish(
    ctx: &AppContext,
    execution_id: Uuid,
    kind: i32,
    note: Option<String>,
    json: bool,
) -> Result<ExitCode> {
    let resp = app::finish_execution(ctx, execution_id, kind, note).await?;
    print_or_json("✓ Finished execution.", &resp, json)?;
    Ok(ExitCode::SUCCESS)
}

fn print_or_json(prefix: &str, resp: &tastile_api::CommandResponse, json: bool) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(resp)?);
    } else {
        println!("{prefix}");
        output::print_command_response(resp);
    }
    Ok(())
}
