//! Authorization-code → bearer-token exchange.
//!
//! # Browser-mediated authorization grant protocol
//!
//! The CLI does not hold, copy, or forward the Better Auth session cookie.
//! The auth flow is split between the web origin (which owns the Better
//! Auth session and the one-time grant store) and the CLI:
//!
//! ```text
//! CLI
//!   generates:
//!     code_verifier        (RFC 7636, 32 random bytes, base64url)
//!     code_challenge       (SHA256(code_verifier), base64url)
//!     state                (16 random bytes, base64url)
//!     loopback redirect_uri
//!
//!        ↓ opens browser
//!
//! Web: GET {web_url}/cli/authorize?
//!       response_type=code
//!       &client_id={client_id}
//!       &redirect_uri={redirect_uri}
//!       &scope={scope}
//!       &state={state}
//!       &code_challenge={code_challenge}
//!       &code_challenge_method=S256
//!
//! Web authorization route (Better Auth session cookie is the only auth):
//!   - confirms user is signed in (Better Auth session)
//!   - asks for user approval
//!   - mints a one-time authorization grant (opaque code) bound to:
//!       * user_id
//!       * code_challenge
//!       * redirect_uri
//!       * expiration
//!       * used = false
//!   - redirects to:
//!       {redirect_uri}?code={opaque_grant}&state={state}
//!
//! CLI
//!   - receives the callback
//!   - verifies state matches (constant time)
//!   - POSTs to {web_url}/api/cli/token
//!       (NO Better Auth cookie in the request)
//!       body: { code, code_verifier, redirect_uri }
//!
//! Web /api/cli/token:
//!   - looks up the one-time grant
//!   - verifies expiration, single-use, code_challenge (PKCE),
//!     redirect_uri
//!   - mints a Tastile API bearer token (no Better Auth involvement)
//!   - returns { token, expires_at, subject }
//!   - marks the grant as used
//!
//! CLI
//!   - stores the bearer token in the OS credential store
//!     (service: tastile-cli, user: default)
//! ```
//!
//! # Status (2026-09-27)
//!
//! The web side does not yet expose the `/cli/authorize` route or the
//! `/api/cli/token` endpoint. Until both endpoints land,
//! `ServerBridge::exchange` returns
//! [`ServerBridgeError::ServerEndpointUnavailable`] with the exact request
//! that would be issued. This keeps the CLI side structurally correct
//! without pretending to talk to endpoints that do not exist.
//!
//! When the server-side routes ship, replace the body of `exchange` and
//! `HttpServerBridge::fetch_token` with the real HTTP call.

use std::time::Duration;

use serde::{Deserialize, Serialize};
use thiserror::Error;
use url::Url;

/// One-time authorization grant returned by the browser callback.
///
/// The grant is opaque to the CLI — it is whatever the web authorization
/// route placed after `?code=…` in the redirect. We never log or echo it.
#[derive(Debug, Clone)]
pub struct AuthorizationCode(pub String);

impl AuthorizationCode {
    pub fn new(code: impl Into<String>) -> Self {
        Self(code.into())
    }

    /// Test-only constructor that returns the grant as a redacted summary.
    /// Production code never constructs this for log output.
    pub fn redacted_summary(&self) -> String {
        let n = self.0.chars().count();
        if n <= 4 {
            format!("<grant: {} chars>", n)
        } else {
            format!("<grant: {} chars, prefix={}…>", n, &self.0[..2])
        }
    }
}

/// Request body for `POST {web_url}/api/cli/token`.
///
/// The endpoint **must not** require the Better Auth cookie; the one-time
/// grant + PKCE + `redirect_uri` are sufficient proof of authorization.
#[derive(Debug, Clone, Serialize)]
pub struct TokenExchangeRequest<'a> {
    pub code: &'a str,
    pub code_verifier: &'a str,
    pub redirect_uri: &'a str,
}

/// Successful response from `POST /api/cli/token`.
///
/// The bearer token is opaque to the CLI; we never log or print it.
#[derive(Debug, Clone, Deserialize)]
pub struct TokenExchangeResponse {
    pub token: String,
    /// ISO-8601 timestamp. Optional because not every server includes it.
    #[serde(default)]
    pub expires_at: Option<String>,
    /// Optional subject identifier (e.g. user id) for display only.
    #[serde(default)]
    pub subject: Option<String>,
}

/// Errors that the CLI bridge can surface.
#[derive(Debug, Error)]
pub enum ServerBridgeError {
    /// The web side has not yet exposed `/api/cli/token`. The CLI prints the
    /// structured request so an operator can drive the exchange by hand.
    #[error(
        "server endpoint `/api/cli/token` is not yet available; \
         manually exchange the captured grant:\n{0}"
    )]
    ServerEndpointUnavailable(String),

    #[error("transport: {0}")]
    Transport(String),

    #[error("server returned {status}: {message}")]
    Http { status: u16, message: String },

    #[error("malformed server response: {0}")]
    Decode(String),
}

/// Trait abstracting the bridge so tests can swap a fake implementation.
pub trait ServerBridge: Send + Sync {
    /// Exchange `(code, verifier, redirect_uri)` for a Tastile API bearer
    /// token.
    fn exchange(
        &self,
        web_base: &Url,
        code: &AuthorizationCode,
        verifier: &str,
        redirect_uri: &str,
    ) -> Result<TokenExchangeResponse, ServerBridgeError>;
}

/// Production implementation. Until the server endpoint exists, returns
/// `ServerEndpointUnavailable` with the exact request body that *would* be
/// sent. Once the endpoint is exposed, `fetch_token` issues the real POST.
#[derive(Debug, Clone)]
pub struct HttpServerBridge {
    pub client: reqwest::Client,
}

impl Default for HttpServerBridge {
    fn default() -> Self {
        Self::new()
    }
}

impl HttpServerBridge {
    pub fn new() -> Self {
        Self {
            // The exchange endpoint is unauthenticated (no Better Auth cookie);
            // reqwest's default is to not persist / forward cookies, which is
            // exactly what we want here.
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(15))
                .build()
                .expect("reqwest client"),
        }
    }

    /// Build the absolute URL of the token endpoint.
    pub fn token_endpoint(web_base: &Url) -> Result<Url, ServerBridgeError> {
        web_base
            .join("/api/cli/token")
            .map_err(|e| ServerBridgeError::Transport(e.to_string()))
    }

    /// Issue the real `POST /api/cli/token`. Body / response code / error
    /// mapping are kept side-effect-free so a future endpoint flip only
    /// changes this method.
    pub async fn fetch_token(
        &self,
        web_base: &Url,
        code: &AuthorizationCode,
        verifier: &str,
        redirect_uri: &str,
    ) -> Result<TokenExchangeResponse, ServerBridgeError> {
        let url = Self::token_endpoint(web_base)?;
        let body = TokenExchangeRequest {
            code: &code.0,
            code_verifier: verifier,
            redirect_uri,
        };
        let resp = self
            .client
            .post(url)
            .header("accept", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| ServerBridgeError::Transport(e.to_string()))?;
        let status = resp.status();
        let text = resp
            .text()
            .await
            .map_err(|e| ServerBridgeError::Transport(e.to_string()))?;
        if !status.is_success() {
            return Err(ServerBridgeError::Http {
                status: status.as_u16(),
                message: redact_token(&text),
            });
        }
        serde_json::from_str(&text).map_err(|e| ServerBridgeError::Decode(e.to_string()))
    }
}

impl ServerBridge for HttpServerBridge {
    fn exchange(
        &self,
        web_base: &Url,
        code: &AuthorizationCode,
        verifier: &str,
        redirect_uri: &str,
    ) -> Result<TokenExchangeResponse, ServerBridgeError> {
        let url = Self::token_endpoint(web_base)?;
        let body = TokenExchangeRequest {
            code: &code.0,
            code_verifier: verifier,
            redirect_uri,
        };
        let pretty = serde_json::to_string_pretty(&body)
            .map_err(|e| ServerBridgeError::Decode(e.to_string()))?;
        Err(ServerBridgeError::ServerEndpointUnavailable(format!(
            "POST {url}\n{pretty}"
        )))
    }
}

/// Redact any accidental bearer-token echoes from a server message.
///
/// Strips both the `bearer ` marker and the value that follows (any case
/// variations). The marker plus token is replaced with `<redacted>` so the
/// surrounding context (headers, log lines) is preserved for debugging
/// without leaking the secret.
fn redact_token(s: &str) -> String {
    const MARKER: &str = "bearer ";
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(pos) = find_ci(rest, MARKER) {
        out.push_str(&rest[..pos]);
        // Skip past any token characters (until whitespace, comma, or end).
        let after = &rest[pos + MARKER.len()..];
        let token_len = after
            .char_indices()
            .take_while(|(_, c)| !c.is_whitespace() && *c != ',')
            .last()
            .map(|(idx, c)| idx + c.len_utf8())
            .unwrap_or(0);
        // Replace `MARKER + token` with a single `<redacted>` marker.
        out.push_str("<redacted>");
        rest = &after[token_len..];
    }
    out.push_str(rest);
    out
}

fn find_ci(haystack: &str, needle: &str) -> Option<usize> {
    if needle.is_empty() {
        return Some(0);
    }
    let needle_bytes = needle.as_bytes();
    let hay_bytes = haystack.as_bytes();
    if needle_bytes.len() > hay_bytes.len() {
        return None;
    }
    for start in 0..=hay_bytes.len() - needle_bytes.len() {
        let mut matched = true;
        for (a, b) in hay_bytes[start..start + needle_bytes.len()]
            .iter()
            .zip(needle_bytes.iter())
        {
            if a.to_ascii_lowercase() != *b {
                matched = false;
                break;
            }
        }
        if matched {
            return Some(start);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bridge_request_serializes_required_fields() {
        let req = TokenExchangeRequest {
            code: "opaque-grant-abc",
            code_verifier: "verifier-xyz",
            redirect_uri: "http://127.0.0.1:54321/callback",
        };
        let json = serde_json::to_value(&req).unwrap();
        assert_eq!(json["code"], "opaque-grant-abc");
        assert_eq!(json["code_verifier"], "verifier-xyz");
        assert_eq!(json["redirect_uri"], "http://127.0.0.1:54321/callback");
        assert!(json.get("client_id").is_none());
    }

    #[test]
    fn bridge_response_deserializes_with_optional_fields() {
        let json = r#"{"token": "secret", "expires_at": "2030-01-01T00:00:00Z", "subject": "u-1"}"#;
        let r: TokenExchangeResponse = serde_json::from_str(json).unwrap();
        assert_eq!(r.token, "secret");
        assert_eq!(r.subject.as_deref(), Some("u-1"));
    }

    #[test]
    fn bridge_response_accepts_minimal_payload() {
        let json = r#"{"token": "secret"}"#;
        let r: TokenExchangeResponse = serde_json::from_str(json).unwrap();
        assert_eq!(r.token, "secret");
        assert!(r.expires_at.is_none());
        assert!(r.subject.is_none());
    }

    #[test]
    fn authorization_code_redaction_does_not_leak_value() {
        let c = AuthorizationCode::new("opaque-grant-with-long-secret");
        let s = c.redacted_summary();
        assert!(!s.contains("opaque-grant-with-long-secret"));
    }

    #[test]
    fn redaction_strips_bearer_prefix() {
        let r = redact_token("server replied: bearer abc123");
        assert!(!r.contains("abc123"));
        assert!(r.contains("<redacted>"));
    }
}
