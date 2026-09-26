//! API error types.

use std::time::Duration;

use thiserror::Error;

/// Result alias for API operations.
pub type ApiResult<T> = Result<T, ApiError>;

/// Errors that can surface from any operation in this crate.
///
/// The error variants are intentionally coarse: the CLI surfaces them to
/// humans in the doctor subcommand and TUI, where a single line per failure
/// is what users actually need. Granular debugging lives in `tracing`.
#[derive(Debug, Error)]
pub enum ApiError {
    /// Connection failure (DNS, TCP, TLS). Distinct from "the server
    /// rejected our request".
    #[error("transport: {0}")]
    Transport(#[from] reqwest::Error),

    /// Server returned non-2xx. The body is included for `tracing` but never
    /// printed to the user (credentials may be present in some responses).
    #[error("http {status}: {message}")]
    Http { status: u16, message: String },

    /// Server returned a payload that does not parse to the schema we
    /// expected. Almost always a sign of contract drift between the pinned
    /// `tastile-openapi` revision and the typed models in this crate.
    #[error("decode: {0}")]
    Decode(#[from] serde_json::Error),

    /// The provided base URL is malformed.
    #[error("invalid base url: {0}")]
    InvalidBaseUrl(String),

    /// Operation requested after the configured timeout elapsed.
    #[error("timeout after {0:?}")]
    Timeout(Duration),

    /// A request that must be authenticated was attempted without a bearer
    /// token. The CLI surfaces this as "you need to run `tastile auth login`".
    #[error("missing bearer token")]
    MissingBearer,
}
