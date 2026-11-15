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

use anyhow::{Context, Result, anyhow, bail};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tastile_api::{
    ApiClient, ApiConfig, ApiError, BearerToken, CancelSourceTileRequest, CommandResponse,
    CreateSourceScheduleDefinition, CreateSourceTilePayload, CreateSourceTileRequest,
    ExecutionLifecycleRequest, FinishExecutionRequest, PlacementTileRead, ReflowSourceTileRequest,
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
    credential_api_base_url: Option<String>,
    credential_expires_at: Option<DateTime<Utc>>,
}

impl AppContext {
    /// Build the context from the current `Config`. Loads the credential
    /// store if present. If no credential is stored, `token` is `None` and
    /// only unauthenticated endpoints can be reached (currently none).
    pub fn load(config: Config) -> Result<Self> {
        let cfg = tastile_config::with_env_overrides(config);
        let api = ApiClient::new(ApiConfig::new(&cfg.api_url)?)
            .context("invalid configured API base URL")?;
        let store = credential_store(&cfg)?;
        let loaded = store
            .load(tastile_auth::DEFAULT_SERVICE, tastile_auth::DEFAULT_USER)
            .context("credential store error")?;
        let (token, subject, credential_api_base_url, credential_expires_at) = match loaded {
            Some(stored) => (
                Some(BearerToken::new(stored.bearer)),
                stored.subject,
                Some(stored.api_base_url),
                stored.expires_at,
            ),
            None => (None, None, None, None),
        };
        Ok(Self {
            api,
            config: cfg,
            token,
            subject,
            credential_api_base_url,
            credential_expires_at,
        })
    }

    /// True iff the loaded bearer is still usable for the configured API
    /// base. A credential with no expiry is treated as non-expiring because
    /// the token-exchange contract permits servers to omit `expires_at`.
    pub fn is_signed_in(&self) -> bool {
        self.token.is_some()
            && self
                .credential_api_base_url
                .as_deref()
                .is_some_and(|stored| crate::safety::api_bases_match(&self.config.api_url, stored))
            && self
                .credential_expires_at
                .is_none_or(|expires_at| expires_at > Utc::now())
    }

    /// Borrow the bearer token, erroring if not signed in.
    pub fn require_token(&self) -> Result<&BearerToken> {
        let token = self
            .token
            .as_ref()
            .ok_or_else(|| anyhow!("not signed in; run `tastile auth login`"))?;
        let stored_api_base = self.credential_api_base_url.as_deref().ok_or_else(|| {
            anyhow!("stored API credential has no API-base binding; re-run `tastile auth login`")
        })?;
        if !crate::safety::api_bases_match(&self.config.api_url, stored_api_base) {
            bail!(
                "the stored credential belongs to a different API base than the current configuration; refusing to send it. Run `tastile auth login` for the configured API"
            );
        }
        if self
            .credential_expires_at
            .is_some_and(|expiry| expiry <= Utc::now())
        {
            bail!("the stored API credential has expired; run `tastile auth login`");
        }
        Ok(token)
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

/// Environment variable that points at a file-backed credential.
///
/// This exists for **test systems only**: an isolated Core stack has no
/// web origin, so the browser-mediated PKCE flow cannot mint a token for
/// it, and a headless container has no reachable OS keychain. The file is
/// therefore only honoured for a loopback API base — a remote (production)
/// target keeps using the OS credential store.
pub const CREDENTIAL_FILE_ENV: &str = "TASTILE_CREDENTIAL_FILE";

/// Pick the credential store for the given configuration.
///
/// The OS credential store is the production path. A file-backed store is
/// accepted only when [`CREDENTIAL_FILE_ENV`] is set **and** the API base
/// is a loopback address.
pub fn credential_store(cfg: &Config) -> Result<Box<dyn CredentialStore>> {
    let path = std::env::var(CREDENTIAL_FILE_ENV)
        .ok()
        .filter(|value| !value.trim().is_empty());
    match path {
        Some(path) => {
            if !crate::safety::is_loopback_url(&cfg.api_url) {
                bail!(
                    "{CREDENTIAL_FILE_ENV} is only honoured for a loopback API base; \
                     a remote target uses the OS credential store"
                );
            }
            Ok(Box::new(tastile_auth::FileCredentialStore::new(path)))
        }
        None => Ok(Box::new(KeyringStore)),
    }
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

/// List tiles with an explicit query built from the documented
/// `GET /v1/tiles` parameters.
pub async fn list_tiles(
    ctx: &AppContext,
    query: api_tiles::ListTilesQuery,
) -> Result<Vec<TileListView>> {
    let token = ctx.require_token()?;
    api_tiles::list_tiles(ctx, token, &query)
        .await
        .context("list_tiles failed")
}

/// Sorted view of the current tiles.
///
/// The contract's `exclude_future` parameter drops tiles whose first
/// upcoming placement is strictly in the future, which is the documented
/// way to ask for "what is on today".
pub async fn today(ctx: &AppContext) -> Result<Vec<TileListView>> {
    let token = ctx.require_token()?;
    let query = api_tiles::ListTilesQuery {
        exclude_future: Some(true),
        ..Default::default()
    };
    let mut tiles = api_tiles::list_tiles(ctx, token, &query)
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
    /// Optional source relations. A full update draft must provide this
    /// field explicitly because the read model cannot round-trip payload
    /// relation definitions losslessly.
    #[serde(default)]
    pub relations: Option<Vec<serde_json::Value>>,
    /// Optional caller-assigned id used to correlate a locally-created
    /// source tile. The read model does not return it, so full update drafts
    /// must provide the original value or explicit `null`.
    #[serde(default)]
    pub source_client_local_id: Option<Uuid>,
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
            relations: None,
            source_client_local_id: None,
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
        generation: draft.generation.clone().unwrap_or_else(|| {
            // The contract requires `kind = 0` (one-time) to carry the
            // occurrence instant; a bare `{"kind": 0}` is rejected.
            serde_json::json!({"kind": 0, "at": draft.horizon_start.to_rfc3339()})
        }),
        split_policy: draft
            .split_policy
            .clone()
            .unwrap_or_else(|| serde_json::json!({"kind": 0})),
        window: draft
            .window
            .clone()
            .unwrap_or_else(crate::simple_input::default_window),
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
        relations: draft.relations.clone(),
        source_client_local_id: draft.source_client_local_id,
    };
    CreateSourceTileRequest::new(payload)
}

/// CLI-level wrapper for `create_source_tile` that takes a draft.
pub async fn create_source_tile_draft(
    ctx: &AppContext,
    draft: &CreateSourceTileDraft,
) -> Result<CommandResponse> {
    guard_write(ctx).await?;
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
    expected_revision: Option<i64>,
) -> Result<CommandResponse> {
    guard_write(ctx).await?;
    let token = ctx.require_token()?;
    let create_payload = build_create_request(draft).payload;
    let payload = UpdateSourceTilePayload {
        tile: create_payload.tile,
        plan: create_payload.plan,
        flows: create_payload.flows,
        schedule: create_payload.schedule,
        horizon: create_payload.horizon,
        relations: create_payload.relations,
        source_client_local_id: create_payload.source_client_local_id,
    };
    let mut req = UpdateSourceTileRequest::new(payload);
    if let Some(revision) = expected_revision {
        req = req.with_expected_revision(revision);
    }
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
    api_source_tiles::list_all_source_tiles(ctx, token, &q)
        .await
        .context("list_source_tiles failed")
}

/// Gate every mutating operation.
///
/// Two independent checks run before a write leaves the process:
///
/// 1. **Target classification** — a loopback API needs an explicit
///    isolation attestation and must not use the reserved operational port;
///    a remote API must use TLS.
/// 2. **Identity probe** — the target has to answer a documented read
///    endpoint with the stored credential. This proves the URL points at a
///    Tastile v1 API that accepts this account; it does *not* prove the
///    stack is isolated, which only the operator can attest.
pub async fn guard_write(ctx: &AppContext) -> Result<()> {
    let target =
        crate::safety::classify_write_target(&ctx.config, crate::safety::isolation_attested())?;
    verify_write_target(ctx, target).await
}

/// The schedule publisher has an additional local-Core-only restriction
/// from the repository's acceptance contract.
pub async fn guard_schedule_publish_write(ctx: &AppContext) -> Result<()> {
    let target = crate::safety::classify_schedule_publish_target(
        &ctx.config,
        crate::safety::isolation_attested(),
    )?;
    verify_write_target(ctx, target).await
}

async fn verify_write_target(ctx: &AppContext, target: crate::safety::WriteTarget) -> Result<()> {
    validate_credential_for_write(
        &ctx.config.api_url,
        ctx.credential_api_base_url.as_deref(),
        ctx.credential_expires_at,
        Utc::now(),
    )?;
    let token = ctx.require_token()?;
    let probe = api_source_tiles::list_source_tiles(
        ctx,
        token,
        &api_source_tiles::ListSourceTilesQuery {
            limit: Some(1),
            ..Default::default()
        },
    )
    .await;
    match probe {
        Ok(_) => Ok(()),
        Err(ApiError::Http { status, .. }) if status == 401 || status == 403 => Err(anyhow!(
            "the write target {} rejected the stored credential (HTTP {status}); refusing to write",
            target.describe()
        )),
        Err(e) => Err(anyhow!(
            "could not verify the write target {}: {e}",
            target.describe()
        )),
    }
}

fn validate_credential_for_write(
    configured_api_base: &str,
    stored_api_base: Option<&str>,
    expires_at: Option<DateTime<Utc>>,
    now: DateTime<Utc>,
) -> Result<()> {
    let stored_api_base = stored_api_base
        .ok_or_else(|| anyhow!("no stored API credential; run `tastile auth login`"))?;
    if !crate::safety::api_bases_match(configured_api_base, stored_api_base) {
        bail!(
            "the stored credential belongs to a different API base than the current configuration; refusing to write. Run `tastile auth login` for the configured API"
        );
    }
    if expires_at.is_some_and(|expiry| expiry <= now) {
        bail!("the stored API credential has expired; run `tastile auth login` before writing");
    }
    Ok(())
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
    reason_code: i32,
    expected_revision: Option<i64>,
) -> Result<CommandResponse> {
    guard_write(ctx).await?;
    let token = ctx.require_token()?;
    let mut req = CancelSourceTileRequest::new(reason_code);
    if let Some(revision) = expected_revision {
        req = req.with_expected_revision(revision);
    }
    api_source_tiles::cancel_source_tile(ctx, token, id, &req)
        .await
        .context("cancel_source_tile failed")
}

pub async fn reflow_source_tile(
    ctx: &AppContext,
    id: Uuid,
    range: Span,
    expected_revision: Option<i64>,
) -> Result<CommandResponse> {
    guard_write(ctx).await?;
    let token = ctx.require_token()?;
    let mut req = ReflowSourceTileRequest::new(range);
    if let Some(revision) = expected_revision {
        req = req.with_expected_revision(revision);
    }
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
// Diff / impact preview / readback.
// ---------------------------------------------------------------------------

/// One field that differs between the stored source tile and a proposed
/// draft.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldDiff {
    pub field: String,
    pub current: String,
    pub proposed: String,
}

/// Compare fields exposed by the source-tile read model with the proposed
/// full-replacement draft. The API read model does not expose plan details,
/// flows, horizon, request-shaped relations, or `source_client_local_id`, so
/// those fields cannot be diffed here and are called out separately by the
/// command.
pub fn diff_source_tile(draft: &CreateSourceTileDraft, current: &SourceTileRead) -> Vec<FieldDiff> {
    let mut diffs: Vec<FieldDiff> = Vec::new();
    let mut push = |field: &str, current_value: String, proposed_value: String| {
        if current_value != proposed_value {
            diffs.push(FieldDiff {
                field: field.to_string(),
                current: current_value,
                proposed: proposed_value,
            });
        }
    };

    push("title", current.title.clone(), draft.title.clone());
    push(
        "description",
        optional(current.description.as_deref()),
        optional(draft.description.as_deref()),
    );
    push(
        "color",
        optional(current.color.as_deref()),
        optional(draft.color.as_deref()),
    );
    push(
        "icon",
        optional(current.icon.as_deref()),
        optional(draft.icon.as_deref()),
    );
    push(
        "external_id",
        optional(current.external_id.as_deref()),
        optional(draft.external_id.as_deref()),
    );
    push(
        "plan_role",
        current.plan_role.to_string(),
        draft.plan_role.to_string(),
    );
    push(
        "schedule.required_duration_ms",
        current.schedule.required_duration_ms.to_string(),
        draft.required_duration_ms.to_string(),
    );
    push(
        "schedule.priority",
        current.schedule.priority.to_string(),
        draft.priority.to_string(),
    );

    if let Some(generation) = draft.generation.as_ref() {
        let current_generation = &current.schedule.generation;
        push(
            "schedule.generation.kind",
            current_generation.kind.to_string(),
            json_number(generation, "kind"),
        );
        push(
            "schedule.generation.at",
            optional(current_generation.at.map(|v| v.to_rfc3339()).as_deref()),
            json_string(generation, "at"),
        );
        push(
            "schedule.generation.starts_at",
            optional(
                current_generation
                    .starts_at
                    .map(|v| v.to_rfc3339())
                    .as_deref(),
            ),
            json_string(generation, "starts_at"),
        );
        push(
            "schedule.generation.interval_ms",
            optional(
                current_generation
                    .interval_ms
                    .map(|v| v.to_string())
                    .as_deref(),
            ),
            json_number(generation, "interval_ms"),
        );
        push(
            "schedule.generation.ends_at",
            optional(
                current_generation
                    .ends_at
                    .map(|v| v.to_rfc3339())
                    .as_deref(),
            ),
            json_string(generation, "ends_at"),
        );
        push(
            "schedule.generation.weekday_mask",
            optional(
                current_generation
                    .weekday_mask
                    .map(|v| v.to_string())
                    .as_deref(),
            ),
            json_number(generation, "weekday_mask"),
        );
        push(
            "schedule.generation.date_range_start",
            optional(current_generation.date_range_start.as_deref()),
            json_string(generation, "date_range_start"),
        );
        push(
            "schedule.generation.date_range_end",
            optional(current_generation.date_range_end.as_deref()),
            json_string(generation, "date_range_end"),
        );
        push(
            "schedule.generation.offset_min",
            optional(
                current_generation
                    .offset_min
                    .map(|v| v.to_string())
                    .as_deref(),
            ),
            json_number(generation, "offset_min"),
        );
        push(
            "schedule.generation.excluded_dates",
            current_generation.excluded_dates.join(","),
            json_string_array(generation, "excluded_dates"),
        );
    }
    if let Some(window) = draft.window.as_ref() {
        push(
            "schedule.window.start_offset_ms",
            current.schedule.window.start_offset_ms.to_string(),
            json_number(window, "start_offset_ms"),
        );
        push(
            "schedule.window.end_offset_ms",
            current.schedule.window.end_offset_ms.to_string(),
            json_number(window, "end_offset_ms"),
        );
    }
    if let Some(split_policy) = draft.split_policy.as_ref() {
        push(
            "schedule.split_policy.kind",
            current.schedule.split_policy.kind.to_string(),
            json_number(split_policy, "kind"),
        );
        push(
            "schedule.split_policy.min_segment_ms",
            optional(
                current
                    .schedule
                    .split_policy
                    .min_segment_ms
                    .map(|v| v.to_string())
                    .as_deref(),
            ),
            json_number(split_policy, "min_segment_ms"),
        );
        push(
            "schedule.split_policy.max_segment_ms",
            optional(
                current
                    .schedule
                    .split_policy
                    .max_segment_ms
                    .map(|v| v.to_string())
                    .as_deref(),
            ),
            json_number(split_policy, "max_segment_ms"),
        );
        push(
            "schedule.split_policy.max_segments",
            optional(
                current
                    .schedule
                    .split_policy
                    .max_segments
                    .map(|v| v.to_string())
                    .as_deref(),
            ),
            json_number(split_policy, "max_segments"),
        );
    }
    diffs
}

fn json_number(value: &serde_json::Value, key: &str) -> String {
    value
        .get(key)
        .and_then(serde_json::Value::as_i64)
        .map(|v| v.to_string())
        .unwrap_or_else(|| "(unset)".to_string())
}

fn json_string(value: &serde_json::Value, key: &str) -> String {
    value
        .get(key)
        .and_then(serde_json::Value::as_str)
        .map(|v| v.to_string())
        .unwrap_or_else(|| "(unset)".to_string())
}

fn json_string_array(value: &serde_json::Value, key: &str) -> String {
    value
        .get(key)
        .and_then(serde_json::Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str())
                .collect::<Vec<&str>>()
                .join(",")
        })
        .unwrap_or_default()
}

fn optional(value: Option<&str>) -> String {
    value.unwrap_or("(unset)").to_string()
}

/// Placement rows a cancel / reflow would touch.
#[derive(Debug, Clone)]
pub struct PlacementImpact {
    /// Every placement the source currently owns.
    pub total: usize,
    /// Placements that are still open.
    pub open: usize,
    /// Placements that overlap the requested range (all of them when no
    /// range was given).
    pub in_range: usize,
    pub rows: Vec<PlacementTileRead>,
}

/// Collect the placement scope of a source tile, optionally restricted to
/// a time range.
///
/// The server decides exactly which placements a cancel or reflow touches;
/// this is the client-side preview of that scope, not a promise about the
/// server's decision.
pub async fn placement_impact(
    ctx: &AppContext,
    id: Uuid,
    range: Option<(DateTime<Utc>, DateTime<Utc>)>,
) -> Result<PlacementImpact> {
    let token = ctx.require_token()?;
    let placements = api_source_tiles::list_all_source_tile_placements(ctx, token, id).await?;
    let total = placements.len();
    let open = placements.iter().filter(|p| !p.closed).count();
    let rows: Vec<PlacementTileRead> = match range {
        None => placements.clone(),
        Some((from, to)) => placements
            .iter()
            .filter(|p| p.start < to && p.end > from)
            .cloned()
            .collect(),
    };
    Ok(PlacementImpact {
        total,
        open,
        in_range: rows.len(),
        rows,
    })
}

/// The state of a source tile after a write, read back from the server.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SourceTileState {
    pub revision: i64,
    /// Numeric lifecycle: 0 ACTIVE .. 3 CANCELLED.
    pub source_state: i32,
    pub placements_total: usize,
    pub placements_open: usize,
}

/// Read the source tile and its placements back from the server.
pub async fn read_back(ctx: &AppContext, id: Uuid) -> Result<SourceTileState> {
    let token = ctx.require_token()?;
    let detail = api_source_tiles::get_source_tile(ctx, token, id)
        .await
        .context("get_source_tile failed")?;
    let placements = api_source_tiles::list_all_source_tile_placements(ctx, token, id).await?;
    Ok(SourceTileState {
        revision: detail.source.revision,
        source_state: detail.source.source_state,
        placements_total: placements.len(),
        placements_open: placements.iter().filter(|p| !p.closed).count(),
    })
}

// ---------------------------------------------------------------------------
// Executions.
// ---------------------------------------------------------------------------

pub async fn start_execution(ctx: &AppContext, placement_id: Uuid) -> Result<CommandResponse> {
    guard_write(ctx).await?;
    let token = ctx.require_token()?;
    let req = StartExecutionRequest::new(placement_id);
    executions::start_execution(ctx, token, placement_id, &req)
        .await
        .context("start_execution failed")
}

pub async fn pause_execution(ctx: &AppContext, execution_id: Uuid) -> Result<CommandResponse> {
    guard_write(ctx).await?;
    let token = ctx.require_token()?;
    let req = ExecutionLifecycleRequest::new();
    executions::pause_execution(ctx, token, execution_id, &req)
        .await
        .context("pause_execution failed")
}

pub async fn resume_execution(ctx: &AppContext, execution_id: Uuid) -> Result<CommandResponse> {
    guard_write(ctx).await?;
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
    guard_write(ctx).await?;
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
    guard_write(ctx).await?;
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
    guard_write(ctx).await?;
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
    guard_write(ctx).await?;
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

    #[test]
    fn writes_refuse_credentials_bound_to_another_api_base() {
        let now = DateTime::parse_from_rfc3339("2026-11-15T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let result = validate_credential_for_write(
            "https://api.example.test",
            Some("https://other.example.test"),
            None,
            now,
        );
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("different API base")
        );
    }

    #[test]
    fn writes_refuse_expired_credentials_before_a_probe() {
        let now = DateTime::parse_from_rfc3339("2026-11-15T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let expired = now - chrono::Duration::seconds(1);
        let result = validate_credential_for_write(
            "https://api.example.test",
            Some("https://api.example.test/"),
            Some(expired),
            now,
        );
        assert!(result.unwrap_err().to_string().contains("has expired"));
    }

    #[test]
    fn writes_accept_a_matching_nonexpired_credential() {
        let now = DateTime::parse_from_rfc3339("2026-11-15T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let future = now + chrono::Duration::hours(1);
        assert!(
            validate_credential_for_write(
                "https://api.example.test:443/",
                Some("https://api.example.test"),
                Some(future),
                now,
            )
            .is_ok()
        );
    }

    #[test]
    fn read_token_is_not_sent_to_a_different_api_base() {
        let mut config = Config::default();
        config.api_url = "https://api.example.test".to_string();
        let api = ApiClient::new(ApiConfig::new(&config.api_url).unwrap()).unwrap();
        let ctx = AppContext {
            api,
            config,
            token: Some(BearerToken::new("test-token")),
            subject: None,
            credential_api_base_url: Some("https://other.example.test".to_string()),
            credential_expires_at: None,
        };
        assert!(
            ctx.require_token()
                .unwrap_err()
                .to_string()
                .contains("different API base")
        );
    }
}
