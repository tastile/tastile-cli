# Development

## Bootstrap

```sh
git clone --recurse-submodules https://github.com/tastile/tastile-cli
cd tastile-cli
mise install
cargo build
cargo test
```

If you already cloned without `--recurse-submodules`:

```sh
git submodule update --init --recursive
```

## Validation entry

```sh
mise run ci
```

Equivalent to:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
scripts/check-openapi-drift.sh
```

## Repository layout

```text
tastile-cli/
├── .github/workflows/         ci.yml, openapi-drift.yml, release-source-check.yml
├── crates/
│   ├── tastile-api/
│   │   ├── build.rs           OpenAPI drift gate
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── client.rs      ApiClient + ApiConfig
│   │       ├── error.rs       ApiError enum
│   │       ├── model.rs       ApiVersion, CommandResponse
│   │       ├── tiles.rs       list_tiles + TileListView
│   │       ├── source_tiles.rs
│   │       ├── prompts.rs
│   │       └── auth.rs        signout()
│   ├── tastile-auth/
│   │   └── src/
│   │       ├── credential.rs  CredentialStore + KeyringStore
│   │       ├── pkce.rs        PkceState, PkcePair
│   │       ├── callback.rs    CallbackListener
│   │       ├── browser.rs     open_browser()
│   │       └── server_bridge.rs  POST /api/cli/api-token
│   ├── tastile-config/
│   │   └── src/{lib.rs, paths.rs}
│   └── tastile-cli/
│       └── src/
│           ├── main.rs
│           ├── cli.rs         clap definitions
│           ├── tracing_init.rs
│           ├── output.rs      table printer
│           ├── tui.rs         ratatui TUI
│           └── commands/      auth.rs, doctor.rs, tiles.rs, today.rs,
│                              source_tiles.rs, prompts.rs,
│                              schedule.rs, completions.rs
├── docs/                      architecture.md, development.md, release.md
├── openapi/                   git submodule → tastile-openapi
├── scripts/                   check-openapi-drift.sh, sync-openapi.sh
├── Cargo.toml                 workspace
├── mise.toml                  toolchain + tasks
├── AGENTS.md                  canonical agent entry point
└── README.md
```

## Bumping the OpenAPI pin

```sh
scripts/sync-openapi.sh           # bump to upstream HEAD
scripts/sync-openapi.sh v1.1.0    # pin to a specific tag
scripts/sync-openapi.sh <sha>     # pin to a specific commit
```

The script:

1. `git submodule update --remote openapi` (or the requested tag/SHA).
2. `cargo build` — exercises the `build.rs` drift gate.
3. `cargo test --workspace --all-features`.
4. `scripts/check-openapi-drift.sh`.

Commit the resulting gitlink bump as a single commit:

```text
chore(openapi): bump pinned revision to <short-sha>
```

## Adding a typed operation

1. Confirm the operationId is in the pinned `openapi/openapi.yaml`. If
   not, bump the pin first.
2. Add the request / response model in `crates/tastile-api/src/<area>.rs`.
3. Add the operationId to the `REQUIRED_OPERATION_IDS` list in
   `crates/tastile-api/build.rs`.
4. Re-export the function in `crates/tastile-api/src/lib.rs`.
5. Add a CLI subcommand (or extend an existing one) under
   `crates/tastile-cli/src/commands/`.
6. Add at least one unit test.
7. Run `mise run ci`.

## Lint and warning posture

- `unsafe_code = "forbid"` (workspace lint).
- Pedantic clippy allowed for ergonomics: `collapsible_if`,
  `needless_collect`, `needless_pass_by_value`, `useless_format`,
  `field_reassign_with_default`, `unnecessary_sort_by`.
- All other warnings are denied via `-D warnings`.

## CI matrix

`.github/workflows/ci.yml` runs on:

- `ubuntu-latest`
- `macos-latest`
- `windows-latest`

Steps: checkout with submodules → `mise install` → cache Cargo → fmt →
clippy → test → openapi drift (Linux only).

`openapi-drift.yml` runs the drift gate on a weekly cron
(Monday 06:00 UTC) plus workflow_dispatch.

`release-source-check.yml` validates that PRs targeting `main` use a
`release-X-Y-Z` head branch.

## Release flow

1. Cut a `release-X-Y-Z` branch from `main`.
2. Tag the merge commit on `main` after the release PR lands.
3. `.github/workflows/release-source-check.yml` enforces the branch
   pattern for direct-to-main PRs.

## Local config override

```sh
export TASTILE_API_URL=https://api.staging.tastile.app
export TASTILE_WEB_URL=https://app.staging.tastile.app
export TASTILE_VERBOSE=1
```

Per-user persistent config: `~/.config/tastile/config.toml`.
