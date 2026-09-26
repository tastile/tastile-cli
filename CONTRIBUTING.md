# Contributing

Thank you for working on `tastile-cli`. This document covers the workflow
you need to follow to land a change.

## Bootstrap

```sh
git clone --recurse-submodules https://github.com/tastile/tastile-cli
cd tastile-cli
mise install
cargo build
cargo test
```

If you cloned without `--recurse-submodules`:

```sh
git submodule update --init --recursive
```

## Day-to-day

```sh
# format
cargo fmt --all

# lint
cargo clippy --workspace --all-targets --all-features -- -D warnings

# tests
cargo test --workspace --all-features

# OpenAPI drift gate
scripts/check-openapi-drift.sh

# all of the above in one go
mise run ci
```

## Workflow

1. Pick or open a GitHub Issue. The issue is the durable ticket.
2. Create a branch named after the issue number (e.g. `42`).
3. Commit in English, with a conventional prefix (`feat:`, `fix:`,
   `chore:`, `docs:`, `test:`).
4. Push and open a Draft PR.
5. Verify CI is green before flipping the PR to Ready.
6. Land via the `release-X-Y-Z` → `main` PR flow.
   - `main` is protected with required CI checks
     (`build, test, lint` on all three OSes + the
     `release-source-check` workflow).
   - **Merge commit only.** Repository settings disable squash and
     rebase merges; PRs land via the default "Create a merge commit"
     button. `required_linear_history` is **off** so merge commits
     are explicitly permitted.
   - `required_conversation_resolution: true` is enabled, so all
     review comments must be resolved before merge.
   - `required_signatures: true` is enabled, so all commits must be
     signed.

## Coding rules

- No `unsafe` (`unsafe_code = "forbid"` in the workspace lints).
- No `tastile-core` imports. The CLI speaks HTTP only.
- New API operations must correspond to an `operationId` in the pinned
  `openapi/openapi.yaml`. If the operation doesn't exist, run
  `scripts/sync-openapi.sh` first.
- New dependencies must be reviewed: `cargo audit`-clean and used by ≥1
  public API of one of the four crates.

## Updating the OpenAPI pin

```sh
# bump to upstream HEAD
scripts/sync-openapi.sh

# pin to a specific tag / SHA
scripts/sync-openapi.sh v1.1.0
scripts/sync-openapi.sh 8461ffa345943c28385a9f06a2fd7b945205de54
```

The script updates the gitlink, runs `cargo build` (build.rs drift gate),
`cargo test`, and the shell drift check. Commit the gitlink bump in a
single commit (`chore(openapi): bump pinned revision to <short-sha>`).

## Adding a new CLI subcommand

1. Add the subcommand definition to `crates/tastile-cli/src/cli.rs`.
2. Add the typed wrapper in `crates/tastile-cli/src/app.rs` (the shared
   application service layer; the TUI uses the same functions).
3. Add the implementation under `crates/tastile-cli/src/commands/<name>.rs`.
4. Wire it into `crates/tastile-cli/src/main.rs`.
5. Add at least one unit test.
6. If the operation is a new typed call, add an `OperationContract` row
   in `crates/tastile-api/build.rs` so the drift gate enforces it.
7. Update `README.md` "What the CLI does today".

## License

By submitting a patch you agree to license it under MIT or Apache-2.0 at
the project's option.
