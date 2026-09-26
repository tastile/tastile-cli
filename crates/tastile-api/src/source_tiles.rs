//! Source-tile operations.
//!
//! Spec surface in this module:
//! - `GET    /v1/source-tiles`           → `list_source_tiles`
//! - `GET    /v1/source-tiles/{id}`      → `get_source_tile`
//! - `POST   /v1/source-tiles/{id}/cancel` → `cancel_source_tile`
//!
//! Each operation here corresponds 1:1 to an `operationId` in the pinned
//! `openapi/openapi.yaml`. The drift gate in `build.rs` enforces that those
//! operationIds still exist.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::client::{ApiClient, BearerToken};
use crate::error::ApiResult;
use crate::model::CommandResponse;

// ---------------------------------------------------------------------------
// Recurrence / Temporal / Source-tile summary view models.
// ---------------------------------------------------------------------------

/// Spec: `components.schemas.RecurrenceView`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecurrenceView {
    pub step_min: i32,
    pub window_start_min: i32,
    pub window_end_min: i32,
    #[serde(default)]
    pub expression: Option<String>,
}

/// Spec: `components.schemas.TemporalView`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TemporalView {
    #[serde(default)]
    pub active_start: Option<DateTime<Utc>>,
    #[serde(default)]
    pub active_end: Option<DateTime<Utc>>,
    #[serde(default)]
    pub due_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub fixed_start: Option<DateTime<Utc>>,
    #[serde(default)]
    pub fixed_end: Option<DateTime<Utc>>,
    #[serde(default)]
    pub release_at: Option<DateTime<Utc>>,
}

/// Spec: `components.schemas.SourceTileSummary`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceTileSummary {
    /// Numeric lifecycle: 0 ACTIVE .. 3 CANCELLED.
    pub source_state: i32,
    pub generation_kind: i32,
    pub split_kind: i32,
    pub priority: i32,
    pub required_duration_ms: i64,
    pub window_start_offset_ms: i64,
    pub window_end_offset_ms: i64,
    #[serde(default)]
    pub color: Option<String>,
    #[serde(default)]
    pub external_id: Option<String>,
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub weekday_mask: Option<i32>,
}

/// Spec: `components.schemas.SourceScheduleDefinitionSchema` — opaque from
/// the CLI's perspective. The CLI surfaces the schedule as JSON to the user.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceScheduleDefinition {
    /// Numeric identifier for the schedule variant.
    pub kind: i32,
    #[serde(default)]
    pub rrule: Option<String>,
    #[serde(default)]
    pub window_start_min: Option<i32>,
    #[serde(default)]
    pub window_end_min: Option<i32>,
    #[serde(default)]
    pub step_min: Option<i32>,
    #[serde(default)]
    pub anchor_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub expression: Option<String>,
}

/// Spec: `components.schemas.SourceTileRead`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceTileRead {
    pub source_tile_id: Uuid,
    pub plan_id: Uuid,
    pub owner_id: Uuid,
    pub revision: i64,
    /// Numeric lifecycle: 0 ACTIVE .. 3 CANCELLED.
    pub source_state: i32,
    pub title: String,
    pub plan_role: i32,
    pub schedule: SourceScheduleDefinition,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    #[serde(default)]
    pub color: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub external_id: Option<String>,
    #[serde(default)]
    pub icon: Option<String>,
}

/// Spec: `components.schemas.PlacementTileRead`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlacementTileRead {
    pub placement_id: Uuid,
    pub source_tile_id: Uuid,
    pub occurrence_id: Uuid,
    pub split_index: i32,
    pub split_count: i32,
    pub split_group_id: Uuid,
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
    pub closed: bool,
    pub revision: i64,
    #[serde(default)]
    pub closed_at: Option<DateTime<Utc>>,
}

/// Spec: `components.schemas.SourceOccurrenceRead` — kept opaque (JSON value)
/// because the CLI does not act on it directly; surfacing it raw to
/// `tracing` is enough.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceOccurrenceRead(pub serde_json::Value);

/// Spec: `components.schemas.RelationDefinitionRead` — opaque JSON.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RelationDefinitionRead(pub serde_json::Value);

/// Spec: `components.schemas.SourceTileDetailRead`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceTileDetailRead {
    pub source: SourceTileRead,
    pub relations: Vec<RelationDefinitionRead>,
    pub occurrences: Vec<SourceOccurrenceRead>,
    pub placements: Vec<PlacementTileRead>,
}

// ---------------------------------------------------------------------------
// Operations.
// ---------------------------------------------------------------------------

/// Query for `GET /v1/source-tiles`. The server accepts a wide variety of
/// filters; we expose the most-used subset for the CLI MVP.
#[derive(Debug, Default, Clone)]
pub struct ListSourceTilesQuery {
    pub owner_id: Option<Uuid>,
    pub plan_id: Option<Uuid>,
    pub state: Option<i32>,
    pub limit: Option<u32>,
}

impl ListSourceTilesQuery {
    fn to_pairs(&self) -> Vec<(&'static str, String)> {
        let mut out = Vec::new();
        if let Some(v) = self.owner_id {
            out.push(("owner_id", v.to_string()));
        }
        if let Some(v) = self.plan_id {
            out.push(("plan_id", v.to_string()));
        }
        if let Some(v) = self.state {
            out.push(("state", v.to_string()));
        }
        if let Some(v) = self.limit {
            out.push(("limit", v.to_string()));
        }
        out
    }
}

/// `GET /v1/source-tiles` → `operationId: list_source_tiles`.
pub async fn list_source_tiles(
    client: &ApiClient,
    token: &BearerToken,
    q: &ListSourceTilesQuery,
) -> ApiResult<Vec<SourceTileRead>> {
    client
        .get_json(token, "/v1/source-tiles", &q.to_pairs())
        .await
}

/// `GET /v1/source-tiles/{id}` → `operationId: get_source_tile`.
pub async fn get_source_tile(
    client: &ApiClient,
    token: &BearerToken,
    id: Uuid,
) -> ApiResult<SourceTileDetailRead> {
    let path = format!("/v1/source-tiles/{id}");
    client.get_json(token, &path, &[]).await
}

// ---------------------------------------------------------------------------
// Cancel.
// ---------------------------------------------------------------------------

/// Spec: `components.schemas.CancelSourceTileRequest`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CancelSourceTileRequest {
    pub idempotency_key: Uuid,
    pub payload: CancelSourceTilePayload,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub occurred_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_revision: Option<i64>,
}

/// Spec: `components.schemas.CancelSourceTilePayloadSchema`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CancelSourceTilePayload {
    pub reason: String,
}

impl CancelSourceTileRequest {
    /// Build a request with a freshly-generated idempotency key.
    pub fn new(reason: impl Into<String>) -> Self {
        Self {
            idempotency_key: Uuid::new_v4(),
            payload: CancelSourceTilePayload {
                reason: reason.into(),
            },
            occurred_at: None,
            expected_revision: None,
        }
    }
}

/// `POST /v1/source-tiles/{id}/cancel` → `operationId: cancel_source_tile`.
pub async fn cancel_source_tile(
    client: &ApiClient,
    token: &BearerToken,
    id: Uuid,
    req: &CancelSourceTileRequest,
) -> ApiResult<CommandResponse> {
    let path = format!("/v1/source-tiles/{id}/cancel");
    client.post_json(token, &path, req).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancel_request_serializes_payload() {
        let req = CancelSourceTileRequest::new("user-requested");
        let json = serde_json::to_value(&req).expect("serializes");
        assert!(json["idempotency_key"].is_string());
        assert_eq!(json["payload"]["reason"], "user-requested");
    }

    #[test]
    fn source_tile_read_roundtrip_minimal() {
        let json = serde_json::json!({
            "source_tile_id": Uuid::nil(),
            "plan_id": Uuid::nil(),
            "owner_id": Uuid::nil(),
            "revision": 1,
            "source_state": 0,
            "title": "demo",
            "plan_role": 0,
            "schedule": {"kind": 0},
            "created_at": "2026-09-19T00:00:00Z",
            "updated_at": "2026-09-19T00:00:00Z"
        });
        let parsed: SourceTileRead = serde_json::from_value(json).expect("parses");
        assert_eq!(parsed.title, "demo");
        assert_eq!(parsed.source_state, 0);
    }
}
