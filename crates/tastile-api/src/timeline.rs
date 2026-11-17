//! `GET /v1/timeline` — placements intersecting a required time range
//! (v1/14 §4, §11-0).
//!
//! The endpoint returns a bare JSON array ordered by
//! `span.start ASC, placement_id ASC`. `include_labels=true` adds the
//! non-blocking LABEL (`role = 1`) placements; they must be rendered in a
//! separate lane and never count as busy time.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{ApiClient, ApiResult, BearerToken};

/// Spec: `components.schemas.TimelineSpanSchema`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimelineSpan {
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
}

/// Spec: `components.schemas.TimelineContentSchema`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimelineContent {
    pub title: String,
    #[serde(default)]
    pub description: Option<String>,
}

/// Spec: `components.schemas.TimelineVisualSchema`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimelineVisual {
    #[serde(default)]
    pub color: Option<String>,
    #[serde(default)]
    pub icon: Option<String>,
}

/// Spec: `components.schemas.TimelineSourceSchema`. `kind` is the numeric
/// `PlacementSource` constant (0 MANUAL / 1 RECURRING / 2 FLOW / 3 IMPORT /
/// 4 SOURCE).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimelineSource {
    pub kind: i32,
    pub detail: String,
}

/// Spec: `components.schemas.TimelineViolationSchema`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimelineViolation {
    pub id: Uuid,
    /// Numeric `ViolationKind` constant.
    pub kind: i32,
    #[serde(default)]
    pub key: Option<serde_json::Value>,
    #[serde(default)]
    pub related_id: Option<Uuid>,
}

/// Spec: `components.schemas.TimelineResolutionSchema`. `state` is the
/// numeric `ResolutionState` constant (0 OPEN / 1 CLOSED / 2 BLOCKED).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimelineResolution {
    pub state: i32,
    pub resolved_at: DateTime<Utc>,
    pub resolution_hash: Uuid,
    #[serde(default)]
    pub violations: Vec<TimelineViolation>,
}

/// Spec: `components.schemas.TimelineInsideSchema`. `scope` is the numeric
/// `ScopeKind` constant (0 ROOT / 1 HOST / 2 GAP).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimelineInside {
    pub parent: Uuid,
    pub scope: i32,
}

/// Spec: `components.schemas.TimelineOwnerRefSchema`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimelineOwnerRef {
    pub kind: i32,
    pub id: Uuid,
}

/// Spec: `components.schemas.TimelineOwnerProfileSchema`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimelineOwnerProfile {
    pub display_name: String,
    #[serde(default)]
    pub avatar_url: Option<String>,
}

/// Spec: `components.schemas.TimelineOwnerEmbedSchema`. The wire field is
/// literally `ref_`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimelineOwnerEmbed {
    pub ref_: TimelineOwnerRef,
    pub profile: TimelineOwnerProfile,
}

/// Spec: `components.schemas.TimelineItemSchema`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimelineItem {
    pub placement_id: Uuid,
    pub revision: i64,
    pub tile_id: Uuid,
    pub content: TimelineContent,
    pub visual: TimelineVisual,
    /// Numeric `PlanRole` constant (0 EXECUTABLE / 1 LABEL).
    pub role: i32,
    pub span: TimelineSpan,
    pub source: TimelineSource,
    pub resolution: TimelineResolution,
    #[serde(default)]
    pub inside: Option<TimelineInside>,
    #[serde(default)]
    pub occurrence_id: Option<Uuid>,
    #[serde(default)]
    pub source_tile_id: Option<Uuid>,
    #[serde(default)]
    pub split_index: Option<i32>,
    #[serde(default)]
    pub split_count: Option<i32>,
    #[serde(default)]
    pub split_group_id: Option<Uuid>,
    #[serde(default)]
    pub owner: Option<TimelineOwnerEmbed>,
}

/// Query for `GET /v1/timeline`. The window may span at most 31 days.
#[derive(Debug, Clone)]
pub struct TimelineQuery {
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
    pub parent_placement_id: Option<Uuid>,
    pub include_labels: bool,
    pub include_closed: bool,
    pub include_blocked: bool,
    pub include_nested: bool,
    /// Comma-separated on the wire; empty means the caller's own scope.
    pub owner_ids: Vec<Uuid>,
    pub limit: Option<u32>,
    pub offset: Option<u32>,
    /// `month` collapses overlapping relation-derived items.
    pub summary_month: bool,
    pub min_duration_ms: Option<i64>,
    pub min_recurring_step_ms: Option<i64>,
    /// Numeric `PlacementSource` values to exclude.
    pub exclude_source_kinds: Vec<i32>,
}

impl TimelineQuery {
    fn to_pairs(&self) -> Vec<(&'static str, String)> {
        let mut out = Vec::new();
        out.push(("start", self.start.to_rfc3339()));
        out.push(("end", self.end.to_rfc3339()));
        if let Some(id) = self.parent_placement_id {
            out.push(("parent_placement_id", id.to_string()));
        }
        for (flag, enabled) in [
            ("include_labels", self.include_labels),
            ("include_closed", self.include_closed),
            ("include_blocked", self.include_blocked),
            ("include_nested", self.include_nested),
        ] {
            if enabled {
                out.push((flag, "true".to_string()));
            }
        }
        if !self.owner_ids.is_empty() {
            out.push((
                "owner_ids",
                self.owner_ids
                    .iter()
                    .map(Uuid::to_string)
                    .collect::<Vec<_>>()
                    .join(","),
            ));
        }
        if let Some(limit) = self.limit {
            out.push(("limit", limit.to_string()));
        }
        if let Some(offset) = self.offset {
            out.push(("offset", offset.to_string()));
        }
        if self.summary_month {
            out.push(("summary", "month".to_string()));
        }
        if let Some(ms) = self.min_duration_ms {
            out.push(("min_duration_ms", ms.to_string()));
        }
        if let Some(ms) = self.min_recurring_step_ms {
            out.push(("min_recurring_step_ms", ms.to_string()));
        }
        if !self.exclude_source_kinds.is_empty() {
            out.push((
                "exclude_source_kinds",
                self.exclude_source_kinds
                    .iter()
                    .map(i32::to_string)
                    .collect::<Vec<_>>()
                    .join(","),
            ));
        }
        out
    }
}

/// `GET /v1/timeline` → `operationId: get_timeline`.
///
/// The response is a bare JSON array (no cursor pagination contract); an
/// omitted `limit` returns the complete in-range set.
pub async fn get_timeline(
    client: &ApiClient,
    token: &BearerToken,
    query: &TimelineQuery,
) -> ApiResult<Vec<TimelineItem>> {
    client
        .get_json(token, "/v1/timeline", &query.to_pairs())
        .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_pairs_omit_default_flags() {
        let query = TimelineQuery {
            start: "2026-11-20T00:00:00Z".parse().unwrap(),
            end: "2026-11-21T00:00:00Z".parse().unwrap(),
            parent_placement_id: None,
            include_labels: false,
            include_closed: false,
            include_blocked: false,
            include_nested: false,
            owner_ids: vec![],
            limit: None,
            offset: None,
            summary_month: false,
            min_duration_ms: None,
            min_recurring_step_ms: None,
            exclude_source_kinds: vec![],
        };
        let pairs = query.to_pairs();
        assert_eq!(pairs.len(), 2);
        assert!(pairs.iter().all(|(k, _)| *k == "start" || *k == "end"));
    }

    #[test]
    fn timeline_item_parses_label_and_blocked() {
        let item: TimelineItem = serde_json::from_value(serde_json::json!({
            "placement_id": Uuid::nil(),
            "revision": 1,
            "tile_id": Uuid::nil(),
            "content": {"title": "label", "description": null},
            "visual": {"color": null, "icon": null},
            "role": 1,
            "span": {"start": "2026-11-20T09:00:00Z", "end": "2026-11-20T09:30:00Z"},
            "source": {"kind": 1, "detail": "recurring"},
            "resolution": {
                "state": 2,
                "resolved_at": "2026-11-20T00:00:00Z",
                "resolution_hash": Uuid::nil(),
                "violations": [{"id": Uuid::nil(), "kind": 7, "related_id": null}]
            }
        }))
        .expect("parses");
        assert_eq!(item.role, 1);
        assert_eq!(item.resolution.state, 2);
        assert_eq!(item.resolution.violations[0].kind, 7);
    }
}
