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
- `crates/tastile-api` — typed HTTP client + build-time drift gate.
- `crates/tastile-auth` — OS credential store + PKCE + loopback callback.
- `crates/tastile-config` — file-based config + env overrides.
- `crates/tastile-cli` — `tastile` binary with subcommands:
  - `auth login | status | logout | exchange`
  - `doctor`
  - `tiles`, `today`
  - `source-tiles list | get | cancel`
  - `prompts list | resolve`
  - `schedule regenerate`
  - `completions <shell>`, `version`
- TUI (`tastile` with no subcommand) — auth state, today's tiles,
  pending prompts, refresh on `r`, quit on `q` / `Esc`.
- `scripts/check-openapi-drift.sh` — semantic drift check (CI + cron).
- `scripts/sync-openapi.sh` — bump the pinned submodule revision.
- GitHub Actions: `ci.yml`, `openapi-drift.yml`, `release-source-check.yml`.
- `mise.toml` with canonical `ci` task.
- README, AGENTS.md, CLAUDE.md, CONTRIBUTING.md.
