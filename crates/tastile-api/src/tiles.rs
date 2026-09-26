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

/// Query parameters for `GET /v1/tiles`. Empty `window_start` / `window_end`
/// cause the server to use the default window (the current local day).
#[derive(Debug, Default, Clone)]
pub struct ListTilesQuery {
    /// Inclusive lower bound for the tile's projected start time. ISO-8601.
    pub window_start: Option<DateTime<Utc>>,
    /// Exclusive upper bound. ISO-8601.
    pub window_end: Option<DateTime<Utc>>,
    /// Filter by plan id.
    pub plan_id: Option<Uuid>,
    /// `true` to include closed/done tiles.
    pub include_closed: Option<bool>,
}

impl ListTilesQuery {
    /// Encode as `(name, value)` pairs for `reqwest`'s `query()` method.
    fn to_pairs(&self) -> Vec<(&'static str, String)> {
        let mut out = Vec::new();
        if let Some(v) = self.window_start {
            out.push(("window_start", v.to_rfc3339()));
        }
        if let Some(v) = self.window_end {
            out.push(("window_end", v.to_rfc3339()));
        }
        if let Some(v) = self.plan_id {
            out.push(("plan_id", v.to_string()));
        }
        if let Some(v) = self.include_closed {
            out.push(("include_closed", v.to_string()));
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
