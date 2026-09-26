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
| `tastile auth login` | Browser-based PKCE authorization. |
| `tastile auth status` | Show whether a bearer token is stored. |
| `tastile auth logout` | Drop the local token and revoke server-side. |
| `tastile doctor` | Diagnostics: toolchain, credential store, API reachability. |
| `tastile today` | Show the next actionable tile for today. |
| `tastile tiles` | List tiles in the current day window. |
| `tastile source-tiles list` | List source tiles. |
| `tastile source-tiles get <id>` | Show one source tile + its placements. |
| `tastile source-tiles cancel <id>` | Cancel a source tile. |
| `tastile prompts list` | List pending prompts. |
| `tastile prompts resolve <id> --answer-kind N` | Resolve a prompt. |
| `tastile schedule regenerate` | Re-publish the schedule definition. |
| `tastile completions <shell>` | Generate a shell completion script. |
| `tastile version` | Print version + pinned OpenAPI metadata. |

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
4. Every operation the typed client claims to use still exists.

The deeper `scripts/check-openapi-drift.sh` is the same check, run as a
shell script so it can also run in CI on every PR and on a weekly cron.

## Authentication

`POST /v1/auth/signout` exists on the API. Login is browser-mediated and
works like the mobile and desktop apps:

```text
CLI
 ↓ PKCE + state
localhost loopback listener
 ↓
browser → app.tastile.app/cli/authorize
 ↓
one-time authorization code
 ↓
code → Tastile API token (via POST /api/cli/api-token — server-side WIP)
 ↓
OS credential store (Keychain / Credential Manager / Secret Service)
```

Until the server-side `/api/cli/api-token` endpoint is exposed (see the
follow-up section in `docs/architecture.md`), the CLI captures the
authorization code from the browser and prints the structured exchange
request so an operator can complete it by hand. The token is then loaded
into the OS credential store under service `tastile-cli`, user `default`.

## Configuration

Default: `https://api.tastile.app` / `https://app.tastile.app`. Override via
`~/.config/tastile/config.toml`:

```toml
api_url = "https://api.staging.tastile.app"
web_url = "https://app.staging.tastile.app"
api_timeout_ms = 30_000
verbose = false
oauth_client_id = "tastile-cli"
```

Or via environment variables:

```sh
TASTILE_API_URL=https://api.staging.tastile.app
TASTILE_WEB_URL=https://app.staging.tastile.app
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
