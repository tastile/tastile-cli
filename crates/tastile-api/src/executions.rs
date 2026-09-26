//! Execution lifecycle operations.
//!
//! Spec surface in this module:
//! - `POST /v1/placements/{id}/executions` → `start_execution`
//! - `POST /v1/executions/{id}/pause`     → `pause_execution`
//! - `POST /v1/executions/{id}/resume`    → `resume_execution`
//! - `POST /v1/executions/{id}/finish`    → `finish_execution`
//!
//! Each operation here corresponds 1:1 to an `operationId` in the pinned
//! `openapi/openapi.yaml`. The drift gate in `build.rs` enforces that those
//! operationIds still exist with the right method, path, body schema,
//! response schema, path parameters, and required envelope fields.

use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

use crate::client::{ApiClient, BearerToken};
use crate::error::ApiResult;
use crate::model::CommandResponse;

// ---------------------------------------------------------------------------
// Start.
// ---------------------------------------------------------------------------

/// Spec: `components.schemas.StartExecutionPayloadSchema`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StartExecutionPayload {
    pub placement_id: Uuid,
}

/// Spec: `components.schemas.StartExecutionRequest`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StartExecutionRequest {
    pub idempotency_key: Uuid,
    pub payload: StartExecutionPayload,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub occurred_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_revision: Option<i64>,
}

impl StartExecutionRequest {
    pub fn new(placement_id: Uuid) -> Self {
        Self {
            idempotency_key: Uuid::new_v4(),
            payload: StartExecutionPayload { placement_id },
            occurred_at: None,
            owner_id: None,
            expected_revision: None,
        }
    }
}

/// `POST /v1/placements/{id}/executions` → `operationId: start_execution`.
pub async fn start_execution(
    client: &ApiClient,
    token: &BearerToken,
    placement_id: Uuid,
    req: &StartExecutionRequest,
) -> ApiResult<CommandResponse> {
    let path = format!("/v1/placements/{placement_id}/executions");
    client.post_json(token, &path, req).await
}

// ---------------------------------------------------------------------------
// Pause / Resume — share `ExecutionLifecycleRequest`.
// ---------------------------------------------------------------------------

/// Spec: `components.schemas.ExecutionLifecycleRequest`. PAUSE / RESUME take
/// no payload of their own — the execution id is in the path, and `payload`
/// is `null`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ExecutionLifecycleRequest {
    pub idempotency_key: Uuid,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub occurred_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_revision: Option<i64>,
}

impl ExecutionLifecycleRequest {
    pub fn new() -> Self {
        Self {
            idempotency_key: Uuid::new_v4(),
            occurred_at: None,
            expected_revision: None,
        }
    }
}

impl Default for ExecutionLifecycleRequest {
    fn default() -> Self {
        Self::new()
    }
}

/// `POST /v1/executions/{id}/pause` → `operationId: pause_execution`.
pub async fn pause_execution(
    client: &ApiClient,
    token: &BearerToken,
    execution_id: Uuid,
    req: &ExecutionLifecycleRequest,
) -> ApiResult<CommandResponse> {
    let path = format!("/v1/executions/{execution_id}/pause");
    client.post_json(token, &path, req).await
}

/// `POST /v1/executions/{id}/resume` → `operationId: resume_execution`.
pub async fn resume_execution(
    client: &ApiClient,
    token: &BearerToken,
    execution_id: Uuid,
    req: &ExecutionLifecycleRequest,
) -> ApiResult<CommandResponse> {
    let path = format!("/v1/executions/{execution_id}/resume");
    client.post_json(token, &path, req).await
}

// ---------------------------------------------------------------------------
// Finish.
// ---------------------------------------------------------------------------

/// Spec: `components.schemas.FinishExecutionPayloadSchema`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FinishExecutionPayload {
    /// Numeric `ExecutionFinish` kind (opaque until the v1.1 registry
    /// publishes the values).
    pub kind: i32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// Spec: `components.schemas.FinishExecutionRequest`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FinishExecutionRequest {
    pub idempotency_key: Uuid,
    pub payload: FinishExecutionPayload,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub occurred_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_revision: Option<i64>,
}

impl FinishExecutionRequest {
    pub fn new(kind: i32, note: Option<String>) -> Self {
        Self {
            idempotency_key: Uuid::new_v4(),
            payload: FinishExecutionPayload { kind, note },
            occurred_at: None,
            expected_revision: None,
        }
    }
}

/// `POST /v1/executions/{id}/finish` → `operationId: finish_execution`.
pub async fn finish_execution(
    client: &ApiClient,
    token: &BearerToken,
    execution_id: Uuid,
    req: &FinishExecutionRequest,
) -> ApiResult<CommandResponse> {
    let path = format!("/v1/executions/{execution_id}/finish");
    client.post_json(token, &path, req).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn start_request_carries_placement_id() {
        let pid = Uuid::new_v4();
        let req = StartExecutionRequest::new(pid);
        let json = serde_json::to_value(&req).expect("serializes");
        assert_eq!(json["payload"]["placement_id"], pid.to_string());
        assert!(json["idempotency_key"].is_string());
    }

    #[test]
    fn lifecycle_request_has_no_payload_field() {
        let req = ExecutionLifecycleRequest::new();
        let json = serde_json::to_value(&req).expect("serializes");
        assert!(json["idempotency_key"].is_string());
        assert!(json.get("payload").is_none());
    }

    #[test]
    fn finish_request_carries_kind_and_optional_note() {
        let req = FinishExecutionRequest::new(2, Some("done".into()));
        let json = serde_json::to_value(&req).expect("serializes");
        assert_eq!(json["payload"]["kind"], 2);
        assert_eq!(json["payload"]["note"], "done");
    }
}
