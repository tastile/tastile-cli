//! `tastile-auth` — credential store + browser-based PKCE login flow.
//!
//! This crate owns:
//!
//! - the **credential store** abstraction (OS keyring today; swap for a mock
//!   in tests),
//! - the **PKCE state machine** for the OAuth-style authorization flow,
//! - the **loopback callback listener** that catches the `?code=…` redirect
//!   from the web login page,
//! - the **browser launcher** (`open::that`).
//!
//! The actual exchange of `code → Tastile API token` is described in
//! `server_bridge.rs`. Until the server-side endpoint
//! (`POST /api/mobile/api-token`-equivalent on `app.tastile.app`) is
//! implemented, the flow is structured but stops at "code captured".
//!
//! # Security principles (see `docs/architecture.md` for the full list)
//!
//! - Bearer tokens are stored in the OS credential store only.
//! - Token values are never logged, never returned by `Display`, never
//!   printed by `doctor`.
//! - The PKCE `state` parameter is checked on callback.
//! - The callback listener binds to loopback (`127.0.0.1` or `::1`) only.

#![doc(html_root_url = "https://docs.rs/tastile-auth/0.1.0")]

pub mod browser;
pub mod callback;
pub mod credential;
pub mod pkce;
pub mod server_bridge;

pub use browser::open_browser;
pub use callback::{CallbackListener, CallbackOutcome};
pub use credential::{CredentialStore, DEFAULT_SERVICE, DEFAULT_USER, KeyringStore, StoredToken};
pub use pkce::{PkcePair, PkceState};
pub use server_bridge::{AuthorizationCode, HttpServerBridge, ServerBridge, ServerBridgeError};
