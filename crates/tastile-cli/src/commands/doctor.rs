//! `tastile doctor` — environment diagnostics. Deliberately hides secrets.

use std::process::ExitCode;

use anyhow::Result;
use tastile_api::{ApiClient, ApiConfig, BearerToken};
use tastile_auth::{CredentialStore, KeyringStore};
use tastile_config::with_env_overrides;

pub async fn run(cfg: tastile_config::Config) -> Result<ExitCode> {
    let cfg = with_env_overrides(cfg);

    println!("tastile doctor");
    println!("--------------");
    println!();
    println!("Versions:");
    println!("  tastile-cli: {}", env!("CARGO_PKG_VERSION"));
    println!(
        "  pinned openapi: {} ({})",
        tastile_api::API_VERSION,
        tastile_api::API_TITLE
    );
    println!("  rustc:        {}", rustc_version_runtime());
    println!();

    println!("Configuration:");
    println!("  api_url:   {}", cfg.api_url);
    println!("  web_url:   {}", cfg.web_url);
    println!("  timeout:   {} ms", cfg.api_timeout_ms);
    println!("  config:    {:?}", tastile_config::default_config_path());
    println!();

    println!("OS credential store:");
    let store = KeyringStore;
    match store.load(tastile_auth::DEFAULT_SERVICE, tastile_auth::DEFAULT_USER) {
        Ok(Some(_)) => println!("  credential: present (token not displayed)"),
        Ok(None) => println!("  credential: not set"),
        Err(e) => println!("  credential: error: {e}"),
    }
    println!();

    println!("API reachability:");
    let api = ApiClient::new(ApiConfig::new(&cfg.api_url)?)?;
    let bearer = BearerToken::new("probe-only-token");
    match tastile_api::list_tiles(
        &api,
        &bearer,
        &tastile_api::tiles::ListTilesQuery::default(),
    )
    .await
    {
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
    // Compile-time capture of rustc version; the `RUSTC_VERSION` env var is
    // set by Cargo. If absent (e.g. some cross-compile setups) we fall back
    // to "unknown".
    option_env!("CARGO_PKG_RUST_VERSION").unwrap_or("unknown")
}
