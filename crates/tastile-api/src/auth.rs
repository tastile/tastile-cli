//! Auth-related operations on the v1 API surface.
//!
//! The CLI uses these to sign out (`POST /v1/auth/signout`). Login is
//! browser-mediated and lives in `tastile-auth`, not here — this module only
//! covers the server-side wire operations that the API exposes.

use crate::client::{ApiClient, BearerToken};
use crate::error::ApiResult;

/// `POST /v1/auth/signout` → `operationId: signout`.
///
/// The server returns `204` on success (or on no-op: revoke is idempotent).
/// This function does not consume the body; the caller is expected to drop
/// the bearer token after this returns.
pub async fn signout(client: &ApiClient, token: &BearerToken) -> ApiResult<()> {
    let status = client.post_empty(token, "/v1/auth/signout").await?;
    debug_assert!(
        status.is_success(),
        "post_empty returns Ok only for 2xx (got {status})"
    );
    Ok(())
}
