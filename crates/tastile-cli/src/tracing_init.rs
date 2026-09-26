//! Tracing setup. The CLI uses `tracing-subscriber` with an env filter that
//! defaults to `info` and is upgraded to `debug` when `--verbose` is passed.

use tracing_subscriber::{EnvFilter, fmt, prelude::*};

pub fn init(verbose: bool) -> Result<(), Box<dyn std::error::Error>> {
    let default_level = if verbose { "debug" } else { "info" };
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        EnvFilter::new(format!(
            "tastile={default_level},tastile_api={default_level},tastile_auth={default_level},warn"
        ))
    });

    let layer = fmt::layer()
        .with_writer(std::io::stderr)
        .with_target(false)
        .compact();

    tracing_subscriber::registry()
        .with(filter)
        .with(layer)
        .try_init()
        .ok();
    Ok(())
}
