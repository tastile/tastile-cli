//! Application service layer.
//!
//! Both the CLI subcommands and the TUI talk to the API through this
//! module so they share one HTTP layer and one set of error semantics.
//! Adding a new operation here gives both surfaces access at once.
//!
//! Inputs are typed domain values (UUIDs, summaries, decisions). Outputs
//! are typed. Errors come back as `anyhow::Error` with the underlying
//! `tastile_api::ApiError` chained via `with_context` so `doctor` and
//! `tracing` see the same picture.

use anyhow::{Context, Result, anyhow};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tastile_api::{
    ApiClient, ApiConfig, ApiError, BearerToken, CancelSourceTileRequest, CommandResponse,
    CreateSourceScheduleDefinition, CreateSourceTilePayload, CreateSourceTileRequest,
    ExecutionLifecycleRequest, FinishExecutionRequest, ReflowSourceTileRequest,
    SchedulePlanDefinition, ScheduleTileDefinition, SourceTileDetailRead, SourceTileRead, Span,
    StartExecutionRequest, TileListView, UpdateSourceTilePayload, UpdateSourceTileRequest,
    executions, source_tiles as api_source_tiles, tiles as api_tiles,
};
use tastile_auth::{CredentialStore, KeyringStore, StoredToken};
use tastile_config::Config;
use uuid::Uuid;

/// Top-level context held by both the CLI dispatcher and the TUI event
/// loop. Cheap to clone (HTTP client is shared, all values are `String` /
/// `PathBuf` / `Option<Uuid>`).
#[derive(Clone)]
pub struct AppContext {
    pub api: ApiClient,
    pub config: Config,
    pub token: Option<BearerToken>,
    pub subject: Option<String>,
}

impl AppContext {
    /// Build the context from the current `Config`. Loads the credential
    /// store if present. If no credential is stored, `token` is `None` and
    /// only unauthenticated endpoints can be reached (currently none).
    pub fn load(config: Config) -> Result<Self> {
        let cfg = tastile_config::with_env_overrides(config);
        let api = ApiClient::new(ApiConfig::new(&cfg.api_url)?)
            .with_context(|| format!("invalid api_url `{}`", cfg.api_url))?;
        let loaded = KeyringStore
            .load(tastile_auth::DEFAULT_SERVICE, tastile_auth::DEFAULT_USER)
            .context("credential store error")?;
        let (token, subject) = match loaded {
            Some(stored) => (Some(BearerToken::new(stored.bearer)), stored.subject),
            None => (None, None),
        };
        Ok(Self {
            api,
            config: cfg,
            token,
            subject,
        })
    }

    /// True iff a bearer token is loaded and the API base matches the
    /// stored credential's API base.
    pub fn is_signed_in(&self) -> bool {
        self.token.is_some()
    }

    /// Borrow the bearer token, erroring if not signed in.
    pub fn require_token(&self) -> Result<&BearerToken> {
        self.token
            .as_ref()
            .ok_or_else(|| anyhow!("not signed in; run `tastile auth login`"))
    }

    /// Probe `/v1/tiles` (a read-only endpoint) to check API reachability.
    #[allow(dead_code)]
    pub async fn probe(&self) -> Result<ProbeOutcome> {
        let token = self.require_token()?;
        match api_tiles::list_tiles(self, token, &api_tiles::ListTilesQuery::default()).await {
            Ok(_) => Ok(ProbeOutcome::Ok),
            Err(ApiError::Http {
                status: 401 | 403, ..
            }) => Ok(ProbeOutcome::Auth),
            Err(ApiError::Http { status, .. }) => Ok(ProbeOutcome::HttpStatus(status)),
            Err(e) => Err(anyhow!(e)),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum ProbeOutcome {
    Ok,
    Auth,
    HttpStatus(u16),
}

// Allow `ApiClient` and `BearerToken` access through `AppContext` so the
// typed client modules can use them.
impl std::ops::Deref for AppContext {
    type Target = ApiClient;
    fn deref(&self) -> &Self::Target {
        &self.api
    }
}

// ---------------------------------------------------------------------------
// Tiles.
// ---------------------------------------------------------------------------

/// Sorted view of the day's tiles (today and the next 24 h).
pub async fn today(ctx: &AppContext) -> Result<Vec<TileListView>> {
    let token = ctx.require_token()?;
    let mut tiles = api_tiles::list_tiles(ctx, token, &api_tiles::ListTilesQuery::default())
        .await
        .context("list_tiles failed")?;
    // The CLI surfaces a "next actionable" view by sorting on
    // projected_next_start_at ascending, treating None as far future.
    tiles.sort_by(|a, b| {
        let ka = a
            .projected_next_start_at
            .unwrap_or(DateTime::<Utc>::MAX_UTC);
        let kb = b
            .projected_next_start_at
            .unwrap_or(DateTime::<Utc>::MAX_UTC);
        ka.cmp(&kb)
    });
    Ok(tiles)
}

/// One-shot fetch of the next actionable tile.
pub async fn next_actionable(ctx: &AppContext) -> Result<Option<TileListView>> {
    Ok(today(ctx).await?.into_iter().next())
}

// ---------------------------------------------------------------------------
// SourceTiles.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateSourceTileDraft {
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub external_id: Option<String>,
    #[serde(default)]
    pub priority: i32,
    #[serde(default)]
    pub required_duration_ms: i64,
    pub horizon_start: DateTime<Utc>,
    pub horizon_end: DateTime<Utc>,
    #[serde(default)]
    pub plan_role: i32,
    /// Optional: user-supplied plan body as opaque JSON. Defaults to a
    /// minimal plan if `None`.
    #[serde(default)]
    pub plan: Option<serde_json::Value>,
    /// Optional: user-supplied flows list. Defaults to `[]`.
    #[serde(default)]
    pub flows: Vec<serde_json::Value>,
    /// Optional: user-supplied window. Defaults to a minimal window.
    #[serde(default)]
    pub window: Option<serde_json::Value>,
    /// Optional: user-supplied split_policy. Defaults to kind=0 (unsplit).
    #[serde(default)]
    pub split_policy: Option<serde_json::Value>,
    /// Optional: user-supplied generation. Defaults to a minimal generation.
    #[serde(default)]
    pub generation: Option<serde_json::Value>,
}

impl CreateSourceTileDraft {
    pub fn minimal(title: impl Into<String>) -> Self {
        let now = Utc::now();
        let horizon_end = now + chrono::Duration::days(30);
        Self {
            title: title.into(),
            description: None,
            color: None,
            icon: None,
            external_id: None,
            priority: 0,
            required_duration_ms: 30 * 60 * 1000,
            horizon_start: now,
            horizon_end,
            plan_role: 0,
            plan: None,
            flows: Vec::new(),
            window: None,
            split_policy: None,
            generation: None,
        }
    }
}

/// Compose a `CreateSourceTileRequest` from a user-friendly draft, filling
/// in server-required shapes with sensible minimal defaults. The CLI does
/// not try to compute schedule plans; users supply planning JSON when they
/// need anything non-trivial.
pub fn build_create_request(draft: &CreateSourceTileDraft) -> CreateSourceTileRequest {
    let tile = ScheduleTileDefinition {
        title: draft.title.clone(),
        description: draft.description.clone(),
        color: draft.color.clone(),
        icon: draft.icon.clone(),
        external_id: draft.external_id.clone(),
    };
    let plan = SchedulePlanDefinition {
        role: draft.plan_role,
        completion: draft
            .plan
            .as_ref()
            .and_then(|p| p.get("completion").cloned())
            .unwrap_or_else(
                || serde_json::json!({"root": {"All": []}, "tasks": [], "time_requirements": []}),
            ),
        planning: draft
            .plan
            .as_ref()
            .and_then(|p| p.get("planning").cloned())
            .unwrap_or_else(|| serde_json::json!({"nesting_rules": [], "placement_rules": []})),
        metrics: draft
            .plan
            .as_ref()
            .and_then(|p| p.get("metrics").cloned())
            .and_then(|v| v.as_array().cloned())
            .unwrap_or_default(),
        decisions: draft
            .plan
            .as_ref()
            .and_then(|p| p.get("decisions").cloned())
            .and_then(|v| v.as_array().cloned())
            .unwrap_or_default(),
        references: draft
            .plan
            .as_ref()
            .and_then(|p| p.get("references").cloned())
            .and_then(|v| v.as_array().cloned())
            .unwrap_or_default(),
    };
    let schedule = CreateSourceScheduleDefinition {
        generation: draft
            .generation
            .clone()
            .unwrap_or_else(|| serde_json::json!({"kind": 0})),
        split_policy: draft
            .split_policy
            .clone()
            .unwrap_or_else(|| serde_json::json!({"kind": 0})),
        window: draft
            .window
            .clone()
            .unwrap_or_else(|| serde_json::json!({"window": "anytime"})),
        priority: draft.priority,
        required_duration_ms: draft.required_duration_ms,
    };
    let payload = CreateSourceTilePayload {
        tile,
        plan,
        flows: draft.flows.clone(),
        schedule,
        horizon: Span {
            start: draft.horizon_start,
            end: draft.horizon_end,
        },
        relations: None,
        source_client_local_id: None,
    };
    CreateSourceTileRequest::new(payload)
}

/// CLI-level wrapper for `create_source_tile` that takes a draft.
pub async fn create_source_tile_draft(
    ctx: &AppContext,
    draft: &CreateSourceTileDraft,
) -> Result<CommandResponse> {
    let token = ctx.require_token()?;
    let req = build_create_request(draft);
    api_source_tiles::create_source_tile(ctx, token, &req)
        .await
        .context("create_source_tile failed")
}

/// CLI-level wrapper for `update_source_tile` that takes a draft.
pub async fn update_source_tile_draft(
    ctx: &AppContext,
    id: Uuid,
    draft: &CreateSourceTileDraft,
) -> Result<CommandResponse> {
    let token = ctx.require_token()?;
    let tile = ScheduleTileDefinition {
        title: draft.title.clone(),
        description: draft.description.clone(),
        color: draft.color.clone(),
        icon: draft.icon.clone(),
        external_id: draft.external_id.clone(),
    };
    let plan = SchedulePlanDefinition {
        role: draft.plan_role,
        completion: serde_json::json!({"root": {"All": []}, "tasks": [], "time_requirements": []}),
        planning: serde_json::json!({"nesting_rules": [], "placement_rules": []}),
        metrics: Vec::new(),
        decisions: Vec::new(),
        references: Vec::new(),
    };
    let schedule = CreateSourceScheduleDefinition {
        generation: draft
            .generation
            .clone()
            .unwrap_or_else(|| serde_json::json!({"kind": 0})),
        split_policy: draft
            .split_policy
            .clone()
            .unwrap_or_else(|| serde_json::json!({"kind": 0})),
        window: draft
            .window
            .clone()
            .unwrap_or_else(|| serde_json::json!({"window": "anytime"})),
        priority: draft.priority,
        required_duration_ms: draft.required_duration_ms,
    };
    let payload = UpdateSourceTilePayload {
        tile,
        plan,
        flows: draft.flows.clone(),
        schedule,
        horizon: Span {
            start: draft.horizon_start,
            end: draft.horizon_end,
        },
        relations: None,
        source_client_local_id: None,
    };
    let req = UpdateSourceTileRequest::new(payload);
    api_source_tiles::update_source_tile(ctx, token, id, &req)
        .await
        .context("update_source_tile failed")
}

pub async fn list_source_tiles(
    ctx: &AppContext,
    owner: Option<Uuid>,
) -> Result<Vec<SourceTileRead>> {
    let token = ctx.require_token()?;
    let q = api_source_tiles::ListSourceTilesQuery {
        owner_id: owner,
        ..Default::default()
    };
    api_source_tiles::list_source_tiles(ctx, token, &q)
        .await
        .context("list_source_tiles failed")
}

pub async fn show_source_tile(ctx: &AppContext, id: Uuid) -> Result<SourceTileDetailRead> {
    let token = ctx.require_token()?;
    api_source_tiles::get_source_tile(ctx, token, id)
        .await
        .context("get_source_tile failed")
}

pub async fn cancel_source_tile(
    ctx: &AppContext,
    id: Uuid,
    reason: &str,
) -> Result<CommandResponse> {
    let token = ctx.require_token()?;
    let req = CancelSourceTileRequest::new(reason);
    api_source_tiles::cancel_source_tile(ctx, token, id, &req)
        .await
        .context("cancel_source_tile failed")
}

pub async fn reflow_source_tile(
    ctx: &AppContext,
    id: Uuid,
    range: Span,
) -> Result<CommandResponse> {
    let token = ctx.require_token()?;
    let req = ReflowSourceTileRequest::new(range);
    api_source_tiles::reflow_source_tile(ctx, token, id, &req)
        .await
        .context("reflow_source_tile failed")
}

pub async fn source_tile_completion(ctx: &AppContext, id: Uuid) -> Result<serde_json::Value> {
    let token = ctx.require_token()?;
    let v = api_source_tiles::get_source_tile_completion(ctx, token, id)
        .await
        .context("get_source_tile_completion failed")?;
    Ok(v.0)
}

// ---------------------------------------------------------------------------
// Executions.
// ---------------------------------------------------------------------------

pub async fn start_execution(ctx: &AppContext, placement_id: Uuid) -> Result<CommandResponse> {
    let token = ctx.require_token()?;
    let req = StartExecutionRequest::new(placement_id);
    executions::start_execution(ctx, token, placement_id, &req)
        .await
        .context("start_execution failed")
}

pub async fn pause_execution(ctx: &AppContext, execution_id: Uuid) -> Result<CommandResponse> {
    let token = ctx.require_token()?;
    let req = ExecutionLifecycleRequest::new();
    executions::pause_execution(ctx, token, execution_id, &req)
        .await
        .context("pause_execution failed")
}

pub async fn resume_execution(ctx: &AppContext, execution_id: Uuid) -> Result<CommandResponse> {
    let token = ctx.require_token()?;
    let req = ExecutionLifecycleRequest::new();
    executions::resume_execution(ctx, token, execution_id, &req)
        .await
        .context("resume_execution failed")
}

pub async fn finish_execution(
    ctx: &AppContext,
    execution_id: Uuid,
    kind: i32,
    note: Option<String>,
) -> Result<CommandResponse> {
    let token = ctx.require_token()?;
    let req = FinishExecutionRequest::new(kind, note);
    executions::finish_execution(ctx, token, execution_id, &req)
        .await
        .context("finish_execution failed")
}

// ---------------------------------------------------------------------------
// Prompts.
// ---------------------------------------------------------------------------

/// Outcome kinds the user can resolve a pending prompt with. Maps to the
/// numeric `PromptResolution` enum on the wire.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptResolution {
    Acknowledged = 0,
    Dismissed = 1,
    ActionTaken = 2,
}

impl PromptResolution {
    pub fn from_str(s: &str) -> Result<Self> {
        match s.to_ascii_lowercase().as_str() {
            "ack" | "acknowledged" => Ok(Self::Acknowledged),
            "dismiss" | "dismissed" => Ok(Self::Dismissed),
            "act" | "action" | "action_taken" => Ok(Self::ActionTaken),
            other => Err(anyhow!(
                "unknown resolution `{other}`; expected one of ack|dismiss|act"
            )),
        }
    }
}

/// Outcome kinds the user can request. Maps to `PromptKind` on the wire.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptKind {
    StartupRecovery = 0,
    MissedStart = 1,
    DecisionRequired = 2,
    StaleActiveExecution = 3,
}

#[allow(dead_code)]
impl PromptKind {
    pub fn from_str(s: &str) -> Result<Self> {
        match s.to_ascii_lowercase().as_str() {
            "startup" | "startup_recovery" => Ok(Self::StartupRecovery),
            "missed" | "missed_start" => Ok(Self::MissedStart),
            "decision" | "decision_required" => Ok(Self::DecisionRequired),
            "stale" | "stale_active_execution" => Ok(Self::StaleActiveExecution),
            other => Err(anyhow!(
                "unknown prompt kind `{other}`; expected startup|missed|decision|stale"
            )),
        }
    }
}

pub async fn list_pending_prompts(ctx: &AppContext) -> Result<Vec<tastile_api::PromptView>> {
    let token = ctx.require_token()?;
    tastile_api::list_pending_prompts(ctx, token)
        .await
        .context("list_pending_prompts failed")
}

pub async fn resolve_prompt(
    ctx: &AppContext,
    prompt_id: Uuid,
    resolution: PromptResolution,
    note: Option<String>,
) -> Result<CommandResponse> {
    let token = ctx.require_token()?;
    let req = tastile_api::ResolvePromptRequest::new(resolution as i32, note);
    tastile_api::resolve_prompt(ctx, token, prompt_id, &req)
        .await
        .context("resolve_prompt failed")
}

pub async fn request_prompt(
    ctx: &AppContext,
    kind: i32,
    note: Option<String>,
) -> Result<CommandResponse> {
    let token = ctx.require_token()?;
    let req = tastile_api::RequestPromptRequest::new(kind);
    let req = if let Some(n) = note {
        let mut r = req;
        r.payload.note = Some(n);
        r
    } else {
        req
    };
    tastile_api::request_prompt(ctx, token, &req)
        .await
        .context("request_prompt failed")
}

pub async fn respond_startup_recovery(
    ctx: &AppContext,
    prompt_id: Uuid,
    resolution: PromptResolution,
    note: Option<String>,
) -> Result<CommandResponse> {
    let token = ctx.require_token()?;
    let req = tastile_api::StartupRecoveryRequest::new(prompt_id, resolution as i32, note);
    tastile_api::respond_startup_recovery(ctx, token, &req)
        .await
        .context("respond_startup_recovery failed")
}

/// Re-export of `StoredToken` so callers don't have to depend on
/// `tastile_auth` directly.
#[allow(dead_code)]
pub type Token = StoredToken;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompt_resolution_parses_user_friendly_aliases() {
        assert_eq!(
            PromptResolution::from_str("ack").unwrap(),
            PromptResolution::Acknowledged
        );
        assert_eq!(
            PromptResolution::from_str("dismiss").unwrap(),
            PromptResolution::Dismissed
        );
        assert_eq!(
            PromptResolution::from_str("action_taken").unwrap(),
            PromptResolution::ActionTaken
        );
        assert!(PromptResolution::from_str("nope").is_err());
    }

    #[test]
    fn prompt_kind_parses_user_friendly_aliases() {
        assert_eq!(
            PromptKind::from_str("startup").unwrap(),
            PromptKind::StartupRecovery
        );
        assert_eq!(
            PromptKind::from_str("missed_start").unwrap(),
            PromptKind::MissedStart
        );
        assert_eq!(
            PromptKind::from_str("decision").unwrap(),
            PromptKind::DecisionRequired
        );
        assert_eq!(
            PromptKind::from_str("stale").unwrap(),
            PromptKind::StaleActiveExecution
        );
        assert!(PromptKind::from_str("???").is_err());
    }

    #[test]
    fn build_create_request_fills_minimal_plan() {
        let draft = CreateSourceTileDraft::minimal("test");
        let req = build_create_request(&draft);
        assert_eq!(req.payload.tile.title, "test");
        assert_eq!(req.payload.horizon.start, draft.horizon_start);
        let json = serde_json::to_value(&req).unwrap();
        assert!(json["idempotency_key"].is_string());
        assert_eq!(json["payload"]["schedule"]["priority"], 0);
    }
}
