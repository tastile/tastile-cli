//! `tastile today` — show the next actionable tile.

use std::process::ExitCode;

use anyhow::{Context, Result};
use chrono::{DateTime, Datelike, Timelike, Utc};
use tastile_api::{ApiClient, ApiConfig, BearerToken, tiles::ListTilesQuery};
use tastile_auth::{CredentialStore, KeyringStore};
use tastile_config::with_env_overrides;

use crate::cli::TodayArgs;

pub async fn run(cfg: tastile_config::Config, args: TodayArgs) -> Result<ExitCode> {
    let cfg = with_env_overrides(cfg);
    let token = load_token(&cfg).await?;
    let api = ApiClient::new(ApiConfig::new(&cfg.api_url)?)?;

    let (start, end) = today_window();
    let q = ListTilesQuery {
        window_start: Some(start),
        window_end: Some(end),
        include_closed: Some(false),
        ..Default::default()
    };

    let tiles = tastile_api::list_tiles(&api, &token, &q).await?;
    if tiles.is_empty() {
        println!("No tiles scheduled for today.");
        return Ok(ExitCode::SUCCESS);
    }
    let mut next = tiles
        .into_iter()
        .min_by_key(|t| t.projected_next_start_at.unwrap_or_else(Utc::now))
        .context("could not find a next tile")?;

    if args.json {
        println!("{}", serde_json::to_string_pretty(&next)?);
        return Ok(ExitCode::SUCCESS);
    }

    println!("{}", next.title);
    if let Some(d) = next.projected_next_start_at {
        println!("Next start: {}", d.format("%Y-%m-%d %H:%M"));
    }
    println!(
        "Worked/break: {}m / {}m",
        next.worked_minutes / 60_000,
        next.break_minutes / 60_000
    );
    if !next.labels.is_empty() {
        println!("Labels: {}", next.labels.join(", "));
    }
    // Silence unused-assignment warning when `--json` takes the early exit.
    let _ = &mut next;
    Ok(ExitCode::SUCCESS)
}

fn today_window() -> (DateTime<Utc>, DateTime<Utc>) {
    let now = Utc::now();
    let start = now
        .with_hour(0)
        .and_then(|d| d.with_minute(0))
        .and_then(|d| d.with_second(0))
        .unwrap_or(now)
        .date_naive()
        .and_hms_opt(0, 0, 0)
        .unwrap();
    let end = start
        .with_year(start.year())
        .and_then(|d| d.with_month(start.month()))
        .and_then(|d| d.with_day(start.day()))
        .unwrap_or(start)
        + chrono::Duration::days(1);
    (start.and_utc(), end.and_utc())
}

async fn load_token(cfg: &tastile_config::Config) -> Result<BearerToken> {
    let loaded = KeyringStore
        .load(tastile_auth::DEFAULT_SERVICE, tastile_auth::DEFAULT_USER)
        .context("credential store error")?
        .ok_or_else(|| anyhow::anyhow!("not signed in; run `tastile auth login` first"))?;
    if loaded.api_base_url != cfg.api_url {
        anyhow::bail!(
            "stored credential is for `{}` but config says `{}`",
            loaded.api_base_url,
            cfg.api_url
        );
    }
    Ok(BearerToken::new(loaded.bearer))
}
