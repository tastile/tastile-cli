//! `tastile prompts` — list / resolve / request / startup-recovery.

use std::process::ExitCode;

use anyhow::Result;

use crate::app::{self, AppContext, PromptResolution};
use crate::cli::{PromptsArgs, PromptsCommand};
use crate::output;

pub async fn run(cfg: tastile_config::Config, args: PromptsArgs) -> Result<ExitCode> {
    let ctx = AppContext::load(cfg)?;
    match args.sub {
        PromptsCommand::List { json } => list(&ctx, json).await,
        PromptsCommand::Resolve {
            id,
            resolution,
            note,
            json,
        } => resolve(&ctx, id, &resolution, note, json).await,
        PromptsCommand::Request { kind, note, json } => request(&ctx, kind, note, json).await,
        PromptsCommand::StartupRecovery {
            id,
            resolution,
            note,
            json,
        } => startup_recovery(&ctx, id, &resolution, note, json).await,
    }
}

async fn list(ctx: &AppContext, json: bool) -> Result<ExitCode> {
    let prompts = app::list_pending_prompts(ctx).await?;
    if prompts.is_empty() {
        println!("(no pending prompts)");
        return Ok(ExitCode::SUCCESS);
    }
    if json {
        println!("{}", serde_json::to_string_pretty(&prompts)?);
    } else {
        output::print_prompts_table(&prompts);
    }
    Ok(ExitCode::SUCCESS)
}

async fn resolve(
    ctx: &AppContext,
    id: uuid::Uuid,
    resolution: &str,
    note: Option<String>,
    json: bool,
) -> Result<ExitCode> {
    let r = PromptResolution::from_str(resolution)?;
    let resp = app::resolve_prompt(ctx, id, r, note).await?;
    if json {
        println!("{}", serde_json::to_string_pretty(&resp)?);
    } else {
        println!("✓ Resolved prompt.");
        output::print_command_response(&resp);
    }
    Ok(ExitCode::SUCCESS)
}

async fn request(
    ctx: &AppContext,
    kind: i32,
    note: Option<String>,
    json: bool,
) -> Result<ExitCode> {
    let resp = app::request_prompt(ctx, kind, note).await?;
    if json {
        println!("{}", serde_json::to_string_pretty(&resp)?);
    } else {
        println!("✓ Prompt requested.");
        output::print_command_response(&resp);
    }
    Ok(ExitCode::SUCCESS)
}

async fn startup_recovery(
    ctx: &AppContext,
    id: uuid::Uuid,
    resolution: &str,
    note: Option<String>,
    json: bool,
) -> Result<ExitCode> {
    let r = PromptResolution::from_str(resolution)?;
    let resp = app::respond_startup_recovery(ctx, id, r, note).await?;
    if json {
        println!("{}", serde_json::to_string_pretty(&resp)?);
    } else {
        println!("✓ Startup-recovery acknowledged.");
        output::print_command_response(&resp);
    }
    Ok(ExitCode::SUCCESS)
}
