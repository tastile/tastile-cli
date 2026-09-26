//! `tastile tiles` — list tiles.

use std::process::ExitCode;

use anyhow::{Context, Result};
use tastile_api::{ApiClient, ApiConfig, BearerToken, tiles::ListTilesQuery};
use tastile_auth::{CredentialStore, KeyringStore};
use tastile_config::with_env_overrides;

use crate::cli::TilesArgs;
use crate::output::print_table;

pub async fn run(cfg: tastile_config::Config, args: TilesArgs) -> Result<ExitCode> {
    let cfg = with_env_overrides(cfg);
    let token = load_token(&cfg).await?;
    let api = ApiClient::new(ApiConfig::new(&cfg.api_url)?)?;

    let mut q = ListTilesQuery::default();
    q.plan_id = args.plan;
    q.include_closed = if args.include_closed {
        Some(true)
    } else {
        None
    };

    let mut tiles = tastile_api::list_tiles(&api, &token, &q).await?;
    if tiles.is_empty() {
        println!("(no tiles)");
        return Ok(ExitCode::SUCCESS);
    }
    if args.next {
        tiles.sort_by(|a, b| {
            a.projected_next_start_at
                .unwrap_or_else(chrono::Utc::now)
                .cmp(&b.projected_next_start_at.unwrap_or_else(chrono::Utc::now))
        });
        tiles.truncate(1);
    } else {
        tiles.sort_by_key(|a| a.id);
    }

    if args.json {
        println!("{}", serde_json::to_string_pretty(&tiles)?);
        return Ok(ExitCode::SUCCESS);
    }

    let mut rows: Vec<Vec<String>> = Vec::with_capacity(tiles.len());
    for t in &tiles {
        rows.push(vec![
            t.id.to_string(),
            short(&t.title, 40),
            format!("{}", t.lifecycle),
            format!(
                "{}m / {}m",
                t.worked_minutes / 60_000,
                t.break_minutes / 60_000
            ),
            t.projected_next_start_at
                .map(|d| d.format("%Y-%m-%d %H:%M").to_string())
                .unwrap_or_else(|| "-".into()),
            t.labels.join(","),
        ]);
    }
    print_table(
        &[
            "id",
            "title",
            "lifecycle",
            "worked/break",
            "next start",
            "labels",
        ],
        rows,
    );

    Ok(ExitCode::SUCCESS)
}

async fn load_token(cfg: &tastile_config::Config) -> Result<BearerToken> {
    let api_url = &cfg.api_url;
    let loaded = KeyringStore
        .load(tastile_auth::DEFAULT_SERVICE, tastile_auth::DEFAULT_USER)
        .context("credential store error")?
        .ok_or_else(|| anyhow::anyhow!("not signed in; run `tastile auth login` first"))?;
    if loaded.api_base_url != *api_url {
        anyhow::bail!(
            "stored credential is for `{}` but config says `{}`",
            loaded.api_base_url,
            api_url
        );
    }
    Ok(BearerToken::new(loaded.bearer))
}

fn short(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
        out.push('…');
        out
    }
}
