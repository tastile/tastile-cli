# Changelog

All notable changes to `tastile-cli` are documented here. The format is
based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the
project adheres to [Semantic Versioning](https://semver.org/).

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
