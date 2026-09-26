//! `tastile-api` — typed surface for the Tastile v1 HTTP API.
//!
//! This crate is the **wire contract** of `tastile-cli`. It exposes a small,
//! deliberately curated subset of the full OpenAPI spec that the CLI MVP
//! actually drives. Every operation here corresponds to an `operationId` in
//! `openapi/openapi.yaml`; the drift gate in `build.rs` enforces that the
//! pinned spec still contains each of those operation IDs.
//!
//! # Repository independence
//!
//! `tastile-api` does **not** depend on `tastile-core`. It speaks HTTP, parses
//! JSON, and stays out of domain internals. The only sources of truth for the
//! wire shape are:
//!
//! 1. `openapi/openapi.yaml` (pinned git submodule)
//! 2. The hand-written types in this crate, which the drift gate keeps in sync
//!    with (1) at build time
//!
//! # Auth
//!
//! All operations accept a `&BearerToken` and inject
//! `Authorization: Bearer <raw token>`. The token itself is owned by
//! `tastile-auth`; this crate never persists it.
//!
//! # Versioning
//!
//! When bumping the pinned `tastile-openapi` submodule:
//!
//! 1. `git submodule update --remote openapi` (operator action, after the
//!    new revision has been published and reviewed in `tastile-openapi`).
//! 2. `mise run sync-openapi` to update the pin commit in this repo.
//! 3. Run `mise run ci` locally — the build script and the
//!    `check-openapi-drift` script will report any wire-shape changes.
//! 4. Update this crate's types, `Cargo.toml` if any version gates apply, and
//!    the README's "Wire contract" section.

#![doc(html_root_url = "https://docs.rs/tastile-api/0.1.0")]

pub mod auth;
pub mod client;
pub mod error;
pub mod model;
pub mod prompts;
pub mod source_tiles;
pub mod tiles;

pub use client::{ApiClient, ApiConfig, BearerToken};
pub use error::{ApiError, ApiResult};
pub use model::{ApiVersion, CommandResponse, OwnerDeleteResponse, PendingWork, TileListView};
pub use prompts::{
    PromptSubject, PromptView, RequestPromptRequest, ResolvePromptRequest, list_pending_prompts,
    request_prompt, resolve_prompt,
};
pub use source_tiles::{
    CancelSourceTilePayload, CancelSourceTileRequest, PlacementTileRead, RecurrenceView,
    SourceOccurrenceRead, SourceScheduleDefinition, SourceTileDetailRead, SourceTileRead,
    SourceTileSummary, TemporalView, cancel_source_tile, get_source_tile, list_source_tiles,
};
pub use tiles::list_tiles;

/// Compile-time API version metadata pulled from the pinned
/// `openapi/openapi.yaml` by `tastile-api`'s `build.rs`.
pub const API_VERSION: &str = env!("TASTILE_API_OPENAPI_VERSION");
pub const API_TITLE: &str = env!("TASTILE_API_OPENAPI_TITLE");
