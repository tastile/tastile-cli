//! `GET /v1/tiles` — list today's tiles as a view-model.
//!
//! Spec: `openapi/openapi.yaml` → `paths./v1/tiles.get` (operationId `list_tiles`).

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::client::{ApiClient, BearerToken};
use crate::error::ApiResult;
use crate::source_tiles::{RecurrenceView, SourceTileSummary, TemporalView};

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
    /// Maximum rows. The server clamps this to its own maximum.
    pub limit: Option<i64>,
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

/// `GET /v1/tiles`.
///
/// Spec: `paths./v1/tiles.get` → `operationId: list_tiles`.
pub async fn list_tiles(
    client: &ApiClient,
    token: &BearerToken,
    q: &ListTilesQuery,
) -> ApiResult<Vec<TileListView>> {
    client.get_json(token, "/v1/tiles", &q.to_pairs()).await
}
