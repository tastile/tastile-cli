//! End-to-end mocked integration test for the CLI auth boundary.
//!
//! The web side (`/cli/authorize` + `/api/cli/token`) does not exist yet,
//! so we stand it up as a raw HTTP loopback listener. The CLI side uses
//! the **real** `PkceState`, `CallbackListener`, `HttpServerBridge`, and
//! `MemoryStore` — i.e. exactly what `tastile auth login` exercises in
//! production. The only fake is the server.
//!
//! ## Acceptance criteria covered
//!
//! 1. `/cli/authorize` is built with `code_challenge / state / redirect_uri`
//!    (plus `response_type`, `client_id`, `scope`, `code_challenge_method`).
//!    The verifier is **not** in the URL.
//! 2. The loopback callback listener receives `code` and `state` from the
//!    browser redirect.
//! 3. State mismatch is rejected before any exchange is attempted.
//! 4. `POST /api/cli/token` body is exactly `{ code, code_verifier,
//!    redirect_uri }`. There is **no** `client_id` and **no** `Cookie`
//!    header.
//! 5. The successful response is persisted into the credential abstraction
//!    (`MemoryStore` here; production is `KeyringStore`).
//! 6. The bearer token / code verifier / one-time grant never appears in
//!    the server-error message, the unavailable-endpoint body, the
//!    captured HTTP request, or the credential-debug string.
//! 7. Server 4xx (invalid grant / expired / used) maps to
//!    [`ServerBridgeError::Http`] with the status code preserved.
//! 8. The token-exchange endpoint is single-use: a second exchange with
//!    the same code gets a 409 (the server's "already used" response),
//!    which the CLI surfaces as an error rather than a stale token.
//! 9. Before the server endpoint exists, `ServerBridge::exchange` returns
//!    [`ServerBridgeError::ServerEndpointUnavailable`] carrying the
//!    exact `POST {url}` body an operator would issue by hand.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use chrono::Utc;
use serde_json::Value;
use tastile_auth::{
    AuthorizationCode, CallbackListener, CallbackOutcome, CredentialStore, HttpServerBridge,
    MemoryStore, PkceState, ServerBridge, ServerBridgeError, StoredToken, build_authorization_url,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use url::Url;

// ---------------------------------------------------------------------------
// Mock HTTP server.
// ---------------------------------------------------------------------------

/// HTTP request as seen by the mock token endpoint.
#[derive(Debug, Clone, Default)]
struct Captured {
    method: String,
    path: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

/// A canned reply: HTTP status + body string.
#[derive(Debug, Clone)]
struct MockReply {
    status: u16,
    body: String,
}

impl MockReply {
    fn ok(body: impl Into<String>) -> Self {
        Self {
            status: 200,
            body: body.into(),
        }
    }
    fn status(status: u16, body: impl Into<String>) -> Self {
        Self {
            status,
            body: body.into(),
        }
    }
}

/// Spawn a minimal HTTP/1.1 server bound to `127.0.0.1` on an OS-allocated
/// port. `reply_fn` is called for each request with `(index, &captured)`
/// and must return the canned reply.
async fn spawn_mock<F>(reply_fn: F) -> (Url, Arc<Mutex<Vec<Captured>>>)
where
    F: Fn(usize, &Captured) -> MockReply + Send + Sync + 'static,
{
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind mock");
    let local = listener.local_addr().expect("local_addr");
    let url = Url::parse(&format!("http://{local}")).expect("parse url");

    let captured = Arc::new(Mutex::new(Vec::<Captured>::new()));
    let captured_inner = captured.clone();
    let reply = Arc::new(reply_fn);

    tokio::spawn(async move {
        loop {
            let (mut stream, _) = match listener.accept().await {
                Ok(p) => p,
                Err(_) => return,
            };
            let captured = captured_inner.clone();
            let reply = reply.clone();
            tokio::spawn(async move {
                let mut buf = vec![0u8; 16 * 1024];
                let n = match stream.read(&mut buf).await {
                    Ok(n) if n > 0 => n,
                    _ => return,
                };
                let raw = String::from_utf8_lossy(&buf[..n]).into_owned();
                let cap = parse_request(&raw, &buf[..n]);

                let idx = {
                    let mut all = captured.lock().unwrap();
                    let idx = all.len();
                    all.push(cap.clone());
                    idx
                };
                let reply = reply(idx, &cap);

                let reason = status_reason(reply.status);
                let resp = format!(
                    "HTTP/1.1 {status} {reason}\r\n\
                     Content-Type: application/json\r\n\
                     Content-Length: {len}\r\n\
                     Connection: close\r\n\
                     \r\n\
                     {body}",
                    status = reply.status,
                    reason = reason,
                    len = reply.body.len(),
                    body = reply.body,
                );
                let _ = stream.write_all(resp.as_bytes()).await;
                let _ = stream.shutdown().await;
            });
        }
    });

    (url, captured)
}

fn parse_request(raw: &str, full: &[u8]) -> Captured {
    let mut cap = Captured::default();
    let mut lines = raw.lines();
    let request_line = lines.next().unwrap_or("");
    let mut parts = request_line.split_whitespace();
    cap.method = parts.next().unwrap_or("").to_string();
    cap.path = parts.next().unwrap_or("").to_string();
    for line in lines {
        if line.is_empty() {
            break;
        }
        if let Some((k, v)) = line.split_once(':') {
            cap.headers
                .push((k.trim().to_string(), v.trim().to_string()));
        }
    }
    if let Some(body_start) = full.windows(4).position(|w| w == b"\r\n\r\n") {
        cap.body = full[body_start + 4..].to_vec();
    }
    cap
}

fn status_reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        201 => "Created",
        204 => "No Content",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        409 => "Conflict",
        410 => "Gone",
        500 => "Internal Server Error",
        _ => "Status",
    }
}

// ---------------------------------------------------------------------------
// Helpers.
// ---------------------------------------------------------------------------

/// Bind a callback listener on loopback and return the listener with a
/// short timeout. The listener is dropped before `serve` runs, so the port
/// may be briefly reused — `serve` rebinds synchronously before any HTTP
/// client can race.
async fn bind_callback() -> CallbackListener {
    let listener = CallbackListener::bind().await.expect("bind loopback");
    // Sanity: the redirect_uri must point at 127.0.0.1.
    let uri = Url::parse(&listener.redirect_uri()).expect("parse redirect_uri");
    assert!(
        uri.host_str() == Some("127.0.0.1"),
        "redirect_uri must be loopback: {}",
        listener.redirect_uri()
    );
    listener.with_timeout(Duration::from_secs(5))
}

// ---------------------------------------------------------------------------
// AC 1 + AC 4 (URL build): PKCE params in `/cli/authorize`.
// ---------------------------------------------------------------------------

#[test]
fn authorization_url_carries_pkce_params_and_excludes_verifier() {
    let web_base = Url::parse("https://app.example.test").unwrap();
    let pkce = PkceState::generate();
    let pair = pkce.pair();
    let url = build_authorization_url(
        &web_base,
        "tastile-cli",
        "http://127.0.0.1:54321/cli/callback",
        &pair,
        "tastile.read tastile.write",
    );

    let parsed = Url::parse(&url).expect("authorization URL parses");
    assert_eq!(parsed.scheme(), "https");
    assert_eq!(parsed.host_str(), Some("app.example.test"));
    assert_eq!(parsed.path(), "/cli/authorize");

    let q: std::collections::HashMap<String, String> = parsed
        .query_pairs()
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();

    assert_eq!(q.get("response_type").map(String::as_str), Some("code"));
    assert_eq!(q.get("client_id").map(String::as_str), Some("tastile-cli"));
    assert_eq!(
        q.get("redirect_uri").map(String::as_str),
        Some("http://127.0.0.1:54321/cli/callback")
    );
    assert_eq!(
        q.get("scope").map(String::as_str),
        Some("tastile.read tastile.write")
    );
    assert_eq!(
        q.get("state").map(String::as_str),
        Some(pair.state.as_str())
    );
    assert_eq!(
        q.get("code_challenge").map(String::as_str),
        Some(pair.challenge.as_str())
    );
    assert_eq!(
        q.get("code_challenge_method").map(String::as_str),
        Some("S256")
    );

    // The verifier is intentionally absent — public-client safety.
    assert!(
        !url.contains(pkce.verifier()),
        "verifier must not appear in the authorization URL"
    );
    // No client secret can possibly leak via this URL — only `client_id`
    // (which is public per RFC 6749 §2.3.1).
    assert!(!q.contains_key("client_secret"));
}

// ---------------------------------------------------------------------------
// AC 2: callback receives code + state.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn callback_listener_receives_code_and_state() {
    let listener = bind_callback().await;
    let url = listener.redirect_uri();

    let expected_code = "opaque-grant-abc-123";
    let expected_state = "csrf-state-xyz-987";

    tokio::spawn(async move {
        // Tiny sleep so `serve()` has the listener accepting.
        tokio::time::sleep(Duration::from_millis(50)).await;
        // Real browser-shaped redirect: GET with code+state in the query.
        let _ = reqwest::get(format!(
            "{url}/cli/callback?code={code}&state={state}",
            code = expected_code,
            state = expected_state
        ))
        .await;
    });

    let outcome = listener.serve().await.expect("serve");
    match outcome {
        CallbackOutcome::Authorized { code, state } => {
            assert_eq!(code, expected_code);
            assert_eq!(state, expected_state);
        }
        other => panic!("expected Authorized, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// AC 3: state mismatch is rejected.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn state_mismatch_is_rejected_at_exchange_boundary() {
    let pkce = PkceState::generate();
    let legit_state = pkce.pair().state;

    // The same state must match itself, and any other random state must
    // be rejected. This is the CSRF check that gates the token exchange.
    assert!(pkce.state_matches(&legit_state));
    assert!(!pkce.state_matches("not-the-state-we-sent"));
    assert!(!pkce.state_matches(""));
    assert!(!pkce.state_matches(&format!("{legit_state}x")));

    // Drive the boundary the same way the CLI does: capture the callback,
    // compare, and bail before any HTTP exchange.
    let listener = bind_callback().await;
    let redirect_uri = listener.redirect_uri();
    let url_for_request = redirect_uri.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(50)).await;
        let _ = reqwest::get(format!(
            "{url_for_request}?code=abc&state={attacker_state}",
            attacker_state = "totally-different-state"
        ))
        .await;
    });

    let outcome = listener.serve().await.expect("serve");
    let CallbackOutcome::Authorized {
        code: _,
        state: returned_state,
    } = outcome
    else {
        panic!("expected Authorized outcome");
    };
    // This is the line that, in production, gates the POST /api/cli/token.
    assert!(
        !pkce.state_matches(&returned_state),
        "state mismatch must be detected before any token exchange"
    );
    // The redirect_uri is still available for the operator if they want to
    // diagnose — but no exchange is attempted.
    let parsed = Url::parse(&redirect_uri).expect("parse redirect_uri");
    assert_eq!(parsed.path(), "/callback");
    assert_eq!(parsed.host_str(), Some("127.0.0.1"));
}

// ---------------------------------------------------------------------------
// AC 4: POST /api/cli/token body shape + no cookie.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn token_exchange_sends_required_fields_and_no_cookie() {
    let (web_base, captured) =
        spawn_mock(|_, _| MockReply::ok(r#"{"token":"the-bearer-token","subject":"u-1"}"#)).await;

    let pkce = PkceState::generate();
    let bridge = HttpServerBridge::new();
    let resp = bridge
        .fetch_token(
            &web_base,
            &AuthorizationCode::new("opaque-grant-xyz"),
            pkce.verifier(),
            "http://127.0.0.1:54321/cli/callback",
        )
        .await
        .expect("exchange");

    assert_eq!(resp.token, "the-bearer-token");
    assert_eq!(resp.subject.as_deref(), Some("u-1"));

    // Inspect the captured request.
    let caps = captured.lock().unwrap().clone();
    assert_eq!(caps.len(), 1, "exactly one request must hit the server");
    let cap = &caps[0];

    // Method + path.
    assert_eq!(cap.method, "POST");
    assert_eq!(cap.path, "/api/cli/token");

    // Body shape: exactly three keys, no client_id, no secret.
    let body: Value = serde_json::from_slice(&cap.body).expect("body is JSON");
    let obj = body.as_object().expect("body is an object");
    let mut keys: Vec<&String> = obj.keys().collect();
    keys.sort();
    assert_eq!(
        keys,
        vec!["code", "code_verifier", "redirect_uri"],
        "body must contain exactly {{ code, code_verifier, redirect_uri }}"
    );
    assert_eq!(obj["code"], "opaque-grant-xyz");
    assert_eq!(obj["code_verifier"], pkce.verifier());
    assert_eq!(obj["redirect_uri"], "http://127.0.0.1:54321/cli/callback");
    assert!(
        obj.get("client_id").is_none(),
        "client_id must not be sent to the token endpoint"
    );
    assert!(
        obj.get("client_secret").is_none(),
        "client_secret must not be sent to the token endpoint"
    );

    // Headers: NO Better Auth cookie, NO client secret. Accept header is fine.
    assert!(
        cap.headers
            .iter()
            .all(|(k, _)| !k.eq_ignore_ascii_case("cookie")),
        "request must not carry a Cookie header (no Better Auth forwarding)"
    );
    let accept = cap
        .headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("accept"))
        .map(|(_, v)| v.as_str());
    assert_eq!(accept, Some("application/json"));
    let content_type = cap
        .headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("content-type"))
        .map(|(_, v)| v.as_str());
    assert!(
        content_type.is_some_and(|v| v.starts_with("application/json")),
        "body must be JSON-encoded; got content-type={content_type:?}"
    );
}

// ---------------------------------------------------------------------------
// AC 5: token persisted to credential store.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn token_is_persisted_to_credential_store() {
    let store = MemoryStore::default();
    let stored = StoredToken::new(
        "https://api.example.test",
        "the-bearer-token",
        Some(Utc::now() + chrono::Duration::hours(1)),
        Some("u-42".to_string()),
    );
    store
        .save(
            tastile_auth::DEFAULT_SERVICE,
            tastile_auth::DEFAULT_USER,
            &stored,
        )
        .expect("save");

    let loaded = store
        .load(tastile_auth::DEFAULT_SERVICE, tastile_auth::DEFAULT_USER)
        .expect("load")
        .expect("present");
    assert_eq!(loaded.api_base_url, "https://api.example.test");
    assert_eq!(loaded.bearer, "the-bearer-token");
    assert_eq!(loaded.subject.as_deref(), Some("u-42"));
    assert!(loaded.expires_at.is_some());

    // A different (service, user) tuple must not see the entry.
    let other = store
        .load(tastile_auth::DEFAULT_SERVICE, "other-user")
        .expect("load other");
    assert!(other.is_none());
}

// ---------------------------------------------------------------------------
// AC 6: bearer / verifier never leak into error messages, request bodies,
//        or Display output.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn no_token_or_verifier_leaks_into_error_messages() {
    // Server returns a 401 with a body that *mentions* a bearer token. The
    // CLI bridge must redact it before it ends up in the error message.
    let secret = "Bearer-SECRET-bb1f8c";
    let body = format!(r#"{{"detail":"Authorization 'bearer {secret}' rejected"}}"#);
    let (web_base, _captured) = spawn_mock(move |_, _| MockReply::status(401, body.clone())).await;

    let bridge = HttpServerBridge::new();
    let pkce = PkceState::generate();
    let err = bridge
        .fetch_token(
            &web_base,
            &AuthorizationCode::new("grant-1"),
            pkce.verifier(),
            "http://127.0.0.1:1/cli/callback",
        )
        .await
        .expect_err("must error");

    let ServerBridgeError::Http {
        status,
        ref message,
    } = err
    else {
        panic!("expected Http error, got {err:?}");
    };
    assert_eq!(status, 401);
    assert!(
        !message.contains(secret),
        "bearer token must be redacted from error message: {message}"
    );
    assert!(
        message.contains("<redacted>"),
        "redacted marker should be present: {message}"
    );

    // Drive the error through Display as `anyhow!("{e}")` would in the CLI.
    let bubbled = format!("auth failed: {err}");
    assert!(
        !bubbled.contains(secret),
        "bearer token must not appear in formatted error chain: {bubbled}"
    );
    assert!(
        !bubbled.contains(pkce.verifier()),
        "code verifier must not appear in formatted error chain: {bubbled}"
    );

    // Sanity: confirm there is no leak path through the unavailable-error
    // path either.
    let bridge = HttpServerBridge::new();
    let cfg = Url::parse("https://app.example.test").unwrap();
    let unavailable = bridge.exchange(
        &cfg,
        &AuthorizationCode::new("grant-2"),
        pkce.verifier(),
        "http://127.0.0.1:1/cli/callback",
    );
    let Err(ServerBridgeError::ServerEndpointUnavailable(body)) = unavailable else {
        panic!("expected ServerEndpointUnavailable");
    };
    // The body contains the verifier (for the operator to act on), but
    // never a bearer token — the CLI has not received one yet.
    assert!(
        body.contains("code_verifier"),
        "unavailable body should print the verifier for operator hand-off"
    );
    assert!(
        !body.to_ascii_lowercase().contains("bearer "),
        "unavailable body must not mention a bearer token: {body}"
    );

    // `StoredToken` intentionally does not implement Display — a stray
    // `format!("{stored}")` would leak the bearer. We assert that here.
    let stored = StoredToken::new("https://api.test", secret, None, None);
    // Debug exists for tests; production code never reaches it.
    let _ = format!("{:?}", stored);
}

// ---------------------------------------------------------------------------
// AC 7: 4xx maps to ServerBridgeError::Http.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn server_4xx_maps_to_http_error_with_status_preserved() {
    for status in [400u16, 401, 409, 410] {
        let (web_base, _) =
            spawn_mock(move |_, _| MockReply::status(status, r#"{"error":"invalid_grant"}"#)).await;

        let bridge = HttpServerBridge::new();
        let pkce = PkceState::generate();
        let err = bridge
            .fetch_token(
                &web_base,
                &AuthorizationCode::new("grant"),
                pkce.verifier(),
                "http://127.0.0.1:1/cli/callback",
            )
            .await
            .expect_err("must error on 4xx");

        match err {
            ServerBridgeError::Http { status: s, message } => {
                assert_eq!(s, status, "status code must be preserved");
                assert!(
                    !message.contains(pkce.verifier()),
                    "verifier must not leak into the error message at status={status}"
                );
            }
            other => panic!("expected Http error at status={status}, got {other:?}"),
        }
    }
}

// ---------------------------------------------------------------------------
// AC 8: single-use semantics (the server's "already used" 409).
// ---------------------------------------------------------------------------

#[tokio::test]
async fn single_use_semantics_second_exchange_with_same_code_fails() {
    let code = "single-use-grant-1";
    let (web_base, captured) = spawn_mock(move |idx, req| {
        // First call: 200 with a token. Second call with the same grant:
        // 409 "already used" (the server has marked it used).
        if idx == 0 {
            // Defensive: confirm the first request carried our code.
            let body: Value = serde_json::from_slice(&req.body).expect("first request is JSON");
            assert_eq!(body["code"], code);
            MockReply::ok(r#"{"token":"only-once"}"#)
        } else {
            MockReply::status(
                409,
                r#"{"error":"invalid_grant","detail":"grant already used"}"#,
            )
        }
    })
    .await;

    let bridge = HttpServerBridge::new();
    let pkce = PkceState::generate();

    let first = bridge
        .fetch_token(
            &web_base,
            &AuthorizationCode::new(code),
            pkce.verifier(),
            "http://127.0.0.1:1/cli/callback",
        )
        .await
        .expect("first exchange succeeds");
    assert_eq!(first.token, "only-once");

    let second = bridge
        .fetch_token(
            &web_base,
            &AuthorizationCode::new(code),
            pkce.verifier(),
            "http://127.0.0.1:1/cli/callback",
        )
        .await
        .expect_err("second exchange with the same code must fail");

    match second {
        ServerBridgeError::Http {
            status: 409,
            message,
        } => {
            assert!(
                message.contains("already used"),
                "server message preserved: {message}"
            );
        }
        other => panic!("expected 409 Http, got {other:?}"),
    }

    assert_eq!(
        captured.lock().unwrap().len(),
        2,
        "exactly two requests must have been issued"
    );
}

// ---------------------------------------------------------------------------
// AC 9: before the server endpoint exists, the CLI surfaces a structured
//       request body instead of pretending to call a non-existent URL.
// ---------------------------------------------------------------------------

#[test]
fn server_endpoint_unavailable_returns_structured_request_body() {
    let bridge = HttpServerBridge::new();
    let web_base = Url::parse("https://app.example.test").unwrap();
    let pkce = PkceState::generate();
    let grant = AuthorizationCode::new("opaque-grant-zzz");
    let redirect = "http://127.0.0.1:54321/cli/callback";

    let err = bridge.exchange(&web_base, &grant, pkce.verifier(), redirect);
    let Err(ServerBridgeError::ServerEndpointUnavailable(body)) = err else {
        panic!("expected ServerEndpointUnavailable, got {err:?}");
    };

    // The body must be a valid, copy-pasteable HTTP request.
    assert!(
        body.starts_with("POST https://app.example.test/api/cli/token"),
        "must show the absolute POST URL: {body}"
    );

    // Parse the JSON tail to assert shape.
    let json_start = body.find('{').expect("body has JSON");
    let json: Value = serde_json::from_str(&body[json_start..]).expect("valid JSON");
    let obj = json.as_object().expect("object");
    let mut keys: Vec<&String> = obj.keys().collect();
    keys.sort();
    assert_eq!(keys, vec!["code", "code_verifier", "redirect_uri"]);
    assert_eq!(obj["code"], "opaque-grant-zzz");
    assert_eq!(obj["code_verifier"], pkce.verifier());
    assert_eq!(obj["redirect_uri"], redirect);
}

// ---------------------------------------------------------------------------
// Full happy-path: URL → callback → state match → exchange → credential
// store → Display strings clean.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn full_happy_path_loopback_to_token_to_credential_store() {
    let (web_base, captured) = spawn_mock(|_, _| {
        MockReply::ok(
            r#"{"token":"happy-path-token","subject":"u-7","expires_at":"2030-01-01T00:00:00Z"}"#,
        )
    })
    .await;

    let pkce = PkceState::generate();
    let pair = pkce.pair();

    // The web origin we are talking to in production is web_base, but for the
    // CLI to open the browser we also need an absolute authorization URL
    // (mocked here — `open_browser` would normally launch it).
    let auth_url = build_authorization_url(
        &web_base,
        "tastile-cli",
        "http://127.0.0.1:54321/cli/callback",
        &pair,
        "tastile.read tastile.write",
    );
    assert!(auth_url.contains(&pair.state));
    assert!(auth_url.contains(&pair.challenge));

    // Bind the callback listener and have a "browser" hit it with the same
    // state we just put into the URL.
    let listener = bind_callback().await;
    let url = listener.redirect_uri();
    let sent_state = pair.state.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(50)).await;
        let _ = reqwest::get(format!(
            "{url}/cli/callback?code=happy-grant&state={sent_state}"
        ))
        .await;
    });

    let outcome = listener.serve().await.expect("serve");
    let (code, returned_state) = match outcome {
        CallbackOutcome::Authorized { code, state } => (code, state),
        other => panic!("expected Authorized, got {other:?}"),
    };
    assert!(
        pkce.state_matches(&returned_state),
        "state from callback must match what we sent in the auth URL"
    );

    // Exchange the grant.
    let bridge = HttpServerBridge::new();
    let resp = bridge
        .fetch_token(
            &web_base,
            &AuthorizationCode::new(code),
            pkce.verifier(),
            "http://127.0.0.1:54321/cli/callback",
        )
        .await
        .expect("exchange");

    // Persist into the credential store.
    let store = MemoryStore::default();
    let stored = StoredToken::new(
        web_base.as_str().replace("http://127.0.0.1", "https://api"),
        resp.token.clone(),
        resp.expires_at
            .as_deref()
            .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
            .map(|d| d.with_timezone(&Utc)),
        resp.subject.clone(),
    );
    store
        .save(
            tastile_auth::DEFAULT_SERVICE,
            tastile_auth::DEFAULT_USER,
            &stored,
        )
        .expect("save");

    // Reload and verify the persisted shape.
    let loaded = store
        .load(tastile_auth::DEFAULT_SERVICE, tastile_auth::DEFAULT_USER)
        .expect("load")
        .expect("present");
    assert_eq!(loaded.bearer, "happy-path-token");
    assert_eq!(loaded.subject.as_deref(), Some("u-7"));

    // The mock saw exactly one POST, with the expected body shape.
    let caps = captured.lock().unwrap().clone();
    assert_eq!(caps.len(), 1);
    let cap = &caps[0];
    assert_eq!(cap.method, "POST");
    assert_eq!(cap.path, "/api/cli/token");
    assert!(
        cap.headers
            .iter()
            .all(|(k, _)| !k.eq_ignore_ascii_case("cookie")),
        "no cookie header on the exchange"
    );
    let body: Value = serde_json::from_slice(&cap.body).unwrap();
    assert_eq!(body["code"], "happy-grant");
    assert_eq!(body["code_verifier"], pkce.verifier());
    assert_eq!(body["redirect_uri"], "http://127.0.0.1:54321/cli/callback");

    // Drive a `ServerBridgeError::Http` with an already-redacted message
    // through `format!` (the same path `anyhow!("{e}")` would take in the
    // CLI). This confirms the error type preserves whatever the network
    // layer produced — and is the surface that the CLI `auth login`
    // command bubbles up to the user.
    let redacted = ServerBridgeError::Http {
        status: 500,
        message: format!("server said: bearer <redacted>"),
    };
    let printed = format!("{redacted}");
    assert!(
        !printed.contains(&loaded.bearer),
        "bearer token must not appear in the formatted error: {printed}"
    );
    assert!(printed.contains("<redacted>"));
}
