//! Prompt operations.
//!
//! Spec surface in this module:
//! - `GET  /v1/prompts/pending`        → `list_pending_prompts`
//! - `POST /v1/prompts`                → `request_prompt`
//! - `POST /v1/prompts/{prompt_id}/resolve` → `resolve_prompt`
//! - `POST /v1/prompts/startup-recovery`    → `respond_startup_recovery`

use std::collections::HashSet;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::client::{ApiClient, BearerToken};
use crate::error::ApiResult;
use crate::model::CommandResponse;
use crate::source_tiles::ListResponse;

/// Spec: `components.schemas.PromptSubjectSchema` — opaque JSON.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PromptSubject(pub serde_json::Value);

/// Spec: `components.schemas.PromptView`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PromptView {
    pub id: Uuid,
    /// Numeric `PromptKind` (v1/14 §13-3).
    pub kind: i32,
    pub created_at: DateTime<Utc>,
    #[serde(default)]
    pub expires_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub note: Option<String>,
    #[serde(default)]
    pub subject: Option<PromptSubject>,
}

/// Query parameters for `GET /v1/prompts/pending`.
///
/// The pinned spec predates the §15 cursor migration, so these parameters
/// are not yet documented there; Core already accepts `limit` / `cursor`
/// and rejects `offset` (v1/14 §15-2).
#[derive(Debug, Default, Clone)]
pub struct ListPendingPromptsQuery {
    /// Page size. The server default is 100 and the maximum is 1000;
    /// an out-of-range value is rejected with `422`, never clamped.
    pub limit: Option<i64>,
    /// Opaque keyset cursor returned by the previous page.
    pub cursor: Option<String>,
}

impl ListPendingPromptsQuery {
    fn to_pairs(&self) -> Vec<(&'static str, String)> {
        let mut out = Vec::new();
        if let Some(v) = self.limit {
            out.push(("limit", v.to_string()));
        }
        if let Some(v) = &self.cursor {
            out.push(("cursor", v.clone()));
        }
        out
    }
}

/// A page size accepted by the §15 cursor contract (`1..=1000`).
pub const PENDING_PROMPTS_PAGE_SIZE: i64 = 1000;

/// `GET /v1/prompts/pending` → `operationId: list_pending_prompts`.
///
/// Returns a single page of the §15 `ListResponse` envelope. Use
/// [`list_all_pending_prompts`] to walk `next_cursor` to completion. The
/// untagged decoder also accepts the pre-cursor bare-array response while
/// the OpenAPI pin predates the migration.
pub async fn list_pending_prompts(
    client: &ApiClient,
    token: &BearerToken,
    q: &ListPendingPromptsQuery,
) -> ApiResult<ListResponse<PromptView>> {
    client
        .get_json(token, "/v1/prompts/pending", &q.to_pairs())
        .await
}

/// Fetch every pending-prompt page until Core returns no `next_cursor`.
///
/// A legacy bare-array response is already the complete set: the
/// pre-cursor endpoint returned all unresolved prompts without a cap, so
/// there is nothing further to page through.
pub async fn list_all_pending_prompts(
    client: &ApiClient,
    token: &BearerToken,
    q: &ListPendingPromptsQuery,
) -> ApiResult<Vec<PromptView>> {
    let mut out: Vec<PromptView> = Vec::new();
    let mut cursor = q.cursor.clone();
    let mut seen = HashSet::new();
    if let Some(first) = &cursor {
        seen.insert(first.clone());
    }
    loop {
        let page_query = ListPendingPromptsQuery {
            limit: Some(PENDING_PROMPTS_PAGE_SIZE),
            cursor,
        };
        let page = list_pending_prompts(client, token, &page_query).await?;
        out.extend(page.items);
        if page.legacy_array {
            break;
        }
        let Some(next_cursor) = page.next_cursor else {
            break;
        };
        if !seen.insert(next_cursor.clone()) {
            return Err(crate::error::ApiError::Pagination(
                "pending-prompt list returned a repeated cursor".into(),
            ));
        }
        cursor = Some(next_cursor);
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// request_prompt
// ---------------------------------------------------------------------------

/// Spec: `components.schemas.RequestPromptRequest`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RequestPromptRequest {
    pub idempotency_key: Uuid,
    pub payload: RequestPromptPayload,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub occurred_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_revision: Option<i64>,
}

/// Spec: `components.schemas.RequestPromptPayloadSchema`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RequestPromptPayload {
    /// Numeric `PromptKind` — 0 STARTUP_RECOVERY / 1 MISSED_START /
    /// 2 DECISION_REQUIRED / 3 STALE_ACTIVE_EXECUTION.
    pub kind: i32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

impl RequestPromptRequest {
    pub fn new(prompt_kind: i32) -> Self {
        Self {
            idempotency_key: Uuid::new_v4(),
            payload: RequestPromptPayload {
                kind: prompt_kind,
                note: None,
            },
            occurred_at: None,
            expected_revision: None,
        }
    }
}

/// `POST /v1/prompts` → `operationId: request_prompt`.
pub async fn request_prompt(
    client: &ApiClient,
    token: &BearerToken,
    req: &RequestPromptRequest,
) -> ApiResult<CommandResponse> {
    client.post_json(token, "/v1/prompts", req).await
}

// ---------------------------------------------------------------------------
// resolve_prompt
// ---------------------------------------------------------------------------

/// Spec: `components.schemas.ResolvePromptRequest`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ResolvePromptRequest {
    pub idempotency_key: Uuid,
    pub payload: ResolvePromptPayload,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub occurred_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_revision: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_id: Option<Uuid>,
}

/// Spec: `components.schemas.ResolvePromptPayloadSchema`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ResolvePromptPayload {
    /// Numeric `PromptResolution` — 0 ACKNOWLEDGED / 1 DISMISSED / 2 ACTION_TAKEN.
    pub resolution: i32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response_note: Option<String>,
}

impl ResolvePromptRequest {
    pub fn new(resolution: i32, response_note: Option<String>) -> Self {
        Self {
            idempotency_key: Uuid::new_v4(),
            payload: ResolvePromptPayload {
                resolution,
                response_note,
            },
            occurred_at: None,
            expected_revision: None,
            owner_id: None,
        }
    }
}

/// `POST /v1/prompts/{prompt_id}/resolve` → `operationId: resolve_prompt`.
pub async fn resolve_prompt(
    client: &ApiClient,
    token: &BearerToken,
    prompt_id: Uuid,
    req: &ResolvePromptRequest,
) -> ApiResult<CommandResponse> {
    let path = format!("/v1/prompts/{prompt_id}/resolve");
    client.post_json(token, &path, req).await
}

// ---------------------------------------------------------------------------
// respond_startup_recovery
// ---------------------------------------------------------------------------

/// Spec: `components.schemas.StartupRecoveryRequest`. The prompt id is in
/// the payload because `POST /v1/prompts/startup-recovery` has no path
/// parameter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StartupRecoveryRequest {
    pub idempotency_key: Uuid,
    pub payload: StartupRecoveryPayload,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub occurred_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_revision: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_id: Option<Uuid>,
}

/// Spec: `components.schemas.StartupRecoveryPayloadSchema`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StartupRecoveryPayload {
    pub prompt_id: Uuid,
    /// Numeric `PromptResolution` — 0 ACKNOWLEDGED / 1 DISMISSED / 2 ACTION_TAKEN.
    pub resolution: i32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response_note: Option<String>,
}

impl StartupRecoveryRequest {
    pub fn new(prompt_id: Uuid, resolution: i32, response_note: Option<String>) -> Self {
        Self {
            idempotency_key: Uuid::new_v4(),
            payload: StartupRecoveryPayload {
                prompt_id,
                resolution,
                response_note,
            },
            occurred_at: None,
            expected_revision: None,
            owner_id: None,
        }
    }
}

/// `POST /v1/prompts/startup-recovery` → `operationId: respond_startup_recovery`.
pub async fn respond_startup_recovery(
    client: &ApiClient,
    token: &BearerToken,
    req: &StartupRecoveryRequest,
) -> ApiResult<CommandResponse> {
    client
        .post_json(token, "/v1/prompts/startup-recovery", req)
        .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_request_serializes() {
        let req = ResolvePromptRequest::new(0, None);
        let json = serde_json::to_value(&req).expect("serializes");
        assert!(json["idempotency_key"].is_string());
        assert_eq!(json["payload"]["resolution"], 0);
    }

    #[test]
    fn startup_recovery_request_serializes() {
        let pid = Uuid::nil();
        let req = StartupRecoveryRequest::new(pid, 2, Some("ack".into()));
        let json = serde_json::to_value(&req).expect("serializes");
        assert_eq!(json["payload"]["prompt_id"], pid.to_string());
        assert_eq!(json["payload"]["resolution"], 2);
        assert_eq!(json["payload"]["response_note"], "ack");
    }
}
