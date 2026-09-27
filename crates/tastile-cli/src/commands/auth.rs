//! `tastile auth login | status | logout`.
//!
//! Implements the browser-mediated authorization grant protocol described in
//! `tastile_auth::server_bridge`. The CLI does not touch the Better Auth
//! session cookie at any point: the web authorization route uses that cookie
//! to authenticate the user, mints a one-time grant bound to
//! (code_challenge, redirect_uri, user, expiration), and the CLI exchanges
//! the grant at `POST {web_url}/api/cli/token` with no cookie attached.

use std::process::ExitCode;

use anyhow::{Context, Result, bail};
use chrono::Utc;
use tastile_api::{ApiClient, ApiConfig, BearerToken};
use tastile_auth::{
    AuthorizationCode, CallbackListener, CredentialStore, KeyringStore, PkceState,
    TokenExchangeResponse, build_authorization_url, open_browser,
};
use tastile_config::{Config, with_env_overrides};
use tracing::{info, warn};
use url::Url;

use crate::cli::{AuthArgs, AuthCommand};

const SCOPE: &str = "tastile.read tastile.write";

pub async fn run(cfg: Config, args: AuthArgs) -> Result<ExitCode> {
    match args.sub {
        AuthCommand::Login {
            print_url,
            client_id,
        } => login(cfg, print_url, client_id).await,
        AuthCommand::Status => status(cfg).await,
        AuthCommand::Logout => logout(cfg).await,
    }
}

async fn login(cfg: Config, print_url: bool, client_id: Option<String>) -> Result<ExitCode> {
    let cfg = with_env_overrides(cfg);
    let client_id = client_id.unwrap_or(cfg.oauth_client_id.clone());
    let web_base = Url::parse(&cfg.web_url).context("invalid web_url in config")?;

    let listener = CallbackListener::bind()
        .await
        .context("could not bind loopback listener")?;
    let redirect_uri = listener.redirect_uri();
    info!(%redirect_uri, "loopback listener ready");

    let pkce = PkceState::generate();
    let pair = pkce.pair();
    let auth_url = build_authorization_url(&web_base, &client_id, &redirect_uri, &pair, SCOPE);

    println!("Opening browser...");
    if print_url {
        println!("{auth_url}");
    } else {
        open_browser(&auth_url).context("could not open browser")?;
    }
    println!("If the browser does not open, visit:\n  {auth_url}");

    println!("Waiting for browser callback (timeout 120s)...");
    let outcome = listener.serve().await.context("callback failed")?;
    let (code, returned_state) = match outcome {
        tastile_auth::CallbackOutcome::Authorized { code, state } => (code, state),
        tastile_auth::CallbackOutcome::Denied { reason } => {
            bail!("authorization denied: {reason}");
        }
        tastile_auth::CallbackOutcome::Error(e) => {
            bail!("callback error: {e}");
        }
    };

    if !pkce.state_matches(&returned_state) {
        bail!("state mismatch — refusing to exchange (possible CSRF)");
    }
    println!("✓ Browser authorization captured (one-time grant received).");

    // Exchange the one-time grant for a bearer token. The exchange endpoint
    // does not require the Better Auth cookie: the (grant, code_verifier,
    // redirect_uri) tuple is sufficient proof of authorization.
    let bridge = tastile_auth::HttpServerBridge::new();
    let TokenExchangeResponse {
        token,
        expires_at,
        subject,
    } = bridge
        .fetch_token(
            &web_base,
            &AuthorizationCode::new(code),
            pkce.verifier(),
            &redirect_uri,
        )
        .await
        .context("token exchange failed")?;

    let stored = tastile_auth::StoredToken::new(
        cfg.api_url.clone(),
        token,
        parse_expires_at(expires_at.as_deref()),
        subject,
    );
    KeyringStore
        .save(
            tastile_auth::DEFAULT_SERVICE,
            tastile_auth::DEFAULT_USER,
            &stored,
        )
        .context("could not save credential")?;
    println!("✓ Bearer token saved to credential store.");
    Ok(ExitCode::SUCCESS)
}

async fn status(cfg: Config) -> Result<ExitCode> {
    let cfg = with_env_overrides(cfg);
    let store = KeyringStore;

    let loaded = store
        .load(tastile_auth::DEFAULT_SERVICE, tastile_auth::DEFAULT_USER)
        .context("credential store error")?;

    match loaded {
        None => {
            println!("Not signed in.");
            println!("Run `tastile auth login` to start the browser flow.");
            Ok(ExitCode::SUCCESS)
        }
        Some(token) => {
            println!("Signed in.");
            println!("  API base:  {}", token.api_base_url);
            if let Some(sub) = token.subject {
                println!("  Subject:   {sub}");
            }
            if let Some(exp) = token.expires_at {
                println!("  Expires:   {exp}");
                if exp < Utc::now() {
                    println!("  (token is EXPIRED — re-run `tastile auth login`)");
                }
            }
            if token.api_base_url != cfg.api_url {
                println!(
                    "  Note: stored API base does not match current config (`{}`)",
                    cfg.api_url
                );
            }
            // Reachability probe against /v1/tiles (read-only, idempotent).
            let api = ApiClient::new(ApiConfig::new(&cfg.api_url)?)?;
            let bearer = BearerToken::new(token.bearer.clone());
            match tastile_api::list_tiles(
                &api,
                &bearer,
                &tastile_api::tiles::ListTilesQuery::default(),
            )
            .await
            {
                Ok(_) => println!("  Reachable:  yes"),
                Err(tastile_api::ApiError::Http { status, .. })
                    if status == 401 || status == 403 =>
                {
                    println!("  Reachable:  yes (token rejected — re-login required)");
                }
                Err(e) => println!("  Reachable:  error: {e}"),
            }
            Ok(ExitCode::SUCCESS)
        }
    }
}

async fn logout(cfg: Config) -> Result<ExitCode> {
    let cfg = with_env_overrides(cfg);
    let store = KeyringStore;

    let loaded = store
        .load(tastile_auth::DEFAULT_SERVICE, tastile_auth::DEFAULT_USER)
        .context("credential store error")?;
    if let Some(token) = loaded {
        let api = ApiClient::new(ApiConfig::new(&cfg.api_url)?)?;
        let bearer = BearerToken::new(token.bearer.clone());
        match tastile_api::auth::signout(&api, &bearer).await {
            Ok(()) => println!("✓ Server credential revoked."),
            Err(e) => warn!("could not revoke server-side: {e}"),
        }
    }
    store
        .delete(tastile_auth::DEFAULT_SERVICE, tastile_auth::DEFAULT_USER)
        .context("credential store delete failed")?;
    println!("✓ Local credential removed.");
    Ok(ExitCode::SUCCESS)
}

fn parse_expires_at(raw: Option<&str>) -> Option<chrono::DateTime<chrono::Utc>> {
    let raw = raw?;
    chrono::DateTime::parse_from_rfc3339(raw)
        .ok()
        .map(|d| d.with_timezone(&chrono::Utc))
}
