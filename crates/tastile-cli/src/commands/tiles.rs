//! `tastile tiles` — list tiles for the current day.

use std::process::ExitCode;

use anyhow::Result;
use tastile_api::tiles::ListTilesQuery;

use crate::app::{self, AppContext};
use crate::cli::TilesArgs;
use crate::output;

pub async fn run(cfg: tastile_config::Config, args: TilesArgs) -> Result<ExitCode> {
    let ctx = AppContext::load(cfg)?;
    let query = ListTilesQuery {
        lifecycle: args.lifecycle.clone(),
        search: args.search.clone(),
        view_mode: args.view_mode.clone(),
        limit: args.limit,
        // "Today" means: exclude tiles whose first upcoming placement is
        // strictly in the future.
        exclude_future: Some(true),
        ..Default::default()
    };
    let mut tiles = app::list_tiles(&ctx, query).await?;
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
    println!("{} tile(s)", tiles.len());
    Ok(ExitCode::SUCCESS)
}
