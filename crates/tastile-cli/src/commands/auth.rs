//! `tastile auth login | status | logout | exchange`.
//!
//! The login flow is structured but does not yet complete: the web side has
//! not exposed a public `/api/cli/api-token` endpoint, so the bridge stops
//! at "code captured from the browser". When the endpoint exists, drop in
//! the real exchange in `tastile_auth::server_bridge::HttpServerBridge` and
//! remove the explicit "server endpoint unavailable" message from `login`.

use std::process::ExitCode;

use anyhow::{Context, Result, anyhow, bail};
use chrono::Utc;
use tastile_api::{ApiClient, ApiConfig, BearerToken};
use tastile_auth::{
    AuthorizationCode, CallbackListener, CredentialStore, KeyringStore, PkceState, ServerBridge,
    open_browser,
};
use tastile_config::{Config, with_env_overrides};
use tracing::{info, warn};
use url::Url;

use crate::cli::{AuthArgs, AuthCommand};

const REDIRECT_PATH: &str = "/cli/callback";
const SCOPE: &str = "tastile.read tastile.write";

pub async fn run(cfg: Config, args: AuthArgs) -> Result<ExitCode> {
    match args.sub {
        AuthCommand::Login {
            print_url,
            client_id,
        } => login(cfg, print_url, client_id).await,
        AuthCommand::Status => status(cfg).await,
        AuthCommand::Logout => logout(cfg).await,
        AuthCommand::Exchange { code, state } => exchange(cfg, code, state).await,
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
    let auth_url = build_auth_url(&web_base, &client_id, &redirect_uri, &pair, SCOPE);

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
    println!("✓ Signed in (authorization code captured).");

    // The server-side token exchange endpoint is not yet exposed. We print
    // the structured request so the operator can complete the exchange by
    // hand. When the endpoint lands, drop in the `HttpServerBridge` here and
    // save the token into the credential store.
    let bridge = tastile_auth::HttpServerBridge::new();
    match bridge.exchange(
        &web_base,
        &client_id,
        &AuthorizationCode::new(code.clone()),
        pkce.verifier(),
        &redirect_uri,
    ) {
        Ok(_token) => {
            // Real exchange path: persist the bearer token and exit.
            // (Not reachable until the endpoint exists.)
            unreachable!(
                "server endpoint returned a token but the placeholder does not produce one"
            )
        }
        Err(tastile_auth::ServerBridgeError::ServerEndpointUnavailable(request)) => {
            println!("✓ Tastile CLI authorized (browser side).");
            println!();
            println!("The server-side token exchange endpoint is not yet exposed.");
            println!("Captured authorization code: <redacted>");
            println!("To complete the exchange by hand, run:");
            println!();
            println!("  {request}");
            println!();
            println!("After exchanging the code, the resulting bearer token is");
            println!("expected to be saved into the OS credential store under");
            println!("service=`{}`.", tastile_auth::DEFAULT_SERVICE);
            Ok(ExitCode::SUCCESS)
        }
        Err(e) => Err(anyhow!(e)),
    }
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
            // Belt-and-braces: print the API base from config so the user can
            // spot a mismatch.
            if token.api_base_url != cfg.api_url {
                println!(
                    "  Note: stored API base does not match current config (`{}`)",
                    cfg.api_url
                );
            }
            // Reachability probe against /v1/auth/signout using the stored
            // token — `signout` is idempotent, so a 204 means the token is
            // valid. We swallow the side-effect by re-saving immediately
            // after, but we DO mutate server state. To avoid that, just
            // attempt a GET on `/v1/tiles` instead.
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
        // Best-effort server-side revoke. If the call fails we still drop
        // the local copy — Better Auth-style revocation is asynchronous on
        // the server and re-trying would just amplify the error.
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

async fn exchange(cfg: Config, code: String, _state: String) -> Result<ExitCode> {
    // Operator escape hatch: re-run the bridge with a captured (code, state).
    // Useful when the server endpoint is being debugged.
    let cfg = with_env_overrides(cfg);
    let web_base = Url::parse(&cfg.web_url).context("invalid web_url in config")?;
    let bridge = tastile_auth::HttpServerBridge::new();
    match bridge.exchange(
        &web_base,
        &cfg.oauth_client_id,
        &AuthorizationCode::new(code),
        // We don't have the verifier after the fact — pass empty. The real
        // bridge endpoint, when it exists, will reject this with a 400.
        "",
        &format!("{REDIRECT_PATH}"),
    ) {
        Ok(token) => {
            let stored = tastile_auth::StoredToken::new(cfg.api_url.clone(), token, None, None);
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
        Err(e) => Err(anyhow!(e)),
    }
}

fn build_auth_url(
    web_base: &Url,
    client_id: &str,
    redirect_uri: &str,
    pair: &tastile_auth::PkcePair,
    scope: &str,
) -> String {
    // Hand-build to keep the URL parameter order stable and obvious.
    use std::fmt::Write as _;
    let mut s = String::with_capacity(256);
    let base_path = web_base
        .join("/cli/authorize")
        .unwrap_or_else(|_| web_base.clone());
    write!(
        &mut s,
        "{}?response_type=code&client_id={}&redirect_uri={}&scope={}&state={}&code_challenge={}&code_challenge_method=S256",
        base_path.as_str(),
        urlencoding::encode(client_id),
        urlencoding::encode(redirect_uri),
        urlencoding::encode(scope),
        urlencoding::encode(&pair.state),
        urlencoding::encode(&pair.challenge),
    )
    .expect("writing to String never fails");
    s
}
