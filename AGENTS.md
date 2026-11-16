# AGENTS — tastile-cli

This file is the canonical entry point for AI agents and human contributors
working on `tastile-cli`. Read fully before opening any tool. It is a
dispatcher: project invariants live here, procedures live in the Skills
referenced below.

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

The pinned spec declares 21 `operationId`s. The CLI surface covers 19
of them, including the isolated local Core `publish_schedule_definition`
operation. The 2 remaining operations (`delete_owner`, `export_owner`) are
administrative endpoints not covered by the drift gate. `schedule publish`
previews without any network access, and `--apply` must reject remote targets.
Only the server can validate scheduling semantics; a successful preview does
not establish that occurrences were correctly materialized.

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

Login is browser-mediated; see `crates/tastile-auth/src/lib.rs`. The CLI
generates PKCE (`code_verifier`, `code_challenge`, `state`,
`redirect_uri`) and opens `{web_url}/cli/authorize?…`. The web side owns
the Better Auth session cookie and mints a one-time authorization
grant. The CLI exchanges the grant for a Tastile API bearer token at
`POST {web_url}/api/cli/token` with body
`{ code, code_verifier, redirect_uri }` — **no Better Auth cookie**.

Until both web-side routes (`/cli/authorize` and `/api/cli/token`) ship,
`tastile auth login` captures the grant and prints the structured
request body so an operator can drive the exchange by hand.

The bearer token is stored in the OS credential store under
service `tastile-cli`, user `default`. Never log the token. Never print it
in `doctor` or `auth status`.

## 5. Toolchain

- Rust pinned via `mise.toml` (`rust = "1.98.1"`).
- `mise install` is the canonical bootstrap (also installs `bun`, used
  only by the Agent Skills tooling).
- `mise run ci` is the canonical validation entry point.
- Run commands through `mise run <task>` or `mise exec -- <command>`.

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

## 7. Working agreements

- **Source of truth**: `main` is the released state; `release-X-Y-Z`
  branches are the only path into `main`; GitHub Issues are the durable
  ticket; secrets live in the OS credential store (never in the repo).
- **Start of work**: pick or open a GitHub Issue → branch named after
  the issue number only (`123`, not `feature/123`) → first meaningful
  commit → push → open a Draft PR immediately → `mise run ci` green →
  release PR to `main` (merge commit only). Details:
  `.claude/skills/github-delivery/SKILL.md` and `docs/release.md`.
- **Implementation judgment**: decide autonomously per
  `.claude/skills/engineering-decisions/SKILL.md`; escalate to the user
  only at the boundaries that skill defines.
- **Prose**: before writing anything that persists (docs, ADRs, Issues,
  PRs, commits, review comments), use
  `.claude/skills/writing-discipline/SKILL.md`.
- **Recovery**: resume from Issue / PR / branch / committed docs — never
  from conversation history. Details:
  `.claude/skills/agent-recovery/SKILL.md`.
- **Worktrees**: ticket work runs in `git worktree`s under `~/work/`
  (e.g. `tcli-<issue>`). `CARGO_TARGET_DIR` is repo-local, so each
  worktree gets its own `target/`; no shared mutable state to arbitrate.
  Lifecycle details: `.claude/skills/worktree-workflow/SKILL.md`.
- **Delegation**: top-level delegation to subagents / workers follows
  `.claude/skills/parallel-orchestration/SKILL.md` with
  `herdr-runtime`, `sandbox-runtime`, and `worktree-workflow` for the
  execution side.

## 8. Agent Skills

Skills are vendored project-locally under `.claude/skills/` (Claude
Code) and `.devin/skills/` (Devin); `skills-lock.json` records sources
and hashes. They are managed by the `skills` CLI via mise:

```sh
mise run skills-bootstrap   # restore from skills-lock.json (fresh-clone fallback)
mise run skills-update      # update, then review the diff before committing
```

Do not edit the vendored copies; upstream changes land via
`skills-update` + PR review.

| Need | Skill |
| --- | --- |
| Issue / branch / PR / release lifecycle | `github-delivery` |
| Resume after interruption / checkpoint | `agent-recovery` |
| Decompose and delegate to workers | `parallel-orchestration`, `herdr-runtime`, `sandbox-runtime`, `worktree-workflow` |
| Quality gate / CI / verification level | `quality-gate` |
| Non-trivial feature / architecture design | `design-refinement` |
| Implementation / design decisions | `engineering-decisions` |
| Reproducible bug diagnosis | `diagnosing-bugs` |
| Regression test for a cheap local target | `tdd` — **manual**: read `.claude/skills/tdd/SKILL.md` only when the user asks for TDD or the bug has an obvious cheap test path |
| Review a diff against standards + spec | `code-review` |
| Naming / rename decisions | `code-naming` |
| Correctness invariants | `correctness-assurance` |
| Dependency vulnerability intake | `security-maintenance` |
| Mid/long-term release estimation | `agent-delivery-estimation` |
| Contributor onboarding docs | `onboarding` |
| Persistent prose | `writing-discipline` |
| User interaction / blockers | `interaction-discipline` |
| Skill or policy changes | `policy-evaluation`, `skill-selection` |

## 9. House rules

- Source code (identifiers, comments, developer-facing logs) and commit
  messages are English. This repository's docs, Issues, and PRs are also
  English — that is the established project convention and overrides the
  organization default for internal documentation.
- Commit messages: `<work-prefix>: <short title>` (conventional commits,
  e.g. `feat(cli):`, `chore:`, `docs:`).
- Text search: `rg` / `rg --files`.
- No new `.py` scripts; use the project language (Rust) or shell.
- Project-specific rules live in this repository, never only in global
  config, home directory, or agent-private memory.

## 10. Project-init alignment

`tastile-cli` follows the [rebuildup/project-init](https://github.com/rebuildup/project-init)
operating profile (`release-driven-solo`). See `docs/development.md` for
local conventions and `docs/architecture.md` for the system design.
