//! `tastile prompts` — list / resolve prompts.

use std::process::ExitCode;

use anyhow::{Context, Result};
use tastile_api::{
    ApiClient, ApiConfig, BearerToken, ResolvePromptRequest, list_pending_prompts, resolve_prompt,
};
use tastile_auth::{CredentialStore, KeyringStore};
use tastile_config::with_env_overrides;

use crate::cli::{PromptsArgs, PromptsCommand};

pub async fn run(cfg: tastile_config::Config, args: PromptsArgs) -> Result<ExitCode> {
    let cfg = with_env_overrides(cfg);
    let token = load_token(&cfg).await?;
    let api = ApiClient::new(ApiConfig::new(&cfg.api_url)?)?;

    match args.sub {
        PromptsCommand::List { json } => {
            let prompts = list_pending_prompts(&api, &token).await?;
            if prompts.is_empty() {
                println!("(no pending prompts)");
                return Ok(ExitCode::SUCCESS);
            }
            if json {
                println!("{}", serde_json::to_string_pretty(&prompts)?);
            } else {
                for p in prompts {
                    println!(
                        "{} kind={} created={} note={}",
                        p.id,
                        p.kind,
                        p.created_at.format("%Y-%m-%d %H:%M"),
                        p.note.unwrap_or_else(|| "-".into())
                    );
                }
            }
            Ok(ExitCode::SUCCESS)
        }
        PromptsCommand::Resolve {
            id,
            answer_kind,
            note,
        } => {
            let mut req = ResolvePromptRequest::new(answer_kind);
            req.payload.note = note;
            let resp = resolve_prompt(&api, &token, id, &req).await?;
            println!(
                "Resolved. command_id={} result={} revision={:?}",
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
