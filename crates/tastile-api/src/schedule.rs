//! Publish a schedule definition through the canonical v1 HTTP endpoint.
//!
//! The payload remains JSON because the OpenAPI contract deliberately contains
//! opaque `recurrence`, `windows` and `reference_targets` values. We validate
//! the documented required fields before an API request and never infer or
//! silently replace unsupported calendar semantics.

use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::Value;
use uuid::Uuid;

use crate::client::{ApiClient, BearerToken};
use crate::error::ApiResult;
use crate::model::CommandResponse;

/// `PublishScheduleDefinitionRequest` in the pinned OpenAPI specification.
#[derive(Debug, Clone, Serialize)]
pub struct PublishScheduleDefinitionRequest {
    pub idempotency_key: Uuid,
    pub payload: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub occurred_at: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expected_revision: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owner_id: Option<Uuid>,
}

impl PublishScheduleDefinitionRequest {
    pub fn new(payload: Value, idempotency_key: Uuid) -> Result<Self, String> {
        validate_publish_payload(&payload)?;
        Ok(Self {
            idempotency_key,
            payload,
            occurred_at: None,
            expected_revision: None,
            owner_id: None,
        })
    }

    /// Attach the revision the caller expects to still be current. The
    /// server answers `409` when the stored revision moved on.
    #[must_use]
    pub fn with_expected_revision(mut self, revision: i64) -> Self {
        self.expected_revision = Some(revision);
        self
    }
}

/// Validate the required OpenAPI shape without pretending to implement Core's
/// scheduling semantics. Detailed validation remains server-side.
///
/// Reject unknown fields instead of allowing a misspelled calendar rule to be
/// silently discarded as an entirely different schedule.
pub fn validate_publish_payload(payload: &Value) -> Result<(), String> {
    let obj = payload
        .as_object()
        .ok_or_else(|| "schedule payload must be a JSON object".to_string())?;
    let allowed = [
        "tile",
        "plan",
        "reference_targets",
        "windows",
        "flows",
        "recurrence",
        "relations",
        "source_client_local_id",
        "source_horizon",
        "source_schedule",
    ];
    for key in obj.keys() {
        if !allowed.contains(&key.as_str()) {
            return Err(format!(
                "unknown schedule payload field `{key}` (refusing to ignore it)"
            ));
        }
    }
    let tile = obj
        .get("tile")
        .and_then(Value::as_object)
        .ok_or_else(|| "schedule payload requires object field `tile`".to_string())?;
    if tile
        .get("title")
        .and_then(Value::as_str)
        .is_none_or(|s| s.trim().is_empty())
    {
        return Err("schedule payload requires non-empty `tile.title`".into());
    }
    if !obj.get("plan").is_some_and(Value::is_object) {
        return Err("schedule payload requires object field `plan`".into());
    }
    // `SchedulePlanDefinitionSchema` requires the plan to carry the whole
    // plan body; the server rejects a partial plan.
    let plan = obj
        .get("plan")
        .and_then(Value::as_object)
        .expect("checked above");
    for (key, kind) in [
        ("references", "array"),
        ("metrics", "array"),
        ("decisions", "array"),
        ("completion", "object"),
        ("planning", "object"),
    ] {
        let present = plan.get(key).is_some_and(|value| match kind {
            "array" => value.is_array(),
            _ => value.is_object(),
        });
        if !present {
            return Err(format!(
                "schedule payload requires plan field `{key}` ({kind})"
            ));
        }
    }
    for key in ["reference_targets", "windows", "flows"] {
        if !obj.get(key).is_some_and(Value::is_array) {
            return Err(format!("schedule payload requires array field `{key}`"));
        }
    }
    let linked_source_fields = [
        "source_client_local_id",
        "source_schedule",
        "source_horizon",
    ];
    let present: Vec<bool> = linked_source_fields
        .iter()
        .map(|field| obj.get(*field).is_some_and(|value| !value.is_null()))
        .collect();
    if present.iter().any(|value| *value) && present.iter().any(|value| !*value) {
        return Err(
            "Core requires `source_client_local_id`, `source_schedule`, and `source_horizon` together"
                .into(),
        );
    }
    Ok(())
}

/// `POST /v1/schedule-definitions` (operationId: publish_schedule_definition).
pub async fn publish_schedule_definition(
    client: &ApiClient,
    token: &BearerToken,
    request: &PublishScheduleDefinitionRequest,
) -> ApiResult<CommandResponse> {
    client
        .post_json(token, "/v1/schedule-definitions", request)
        .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn valid_payload() -> Value {
        json!({
            "tile": {"title": "Local test"},
            "plan": {
                "role": 0,
                "references": [],
                "completion": {"root": {"All": []}, "tasks": [], "time_requirements": []},
                "planning": {"placement_rules": [], "nesting_rules": []},
                "metrics": [],
                "decisions": []
            },
            "reference_targets": [],
            "windows": [],
            "flows": []
        })
    }

    #[test]
    fn accepts_minimum_openapi_required_fields() {
        assert!(validate_publish_payload(&valid_payload()).is_ok());
    }

    #[test]
    fn fails_closed_on_misspelled_monthly_rule_or_missing_fields() {
        let mut p = valid_payload();
        p["monthy_rule"] = json!({"by_day": {"day_of_month": 15}});
        assert!(
            validate_publish_payload(&p)
                .unwrap_err()
                .contains("monthy_rule")
        );
        p.as_object_mut().unwrap().remove("monthy_rule");
        p.as_object_mut().unwrap().remove("windows");
        assert!(
            validate_publish_payload(&p)
                .unwrap_err()
                .contains("windows")
        );
    }

    #[test]
    fn fails_closed_on_an_incomplete_plan() {
        let mut p = valid_payload();
        p["plan"]
            .as_object_mut()
            .expect("plan object")
            .remove("references");
        assert!(
            validate_publish_payload(&p)
                .unwrap_err()
                .contains("references")
        );
    }

    #[test]
    fn source_definition_fields_must_be_supplied_together() {
        let mut p = valid_payload();
        p["source_schedule"] = json!({"generation": {"kind": 0}});
        assert!(
            validate_publish_payload(&p).unwrap_err().contains(
                "source_client_local_id`, `source_schedule`, and `source_horizon` together"
            )
        );
    }

    #[test]
    fn request_serializes_idempotency_and_payload_without_optional_nulls() {
        let key = Uuid::parse_str("aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee").unwrap();
        let request = PublishScheduleDefinitionRequest::new(valid_payload(), key).unwrap();
        let body = serde_json::to_value(&request).unwrap();
        assert_eq!(body["idempotency_key"], key.to_string());
        assert_eq!(body["payload"]["tile"]["title"], "Local test");
        assert!(body.get("owner_id").is_none());
        assert!(body.get("occurred_at").is_none());
    }
}
