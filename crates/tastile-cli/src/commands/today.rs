//! `tastile today` — show the next actionable tile.

use std::process::ExitCode;

use anyhow::Result;

use crate::app::{self, AppContext};
use crate::cli::TodayArgs;
use crate::output;

pub async fn run(cfg: tastile_config::Config, args: TodayArgs) -> Result<ExitCode> {
    let ctx = AppContext::load(cfg)?;
    match app::next_actionable(&ctx).await? {
        None => {
            println!("No tiles scheduled for today.");
            Ok(ExitCode::SUCCESS)
        }
        Some(t) => {
            if args.json {
                println!("{}", serde_json::to_string_pretty(&t)?);
            } else {
                output::print_tile(&t);
            }
            Ok(ExitCode::SUCCESS)
        }
    }
}
