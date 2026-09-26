//! `tastile source-tiles` — list / get / cancel source tiles.

use std::process::ExitCode;

use anyhow::{Context, Result};
use tastile_api::{
    ApiClient, ApiConfig, BearerToken, cancel_source_tile, get_source_tile, list_source_tiles,
    source_tiles::CancelSourceTileRequest,
};
use tastile_auth::{CredentialStore, KeyringStore};
use tastile_config::with_env_overrides;

use crate::cli::{SourceTilesArgs, SourceTilesCommand};

pub async fn run(cfg: tastile_config::Config, args: SourceTilesArgs) -> Result<ExitCode> {
    let cfg = with_env_overrides(cfg);
    let token = load_token(&cfg).await?;
    let api = ApiClient::new(ApiConfig::new(&cfg.api_url)?)?;

    match args.sub {
        SourceTilesCommand::List { json, plan } => {
            let q = tastile_api::source_tiles::ListSourceTilesQuery {
                plan_id: plan,
                ..Default::default()
            };
            let tiles = list_source_tiles(&api, &token, &q).await?;
            if json {
                println!("{}", serde_json::to_string_pretty(&tiles)?);
            } else {
                for t in tiles {
                    println!(
                        "{} {} state={} rev={} title={}",
                        t.source_tile_id, t.plan_role, t.source_state, t.revision, t.title
                    );
                }
            }
            Ok(ExitCode::SUCCESS)
        }
        SourceTilesCommand::Get { id, json } => {
            let detail = get_source_tile(&api, &token, id).await?;
            if json {
                println!("{}", serde_json::to_string_pretty(&detail)?);
            } else {
                println!("source: {}", detail.source.title);
                println!("  id:    {}", detail.source.source_tile_id);
                println!("  rev:   {}", detail.source.revision);
                println!(
                    "  placements: {}  occurrences: {}  relations: {}",
                    detail.placements.len(),
                    detail.occurrences.len(),
                    detail.relations.len()
                );
                for p in &detail.placements {
                    println!(
                        "    {} {} → {}",
                        p.placement_id,
                        p.start.format("%Y-%m-%d %H:%M"),
                        p.end.format("%Y-%m-%d %H:%M")
                    );
                }
            }
            Ok(ExitCode::SUCCESS)
        }
        SourceTilesCommand::Cancel { id, reason } => {
            let req = CancelSourceTileRequest::new(reason);
            let resp = cancel_source_tile(&api, &token, id, &req).await?;
            println!(
                "Cancelled. command_id={} result={} revision={:?}",
                resp.command_id, resp.result, resp.revision
            );
            Ok(ExitCode::SUCCESS)
        }
    }
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
