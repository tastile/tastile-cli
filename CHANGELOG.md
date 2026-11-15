# Changelog

All notable changes to `tastile-cli` are documented here. The format is
based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the
project adheres to [Semantic Versioning](https://semver.org/).

## [Unreleased]

Local-first hardening for issue
[tastile/tastile-cli#6](https://github.com/tastile/tastile-cli/issues/6).

### Added

- **Write-target gate for every mutating command** (`source-tiles`
  create/update/cancel/reflow, `executions`, `prompts`,
  `schedule publish --apply`, server-side revocation in `auth logout`, and
  the matching TUI actions). A loopback
  target requires `TASTILE_ISOLATED_TEST_STACK=confirmed` and must not
  use the reserved operational port 31400; remote API bases require HTTPS.
  Before the request leaves the process the CLI probes the target with a
  documented read endpoint and requires the stored credential to be
  accepted. Credentials are API-base-bound and checked for expiry before
  every authenticated API request.
- **Short recurrence input** for `source-tiles create`: `--once`,
  `--daily`, `--weekly --days`, `--exclude`, `--duration`,
  `--horizon-days`, `--priority`, `--utc-offset`. Monthly is refused with
  an explicit error instead of being approximated as daily.
- **Non-destructive diff** (`source-tiles update --dry-run`), **impact
  preview** (`cancel` / `reflow --dry-run`) and **post-write readback**
  of the resulting source-tile state.
- **Revision conflict detection** via `--expect-revision`, mapped to the
  contract's `expected_revision` envelope field.
- **Reusable schedule-publish retries** — `schedule publish --apply` now
  requires an explicit `--idempotency-key`.
- `docs/cli-operations.md` — operation guide, expected error messages and
  a safe stop/resume procedure.
- `scripts/e2e-local-stack.sh` — spins up a dedicated isolated Core stack
  (own PostgreSQL container, database, owner, API and worker on a port
  other than 31400) for CLI end-to-end tests.
- `scripts/e2e-cli-smoke.sh` — drives the CLI against that stack and
  asserts the guards, the CRUD path, the diff, the revision conflict, the
  impact preview, the readback and the execution lifecycle.
- `TASTILE_CREDENTIAL_FILE` — a **test-only** file-backed credential
  store, honoured only for a loopback API base (the OS credential store
  stays the production path). It exists because an isolated Core stack has
  no web origin to mint a PKCE token and a headless runner has no
  reachable keychain. The file must not be readable by other users.

### Fixed

- `source-tiles cancel` sent a free-text reason; the contract defines
  `CancelSourceTilePayloadSchema.reason` as a numeric registry code, so
  the server rejected it. The CLI now sends `--reason-code` (0 =
  user-requested).
- The source-tile **read model** did not match the contract:
  `SourceTileRead.schedule` is `SourceScheduleDefinitionSchema`
  (duration, generation, window, split policy, priority), but the CLI
  modelled a different projection, so `source-tiles list` / `get` failed
  to decode against a real Core. The model now follows the contract, and
  the update diff compares the schedule fields exposed by the read model.
- `executions pause` / `resume` omitted the envelope's `payload` member,
  which the server requires (`422`). The CLI now sends `payload: null`
  as the contract describes.
- `prompts request` sent `payload.prompt_kind`; the contract names the
  field `kind`, so the request was rejected with `422`.
- `source-tiles list` and `source-tiles placements` silently stopped at
  the server's default page size. Source-tile list, detail, and placements
  now walk the documented `limit` / `offset` pages until the end of the
  result set. `GET /v1/tiles` and pending-prompt list have no cursor or
  offset in the pinned contract, so those views remain single-request.
- `source-tiles create` sent a window object the contract does not
  define and a one-time generation without its instant, so the server
  rejected the payload. The defaults now match
  `SourceWindowSchema` / `SourceGenerationSchema`.
- `tiles` sent query parameters that the pinned contract does not define
  (`window_start`, `window_end`, `plan_id`, `include_closed`); the CLI
  now only sends the documented ones (`lifecycle`, `search`, `view_mode`,
  `limit`, `exclude_future`).
- `executions start` did not print the id of the execution it created;
  the aggregate id is now shown, so the execution can be paused, resumed
  or finished.
- Errors printed only the outer context; the CLI now prints the whole
  error chain, so the root cause (for example a credential-store backend
  error) is visible.
- `cargo fmt --all -- --check` failed on the branch (the schedule publish
  code was not formatted).
- Stored credentials are now bound to their API base and expiry before
  writes; bearer values are redacted from `Debug` output. `auth logout`
  passes server-side revocation through the same gate and always removes
  the local credential.
- Source-tile update now rejects partial JSON drafts because the pinned
  `PUT` contract is a full replacement and the read model omits plan
  details, flows, horizon, request-shaped relations and
  `source_client_local_id`.
- Isolated Core E2E showed that schedule publication requires
  `source_client_local_id`, `source_schedule`, and `source_horizon` together
  when any is supplied; the CLI now refuses partial linkage offline. Core
  also rejects empty completion condition trees, which remains a
  server-owned semantic check beyond the pinned schema shape.

## [1.0.0] — 2026-09-27

First production release of `tastile-cli`. The binary speaks directly to
the Tastile v1 HTTP API and integrates with the browser-mediated PKCE
flow exposed by `tastile-web` (`/cli/authorize` + `/api/cli/token`,
tracked in [tastile/tastile-web#153](https://github.com/tastile/tastile-web/issues/153)).

### Added

- Standalone Git repository with no sibling-repo runtime/build dependency.
- `openapi/` git submodule pinned to `tastile-openapi@v1.0.1`
  (`0a8a66b54720587f238ba35207c0c7ddfd2ad4d5`, tag `66b9d8e`). The pin
  was originally v1.0.0 (`b0c781d`) at scaffold time and was bumped in
  commit `cb935a0` to track Core 1.0.1's wire contract refresh
  (Issue #153 §A.1/A.2 granular API-token scope +
  `x-tastile-required-scope` operation extension). The
  `scripts/sync-openapi.sh` script bumps the pin and re-runs the
  drift gate.
- `crates/tastile-api` — typed HTTP client + structural build-time drift
  gate. The `OperationContract` table in `build.rs` enforces for every
  typed call:
  - path, method, and `operationId`;
  - merged method-level + path-item-level path parameters;
  - request body `$ref` and required `payload` field names;
  - 200 response shape (`None` / `Ref` / `ArrayOf` / `Empty`).
  - **18 of 21** operations in the pinned spec are wired in this CLI
    surface (`tiles`, `prompts`, `source_tiles`, `executions`,
    `auth/signout`). The 3 admin operations
    (`delete_owner`, `export_owner`, `publish_schedule_definition`)
    are out of scope for the initial CLI and intentionally absent
    from the contract table.
- `crates/tastile-auth` — OS credential store + PKCE (RFC 7636) +
  loopback callback + browser-mediated **authorization-code grant**
  bridge. The CLI does **not** touch the Better Auth session cookie:
  - `POST {web_url}/api/cli/token` with `{code, code_verifier,
    redirect_uri}` (no cookie, no `client_id`).
  - `redact_token()` strips `bearer <value>` from server messages
    case-insensitively.
- `crates/tastile-config` — file-based config + env overrides.
- `crates/tastile-cli` — `tastile` binary with a **shared
  application service layer** (`src/app.rs`) used by both the CLI
  subcommands and the TUI:
  - `auth login | status | logout`
  - `doctor`
  - `tiles`, `today`
  - `source-tiles list | get | create | update | cancel | completion | placements | reflow`
  - `executions start | pause | resume | finish`
  - `prompts list | request | startup-recovery | resolve`
    (with `--resolution ack|dismiss|act`)
  - `schedule regenerate`
  - `completions <shell>`, `version`
- TUI (`tastile` with no subcommand) — 3-pane layout (tiles,
  source tiles, prompts) backed by the same `app::*` functions as
  the CLI. Keybindings: `q/Esc` quit, `r` refresh, `↑↓` navigate,
  `n` new tile, `c` cancel focused, `P` resolve focused prompt,
  `a` trigger auth login, `s/p/R/f` execution command hints.
- `scripts/check-openapi-drift.sh` — structural drift check that
  mirrors the `build.rs` table (CI + weekly cron).
- `scripts/sync-openapi.sh` — bump the pinned submodule revision.
- GitHub Actions: `ci.yml`, `openapi-drift.yml`, `release-source-check.yml`.
- `mise.toml` with canonical `ci` task.
- README, AGENTS.md, CLAUDE.md, CONTRIBUTING.md.
- 10 mocked integration tests in
  `crates/tastile-auth/tests/auth_flow.rs` covering the wire contract,
  scope boundary, open-redirect, and atomic single-use consume (the
  N=8 parallel exchange assertion).

### Fixed

- `tastile auth login` now actually issues the HTTP `POST /api/cli/token`
  call via `HttpServerBridge::fetch_token` against the live web origin,
  closing the CLI half of [tastile/tastile-web#153](https://github.com/tastile/tastile-web/issues/153).
- `tastile auth login` no longer appends `/cli/callback` on top of the
  listener's `redirect_uri` — the listener URL is now used verbatim.
- Merge policy documented as **merge commit only** (squash / rebase
  forbidden).

### Removed

- `tastile auth exchange` subcommand. It required the PKCE verifier
  and the loopback `redirect_uri` at call time, but the CLI invocation
  only had the OAuth authorization code (the verifier stays in the
  `auth login` flow that owns the loopback listener). Every call
  therefore sent `code_verifier=''` and
  `redirect_uri='/cli/callback'` to `/api/cli/token`, which always
  failed with `redirect_uri_mismatch` / PKCE failure. The end-to-end
  `tastile auth login` flow is the supported way to exchange an
  authorization code.

### Server-side deployment dependency

The CLI calls `POST {web_url}/api/cli/token` directly through
`HttpServerBridge::fetch_token`. The web origin's `/cli/authorize`
and `/api/cli/token` endpoints are the deployment dependency tracked
in [tastile/tastile-web#153](https://github.com/tastile/tastile-web/issues/153).
Until that ships to the target environment, `tastile auth login` will
surface a real HTTP / transport error from the bridge.
