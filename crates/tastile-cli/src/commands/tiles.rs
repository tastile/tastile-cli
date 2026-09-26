//! `tastile tiles` — list tiles for the current day.

use std::process::ExitCode;

use anyhow::Result;

use crate::app::{self, AppContext};
use crate::cli::TilesArgs;
use crate::output;

pub async fn run(cfg: tastile_config::Config, args: TilesArgs) -> Result<ExitCode> {
    let ctx = AppContext::load(cfg)?;
    let mut tiles = app::today(&ctx).await?;
    if args.next {
        tiles.truncate(1);
    }
    if tiles.is_empty() {
        println!("(no tiles)");
        return Ok(ExitCode::SUCCESS);
    }
    if args.json {
        println!("{}", serde_json::to_string_pretty(&tiles)?);
        return Ok(ExitCode::SUCCESS);
    }
    output::print_tiles_table(&tiles);
    Ok(ExitCode::SUCCESS)
}
