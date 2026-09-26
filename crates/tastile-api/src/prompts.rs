//! Prompt operations.
//!
//! Spec surface in this module:
//! - `GET  /v1/prompts/pending`        → `list_pending_prompts`
//! - `POST /v1/prompts`                → `request_prompt`
//! - `POST /v1/prompts/{prompt_id}/resolve` → `resolve_prompt`
//! - `POST /v1/prompts/startup-recovery`    → `respond_startup_recovery`

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::client::{ApiClient, BearerToken};
use crate::error::ApiResult;
use crate::model::CommandResponse;

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

/// `GET /v1/prompts/pending` → `operationId: list_pending_prompts`.
pub async fn list_pending_prompts(
    client: &ApiClient,
    token: &BearerToken,
) -> ApiResult<Vec<PromptView>> {
    client.get_json(token, "/v1/prompts/pending", &[]).await
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
    pub prompt_kind: i32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

impl RequestPromptRequest {
    pub fn new(prompt_kind: i32) -> Self {
        Self {
            idempotency_key: Uuid::new_v4(),
            payload: RequestPromptPayload {
                prompt_kind,
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
