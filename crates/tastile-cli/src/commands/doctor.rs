//! `tastile doctor` — environment diagnostics. Deliberately hides secrets.

use std::process::ExitCode;

use anyhow::Result;
use tastile_api::{ApiClient, ApiConfig};
use tastile_auth::{CredentialStore, KeyringStore};

use crate::app::AppContext;

pub async fn run(cfg: tastile_config::Config) -> Result<ExitCode> {
    let ctx = AppContext::load(cfg)?;

    println!("tastile doctor");
    println!("--------------");
    println!();

    println!("Versions:");
    println!("  tastile-cli:    {}", env!("CARGO_PKG_VERSION"));
    println!(
        "  pinned openapi: {} ({})",
        tastile_api::API_VERSION,
        tastile_api::API_TITLE
    );
    println!("  rustc:          {}", rustc_version_runtime());
    println!();

    println!("Configuration:");
    println!("  api_url:   {}", ctx.config.api_url);
    println!("  web_url:   {}", ctx.config.web_url);
    println!("  timeout:   {} ms", ctx.config.api_timeout_ms);
    println!("  config:    {:?}", tastile_config::default_config_path());
    println!();

    println!("OS credential store:");
    match KeyringStore.load(tastile_auth::DEFAULT_SERVICE, tastile_auth::DEFAULT_USER) {
        Ok(Some(_)) => println!("  credential: present (token not displayed)"),
        Ok(None) => println!("  credential: not set"),
        Err(e) => println!("  credential: error: {e}"),
    }
    println!();

    println!("API reachability:");
    // Probe /v1/tiles with a deliberately-invalid token to confirm the
    // endpoint is reachable. A 401/403 means "up and authenticating".
    let api = ApiClient::new(ApiConfig::new(&ctx.config.api_url)?)?;
    let probe = tastile_api::tiles::list_tiles(
        &api,
        &tastile_api::BearerToken::new("probe-only-token"),
        &tastile_api::tiles::ListTilesQuery::default(),
    )
    .await;
    match probe {
        Ok(_) => println!("  GET /v1/tiles: 200 (unexpected with a probe token)"),
        Err(tastile_api::ApiError::Http { status, message }) => {
            println!("  GET /v1/tiles: {status} {message}");
        }
        Err(e) => println!("  GET /v1/tiles: error: {e}"),
    }

    println!();
    println!("Done.");
    Ok(ExitCode::SUCCESS)
}

fn rustc_version_runtime() -> &'static str {
    option_env!("CARGO_PKG_RUST_VERSION").unwrap_or("unknown")
}
