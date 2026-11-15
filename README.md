# tastile-cli

Tastile's official command-line and TUI client. `tastile` talks directly to the
Tastile v1 HTTP API; it does not embed or shell out to `tastile-core`.

## Repository independence

`tastile-cli` is a **standalone Git repository**. It does not depend on any
sibling repository at build time or runtime. The Tastile API contract is
pulled in via the [`openapi/`](./openapi/) submodule, which is pinned to a
specific revision recorded in the parent commit.

```text
$ git clone --recurse-submodules https://github.com/tastile/tastile-cli
$ cd tastile-cli
$ mise install
$ cargo build
$ cargo test
```

No `tastile-root`, no `tastile-core`, no `tastile-web` checkout required.

## Quick start

```sh
# install mise (if you don't already have it)
#   https://mise.jdx.dev/getting-started.html

# install the pinned toolchain (rust + jq + yq)
mise install

# build the binary
cargo build --release

# check the binary against your local config
./target/release/tastile doctor

# run the TUI
./target/release/tastile

# sign in via the browser
./target/release/tastile auth login

# show the next actionable tile
./target/release/tastile today
```

## What the CLI does today

| Subcommand | Description |
| --- | --- |
| `tastile` (no args) | Launch the TUI. |
| `tastile auth login` | Browser-based PKCE authorization (loopback callback). |
| `tastile auth status` | Show whether a bearer token is stored. |
| `tastile auth logout` | Remove the local token; attempt guarded server-side revocation. |
| `tastile doctor` | Diagnostics: toolchain, credential store, API reachability. |
| `tastile today` | Show the next actionable tile for today. |
| `tastile tiles` | List tiles in the current day window. |
| `tastile source-tiles list` | List every source tile (paginated; no silent 100-row cap). |
| `tastile source-tiles get <id>` | Show one source tile + every occurrence and placement (paginated). |
| `tastile source-tiles create --title … --once/--daily/--weekly` | Create a source tile from a short recurrence description (or `--from-json`). |
| `tastile source-tiles update <id> --from-json` | Full replacement update; requires the complete payload (`--dry-run` compares visible fields, `--expect-revision` detects conflicts). |
| `tastile source-tiles cancel <id>` | Cancel a source tile (`--dry-run` shows the affected placements). |
| `tastile source-tiles completion <id>` | Show the completion plan tree. |
| `tastile source-tiles placements <id>` | List every placement of a source tile (paginated). |
| `tastile source-tiles reflow <id> --from … --to …` | Re-flow placements within a window (`--dry-run` shows the scope). |
| `tastile executions start <placement-id>` | Start an execution. |
| `tastile executions pause <execution-id>` | Pause an execution. |
| `tastile executions resume <execution-id>` | Resume an execution. |
| `tastile executions finish <execution-id> --kind N` | Finish an execution. |
| `tastile prompts list` | List pending prompts. |
| `tastile prompts request <kind> …` | Request a new prompt (server-driven). |
| `tastile prompts startup-recovery …` | Trigger a startup-recovery prompt. |
| `tastile prompts resolve <id> --resolution ack|dismiss|act` | Resolve a prompt. |
| `tastile schedule publish --from-json PATH` | Validate and preview the canonical schedule payload locally (no API call). |
| `tastile schedule publish --from-json PATH --apply` | Publish only to an explicitly confirmed **isolated** loopback Core, never the Muse operational port 31400. |
| `tastile schedule regenerate` | Deprecated placeholder: exits with an error instead of falsely reporting success. |
| `tastile completions <shell>` | Generate a shell completion script. |
| `tastile version` | Print version + pinned OpenAPI metadata. |

## Local-first schedule publication (experimental)

The schedule command is **not** proof of Muse replacement. It validates the
required OpenAPI payload envelope and supports an offline preview; the Core
server determines the actual recurrence, placement and feasibility. Use a
**dedicated isolated Core database**: never point this flow at the current
operational Muse/Tastile schedule or reuse its notification queue.

Prepare a JSON payload containing `tile` (with a nonempty `title`),
`plan`, `reference_targets`, `windows`, and `flows`. Additional fields
such as `source_schedule`, `source_horizon` and `recurrence` must follow
the pinned OpenAPI and Core version. A CLI preview does not mean the payload
is schedulable; check Core read-back and materialized placements before
considering any success.
For source-backed publishes, Core requires `source_client_local_id`,
`source_schedule` and `source_horizon` together, and rejects empty or
invalid completion conditions. Offline preview does not prove these
server-side semantics.

```sh
# Validate the required payload envelope completely offline.
tastile schedule publish --from-json ./local-schedule.json

# Configure an isolated test API + separate database, token and notification sink.
# Port 31400 is reserved by the operational Muse stack and cannot be used.
TASTILE_API_URL=http://127.0.0.1:31402 \
  TASTILE_ISOLATED_TEST_STACK=confirmed \
  tastile schedule publish --from-json ./local-schedule.json --apply \
  --idempotency-key <stable-uuid>
```

The `--idempotency-key` is required for `--apply` and **must be reused**
for an uncertain retry. An omitted `--apply` never opens the credential store
or network. Non-loopback `--apply`, operational port 31400, and missing isolated-stack
confirmation all fail closed. The confirmation flag is an operator safeguard,
not a substitute for verifying the actual database, owner and notification
configuration. An invalid payload fails instead of silently becoming daily. The old `regenerate`
subcommand now returns an error rather than claiming to have regenerated.

See [CLI acceptance and Muse shadow matrix](./docs/local-first-acceptance.md)
for the outstanding integration checks.

## Write-target safety (all mutating commands)

The same gate protects **every** write, not only `schedule publish`:

`ApiConfig` refuses API base port 31400 for **all** requests, including
read-only commands, before opening a connection.

| Target | Decision |
| --- | --- |
| Any other loopback target | Allowed only with `TASTILE_ISOLATED_TEST_STACK=confirmed`. |
| Remote `http://` | Refused — remote API bases must use HTTPS for reads and writes. |
| Remote `https://` | Allowed for ordinary API writes when explicitly configured; `schedule publish --apply` remains loopback-only. |

Before a write leaves the process, the CLI also probes the target with a
documented read endpoint (`GET /v1/source-tiles`) and requires the stored
credential to be accepted. Every authenticated request checks that the
stored credential was issued for the configured API base and is not expired.
The pinned contract has
no `whoami` or version operation, so the probe cannot independently identify
the owner or prove the stack is isolated; only the operator can attest isolation.

See [docs/cli-operations.md](./docs/cli-operations.md) for the full
operation guide, expected error messages and a safe stop/resume procedure.
The pinned `GET /v1/tiles` and pending-prompt list endpoints have no offset
or cursor, so those views make one request and cannot guarantee completeness
if Core applies a result cap. The CLI warns when `/v1/tiles` reaches its
current 500-row cap. Source-tile list, detail and placements use the cursor
contract from Core PRs #302/#305 ahead of the OpenAPI pin; the pin is pending
Core merge and publication. Legacy array responses still decode, but a full
500-row legacy page fails explicitly because it cannot prove completeness.

## Architecture

```text
CLI ─┐
     ├─ application / service layer
TUI ─┘
              │
              ▼
        Tastile API client (tastile-api crate)
              │
              ▼
         HTTPS / Core API
```

- `crates/tastile-api` — typed HTTP client + request/response models. The
  drift gate in `build.rs` enforces that every `operationId` we use is
  still present in the pinned `openapi/openapi.yaml`.
- `crates/tastile-auth` — credential store (OS keyring) + PKCE flow +
  loopback callback listener + browser launcher. Holds no domain state.
- `crates/tastile-config` — file-based config (`~/.config/tastile/config.toml`)
  + env-var overrides.
- `crates/tastile-cli` — the `tastile` binary: clap subcommands + TUI.

### Wire contract

The pinned `openapi/openapi.yaml` is the only source of truth for the
HTTP shape. The drift gate in `crates/tastile-api/build.rs` enforces:

1. The spec parses as OpenAPI 3.1.
2. `info.title` and `info.version` are present.
3. Every path starts with `/v1/`.
4. Every operation the typed client claims to use still exists, with
   the right method, path parameters, request body schema, required
   payload fields, and 200 response shape.

The CLI surface covers **19 of the 21** operations in the pinned spec.
The remaining 2 administrative operations (`delete_owner`, `export_owner`)
are intentionally out of scope and not in the drift-gate contract table.
The newly covered `publish_schedule_definition` operation requires a full
OpenAPI payload and is only writable to a loopback Core in this local-first phase.

The deeper `scripts/check-openapi-drift.sh` is the same check, run as a
shell script so it can also run in CI on every PR and on a weekly cron.

## Authentication

The CLI does **not** hold, copy, or forward the Better Auth session
cookie. Login is browser-mediated and uses the standard
authorization-code grant with PKCE (RFC 7636):

```text
CLI
  generates: code_verifier, code_challenge, state, loopback redirect_uri
 ↓
browser → app.tastile.app/cli/authorize?…
   (Better Auth session cookie authenticates the user)
   web mints a one-time authorization grant bound to (challenge, redirect_uri, user)
 ↓
browser → http://127.0.0.1:<port>/cli/callback?code=…&state=…
 ↓
CLI
  - verifies state matches (constant time)
  - POSTs /api/cli/token { code, code_verifier, redirect_uri }  (NO cookie)
  - receives { token, expires_at, subject }
 ↓
OS credential store (Keychain / Credential Manager / Secret Service)
```

Until the server-side `/api/cli/token` endpoint is exposed (see the
follow-up section in `docs/architecture.md`), the CLI captures the
authorization code from the browser and prints the structured exchange
request so an operator can complete it by hand. The token is then loaded
into the OS credential store under service `tastile-cli`, user `default`.

## Configuration

Default: `https://api.tastile.app` / `https://app.tastile.app`. Override via
`~/.config/tastile/config.toml`:

```toml
api_url = "https://api.staging.app.tastile.app"
web_url = "https://staging.app.tastile.app"
api_timeout_ms = 30_000
verbose = false
oauth_client_id = "tastile-cli"
```

Or via environment variables:

```sh
TASTILE_API_URL=https://api.staging.app.tastile.app
TASTILE_WEB_URL=https://staging.app.tastile.app
TASTILE_API_TIMEOUT_MS=30000
TASTILE_VERBOSE=1
```

Precedence: env var > config file > compiled-in default.

## Supported platforms

- Linux x86_64
- macOS aarch64 / x86_64
- Windows x86_64 (MSVC)

The credential store uses the OS-native backend:

| Platform | Backend |
| --- | --- |
| macOS | Keychain (`apple-native`) |
| Windows | Credential Manager (`windows-native`) |
| Linux | Secret Service (`sync-secret-service`) |

## Development

```sh
# fmt + clippy + test + openapi-drift in one go
mise run ci

# bump the pinned OpenAPI revision
scripts/sync-openapi.sh           # bump to upstream HEAD
scripts/sync-openapi.sh v1.1.0    # pin to a specific tag
scripts/sync-openapi.sh <sha>     # pin to a specific commit
```

### Layout

```text
.
├── .github/workflows/         CI: ci, openapi-drift, release-source-check
├── crates/
│   ├── tastile-api/           typed API client + drift gate (build.rs)
│   ├── tastile-auth/          credential store + PKCE + callback
│   ├── tastile-config/        config file + env overrides
│   └── tastile-cli/           the `tastile` binary
├── docs/                      architecture.md, development.md, release.md
├── openapi/                   git submodule → tastile/tastile-openapi
├── scripts/                   check-openapi-drift.sh, sync-openapi.sh
├── Cargo.toml                 workspace
├── mise.toml                  toolchain + tasks
├── AGENTS.md                  agent entry point
└── README.md
```

## License

Dual-licensed under MIT or Apache-2.0 at your option. See `LICENSE-MIT` and
`LICENSE-APACHE`.
