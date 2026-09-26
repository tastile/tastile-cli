//! Server bridge: exchange the one-time authorization code for a Tastile API
//! token.
//!
//! # Status (2026-09-27)
//!
//! The web side does not yet expose a public `/api/cli/api-token` endpoint
//! that mirrors the existing `/api/mobile/api-token` shape. Until that
//! endpoint lands, `ServerBridge::exchange` is structured to call it but
//! returns [`ServerBridgeError::ServerEndpointUnavailable`] with the exact
//! request that would be issued. This keeps the CLI flow correct without
//! pretending to talk to a server that does not exist.
//!
//! When the server endpoint ships:
//! 1. Replace the body of `exchange` with the real HTTP call.
//! 2. Remove `ServerEndpointUnavailable` from the error enum.
//! 3. Update `tastile-cli` integration test to assert a real round trip.

use serde::Serialize;
use thiserror::Error;
use url::Url;

/// One-time authorization code returned by the browser callback.
#[derive(Debug, Clone)]
pub struct AuthorizationCode(pub String);

impl AuthorizationCode {
    pub fn new(code: impl Into<String>) -> Self {
        Self(code.into())
    }
}

/// Request body for the (future) `POST /api/cli/api-token` endpoint. Mirrors
/// the `/api/mobile/api-token` payload that already exists for the mobile
/// apps.
#[derive(Debug, Clone, Serialize)]
pub struct ServerBridgeRequest<'a> {
    pub code: &'a str,
    pub code_verifier: &'a str,
    pub client_id: &'a str,
    pub redirect_uri: &'a str,
}

/// Errors that the CLI bridge can surface.
#[derive(Debug, Error)]
pub enum ServerBridgeError {
    /// The web side has not yet exposed the CLI token endpoint. The CLI
    /// prints the structured request so an operator can drive the exchange
    /// by hand.
    #[error(
        "server endpoint `/api/cli/api-token` is not yet available; \
         manually exchange the captured code:\n{0}"
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
    /// Exchange `(code, verifier)` for a Tastile API bearer token.
    fn exchange(
        &self,
        web_base: &Url,
        client_id: &str,
        code: &AuthorizationCode,
        verifier: &str,
        redirect_uri: &str,
    ) -> Result<String, ServerBridgeError>;
}

/// Production implementation. Currently a placeholder that returns the
/// structured request so the CLI can show the operator what *would* be
/// sent.
#[derive(Debug, Default, Clone)]
pub struct HttpServerBridge {
    /// HTTP client used for the real call once the endpoint exists.
    pub client: reqwest::Client,
}

impl HttpServerBridge {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(15))
                .build()
                .expect("reqwest client"),
        }
    }
}

impl ServerBridge for HttpServerBridge {
    fn exchange(
        &self,
        web_base: &Url,
        client_id: &str,
        code: &AuthorizationCode,
        verifier: &str,
        redirect_uri: &str,
    ) -> Result<String, ServerBridgeError> {
        let url = web_base
            .join("/api/cli/api-token")
            .map_err(|e| ServerBridgeError::Transport(e.to_string()))?;
        let body = ServerBridgeRequest {
            code: &code.0,
            code_verifier: verifier,
            client_id,
            redirect_uri,
        };
        let pretty = serde_json::to_string_pretty(&body)
            .map_err(|e| ServerBridgeError::Decode(e.to_string()))?;
        // Until the server endpoint exists, return the structured request
        // so the operator can complete the exchange by hand. The CLI then
        // surfaces this through `tastile doctor` and `auth login --verbose`.
        Err(ServerBridgeError::ServerEndpointUnavailable(format!(
            "POST {url}\n{pretty}"
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bridge_request_serializes_required_fields() {
        let req = ServerBridgeRequest {
            code: "abc",
            code_verifier: "v",
            client_id: "tastile-cli",
            redirect_uri: "http://127.0.0.1:0/callback",
        };
        let json = serde_json::to_value(&req).unwrap();
        assert_eq!(json["code"], "abc");
        assert_eq!(json["code_verifier"], "v");
        assert_eq!(json["client_id"], "tastile-cli");
        assert_eq!(json["redirect_uri"], "http://127.0.0.1:0/callback");
    }
}
