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

use std::collections::HashSet;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Deserializer, Serialize};
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

/// Spec: `components.schemas.SourceTileDefinitionSchema` (v1/14 §17) — the
/// write-shaped definition read back from normalized persisted state for
/// merge-or-replace editing. `schedule` stays on `SourceTileRead.schedule`,
/// `horizon` is never persisted, and `source_client_local_id` is always
/// null on read-back. Relation refs are normalized to `existing`; clients
/// re-author `local` refs explicitly when needed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceTileDefinition {
    pub tile: ScheduleTileDefinition,
    pub plan: SchedulePlanDefinition,
    pub flows: Vec<FlowDefinition>,
    pub relations: Vec<serde_json::Value>,
    #[serde(default)]
    pub source_client_local_id: Option<Uuid>,
}

/// Spec: `components.schemas.SourceTileDetailRead`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceTileDetailRead {
    pub source: SourceTileRead,
    pub relations: Vec<RelationDefinitionRead>,
    pub occurrences: Vec<SourceOccurrenceRead>,
    pub placements: Vec<PlacementTileRead>,
    /// Optional for tolerance of pre-v3.2.0 servers; `update` requires it.
    #[serde(default)]
    pub definition: Option<SourceTileDefinition>,
}

/// Canonical cursor page from Core. The untagged decoder also accepts the
/// pinned pre-cursor array response while the OpenAPI revision is pending.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ListResponse<T> {
    pub items: Vec<T>,
    /// Number of items in this page (not the total result count).
    pub count: usize,
    pub next_cursor: Option<String>,
    #[serde(skip)]
    pub(crate) legacy_array: bool,
}

impl<'de, T> Deserialize<'de> for ListResponse<T>
where
    T: Deserialize<'de>,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Page<T> {
            Cursor {
                items: Vec<T>,
                count: usize,
                #[serde(default)]
                next_cursor: Option<String>,
            },
            Legacy(Vec<T>),
        }

        match Page::deserialize(deserializer)? {
            Page::Cursor {
                items,
                count,
                next_cursor,
            } => {
                if count != items.len() {
                    return Err(serde::de::Error::custom(format!(
                        "cursor page count {count} does not match items length {}",
                        items.len()
                    )));
                }
                Ok(Self {
                    items,
                    count,
                    next_cursor,
                    legacy_array: false,
                })
            }
            Page::Legacy(items) => Ok(Self {
                count: items.len(),
                items,
                next_cursor: None,
                legacy_array: true,
            }),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
struct SourceTileDetailPage {
    source: SourceTileRead,
    relations: Vec<RelationDefinitionRead>,
    occurrences: ListResponse<SourceOccurrenceRead>,
    placements: ListResponse<PlacementTileRead>,
    #[serde(default)]
    definition: Option<SourceTileDefinition>,
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
    /// Opaque keyset cursor returned by the previous page.
    pub cursor: Option<String>,
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
        if let Some(v) = &self.cursor {
            out.push(("cursor", v.clone()));
        }
        out
    }
}

/// A page size accepted by the pinned cursor contract (`1..=1000`).
pub const SOURCE_TILE_PAGE_SIZE: u32 = 500;

/// `GET /v1/source-tiles` → `operationId: list_source_tiles`.
pub async fn list_source_tiles(
    client: &ApiClient,
    token: &BearerToken,
    q: &ListSourceTilesQuery,
) -> ApiResult<ListResponse<SourceTileRead>> {
    client
        .get_json(token, "/v1/source-tiles", &q.to_pairs())
        .await
}

/// Fetch every source-tile page until Core returns no `next_cursor`.
pub async fn list_all_source_tiles(
    client: &ApiClient,
    token: &BearerToken,
    q: &ListSourceTilesQuery,
) -> ApiResult<Vec<SourceTileRead>> {
    let mut out: Vec<SourceTileRead> = Vec::new();
    let mut cursor = q.cursor.clone();
    let mut seen = HashSet::new();
    if let Some(first) = &cursor {
        seen.insert(first.clone());
    }
    loop {
        let page_query = ListSourceTilesQuery {
            limit: Some(SOURCE_TILE_PAGE_SIZE),
            cursor,
            ..q.clone()
        };
        let page = list_source_tiles(client, token, &page_query).await?;
        if page.legacy_array && page.items.len() >= SOURCE_TILE_PAGE_SIZE as usize {
            return Err(crate::error::ApiError::Pagination(
                "legacy source-tile response filled the page; cursor pagination is required to prove completeness".into(),
            ));
        }
        out.extend(page.items);
        let Some(next_cursor) = page.next_cursor else {
            break;
        };
        if !seen.insert(next_cursor.clone()) {
            return Err(crate::error::ApiError::Pagination(
                "source-tile list returned a repeated cursor".into(),
            ));
        }
        cursor = Some(next_cursor);
    }
    Ok(out)
}

/// `GET /v1/source-tiles/{id}` → `operationId: get_source_tile`.
pub async fn get_source_tile(
    client: &ApiClient,
    token: &BearerToken,
    id: Uuid,
) -> ApiResult<SourceTileDetailRead> {
    let mut combined: Option<SourceTileDetailRead> = None;
    let mut occurrence_cursor = None;
    let mut placement_cursor = None;
    let mut occurrence_done = false;
    let mut placement_done = false;
    let mut seen_occurrence_cursors = HashSet::new();
    let mut seen_placement_cursors = HashSet::new();
    loop {
        let page = get_source_tile_page(
            client,
            token,
            id,
            occurrence_cursor.as_deref(),
            placement_cursor.as_deref(),
        )
        .await?;
        let detail = combined.get_or_insert_with(|| SourceTileDetailRead {
            source: page.source.clone(),
            relations: page.relations.clone(),
            occurrences: Vec::new(),
            placements: Vec::new(),
            definition: page.definition.clone(),
        });
        if detail.definition.is_none() {
            detail.definition = page.definition.clone();
        }

        if !occurrence_done {
            if page.occurrences.legacy_array
                && page.occurrences.items.len() >= SOURCE_TILE_PAGE_SIZE as usize
            {
                return Err(crate::error::ApiError::Pagination(
                    "legacy source-tile detail occurrences filled the page; cursor pagination is required to prove completeness".into(),
                ));
            }
            detail.occurrences.extend(page.occurrences.items);
            match page.occurrences.next_cursor {
                Some(next) if seen_occurrence_cursors.insert(next.clone()) => {
                    occurrence_cursor = Some(next);
                }
                Some(_) => {
                    return Err(crate::error::ApiError::Pagination(
                        "source-tile detail returned a repeated occurrence cursor".into(),
                    ));
                }
                None => {
                    occurrence_done = true;
                    occurrence_cursor = None;
                }
            }
        }

        if !placement_done {
            if page.placements.legacy_array
                && page.placements.items.len() >= SOURCE_TILE_PAGE_SIZE as usize
            {
                return Err(crate::error::ApiError::Pagination(
                    "legacy source-tile detail placements filled the page; cursor pagination is required to prove completeness".into(),
                ));
            }
            detail.placements.extend(page.placements.items);
            match page.placements.next_cursor {
                Some(next) if seen_placement_cursors.insert(next.clone()) => {
                    placement_cursor = Some(next);
                }
                Some(_) => {
                    return Err(crate::error::ApiError::Pagination(
                        "source-tile detail returned a repeated placement cursor".into(),
                    ));
                }
                None => {
                    placement_done = true;
                    placement_cursor = None;
                }
            }
        }

        if occurrence_done && placement_done {
            return Ok(combined.expect("a detail page was fetched"));
        }
    }
}

async fn get_source_tile_page(
    client: &ApiClient,
    token: &BearerToken,
    id: Uuid,
    occurrence_cursor: Option<&str>,
    placement_cursor: Option<&str>,
) -> ApiResult<SourceTileDetailPage> {
    let path = format!("/v1/source-tiles/{id}");
    let mut pairs = vec![
        ("occurrence_limit", SOURCE_TILE_PAGE_SIZE.to_string()),
        ("placement_limit", SOURCE_TILE_PAGE_SIZE.to_string()),
    ];
    if let Some(cursor) = occurrence_cursor {
        pairs.push(("occurrence_cursor", cursor.to_string()));
    }
    if let Some(cursor) = placement_cursor {
        pairs.push(("placement_cursor", cursor.to_string()));
    }
    client.get_json(token, &path, &pairs).await
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
    cursor: Option<&str>,
) -> ApiResult<ListResponse<PlacementTileRead>> {
    let path = format!("/v1/source-tiles/{id}/placements");
    let mut pairs: Vec<(&'static str, String)> = Vec::new();
    if let Some(l) = limit {
        pairs.push(("limit", l.to_string()));
    }
    if let Some(cursor) = cursor {
        pairs.push(("cursor", cursor.to_string()));
    }
    client.get_json(token, &path, &pairs).await
}

/// Fetch every placement page until Core returns no `next_cursor`.
pub async fn list_all_source_tile_placements(
    client: &ApiClient,
    token: &BearerToken,
    id: Uuid,
) -> ApiResult<Vec<PlacementTileRead>> {
    let mut out: Vec<PlacementTileRead> = Vec::new();
    let mut cursor: Option<String> = None;
    let mut seen = HashSet::new();
    loop {
        let page = list_source_tile_placements(
            client,
            token,
            id,
            Some(SOURCE_TILE_PAGE_SIZE),
            cursor.as_deref(),
        )
        .await?;
        if page.legacy_array && page.items.len() >= SOURCE_TILE_PAGE_SIZE as usize {
            return Err(crate::error::ApiError::Pagination(
                "legacy placement response filled the page; cursor pagination is required to prove completeness".into(),
            ));
        }
        out.extend(page.items);
        let Some(next_cursor) = page.next_cursor else {
            break;
        };
        if !seen.insert(next_cursor.clone()) {
            return Err(crate::error::ApiError::Pagination(
                "source-tile placements returned a repeated cursor".into(),
            ));
        }
        cursor = Some(next_cursor);
    }
    Ok(out)
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
    fn legacy_list_array_decodes_as_a_terminal_page() {
        let page: ListResponse<i32> = serde_json::from_str("[1,2,3]").unwrap();
        assert_eq!(page.items, [1, 2, 3]);
        assert_eq!(page.count, 3);
        assert_eq!(page.next_cursor, None);
    }

    #[test]
    fn cursor_list_decodes_opaque_next_cursor_and_page_count() {
        let page: ListResponse<i32> =
            serde_json::from_str(r#"{"items":[4,5],"count":2,"next_cursor":"opaque/keyset"}"#)
                .unwrap();
        assert_eq!(page.items, [4, 5]);
        assert_eq!(page.count, 2);
        assert_eq!(page.next_cursor.as_deref(), Some("opaque/keyset"));
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

    #[test]
    fn update_request_carries_expected_revision() {
        let payload = UpdateSourceTilePayload {
            tile: ScheduleTileDefinition {
                title: "t".to_string(),
                description: None,
                color: None,
                icon: None,
                external_id: None,
            },
            plan: SchedulePlanDefinition {
                role: 0,
                completion: serde_json::json!({}),
                planning: serde_json::json!({}),
                metrics: vec![],
                decisions: vec![],
                references: vec![],
            },
            flows: vec![],
            schedule: CreateSourceScheduleDefinition {
                generation: serde_json::json!({"kind": 0}),
                split_policy: serde_json::json!({"kind": 0}),
                window: serde_json::json!({"start_offset_ms": 0, "end_offset_ms": 1}),
                priority: 0,
                required_duration_ms: 1,
            },
            horizon: Span {
                start: "2026-01-01T00:00:00Z".parse().unwrap(),
                end: "2026-01-01T01:00:00Z".parse().unwrap(),
            },
            relations: None,
            source_client_local_id: None,
        };
        let req = UpdateSourceTileRequest::new(payload).with_expected_revision(42);
        let json = serde_json::to_value(&req).expect("serializes");
        assert_eq!(json["expected_revision"], 42);
        assert!(
            json["payload"]["relations"].is_null() || json["payload"].get("relations").is_none()
        );
    }

    #[test]
    fn detail_read_parses_definition_read_back() {
        let json = serde_json::json!({
            "source": {
                "source_tile_id": Uuid::nil(),
                "plan_id": Uuid::nil(),
                "owner_id": Uuid::nil(),
                "revision": 3,
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
            },
            "relations": [],
            "occurrences": [],
            "placements": [],
            "definition": {
                "tile": {"title": "demo"},
                "plan": {
                    "role": 0,
                    "completion": {"root": {"All": []}, "tasks": [], "time_requirements": []},
                    "planning": {"nesting_rules": [], "placement_rules": []},
                    "metrics": [],
                    "decisions": [],
                    "references": []
                },
                "flows": [],
                "relations": [],
                "source_client_local_id": null
            }
        });
        let parsed: SourceTileDetailRead = serde_json::from_value(json).expect("parses");
        let definition = parsed.definition.expect("definition read-back");
        assert_eq!(definition.tile.title, "demo");
        assert_eq!(definition.source_client_local_id, None);
    }
}
