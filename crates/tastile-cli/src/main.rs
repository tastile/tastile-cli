//! `tastile` — command-line and TUI client for the Tastile v1 API.
//!
//! Run with no arguments to launch the TUI. Run with a subcommand to invoke
//! a single CLI operation and exit. See `docs/architecture.md` for the
//! overall design.
//!
//! Both the CLI subcommands and the TUI share [`app::AppContext`] and the
//! helper functions in [`app`] so the HTTP layer is implemented exactly
//! once.

#![doc(html_root_url = "https://docs.rs/tastile-cli/1.0.0")]

use std::process::ExitCode;

use clap::Parser;
use tracing::{error, info};

mod app;
mod cli;
mod commands;
mod output;
mod tracing_init;
mod tui;

use cli::{Cli, Command};

fn main() -> ExitCode {
    let cli = Cli::parse();

    if let Err(e) = tracing_init::init(cli.verbose) {
        eprintln!("warning: could not initialise tracing: {e}");
    }

    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            error!("could not start tokio runtime: {e}");
            return ExitCode::FAILURE;
        }
    };

    let result = runtime.block_on(run(cli));
    match result {
        Ok(code) => code,
        Err(e) => {
            error!("{e}");
            ExitCode::FAILURE
        }
    }
}

async fn run(cli: Cli) -> anyhow::Result<ExitCode> {
    info!("tastile {}", env!("CARGO_PKG_VERSION"));

    let cfg = tastile_config::load().unwrap_or_default();

    match cli.command {
        None => {
            let cfg = tastile_config::with_env_overrides(cfg);
            tui::run(cfg).await.map(|()| ExitCode::SUCCESS)
        }
        Some(Command::Auth(args)) => commands::auth::run(cfg, args).await,
        Some(Command::Doctor) => commands::doctor::run(cfg).await,
        Some(Command::Tiles(args)) => commands::tiles::run(cfg, args).await,
        Some(Command::Today(args)) => commands::today::run(cfg, args).await,
        Some(Command::Schedule(args)) => commands::schedule::run(cfg, args).await,
        Some(Command::SourceTiles(args)) => commands::source_tiles::run(cfg, args).await,
        Some(Command::Executions(args)) => commands::executions::run(cfg, args).await,
        Some(Command::Prompts(args)) => commands::prompts::run(cfg, args).await,
        Some(Command::Completions(args)) => commands::completions::run(args.shell),
        Some(Command::Version) => {
            println!("tastile {}", env!("CARGO_PKG_VERSION"));
            println!(
                "openapi: {} ({})",
                tastile_api::API_VERSION,
                tastile_api::API_TITLE
            );
            println!("wire:");
            println!("  GET    /v1/tiles");
            println!("  GET    /v1/prompts/pending");
            println!("  POST   /v1/prompts");
            println!("  POST   /v1/prompts/{{id}}/resolve");
            println!("  POST   /v1/prompts/startup-recovery");
            println!("  GET    /v1/source-tiles");
            println!("  POST   /v1/source-tiles");
            println!("  GET    /v1/source-tiles/{{id}}");
            println!("  PUT    /v1/source-tiles/{{id}}");
            println!("  POST   /v1/source-tiles/{{id}}/cancel");
            println!("  GET    /v1/source-tiles/{{id}}/completion");
            println!("  GET    /v1/source-tiles/{{id}}/placements");
            println!("  POST   /v1/source-tiles/{{id}}/reflow");
            println!("  POST   /v1/placements/{{id}}/executions");
            println!("  POST   /v1/executions/{{id}}/pause");
            println!("  POST   /v1/executions/{{id}}/resume");
            println!("  POST   /v1/executions/{{id}}/finish");
            println!("  POST   /v1/auth/signout");
            Ok(ExitCode::SUCCESS)
        }
    }
}
