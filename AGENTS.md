# AGENTS — tastile-cli

This file is the canonical entry point for AI agents and human contributors
working on `tastile-cli`. Read fully before opening any tool.

## 1. Repository independence

`tastile-cli` is a **standalone Git repository**. It must never depend on
`tastile-root` or any sibling repository at build time or runtime. The
Tastile v1 API contract is pulled in via the `openapi/` git submodule,
which is pinned to a specific revision per `tastile-cli` commit.

CI runs against a fresh checkout of this repository alone — no
`tastile-root`, no `tastile-core`, no `tastile-web`. If your change breaks
that invariant, the change is wrong.

## 2. Canonical wire contract

| Concern | SoT |
| --- | --- |
| HTTP shape (paths, methods, schemas) | `openapi/openapi.yaml` (pinned submodule) |
| Typed Rust client surface | `crates/tastile-api/src/*.rs` |
| Drift gate | `crates/tastile-api/build.rs` + `scripts/check-openapi-drift.sh` |

When you bump the pinned OpenAPI revision, run:

```sh
scripts/sync-openapi.sh
```

This updates the gitlink, runs `cargo build` (which exercises the build.rs
drift gate), runs `cargo test`, and runs the shell drift check.

## 3. Code boundaries

`tastile-cli` talks to the Tastile API via HTTPS only.

Forbidden:

- Importing `tastile-core` (no `domain`, no `storage`, no `api`).
- Direct PostgreSQL access.
- Embedding web bridge secrets or core internal secrets.
- Generating code that duplicates the OpenAPI contract.

Allowed:

- The pinned `openapi.yaml` as the only source of truth.
- Any direct REST call to `https://<api-base>/v1/*` that is documented in
  the pinned spec.

## 4. Authentication

Login is browser-mediated; see `crates/tastile-auth/src/lib.rs`. The token
exchange relies on a server-side endpoint (`POST /api/cli/api-token`) that
mirrors `/api/mobile/api-token` and is **not yet exposed**. Until that
endpoint lands, `tastile auth login` captures the authorization code and
prints the structured request for manual exchange.

The bearer token is stored in the OS credential store under
service `tastile-cli`, user `default`. Never log the token. Never print it
in `doctor` or `auth status`.

## 5. Toolchain

- Rust pinned via `mise.toml` (`rust = "1.98.1"`).
- `mise install` is the canonical bootstrap.
- `mise run ci` is the canonical validation entry point.

## 6. Quality gates

Before commit:

```sh
cargo fmt --all
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
scripts/check-openapi-drift.sh
```

CI runs the same checks plus a scheduled OpenAPI drift cron. See
`.github/workflows/`.

## 7. Branch / PR lifecycle

- Main branch is protected.
- Release branches follow `release-X-Y-Z`.
- Ticket branches follow the issue number (`123`, not `feature/123`).
- Use `git commit -m 'chore: ...'` style; English commit messages.

## 8. Project-init alignment

`tastile-cli` follows the Tastile project-init policy at canonical version
[`rebuildup/project-init`](https://github.com/rebuildup/project-init). The
relevant shared artifacts are mirrored as `.claude/skills/` for the Claude
Code adapter. See `docs/development.md` for the local conventions.

## 9. Recovery

If you arrive mid-task:

1. Read this file (`AGENTS.md`).
2. Read `docs/architecture.md`.
3. Check `git status` for in-flight changes.
4. Run `mise run ci` to see the current state.
5. Read the most recent memory file under `~/.claude/projects/.../memory/`.
