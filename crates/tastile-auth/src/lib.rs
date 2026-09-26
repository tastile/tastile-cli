//! `tastile-auth` — credential store + browser-based PKCE login flow.
//!
//! This crate owns:
//!
//! - the **credential store** abstraction (OS keyring today; swap for a mock
//!   in tests),
//! - the **PKCE state machine** for the OAuth-style authorization flow,
//! - the **loopback callback listener** that catches the `?code=…` redirect
//!   from the web authorization route,
//! - the **browser launcher** (`open::that`),
//! - the **`code → bearer token` exchange** against
//!   `POST {web_url}/api/cli/token` (no Better Auth cookie required).
//!
//! The wire contract is documented in `server_bridge.rs`. Until the
//! web-side `/cli/authorize` route and `/api/cli/token` endpoint are
//! exposed, `ServerBridge::exchange` returns
//! `ServerEndpointUnavailable` with the structured request body so an
//! operator can drive the exchange by hand. The CLI surface never logs
//! or echoes the one-time grant, code verifier, or bearer token.
//!
//! # Security principles (see `docs/architecture.md` for the full list)
//!
//! - Bearer tokens are stored in the OS credential store only.
//! - Token values are never logged, never returned by `Display`, never
//!   printed by `doctor`.
//! - One-time grants and code verifiers are never logged.
//! - The PKCE `state` parameter is checked on callback.
//! - The callback listener binds to loopback (`127.0.0.1` or `::1`) only.
//! - The token-exchange HTTP client is configured with `cookie_store(false)`
//!   so the CLI never carries the Better Auth session cookie.

#![doc(html_root_url = "https://docs.rs/tastile-auth/0.1.0")]

pub mod browser;
pub mod callback;
pub mod credential;
pub mod pkce;
pub mod server_bridge;

pub use browser::open_browser;
pub use callback::{CallbackListener, CallbackOutcome};
pub use credential::{
    CredentialStore, DEFAULT_SERVICE, DEFAULT_USER, KeyringStore, MemoryStore, StoredToken,
};
pub use pkce::{PkcePair, PkceState, build_authorization_url};
pub use server_bridge::{
    AuthorizationCode, HttpServerBridge, ServerBridge, ServerBridgeError, TokenExchangeRequest,
    TokenExchangeResponse,
};
