//! Source-tile operations.
//!
//! Spec surface in this module:
//! - `GET    /v1/source-tiles`                  → `list_source_tiles`
//! - `POST   /v1/source-tiles`                  → `create_source_tile`
//! - `GET    /v1/source-tiles/{id}`             → `get_source_tile`
//! - `PUT    /v1/source-tiles/{id}`             → `update_source_tile`
//! - `POST   /v1/source-tiles/{id}/cancel`      → `cancel_source_tile`
//! - `GET    /v1/source-tiles/{id}/completion`  → `get_source_tile_completion`
//! - `GET    /v1/source-tiles/{id}/placements`  → `list_source_tile_placements`
//! - `POST   /v1/source-tiles/{id}/reflow`      → `reflow_source_tile`
//!
//! Each operation here corresponds 1:1 to an `operationId` in the pinned
//! `openapi/openapi.yaml`. The drift gate in `build.rs` enforces that those
//! operationIds still exist with the right method, path, body schema,
//! response schema, path parameters, and required envelope fields.

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

/// Spec: `components.schemas.SourceGenerationSchema`.
///
/// `kind` is the numeric generation kind: `0 = one-time`, `1 = recurring`.
/// The remaining fields describe the recurrence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceGeneration {
    pub kind: i32,
    #[serde(default)]
    pub at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub starts_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub interval_ms: Option<i64>,
    #[serde(default)]
    pub ends_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub weekday_mask: Option<i32>,
    #[serde(default)]
    pub date_range_start: Option<String>,
    #[serde(default)]
    pub date_range_end: Option<String>,
    #[serde(default)]
    pub excluded_dates: Vec<String>,
    /// Local calendar offset in minutes east of UTC.
    #[serde(default)]
    pub offset_min: Option<i32>,
}

/// Spec: `components.schemas.SourceWindowSchema` — the placement window
/// relative to the nominal time. The server requires
/// `start_offset_ms < end_offset_ms`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceWindow {
    pub start_offset_ms: i64,
    pub end_offset_ms: i64,
}

/// Spec: `components.schemas.SplitPolicySchema`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceSplitPolicy {
    /// `0 = unsplit`, `1 = split`.
    pub kind: i32,
    #[serde(default)]
    pub min_segment_ms: Option<i64>,
    #[serde(default)]
    pub max_segment_ms: Option<i64>,
    #[serde(default)]
    pub max_segments: Option<i32>,
}

/// Spec: `components.schemas.SourceScheduleDefinitionSchema` — the schedule
/// as the server stores and returns it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceScheduleDefinition {
    pub required_duration_ms: i64,
    pub generation: SourceGeneration,
    pub window: SourceWindow,
    pub split_policy: SourceSplitPolicy,
    pub priority: i32,
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
    pub limit: Option<u32>,
    /// Rows to skip. The server defaults to `0` and caps `limit` at 500.
    pub offset: Option<u32>,
}

impl ListSourceTilesQuery {
    fn to_pairs(&self) -> Vec<(&'static str, String)> {
        let mut out = Vec::new();
        if let Some(v) = self.owner_id {
            out.push(("owner_id", v.to_string()));
        }
        if let Some(v) = self.limit {
            out.push(("limit", v.to_string()));
        }
        if let Some(v) = self.offset {
            out.push(("offset", v.to_string()));
        }
        out
    }
}

/// Largest page the pinned contract allows for the source-tile listings
/// (`limit` is documented as `1..500`).
pub const SOURCE_TILE_PAGE_SIZE: u32 = 500;

/// Advance to the next page while respecting the OpenAPI `int32` offset.
fn next_page_offset(offset: u32, page_len: usize) -> ApiResult<u32> {
    let page_len = u32::try_from(page_len).map_err(|_| {
        crate::error::ApiError::Pagination(
            "source-tile pagination page length exceeds the contract limit".into(),
        )
    })?;
    let next = offset.checked_add(page_len).ok_or_else(|| {
        crate::error::ApiError::Pagination(
            "source-tile pagination offset exceeded the contract limit".into(),
        )
    })?;
    if next > i32::MAX as u32 {
        return Err(crate::error::ApiError::Pagination(
            "source-tile pagination offset exceeded the contract int32 limit".into(),
        ));
    }
    Ok(next)
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

/// Fetch **every** source tile of the owner by walking the documented
/// `limit` / `offset` pagination.
///
/// The server applies its own default page size (100 rows) when `limit` is
/// omitted, so a single request silently truncates large owners. This
/// helper keeps requesting pages until a short page proves the end of the
/// result set.
pub async fn list_all_source_tiles(
    client: &ApiClient,
    token: &BearerToken,
    q: &ListSourceTilesQuery,
) -> ApiResult<Vec<SourceTileRead>> {
    let mut out: Vec<SourceTileRead> = Vec::new();
    let mut offset: u32 = 0;
    loop {
        let page_query = ListSourceTilesQuery {
            limit: Some(SOURCE_TILE_PAGE_SIZE),
            offset: Some(offset),
            ..q.clone()
        };
        let page = list_source_tiles(client, token, &page_query).await?;
        let page_len = page.len();
        out.extend(page);
        if page_len < SOURCE_TILE_PAGE_SIZE as usize {
            return Ok(out);
        }
        offset = next_page_offset(offset, page_len)?;
    }
}

/// `GET /v1/source-tiles/{id}` → `operationId: get_source_tile`.
pub async fn get_source_tile(
    client: &ApiClient,
    token: &BearerToken,
    id: Uuid,
) -> ApiResult<SourceTileDetailRead> {
    let mut combined: Option<SourceTileDetailRead> = None;
    let mut offset = 0_u32;
    loop {
        let page = get_source_tile_page(client, token, id, SOURCE_TILE_PAGE_SIZE, offset).await?;
        let occurrence_count = page.occurrences.len();
        let placement_count = page.placements.len();
        if let Some(detail) = combined.as_mut() {
            detail.occurrences.extend(page.occurrences);
            detail.placements.extend(page.placements);
        } else {
            combined = Some(page);
        }
        if occurrence_count < SOURCE_TILE_PAGE_SIZE as usize
            && placement_count < SOURCE_TILE_PAGE_SIZE as usize
        {
            return Ok(combined.expect("a page was fetched"));
        }
        offset = next_page_offset(offset, SOURCE_TILE_PAGE_SIZE as usize)?;
    }
}

async fn get_source_tile_page(
    client: &ApiClient,
    token: &BearerToken,
    id: Uuid,
    limit: u32,
    offset: u32,
) -> ApiResult<SourceTileDetailRead> {
    let path = format!("/v1/source-tiles/{id}");
    client
        .get_json(
            token,
            &path,
            &[("limit", limit.to_string()), ("offset", offset.to_string())],
        )
        .await
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
///
/// The wire value is a **numeric** cancel-reason registry code, not free
/// text: the schema declares `reason` as `integer / int32` and points at
/// the server-side `domain::source_tile::state::cancel_reason` registry.
/// A JSON string is rejected by the server, so the CLI keeps the payload
/// numeric.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CancelSourceTilePayload {
    pub reason: i32,
}

impl CancelSourceTileRequest {
    /// Build a request with a freshly-generated idempotency key.
    ///
    /// `reason` is the numeric cancel-reason registry code.
    pub fn new(reason: i32) -> Self {
        Self {
            idempotency_key: Uuid::new_v4(),
            payload: CancelSourceTilePayload { reason },
            occurred_at: None,
            expected_revision: None,
        }
    }

    /// Attach the source-tile revision the caller expects to still be
    /// current. The server answers `409` when the stored revision moved on.
    #[must_use]
    pub fn with_expected_revision(mut self, revision: i64) -> Self {
        self.expected_revision = Some(revision);
        self
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

// ---------------------------------------------------------------------------
// Create / Update.
// ---------------------------------------------------------------------------

/// Spec: `components.schemas.ScheduleTileDefinitionSchema`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScheduleTileDefinition {
    pub title: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub color: Option<String>,
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub external_id: Option<String>,
}

/// Spec: `components.schemas.SchedulePlanDefinitionSchema`. The CLI exposes
/// only the fields it actually sets; the remainder is forwarded as
/// `serde_json::Value` so the CLI can stay aligned with server changes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SchedulePlanDefinition {
    pub role: i32,
    pub completion: serde_json::Value,
    pub planning: serde_json::Value,
    pub metrics: Vec<serde_json::Value>,
    pub decisions: Vec<serde_json::Value>,
    pub references: Vec<serde_json::Value>,
}

/// Spec: `components.schemas.SourceScheduleDefinitionSchema`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateSourceScheduleDefinition {
    pub generation: serde_json::Value,
    pub split_policy: serde_json::Value,
    pub window: serde_json::Value,
    pub priority: i32,
    pub required_duration_ms: i64,
}

/// Spec: `components.schemas.SpanSchema`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Span {
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
}

/// Spec: `components.schemas.FlowDefinitionSchema` — opaque JSON, the CLI
/// builds these server-side via the planning engine.
pub type FlowDefinition = serde_json::Value;

/// Spec: `components.schemas.CreateSourceTilePayloadSchema`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CreateSourceTilePayload {
    pub tile: ScheduleTileDefinition,
    pub plan: SchedulePlanDefinition,
    pub flows: Vec<FlowDefinition>,
    pub schedule: CreateSourceScheduleDefinition,
    pub horizon: Span,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub relations: Option<Vec<serde_json::Value>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_client_local_id: Option<Uuid>,
}

/// Spec: `components.schemas.CreateSourceTileRequest`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CreateSourceTileRequest {
    pub idempotency_key: Uuid,
    pub payload: CreateSourceTilePayload,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub occurred_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_revision: Option<i64>,
}

impl CreateSourceTileRequest {
    pub fn new(payload: CreateSourceTilePayload) -> Self {
        Self {
            idempotency_key: Uuid::new_v4(),
            payload,
            occurred_at: None,
            expected_revision: None,
        }
    }
}

/// `POST /v1/source-tiles` → `operationId: create_source_tile`.
pub async fn create_source_tile(
    client: &ApiClient,
    token: &BearerToken,
    req: &CreateSourceTileRequest,
) -> ApiResult<CommandResponse> {
    client.post_json(token, "/v1/source-tiles", req).await
}

/// Spec: `components.schemas.UpdateSourceTilePayloadSchema`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct UpdateSourceTilePayload {
    pub tile: ScheduleTileDefinition,
    pub plan: SchedulePlanDefinition,
    pub flows: Vec<FlowDefinition>,
    pub schedule: CreateSourceScheduleDefinition,
    pub horizon: Span,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub relations: Option<Vec<serde_json::Value>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_client_local_id: Option<Uuid>,
}

/// Spec: `components.schemas.UpdateSourceTileRequest`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct UpdateSourceTileRequest {
    pub idempotency_key: Uuid,
    pub payload: UpdateSourceTilePayload,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub occurred_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_revision: Option<i64>,
}

impl UpdateSourceTileRequest {
    pub fn new(payload: UpdateSourceTilePayload) -> Self {
        Self {
            idempotency_key: Uuid::new_v4(),
            payload,
            occurred_at: None,
            expected_revision: None,
        }
    }

    /// Attach the source-tile revision the caller expects to still be
    /// current. The server answers `409` when the stored revision moved on.
    #[must_use]
    pub fn with_expected_revision(mut self, revision: i64) -> Self {
        self.expected_revision = Some(revision);
        self
    }
}

/// `PUT /v1/source-tiles/{id}` → `operationId: update_source_tile`.
pub async fn update_source_tile(
    client: &ApiClient,
    token: &BearerToken,
    id: Uuid,
    req: &UpdateSourceTileRequest,
) -> ApiResult<CommandResponse> {
    let path = format!("/v1/source-tiles/{id}");
    client.put_json(token, &path, req).await
}

// ---------------------------------------------------------------------------
// Completion / Placements / Reflow.
// ---------------------------------------------------------------------------

/// Spec: `GET /v1/source-tiles/{id}/completion` → `operationId: get_source_tile_completion`.
///
/// The response schema is intentionally empty (`schema: {}`), so the CLI
/// surfaces the raw JSON value to the user.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceTileCompletion(pub serde_json::Value);

/// `GET /v1/source-tiles/{id}/completion` → `operationId: get_source_tile_completion`.
pub async fn get_source_tile_completion(
    client: &ApiClient,
    token: &BearerToken,
    id: Uuid,
) -> ApiResult<SourceTileCompletion> {
    let path = format!("/v1/source-tiles/{id}/completion");
    let value: serde_json::Value = client.get_json(token, &path, &[]).await?;
    Ok(SourceTileCompletion(value))
}

/// `GET /v1/source-tiles/{id}/placements` → `operationId: list_source_tile_placements`.
pub async fn list_source_tile_placements(
    client: &ApiClient,
    token: &BearerToken,
    id: Uuid,
    limit: Option<u32>,
    offset: Option<u32>,
) -> ApiResult<Vec<PlacementTileRead>> {
    let path = format!("/v1/source-tiles/{id}/placements");
    let mut pairs: Vec<(&'static str, String)> = Vec::new();
    if let Some(l) = limit {
        pairs.push(("limit", l.to_string()));
    }
    if let Some(o) = offset {
        pairs.push(("offset", o.to_string()));
    }
    client.get_json(token, &path, &pairs).await
}

/// Fetch **every** placement of a source tile by walking the documented
/// `limit` / `offset` pagination.
pub async fn list_all_source_tile_placements(
    client: &ApiClient,
    token: &BearerToken,
    id: Uuid,
) -> ApiResult<Vec<PlacementTileRead>> {
    let mut out: Vec<PlacementTileRead> = Vec::new();
    let mut offset: u32 = 0;
    loop {
        let page = list_source_tile_placements(
            client,
            token,
            id,
            Some(SOURCE_TILE_PAGE_SIZE),
            Some(offset),
        )
        .await?;
        let page_len = page.len();
        out.extend(page);
        if page_len < SOURCE_TILE_PAGE_SIZE as usize {
            return Ok(out);
        }
        offset = next_page_offset(offset, page_len)?;
    }
}

/// Spec: `components.schemas.ReflowSourceTilePayloadSchema`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ReflowSourceTilePayload {
    pub range: Span,
}

/// Spec: `components.schemas.ReflowSourceTileRequest`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ReflowSourceTileRequest {
    pub idempotency_key: Uuid,
    pub payload: ReflowSourceTilePayload,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub occurred_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_revision: Option<i64>,
}

impl ReflowSourceTileRequest {
    pub fn new(range: Span) -> Self {
        Self {
            idempotency_key: Uuid::new_v4(),
            payload: ReflowSourceTilePayload { range },
            occurred_at: None,
            expected_revision: None,
        }
    }

    /// Attach the source-tile revision the caller expects to still be
    /// current. The server answers `409` when the stored revision moved on.
    #[must_use]
    pub fn with_expected_revision(mut self, revision: i64) -> Self {
        self.expected_revision = Some(revision);
        self
    }
}

/// `POST /v1/source-tiles/{id}/reflow` → `operationId: reflow_source_tile`.
pub async fn reflow_source_tile(
    client: &ApiClient,
    token: &BearerToken,
    id: Uuid,
    req: &ReflowSourceTileRequest,
) -> ApiResult<CommandResponse> {
    let path = format!("/v1/source-tiles/{id}/reflow");
    client.post_json(token, &path, req).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pagination_offset_stops_at_the_contract_int32_limit() {
        assert_eq!(next_page_offset(500, 500).unwrap(), 1_000);
        assert!(next_page_offset(i32::MAX as u32, 500).is_err());
    }

    #[test]
    fn cancel_request_serializes_numeric_payload() {
        let req = CancelSourceTileRequest::new(0);
        let json = serde_json::to_value(&req).expect("serializes");
        assert!(json["idempotency_key"].is_string());
        assert_eq!(json["payload"]["reason"], 0);
    }

    #[test]
    fn expected_revision_is_carried_on_the_envelope() {
        let req = CancelSourceTileRequest::new(0).with_expected_revision(7);
        let json = serde_json::to_value(&req).expect("serializes");
        assert_eq!(json["expected_revision"], 7);
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
            "schedule": {
                "required_duration_ms": 1800000,
                "generation": {"kind": 0, "at": "2026-09-19T09:00:00Z"},
                "window": {"start_offset_ms": 0, "end_offset_ms": 14400000},
                "split_policy": {"kind": 0},
                "priority": 0
            },
            "created_at": "2026-09-19T00:00:00Z",
            "updated_at": "2026-09-19T00:00:00Z"
        });
        let parsed: SourceTileRead = serde_json::from_value(json).expect("parses");
        assert_eq!(parsed.title, "demo");
        assert_eq!(parsed.source_state, 0);
    }
}
