//! Common model types shared across operations.
//!
//! Each type here is a **mirror** of a component in `openapi/openapi.yaml`.
//! Comments link to the spec location so reviewers can cross-check. When the
//! pinned OpenAPI revision moves, run `scripts/check-openapi-drift.sh` to
//! confirm this file still matches.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// API version metadata embedded in the build from `openapi/openapi.yaml`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ApiVersion {
    /// OpenAPI `info.version` at build time.
    pub version: &'static str,
    /// OpenAPI `info.title` at build time.
    pub title: &'static str,
    /// Pinned submodule commit SHA at build time (set by `tastile-cli`
    /// after reading the submodule pointer).
    pub pinned_revision: Option<&'static str>,
}

/// `CommandResponse` — universal response for write endpoints.
///
/// Spec: `openapi/openapi.yaml` → `components.schemas.CommandResponse`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandResponse {
    pub command_id: Uuid,
    pub accepted_at: DateTime<Utc>,
    /// 0 = applied, 1 = already applied, 2 = accepted.
    pub result: i32,
    #[serde(default)]
    pub pending: Vec<PendingWork>,
    #[serde(default)]
    pub revision: Option<i64>,
    #[serde(default)]
    pub aggregate: Option<serde_json::Value>,
    #[serde(default)]
    pub aggregate_meta: Option<serde_json::Value>,
}

/// `PendingWorkSchema` — work the server is waiting on to complete the
/// command. We keep it as `serde_json::Value` because the CLI does not act
/// on it directly; it is surfaced through `tracing` only.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingWork {
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub target: Option<serde_json::Value>,
}

/// `OwnerDeleteResponseSchema` — response for `DELETE /v1/owners/{kind}/{id}`.
///
/// Spec: `openapi/openapi.yaml` → `components.schemas.OwnerDeleteResponseSchema`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OwnerDeleteResponse {
    pub deleted_at: DateTime<Utc>,
    pub retention_until: DateTime<Utc>,
    pub kept: Vec<String>,
}

/// Re-export of `TileListView` so callers can `use tastile_api::TileListView`.
///
/// The authoritative definition lives in `tiles.rs` because the field set
/// is closely coupled to the `list_tiles` query parameters.
pub use crate::tiles::TileListView;
