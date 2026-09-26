# Changelog

All notable changes to `tastile-cli` are documented here. The format is
based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the
project adheres to [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added

- Initial scaffold.
- Standalone Git repository with no sibling-repo runtime/build dependency.
- `openapi/` git submodule pinned to `tastile-openapi@v1.0.0`
  (`b0c781dc18111645324e2abc38621b1564d4c518`).
- `crates/tastile-api` — typed HTTP client + structural build-time
  drift gate. The `OperationContract` table in `build.rs` enforces
  for every typed call:
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
  - Server-side route is not yet exposed; bridge returns
    `ServerEndpointUnavailable` with a copy-pasteable curl body so
    the exchange can be driven by hand.
  - `redact_token()` strips `bearer <value>` from server messages
    case-insensitively.
- `crates/tastile-config` — file-based config + env overrides.
- `crates/tastile-cli` — `tastile` binary with a **shared
  application service layer** (`src/app.rs`) used by both the CLI
  subcommands and the TUI:
  - `auth login | status | logout | exchange`
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

### Known gaps (server-side)

The end-to-end auth flow stops at `ServerEndpointUnavailable` until the
web origin exposes:

| Endpoint | Method | Auth | Body | Returns |
| --- | --- | --- | --- | --- |
| `/cli/authorize` | GET | Better Auth session | (query) | `302 {redirect_uri}?code=…&state=…` |
| `/api/cli/token` | POST | none (PKCE + grant) | `{code, code_verifier, redirect_uri}` | `{token, expires_at?, subject?}` |

CLI side is wired; flip the bridge body once both routes ship.
