//! `GET /v1/tiles` — list today's tiles as a view-model.
//!
//! Spec: `openapi/openapi.yaml` → `paths./v1/tiles.get` (operationId `list_tiles`).

use std::collections::HashSet;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::client::{ApiClient, BearerToken};
use crate::error::ApiResult;
use crate::source_tiles::{ListResponse, RecurrenceView, SourceTileSummary, TemporalView};

/// `TileListView` — rich tile-list view-model.
///
/// Spec: `openapi/openapi.yaml` → `components.schemas.TileListView`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TileListView {
    pub id: Uuid,
    pub title: String,
    pub lifecycle: i32,
    pub worked_minutes: i64,
    pub break_minutes: i64,
    #[serde(default)]
    pub labels: Vec<String>,
    pub objective_mode: i32,
    #[serde(default)]
    pub plan_id: Option<Uuid>,
    #[serde(default)]
    pub projected_next_start_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub done_definition: Option<String>,
    #[serde(default)]
    pub done_rule: Option<i32>,
    #[serde(default)]
    pub next_action: Option<String>,
    #[serde(default)]
    pub resume_note: Option<String>,
    #[serde(default)]
    pub recurrence: Option<RecurrenceView>,
    #[serde(default)]
    pub source: Option<SourceTileSummary>,
    #[serde(default)]
    pub target_rest_min: Option<i32>,
    #[serde(default)]
    pub target_work_min: Option<i32>,
    #[serde(default)]
    pub temporal: Option<TemporalView>,
}

/// Query parameters for `GET /v1/tiles`.
///
/// The pinned contract documents exactly these parameters; anything else
/// is ignored by the server, so the CLI never sends an undocumented one.
#[derive(Debug, Default, Clone)]
pub struct ListTilesQuery {
    /// Comma-separated owner ids. Defaults to the authenticated owner.
    pub owner_ids: Option<String>,
    /// Page size. The server default is 100 and the maximum is 1000;
    /// an out-of-range value is rejected with `422` (v1/14 §15-2), it is
    /// never silently clamped.
    pub limit: Option<i64>,
    /// Opaque keyset cursor returned by the previous page (v1/14 §15).
    pub cursor: Option<String>,
    /// `"flat"` (default) or `"by_state"`.
    pub view_mode: Option<String>,
    /// One of `ready` | `started` | `done` | `closed`.
    pub lifecycle: Option<String>,
    /// Range string for the `by_state` projection, e.g. `"7d"`.
    pub range: Option<String>,
    /// Granularity string, e.g. `"no_breaks,min_0m"`.
    pub granularity: Option<String>,
    /// Free-text search across title / description / labels.
    pub search: Option<String>,
    /// Exclude tiles whose first upcoming placement is strictly in the
    /// future — the server-side equivalent of "what is on today".
    pub exclude_future: Option<bool>,
}

impl ListTilesQuery {
    /// Encode as `(name, value)` pairs for `reqwest`'s `query()` method.
    fn to_pairs(&self) -> Vec<(&'static str, String)> {
        let mut out = Vec::new();
        if let Some(v) = &self.owner_ids {
            out.push(("owner_ids", v.clone()));
        }
        if let Some(v) = self.limit {
            out.push(("limit", v.to_string()));
        }
        if let Some(v) = &self.cursor {
            out.push(("cursor", v.clone()));
        }
        if let Some(v) = &self.view_mode {
            out.push(("view_mode", v.clone()));
        }
        if let Some(v) = &self.lifecycle {
            out.push(("lifecycle", v.clone()));
        }
        if let Some(v) = &self.range {
            out.push(("range", v.clone()));
        }
        if let Some(v) = &self.granularity {
            out.push(("granularity", v.clone()));
        }
        if let Some(v) = &self.search {
            out.push(("search", v.clone()));
        }
        if let Some(v) = self.exclude_future {
            out.push(("exclude_future", v.to_string()));
        }
        out
    }
}

/// A page size accepted by the pinned contract (`1..=1000`).
pub const TILE_LIST_PAGE_SIZE: i64 = 1000;

/// `GET /v1/tiles` → `operationId: list_tiles`.
///
/// Returns a single page of the `TileListResponse` envelope. Use
/// [`list_all_tiles`] to walk `next_cursor` to completion.
pub async fn list_tiles(
    client: &ApiClient,
    token: &BearerToken,
    q: &ListTilesQuery,
) -> ApiResult<ListResponse<TileListView>> {
    client.get_json(token, "/v1/tiles", &q.to_pairs()).await
}

/// Fetch every tile page until Core returns no `next_cursor`.
pub async fn list_all_tiles(
    client: &ApiClient,
    token: &BearerToken,
    q: &ListTilesQuery,
) -> ApiResult<Vec<TileListView>> {
    let mut out: Vec<TileListView> = Vec::new();
    let mut cursor = q.cursor.clone();
    let mut seen = HashSet::new();
    if let Some(first) = &cursor {
        seen.insert(first.clone());
    }
    loop {
        let page_query = ListTilesQuery {
            limit: Some(TILE_LIST_PAGE_SIZE),
            cursor,
            ..q.clone()
        };
        let page = list_tiles(client, token, &page_query).await?;
        if page.legacy_array && page.items.len() >= TILE_LIST_PAGE_SIZE as usize {
            return Err(crate::error::ApiError::Pagination(
                "legacy tile-list response filled the page; cursor pagination is required to prove completeness".into(),
            ));
        }
        out.extend(page.items);
        let Some(next_cursor) = page.next_cursor else {
            break;
        };
        if !seen.insert(next_cursor.clone()) {
            return Err(crate::error::ApiError::Pagination(
                "tile list returned a repeated cursor".into(),
            ));
        }
        cursor = Some(next_cursor);
    }
    Ok(out)
}
