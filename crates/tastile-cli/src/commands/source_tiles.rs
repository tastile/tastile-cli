//! `tastile source-tiles` — list / create / get / update / cancel /
//! completion / placements / reflow source tiles.
//!
//! All HTTP work goes through `crate::app::*`.

use std::fs;
use std::process::ExitCode;

use anyhow::{Context, Result, bail};
use uuid::Uuid;

use crate::app::{self, AppContext, CreateSourceTileDraft};
use crate::cli::{SourceTilesArgs, SourceTilesCommand};
use crate::output;

pub async fn run(cfg: tastile_config::Config, args: SourceTilesArgs) -> Result<ExitCode> {
    let ctx = AppContext::load(cfg)?;
    match args.sub {
        SourceTilesCommand::List { json, plan: _ } => list(&ctx, json).await,
        SourceTilesCommand::Create { from_json, title } => create(&ctx, from_json, title).await,
        SourceTilesCommand::Get { id, json } => get(&ctx, id, json).await,
        SourceTilesCommand::Update { id, from_json } => update(&ctx, id, from_json).await,
        SourceTilesCommand::Cancel { id, reason } => cancel(&ctx, id, &reason).await,
        SourceTilesCommand::Completion { id, json } => completion(&ctx, id, json).await,
        SourceTilesCommand::Placements { id, json } => placements(&ctx, id, json).await,
        SourceTilesCommand::Reflow { id, from, to } => reflow(&ctx, id, from, to).await,
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
    }
    Ok(ExitCode::SUCCESS)
}

async fn create(
    ctx: &AppContext,
    from_json: Option<String>,
    title: Option<String>,
) -> Result<ExitCode> {
    let draft = match (from_json, title) {
        (Some(path), _) => load_draft(&path).context("could not load --from-json draft")?,
        (None, Some(t)) => CreateSourceTileDraft::minimal(t),
        (None, None) => bail!("provide either --title TITLE or --from-json PATH"),
    };
    let resp = app::create_source_tile_draft(ctx, &draft).await?;
    println!("✓ Created source tile.");
    output::print_command_response(&resp);
    Ok(ExitCode::SUCCESS)
}

async fn update(ctx: &AppContext, id: Uuid, from_json: Option<String>) -> Result<ExitCode> {
    let Some(path) = from_json else {
        bail!("update requires --from-json PATH; see `docs/architecture.md` for the schema");
    };
    let draft = load_draft(&path).context("could not load --from-json draft")?;
    let resp = app::update_source_tile_draft(ctx, id, &draft).await?;
    println!("✓ Updated source tile.");
    output::print_command_response(&resp);
    Ok(ExitCode::SUCCESS)
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

async fn cancel(ctx: &AppContext, id: Uuid, reason: &str) -> Result<ExitCode> {
    let resp = app::cancel_source_tile(ctx, id, reason).await?;
    println!("✓ Cancelled source tile.");
    output::print_command_response(&resp);
    Ok(ExitCode::SUCCESS)
}

async fn completion(ctx: &AppContext, id: Uuid, json: bool) -> Result<ExitCode> {
    let v = app::source_tile_completion(ctx, id).await?;
    // Completion is opaque nested JSON (tasks + time_requirements tree);
    // we always pretty-print it. `--json` is accepted for symmetry.
    let _ = json;
    println!("{}", serde_json::to_string_pretty(&v)?);
    Ok(ExitCode::SUCCESS)
}

async fn placements(ctx: &AppContext, id: Uuid, json: bool) -> Result<ExitCode> {
    let items = tastile_api::list_source_tile_placements(ctx, ctx.require_token()?, id, Some(500))
        .await
        .context("list_source_tile_placements failed")?;
    if json {
        println!("{}", serde_json::to_string_pretty(&items)?);
    } else if items.is_empty() {
        println!("(no placements)");
    } else {
        output::print_placements_table(&items);
    }
    Ok(ExitCode::SUCCESS)
}

async fn reflow(
    ctx: &AppContext,
    id: Uuid,
    from: chrono::DateTime<chrono::Utc>,
    to: chrono::DateTime<chrono::Utc>,
) -> Result<ExitCode> {
    let resp = app::reflow_source_tile(
        ctx,
        id,
        tastile_api::Span {
            start: from,
            end: to,
        },
    )
    .await?;
    println!("✓ Reflow dispatched.");
    output::print_command_response(&resp);
    Ok(ExitCode::SUCCESS)
}

fn load_draft(path: &str) -> Result<CreateSourceTileDraft> {
    let text = if path == "-" {
        use std::io::Read as _;
        let mut buf = String::new();
        std::io::stdin().read_to_string(&mut buf)?;
        buf
    } else {
        fs::read_to_string(path).with_context(|| format!("reading draft from `{path}`"))?
    };
    Ok(serde_json::from_str(&text)?)
}
