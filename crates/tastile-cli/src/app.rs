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
    StartExecutionRequest, TileListView, TimelineItem, TimelineQuery, UpdateSourceTilePayload,
    UpdateSourceTileRequest, executions, source_tiles as api_source_tiles, tiles as api_tiles,
    timeline as api_timeline,
};
use tastile_auth::{CredentialStore, KeyringStore};
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
    credential_owner_id: Option<String>,
}

/// Proof that the configured target and credential passed write preflight.
/// Its private field prevents call sites from constructing a permit directly.
#[derive(Debug)]
pub struct WritePermit {
    _private: (),
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
        let (token, subject, credential_api_base_url, credential_expires_at, credential_owner_id) =
            match loaded {
                Some(stored) => (
                    Some(BearerToken::new(stored.bearer)),
                    stored.subject,
                    Some(stored.api_base_url),
                    stored.expires_at,
                    stored.owner_id,
                ),
                None => (None, None, None, None, None),
            };
        Ok(Self {
            api,
            config: cfg,
            token,
            subject,
            credential_api_base_url,
            credential_expires_at,
            credential_owner_id,
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

    /// Fail closed when the owner scope observed through the target does
    /// not match the owner pinned on the stored credential.
    ///
    /// The pinned OpenAPI contract has no `whoami` operation, so the owner
    /// is learned from read-model rows (`SourceTileRead.owner_id`) instead
    /// of a dedicated identity endpoint. The first non-empty observation
    /// pins the owner into the credential store; later observations must
    /// match. An empty result set cannot identify the owner and is allowed
    /// — the pin is established by the first read that returns rows (the
    /// guard probe or a post-write read-back).
    pub fn confirm_write_owner(&self, observed: impl IntoIterator<Item = Uuid>) -> Result<()> {
        let observed: std::collections::BTreeSet<String> =
            observed.into_iter().map(|id| id.to_string()).collect();
        if observed.len() > 1 {
            bail!(
                "the write target returned rows from {} distinct owners; \
                 refusing to write because the credential scope is ambiguous",
                observed.len()
            );
        }
        let Some(owner) = observed.iter().next() else {
            return Ok(());
        };
        let owner = Uuid::parse_str(owner)
            .map_err(|e| anyhow!("write target reported a malformed owner id: {e}"))?;
        if let Some(pinned) = self
            .credential_owner_id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            let pinned = Uuid::parse_str(pinned).map_err(|e| {
                anyhow!("the stored credential owner pin `{pinned}` is not a UUID: {e}")
            })?;
            if pinned != owner {
                bail!(
                    "the stored credential is pinned to owner {pinned} but the write target \
                     reports owner {owner}; refusing to write. Run `tastile auth login` \
                     for the intended account"
                );
            }
            return Ok(());
        }
        self.persist_owner_pin(owner)
    }

    /// Record the observed Core owner id on the stored credential so later
    /// processes can detect a token/account mismatch on the same API base.
    ///
    /// The store is re-read before writing: a pin committed by another
    /// process (or earlier in this context's lifetime) is compared against
    /// the fresh observation and mismatches fail closed instead of being
    /// silently accepted. The reloaded credential must still bind to the
    /// configured API base so a concurrent `auth login` cannot pin the
    /// observed owner onto a credential minted for a different target.
    fn persist_owner_pin(&self, owner_id: Uuid) -> Result<()> {
        let store = credential_store(&self.config)?;
        Self::persist_owner_pin_with(store.as_ref(), &self.config.api_url, owner_id)
    }

    /// Store-injectable core of [`Self::persist_owner_pin`]; tests pass a
    /// file-backed store directly instead of mutating process env.
    fn persist_owner_pin_with(
        store: &dyn CredentialStore,
        api_url: &str,
        owner_id: Uuid,
    ) -> Result<()> {
        let mut stored = store
            .load(tastile_auth::DEFAULT_SERVICE, tastile_auth::DEFAULT_USER)
            .context("credential store error")?
            .ok_or_else(|| anyhow!("no stored API credential; run `tastile auth login`"))?;
        if !crate::safety::api_bases_match(api_url, &stored.api_base_url) {
            bail!(
                "the stored credential changed while pinning the owner id; \
                 refusing to write. Retry the command"
            );
        }
        if let Some(existing) = stored.owner_id.as_deref() {
            let existing = Uuid::parse_str(existing.trim())
                .map_err(|e| anyhow!("the stored credential owner pin is not a UUID: {e}"))?;
            if existing != owner_id {
                bail!(
                    "the stored credential is pinned to owner {existing} but the write target \
                     reports owner {owner_id}; refusing to write. Run `tastile auth login` \
                     for the intended account"
                );
            }
            return Ok(());
        }
        stored.owner_id = Some(owner_id.to_string());
        store
            .save(
                tastile_auth::DEFAULT_SERVICE,
                tastile_auth::DEFAULT_USER,
                &stored,
            )
            .context("could not persist the observed owner id on the credential")
    }
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
///
/// The endpoint pages via `next_cursor` (v1/14 §15), so every matching row
/// is fetched rather than truncated at the server page size.
pub async fn list_tiles(
    ctx: &AppContext,
    query: api_tiles::ListTilesQuery,
) -> Result<Vec<TileListView>> {
    let token = ctx.require_token()?;
    api_tiles::list_all_tiles(ctx, token, &query)
        .await
        .context("list_tiles failed")
}

/// Sorted view of tiles with a projected start that has not passed yet.
///
/// Core's `exclude_future=true` keeps projected starts at or before now,
/// including stale past starts. Request the full projection and filter
/// locally so `today` can select the next scheduled tile instead.
pub async fn today(ctx: &AppContext) -> Result<Vec<TileListView>> {
    let token = ctx.require_token()?;
    let query = api_tiles::ListTilesQuery {
        exclude_future: Some(false),
        ..Default::default()
    };
    let mut tiles = api_tiles::list_all_tiles(ctx, token, &query)
        .await
        .context("list_tiles failed")?;
    let now = Utc::now();
    tiles.retain(|tile| {
        tile.projected_next_start_at
            .is_some_and(|next_start| next_start >= now)
    });
    // The CLI surfaces a "next actionable" view by sorting on
    // projected_next_start_at ascending.
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
#[serde(deny_unknown_fields)]
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
pub fn build_create_request(draft: &CreateSourceTileDraft) -> Result<CreateSourceTileRequest> {
    validate_plan_role(draft)?;
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
    Ok(CreateSourceTileRequest::new(payload))
}

fn validate_plan_role(draft: &CreateSourceTileDraft) -> Result<()> {
    if let Some(value) = draft.plan.as_ref().and_then(|plan| plan.get("role")) {
        let role = value
            .as_i64()
            .and_then(|value| i32::try_from(value).ok())
            .ok_or_else(|| anyhow!("draft `plan.role` must be an int32"))?;
        if role != draft.plan_role {
            bail!(
                "draft `plan.role` ({role}) disagrees with top-level `plan_role` ({}); make them match or omit `plan.role`",
                draft.plan_role
            );
        }
    }
    Ok(())
}

/// CLI-level wrapper for `create_source_tile` that takes a draft.
pub async fn create_source_tile_draft(
    ctx: &AppContext,
    draft: &CreateSourceTileDraft,
) -> Result<CommandResponse> {
    let req = build_create_request(draft)?;
    guard_write(ctx).await?;
    let token = ctx.require_token()?;
    api_source_tiles::create_source_tile(ctx, token, &req)
        .await
        .context("create_source_tile failed")
}

/// CLI-level wrapper for `update_source_tile`. The payload must already be
/// merged against the server's definition read-back (see
/// [`merge_source_tile_update`]); this function only attaches CAS revision
/// and sends it.
pub(crate) async fn update_source_tile_with_permit(
    ctx: &AppContext,
    id: Uuid,
    payload: UpdateSourceTilePayload,
    expected_revision: Option<i64>,
    _permit: &WritePermit,
) -> Result<CommandResponse> {
    let token = ctx.require_token()?;
    let revision = match expected_revision {
        Some(revision) => revision,
        None => {
            api_source_tiles::get_source_tile(ctx, token, id)
                .await
                .context("fetching source tile revision before update failed")?
                .source
                .revision
        }
    };
    let req = UpdateSourceTileRequest::new(payload).with_expected_revision(revision);
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

/// `GET /v1/timeline` — placements intersecting a required range. Read-only;
/// LABEL (`role = 1`) items are non-blocking and never count as busy time.
pub async fn timeline(ctx: &AppContext, query: &TimelineQuery) -> Result<Vec<TimelineItem>> {
    let token = ctx.require_token()?;
    api_timeline::get_timeline(ctx, token, query)
        .await
        .context("get_timeline failed")
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
pub async fn guard_write(ctx: &AppContext) -> Result<WritePermit> {
    let target =
        crate::safety::classify_write_target(&ctx.config, crate::safety::isolation_attested())?;
    verify_write_target(ctx, target).await?;
    Ok(WritePermit { _private: () })
}

/// The schedule publisher has an additional local-Core-only restriction
/// from the repository's acceptance contract.
pub async fn guard_schedule_publish_write(ctx: &AppContext) -> Result<WritePermit> {
    let target = crate::safety::classify_schedule_publish_target(
        &ctx.config,
        crate::safety::isolation_attested(),
    )?;
    verify_write_target(ctx, target).await?;
    Ok(WritePermit { _private: () })
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
            // A single page of up to 100 rows: large enough to notice a
            // target serving tiles under more than one owner, small enough
            // to keep the pre-write probe cheap.
            limit: Some(100),
            ..Default::default()
        },
    )
    .await;
    match probe {
        Ok(page) => ctx.confirm_write_owner(page.items.iter().map(|row| row.owner_id)),
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
    let permit = guard_write(ctx).await?;
    cancel_source_tile_with_permit(ctx, id, reason_code, expected_revision, &permit).await
}

pub(crate) async fn cancel_source_tile_with_permit(
    ctx: &AppContext,
    id: Uuid,
    reason_code: i32,
    expected_revision: Option<i64>,
    _permit: &WritePermit,
) -> Result<CommandResponse> {
    let token = ctx.require_token()?;
    let revision = match expected_revision {
        Some(revision) => revision,
        None => {
            api_source_tiles::get_source_tile(ctx, token, id)
                .await
                .context("fetching source tile revision before cancel failed")?
                .source
                .revision
        }
    };
    let req = CancelSourceTileRequest::new(reason_code).with_expected_revision(revision);
    api_source_tiles::cancel_source_tile(ctx, token, id, &req)
        .await
        .context("cancel_source_tile failed")
}

pub(crate) async fn reflow_source_tile_with_permit(
    ctx: &AppContext,
    id: Uuid,
    range: Span,
    expected_revision: Option<i64>,
    _permit: &WritePermit,
) -> Result<CommandResponse> {
    let token = ctx.require_token()?;
    let revision = match expected_revision {
        Some(revision) => revision,
        None => {
            api_source_tiles::get_source_tile(ctx, token, id)
                .await
                .context("fetching source tile revision before reflow failed")?
                .source
                .revision
        }
    };
    let req = ReflowSourceTileRequest::new(range).with_expected_revision(revision);
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

// ---------------------------------------------------------------------------
// Source-tile update merge (v1/14 §17 definition read-back).
//
// `PUT /v1/source-tiles/{id}` is a full replacement on the wire, so the CLI
// rebuilds the complete payload from the server's `definition` read-back and
// applies the user's patch on top. Omitted patch fields are preserved; an
// explicit `null` clears nullable fields; `[]` clears collections. `horizon`
// is a materialization input Core never persists, so every update must
// supply it explicitly.
// ---------------------------------------------------------------------------

const UPDATE_TILE_FIELDS: &[&str] = &["title", "description", "color", "icon", "external_id"];
const UPDATE_PLAN_FIELDS: &[&str] = &[
    "role",
    "completion",
    "planning",
    "metrics",
    "decisions",
    "references",
];
const UPDATE_SCHEDULE_OBJECT_FIELDS: &[&str] = &["generation", "window", "split_policy"];
const UPDATE_PATCH_FIELDS: &[&str] = &[
    "title",
    "description",
    "color",
    "icon",
    "external_id",
    "plan",
    "plan_role",
    "flows",
    "generation",
    "window",
    "split_policy",
    "priority",
    "required_duration_ms",
    "relations",
    "source_client_local_id",
    "horizon",
    "horizon_start",
    "horizon_end",
];

/// Merge an update patch into a complete `UpdateSourceTilePayload` using the
/// detail read's `definition` (v1/14 §17) plus `source.schedule` as the
/// preservation base.
pub fn merge_source_tile_update(
    detail: &SourceTileDetailRead,
    patch: &serde_json::Value,
) -> Result<UpdateSourceTilePayload> {
    let definition = detail.definition.as_ref().ok_or_else(|| {
        anyhow!(
            "the server did not return `definition` read-back (requires Core              OpenAPI v3.2.0 / v1/14 §17); refusing to merge without the              persisted definition"
        )
    })?;
    let patch_obj = patch
        .as_object()
        .ok_or_else(|| anyhow!("update patch must be a JSON object"))?;
    for key in patch_obj.keys() {
        if !UPDATE_PATCH_FIELDS.contains(&key.as_str()) {
            bail!(
                "unknown update field `{key}`; accepted fields: {}",
                UPDATE_PATCH_FIELDS.join(", ")
            );
        }
    }

    // Tile: absent keeps the read-back value; null clears the nullable
    // metadata fields; `title` is required non-null.
    let mut tile = serde_json::to_value(&definition.tile)?;
    let tile_obj = tile
        .as_object_mut()
        .ok_or_else(|| anyhow!("internal: tile definition did not serialize to an object"))?;
    for key in UPDATE_TILE_FIELDS {
        let Some(value) = patch_obj.get(*key) else {
            continue;
        };
        if *key == "title" {
            if !value.is_string() {
                bail!("`title` must be a string");
            }
        } else if !(value.is_string() || value.is_null()) {
            bail!("`{key}` must be a string or null");
        }
        tile_obj.insert((*key).to_string(), value.clone());
    }

    // Plan: `plan` is an object whose subfields merge individually, so a
    // patch can change only `metrics` without restating `completion`.
    let mut plan = serde_json::to_value(&definition.plan)?;
    let plan_obj = plan
        .as_object_mut()
        .ok_or_else(|| anyhow!("internal: plan definition did not serialize to an object"))?;
    if let Some(plan_patch) = patch_obj.get("plan") {
        let plan_patch_obj = plan_patch
            .as_object()
            .ok_or_else(|| anyhow!("`plan` must be a JSON object"))?;
        for key in plan_patch_obj.keys() {
            if !UPDATE_PLAN_FIELDS.contains(&key.as_str()) {
                bail!(
                    "unknown `plan` field `{key}`; accepted fields: {}",
                    UPDATE_PLAN_FIELDS.join(", ")
                );
            }
        }
        for key in UPDATE_PLAN_FIELDS {
            let Some(value) = plan_patch_obj.get(*key) else {
                continue;
            };
            match *key {
                "role" => {
                    if value.as_i64().is_none() {
                        bail!("`plan.role` must be an integer");
                    }
                }
                "completion" | "planning" => {
                    if !value.is_object() {
                        bail!("`plan.{key}` must be a JSON object");
                    }
                }
                _ => {
                    if !value.is_array() {
                        bail!("`plan.{key}` must be an array");
                    }
                }
            }
            plan_obj.insert((*key).to_string(), value.clone());
        }
    }
    if let Some(role_value) = patch_obj.get("plan_role") {
        let role = role_value
            .as_i64()
            .and_then(|value| i32::try_from(value).ok())
            .ok_or_else(|| anyhow!("`plan_role` must be an int32"))?;
        if let Some(existing) = plan_obj.get("role").and_then(|v| v.as_i64()) {
            if existing != i64::from(role) {
                bail!(
                    "`plan_role` ({role}) disagrees with `plan.role` ({existing}); make them match or omit one"
                );
            }
        }
        plan_obj.insert("role".to_string(), serde_json::json!(role));
    }

    // Schedule: read shape and write shape share SourceScheduleDefinitionSchema,
    // so the read-back value round-trips unchanged unless the patch overrides
    // individual subfields.
    let mut schedule = serde_json::to_value(&detail.source.schedule)?;
    let schedule_obj = schedule
        .as_object_mut()
        .ok_or_else(|| anyhow!("internal: schedule did not serialize to an object"))?;
    for key in UPDATE_SCHEDULE_OBJECT_FIELDS {
        if let Some(value) = patch_obj.get(*key) {
            if !value.is_object() {
                bail!("`{key}` must be a JSON object");
            }
            schedule_obj.insert((*key).to_string(), value.clone());
        }
    }
    for key in ["priority", "required_duration_ms"] {
        if let Some(value) = patch_obj.get(key) {
            if value.as_i64().is_none() {
                bail!("`{key}` must be an integer");
            }
            schedule_obj.insert((*key).to_string(), value.clone());
        }
    }

    let flows = match patch_obj.get("flows") {
        Some(value) => {
            if !value.is_array() {
                bail!("`flows` must be an array");
            }
            value.clone()
        }
        None => serde_json::to_value(&definition.flows)?,
    };

    // Relations: absent preserves the read-back payload refs (already
    // normalized to `existing` per v1/14 §17-2); explicit null omits the
    // field; an array replaces the complete set.
    let relations: Option<Vec<serde_json::Value>> = match patch_obj.get("relations") {
        Some(value) if value.is_null() => None,
        Some(value) => Some(
            serde_json::from_value(value.clone())
                .context("`relations` must be an array of relation payloads")?,
        ),
        None => Some(definition.relations.clone()),
    };

    // `source_client_local_id` is never persisted, so read-back is always
    // null; absent keeps null, an explicit UUID re-assigns it.
    let source_client_local_id: Option<Uuid> = match patch_obj.get("source_client_local_id") {
        Some(value) if value.is_null() => None,
        Some(value) => Some(
            value
                .as_str()
                .and_then(|raw| Uuid::parse_str(raw).ok())
                .ok_or_else(|| anyhow!("`source_client_local_id` must be a UUID string or null"))?,
        ),
        None => definition.source_client_local_id,
    };

    let horizon: Span = if let Some(horizon) = patch_obj.get("horizon") {
        serde_json::from_value(horizon.clone())
            .context("`horizon` must be an object with RFC3339 `start` and `end`")?
    } else {
        match (patch_obj.get("horizon_start"), patch_obj.get("horizon_end")) {
            (Some(start), Some(end)) => Span {
                start: parse_rfc3339_field(start, "horizon_start")?,
                end: parse_rfc3339_field(end, "horizon_end")?,
            },
            _ => bail!(
                "`horizon` is a materialization input Core never persists; supply                  `horizon` as an object with `start`/`end`, or `horizon_start` + `horizon_end`"
            ),
        }
    };

    let tile: ScheduleTileDefinition =
        serde_json::from_value(tile).context("merged `tile` failed validation")?;
    let plan: SchedulePlanDefinition =
        serde_json::from_value(plan).context("merged `plan` failed validation")?;
    let schedule: CreateSourceScheduleDefinition =
        serde_json::from_value(schedule).context("merged `schedule` failed validation")?;
    let flows: Vec<serde_json::Value> =
        serde_json::from_value(flows).context("merged `flows` failed validation")?;

    Ok(UpdateSourceTilePayload {
        tile,
        plan,
        flows,
        schedule,
        horizon,
        relations,
        source_client_local_id,
    })
}

fn parse_rfc3339_field(value: &serde_json::Value, field: &str) -> Result<DateTime<Utc>> {
    let raw = value
        .as_str()
        .ok_or_else(|| anyhow!("`{field}` must be an RFC3339 timestamp string"))?;
    DateTime::parse_from_rfc3339(raw.trim())
        .map(|dt| dt.with_timezone(&Utc))
        .map_err(|_| anyhow!("invalid `{field}` `{raw}`; expected RFC 3339"))
}

/// The payload Core effectively has stored, rebuilt from `definition` +
/// `source.schedule`. `horizon` is excluded because Core never persists it.
fn stored_update_payload_json(detail: &SourceTileDetailRead) -> Result<serde_json::Value> {
    let definition = detail
        .definition
        .as_ref()
        .ok_or_else(|| anyhow!("the server did not return `definition` read-back; cannot diff"))?;
    Ok(serde_json::json!({
        "tile": definition.tile,
        "plan": definition.plan,
        "schedule": detail.source.schedule,
        "flows": definition.flows,
        "relations": definition.relations,
        "source_client_local_id": definition.source_client_local_id,
    }))
}

/// Field-level diff between the stored definition and the merged update
/// payload. Unlike the old read-model diff this covers plan, flows,
/// relations, and schedule because both sides are now write-shaped.
pub fn diff_source_tile_update(
    detail: &SourceTileDetailRead,
    merged: &UpdateSourceTilePayload,
) -> Vec<FieldDiff> {
    let mut diffs = Vec::new();
    let Ok(stored) = stored_update_payload_json(detail) else {
        return diffs;
    };
    let Ok(proposed) = serde_json::to_value(merged) else {
        return diffs;
    };
    for key in [
        "tile",
        "plan",
        "schedule",
        "flows",
        "relations",
        "source_client_local_id",
    ] {
        diff_json_value(
            stored.get(key).unwrap_or(&serde_json::Value::Null),
            proposed.get(key).unwrap_or(&serde_json::Value::Null),
            key,
            &mut diffs,
            0,
        );
    }
    diffs
}

const MAX_UPDATE_DIFF_FIELDS: usize = 200;
const MAX_UPDATE_DIFF_DEPTH: usize = 8;

fn diff_json_value(
    current: &serde_json::Value,
    proposed: &serde_json::Value,
    path: &str,
    out: &mut Vec<FieldDiff>,
    depth: usize,
) {
    if current == proposed || out.len() >= MAX_UPDATE_DIFF_FIELDS {
        return;
    }
    if depth < MAX_UPDATE_DIFF_DEPTH {
        if let (Some(current_obj), Some(proposed_obj)) = (current.as_object(), proposed.as_object())
        {
            let mut keys: Vec<&String> = current_obj
                .keys()
                .chain(proposed_obj.keys())
                .collect::<std::collections::BTreeSet<_>>()
                .into_iter()
                .collect();
            keys.sort();
            for key in keys {
                diff_json_value(
                    current_obj.get(key).unwrap_or(&serde_json::Value::Null),
                    proposed_obj.get(key).unwrap_or(&serde_json::Value::Null),
                    &format!("{path}.{key}"),
                    out,
                    depth + 1,
                );
            }
            return;
        }
    }
    out.push(FieldDiff {
        field: path.to_string(),
        current: summarize_json(current),
        proposed: summarize_json(proposed),
    });
}

fn summarize_json(value: &serde_json::Value) -> String {
    let rendered = match value {
        serde_json::Value::Array(items) => format!("[{} item(s)]", items.len()),
        serde_json::Value::Null => "null".to_string(),
        other => serde_json::to_string(other).unwrap_or_else(|_| other.to_string()),
    };
    const MAX: usize = 120;
    if rendered.chars().count() > MAX {
        let truncated: String = rendered.chars().take(MAX - 1).collect();
        format!("{truncated}…")
    } else {
        rendered
    }
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
    pub owner_id: Uuid,
    pub placements_total: usize,
    pub placements_open: usize,
}

/// Read the source tile and its placements back from the server.
///
/// Post-write read-backs also pin the credential's owner identity: the
/// detail read always carries `owner_id`, so a mutation confirms the
/// account scope even on a target whose tile list is still empty.
pub async fn read_back(ctx: &AppContext, id: Uuid) -> Result<SourceTileState> {
    let token = ctx.require_token()?;
    let detail = api_source_tiles::get_source_tile(ctx, token, id)
        .await
        .context("get_source_tile failed")?;
    ctx.confirm_write_owner([detail.source.owner_id])?;
    Ok(SourceTileState {
        revision: detail.source.revision,
        source_state: detail.source.source_state,
        owner_id: detail.source.owner_id,
        placements_total: detail.placements.len(),
        placements_open: detail.placements.iter().filter(|p| !p.closed).count(),
    })
}

/// Confirm the credential's owner pin from a source tile detail read.
///
/// `create` needs this: its `CommandResponse` carries ids but no
/// `owner_id`, so the pin is taken from the detail read of the created
/// source instead of the write response itself.
pub async fn confirm_owner_from_source_tile(ctx: &AppContext, id: Uuid) -> Result<()> {
    let token = ctx.require_token()?;
    let detail = api_source_tiles::get_source_tile(ctx, token, id)
        .await
        .context("get_source_tile failed")?;
    ctx.confirm_write_owner([detail.source.owner_id])
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
    tastile_api::list_all_pending_prompts(
        ctx,
        token,
        &tastile_api::ListPendingPromptsQuery::default(),
    )
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

#[cfg(test)]
mod tests {
    use super::*;

    async fn context_capturing_first_request_line()
    -> (AppContext, WritePermit, tokio::task::JoinHandle<String>) {
        use tokio::io::AsyncBufReadExt;

        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .expect("bind request capture listener");
        let address = listener.local_addr().expect("listener address");
        let request_line = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.expect("accept API request");
            let mut reader = tokio::io::BufReader::new(stream);
            let mut line = String::new();
            reader
                .read_line(&mut line)
                .await
                .expect("read request line");
            line
        });

        let api_url = format!("http://{address}");
        let api = ApiClient::new(ApiConfig::new(&api_url).expect("valid loopback URL"))
            .expect("API client");
        let mut config = Config::default();
        config.api_url.clone_from(&api_url);
        let ctx = AppContext {
            api,
            config,
            token: Some(BearerToken::new("test-token")),
            subject: None,
            credential_api_base_url: Some(api_url),
            credential_expires_at: None,
            credential_owner_id: None,
        };
        (ctx, WritePermit { _private: () }, request_line)
    }

    #[tokio::test]
    async fn explicit_cancel_and_reflow_revisions_skip_detail_pre_reads() {
        for operation in ["cancel", "reflow"] {
            let (ctx, permit, request_line) = context_capturing_first_request_line().await;
            let id = Uuid::new_v4();
            let result = if operation == "cancel" {
                cancel_source_tile_with_permit(&ctx, id, 0, Some(12), &permit).await
            } else {
                let now = Utc::now();
                reflow_source_tile_with_permit(
                    &ctx,
                    id,
                    Span {
                        start: now,
                        end: now + chrono::Duration::hours(1),
                    },
                    Some(12),
                    &permit,
                )
                .await
            };

            assert!(result.is_err(), "capture server closes after request line");
            let request_line = request_line.await.expect("captured request");
            let expected_suffix = if operation == "cancel" {
                "cancel"
            } else {
                "reflow"
            };
            assert_eq!(
                request_line,
                format!("POST /v1/source-tiles/{id}/{expected_suffix} HTTP/1.1\r\n"),
                "an explicit revision should go straight to the mutation endpoint"
            );
        }
    }

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
        let req = build_create_request(&draft).unwrap();
        assert_eq!(req.payload.tile.title, "test");
        assert_eq!(req.payload.horizon.start, draft.horizon_start);
        let json = serde_json::to_value(&req).unwrap();
        assert!(json["idempotency_key"].is_string());
        assert_eq!(json["payload"]["schedule"]["priority"], 0);
    }

    #[test]
    fn create_draft_rejects_unknown_fields_instead_of_ignoring_typos() {
        let draft = CreateSourceTileDraft::minimal("test");
        let mut value = serde_json::to_value(draft).unwrap();
        value["horizion_start"] = value["horizon_start"].clone();

        assert!(serde_json::from_value::<CreateSourceTileDraft>(value).is_err());
    }

    #[test]
    fn plan_role_must_match_the_top_level_plan_role() {
        let mut draft = CreateSourceTileDraft::minimal("test");
        draft.plan = Some(serde_json::json!({
            "role": 1,
            "completion": {},
            "planning": {},
            "metrics": [],
            "decisions": [],
            "references": []
        }));
        assert!(validate_plan_role(&draft).is_err());

        draft.plan_role = 1;
        assert!(validate_plan_role(&draft).is_ok());
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
    fn owner_pin_rejects_a_different_observed_owner() {
        let mut config = Config::default();
        config.api_url = "https://api.example.test".to_string();
        let api = ApiClient::new(ApiConfig::new(&config.api_url).unwrap()).unwrap();
        let pinned = Uuid::new_v4();
        let other = Uuid::new_v4();
        let ctx = AppContext {
            api,
            config,
            token: Some(BearerToken::new("test-token")),
            subject: None,
            credential_api_base_url: Some("https://api.example.test".to_string()),
            credential_expires_at: None,
            credential_owner_id: Some(pinned.to_string()),
        };
        let err = ctx
            .confirm_write_owner([other])
            .expect_err("a different owner must fail closed");
        assert!(err.to_string().contains("pinned to owner"), "{err}");
        ctx.confirm_write_owner([pinned, pinned])
            .expect("the pinned owner must pass");
    }

    #[test]
    fn owner_pin_rejects_ambiguous_multi_owner_scope() {
        let mut config = Config::default();
        config.api_url = "https://api.example.test".to_string();
        let api = ApiClient::new(ApiConfig::new(&config.api_url).unwrap()).unwrap();
        let ctx = AppContext {
            api,
            config,
            token: Some(BearerToken::new("test-token")),
            subject: None,
            credential_api_base_url: Some("https://api.example.test".to_string()),
            credential_expires_at: None,
            credential_owner_id: None,
        };
        let err = ctx
            .confirm_write_owner([Uuid::new_v4(), Uuid::new_v4()])
            .expect_err("multiple owners must fail closed");
        assert!(err.to_string().contains("distinct owners"), "{err}");
    }

    #[test]
    fn owner_pin_store_conflict_fails_closed() {
        // A pin already committed in the store must be compared against the
        // fresh observation even when this context's snapshot predates it.
        let dir = std::env::temp_dir().join(format!("tastile-owner-pin-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let credential_path = dir.join("credential.json");
        let api_url = "http://127.0.0.1:39999";
        let pinned = Uuid::new_v4();
        let other = Uuid::new_v4();
        std::fs::write(
            &credential_path,
            serde_json::json!({
                "api_base_url": api_url,
                "bearer": "test-only-token",
                "expires_at": null,
                "subject": null,
                "owner_id": pinned.to_string()
            })
            .to_string(),
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&credential_path, std::fs::Permissions::from_mode(0o600))
                .unwrap();
        }
        let store = tastile_auth::FileCredentialStore::new(&credential_path);

        let err = AppContext::persist_owner_pin_with(&store, api_url, other)
            .expect_err("a store pin conflicting with the observation must fail closed");
        assert!(err.to_string().contains("pinned to owner"), "{err}");
        AppContext::persist_owner_pin_with(&store, api_url, pinned)
            .expect("an observation matching the store pin must pass");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn owner_pin_allows_an_empty_observation() {
        let mut config = Config::default();
        config.api_url = "https://api.example.test".to_string();
        let api = ApiClient::new(ApiConfig::new(&config.api_url).unwrap()).unwrap();
        let ctx = AppContext {
            api,
            config,
            token: Some(BearerToken::new("test-token")),
            subject: None,
            credential_api_base_url: Some("https://api.example.test".to_string()),
            credential_expires_at: None,
            credential_owner_id: None,
        };
        ctx.confirm_write_owner(std::iter::empty())
            .expect("an empty read cannot identify an owner and must not block");
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
            credential_owner_id: None,
        };
        assert!(
            ctx.require_token()
                .unwrap_err()
                .to_string()
                .contains("different API base")
        );
    }

    #[tokio::test]
    async fn today_returns_only_tiles_with_a_non_past_projected_start() {
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt};

        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .expect("bind API mock");
        let address = listener.local_addr().expect("API mock address");
        let api_url = format!("http://{address}");
        let now = Utc::now();
        let stale = tile_list_item("stale", Some(now - chrono::Duration::hours(1)));
        let upcoming = tile_list_item("upcoming", Some(now + chrono::Duration::hours(1)));
        let no_projection = tile_list_item("none", None);
        let body = serde_json::to_vec(&[stale, upcoming, no_projection]).unwrap();
        let request_line = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.expect("accept API request");
            let (reader, mut writer) = stream.into_split();
            let mut reader = tokio::io::BufReader::new(reader);
            let mut line = String::new();
            reader
                .read_line(&mut line)
                .await
                .expect("read request line");
            loop {
                let mut header = String::new();
                reader
                    .read_line(&mut header)
                    .await
                    .expect("read request header");
                if header == "\r\n" || header.is_empty() {
                    break;
                }
            }
            let response = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                body.len()
            );
            writer
                .write_all(response.as_bytes())
                .await
                .expect("write response headers");
            writer.write_all(&body).await.expect("write response body");
            line
        });

        let api = ApiClient::new(ApiConfig::new(&api_url).unwrap()).unwrap();
        let mut config = Config::default();
        config.api_url.clone_from(&api_url);
        let ctx = AppContext {
            api,
            config,
            token: Some(BearerToken::new("test-token")),
            subject: None,
            credential_api_base_url: Some(api_url),
            credential_expires_at: None,
            credential_owner_id: None,
        };

        let tiles = today(&ctx).await.unwrap();
        let request_line = request_line.await.unwrap();

        assert_eq!(tiles.len(), 1);
        assert_eq!(tiles[0].title, "upcoming");
        assert!(request_line.contains("exclude_future=false"));
    }

    fn tile_list_item(title: &str, projected_next_start_at: Option<DateTime<Utc>>) -> TileListView {
        TileListView {
            id: Uuid::new_v4(),
            title: title.to_string(),
            lifecycle: 0,
            worked_minutes: 0,
            break_minutes: 0,
            labels: Vec::new(),
            objective_mode: 0,
            plan_id: None,
            projected_next_start_at,
            done_definition: None,
            done_rule: None,
            next_action: None,
            resume_note: None,
            recurrence: None,
            source: None,
            target_rest_min: None,
            target_work_min: None,
            temporal: None,
        }
    }
    fn update_test_detail() -> SourceTileDetailRead {
        serde_json::from_value(serde_json::json!({
            "source": {
                "source_tile_id": Uuid::new_v4(),
                "plan_id": Uuid::new_v4(),
                "owner_id": Uuid::new_v4(),
                "revision": 7,
                "source_state": 0,
                "title": "stored title",
                "plan_role": 0,
                "schedule": {
                    "required_duration_ms": 1800000,
                    "priority": 3,
                    "generation": {"kind": 1, "interval_ms": 86400000},
                    "window": {"start_offset_ms": -3600000, "end_offset_ms": 3600000},
                    "split_policy": {"kind": 0}
                },
                "created_at": "2026-01-01T00:00:00Z",
                "updated_at": "2026-01-01T00:00:00Z"
            },
            "relations": [],
            "occurrences": [],
            "placements": [],
            "definition": {
                "tile": {"title": "stored title", "description": "keep me"},
                "plan": {
                    "role": 0,
                    "completion": {"root": {"All": []}, "tasks": [{"id": "t1"}], "time_requirements": []},
                    "planning": {"nesting_rules": [{"a": 1}], "placement_rules": []},
                    "metrics": [{"m": 1}],
                    "decisions": [{"d": 1}],
                    "references": [{"r": 1}]
                },
                "flows": [{"flow": 1}],
                "relations": [{"subject": {"kind": "existing", "source_tile_id": Uuid::new_v4()}}],
                "source_client_local_id": null
            }
        }))
        .expect("detail")
    }

    fn horizon_patch() -> serde_json::Value {
        serde_json::json!({
            "horizon": {"start": "2026-11-20T09:00:00Z", "end": "2026-11-20T10:00:00Z"}
        })
    }

    #[test]
    fn update_merge_preserves_unmentioned_fields() {
        let detail = update_test_detail();
        let mut patch = horizon_patch();
        patch["plan"]["metrics"] = serde_json::json!([{"m": 2}]);
        let merged = merge_source_tile_update(&detail, &patch).expect("merge");

        assert_eq!(merged.plan.metrics, vec![serde_json::json!({"m": 2})]);
        assert_eq!(
            merged.plan.completion["tasks"],
            serde_json::json!([{"id": "t1"}])
        );
        assert_eq!(
            merged.plan.planning["nesting_rules"],
            serde_json::json!([{"a": 1}])
        );
        assert_eq!(merged.plan.decisions, vec![serde_json::json!({"d": 1})]);
        assert_eq!(merged.plan.references, vec![serde_json::json!({"r": 1})]);
        assert_eq!(merged.flows, vec![serde_json::json!({"flow": 1})]);
        assert_eq!(merged.relations.as_ref().map(Vec::len), Some(1));
        assert_eq!(merged.schedule.priority, 3);
        assert_eq!(merged.tile.description.as_deref(), Some("keep me"));
        assert_eq!(
            merged.horizon.start.to_rfc3339(),
            "2026-11-20T09:00:00+00:00"
        );
    }

    #[test]
    fn update_merge_null_and_empty_clear_semantics() {
        let detail = update_test_detail();
        let mut patch = horizon_patch();
        patch["description"] = serde_json::Value::Null;
        patch["relations"] = serde_json::Value::Null;
        patch["flows"] = serde_json::json!([]);
        let merged = merge_source_tile_update(&detail, &patch).expect("merge");

        assert_eq!(merged.tile.description, None);
        assert_eq!(merged.relations, None);
        assert!(merged.flows.is_empty());
    }

    #[test]
    fn update_merge_requires_horizon() {
        let detail = update_test_detail();
        let patch = serde_json::json!({"title": "new title"});
        assert!(merge_source_tile_update(&detail, &patch).is_err());
    }

    #[test]
    fn update_merge_rejects_unknown_fields() {
        let detail = update_test_detail();
        let mut patch = horizon_patch();
        patch["bogus"] = serde_json::json!(1);
        assert!(merge_source_tile_update(&detail, &patch).is_err());

        let mut patch = horizon_patch();
        patch["plan"]["bogus"] = serde_json::json!(1);
        assert!(merge_source_tile_update(&detail, &patch).is_err());
    }

    #[test]
    fn update_merge_rejects_plan_role_conflict() {
        let detail = update_test_detail();
        let mut patch = horizon_patch();
        patch["plan_role"] = serde_json::json!(1);
        patch["plan"]["role"] = serde_json::json!(0);
        assert!(merge_source_tile_update(&detail, &patch).is_err());
    }

    #[test]
    fn update_diff_covers_plan_and_schedule() {
        let detail = update_test_detail();
        let mut patch = horizon_patch();
        patch["plan"]["metrics"] = serde_json::json!([{"m": 2}]);
        patch["priority"] = serde_json::json!(9);
        let merged = merge_source_tile_update(&detail, &patch).expect("merge");
        let diffs = diff_source_tile_update(&detail, &merged);
        let fields: Vec<&str> = diffs.iter().map(|d| d.field.as_str()).collect();

        assert!(fields.contains(&"plan.metrics"), "diffs: {fields:?}");
        assert!(fields.contains(&"schedule.priority"), "diffs: {fields:?}");
        assert!(
            !fields.iter().any(|f| f.starts_with("tile.")),
            "diffs: {fields:?}"
        );
        assert!(
            !fields.iter().any(|f| f.starts_with("flows")),
            "diffs: {fields:?}"
        );
    }
}
