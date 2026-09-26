# Architecture

`tastile-cli` is a four-crate Rust workspace that talks to the Tastile v1
HTTP API. The wire contract is the pinned `openapi/openapi.yaml` submodule;
nothing else.

## Crates

```text
crates/
├── tastile-api/     typed HTTP client + drift gate (build.rs)
├── tastile-auth/    credential store + PKCE + loopback callback
├── tastile-config/  config file + env overrides
└── tastile-cli/     the `tastile` binary (clap + TUI)
```

### `tastile-api`

The only crate that knows HTTP shapes. Every method corresponds 1:1 to an
`operationId` in the pinned spec. The crate's `build.rs`:

1. Reads `openapi/openapi.yaml` (relative to the workspace root via
   `CARGO_MANIFEST_DIR`).
2. Parses it as OpenAPI 3.1.
3. Validates `info.title` / `info.version`.
4. Walks `paths` and confirms every path begins with `/v1/`.
5. Confirms every operation the typed client claims to use is present
   (operationId-by-operationId).
6. Sets `TASTILE_API_OPENAPI_VERSION` and `TASTILE_API_OPENAPI_TITLE` as
   rustc env vars, exported to dependents as
   `tastile_api::API_VERSION` and `tastile_api::API_TITLE`.

If any of these checks fail, the build fails with `cargo:error=...`.

### `tastile-auth`

Stateless auth crate:

- `credential.rs` — `CredentialStore` trait with `KeyringStore`
  (production, OS-native) and `MemoryStore` (tests).
- `pkce.rs` — PKCE state machine (RFC 7636) with SHA256 challenge and
  constant-time state matching.
- `callback.rs` — loopback listener bound to `127.0.0.1:0`. Serves a
  one-page HTML response after capturing the authorization code.
- `browser.rs` — cross-platform browser launch via the `open` crate.
- `server_bridge.rs` — `HttpServerBridge::exchange()` issues
  `POST /api/cli/api-token`. While the server endpoint is not yet
  exposed, the bridge returns a structured `ServerEndpointUnavailable`
  with a curl-able request body so an operator can drive the exchange.

### `tastile-config`

`Config` struct with `#[serde(deny_unknown_fields)]`. Two loaders:

- `load()` — permissive: missing file → defaults, invalid file → error.
- `load_strict()` — strict: missing file → error.

Env overrides:

```text
TASTILE_API_URL
TASTILE_WEB_URL
TASTILE_API_TIMEOUT_MS
TASTILE_VERBOSE
TASTILE_OAUTH_CLIENT_ID
```

Env > config file > compiled-in default.

### `tastile-cli`

The binary. Single `tokio` runtime, subcommand-dispatched. Subcommands
live under `src/commands/`. The TUI is `src/tui.rs` and uses `ratatui` +
`crossterm` with a 30 s background refresh.

## Wire contract: how an operation lands

1. Bump the pinned OpenAPI revision (`scripts/sync-openapi.sh v1.1.0`).
2. Add the typed request/response in `tastile-api/src/<area>.rs`.
3. Add the operationId to `REQUIRED_OPERATION_IDS` in `build.rs`.
4. Add a `pub use` re-export in `tastile-api/src/lib.rs`.
5. Add the subcommand wiring in `tastile-cli/src/commands/`.
6. `scripts/check-openapi-drift.sh` confirms drift-free.

## Authentication: the full flow

```text
CLI                              app.tastile.app              CLI loopback
 │                                       │                          │
 │ generate PKCE                         │                          │
 │ bind 127.0.0.1:0                      │                          │
 ├──────────────────────────────────────►│                          │
 │   open /cli/authorize?…               │                          │
 │   (response_type, client_id,          │                          │
 │    redirect_uri, scope, state,        │                          │
 │    code_challenge, code_challenge_    │                          │
 │    method=S256)                       │                          │
 │                                       │                          │
 │                                       │ user clicks "Authorize"  │
 │                                       │ Better Auth issues code  │
 │                                       │ redirects to             │
 │                                       │ http://127.0.0.1:<port>/  │
 │                                       │   callback?code=…&state=…
 │                                       ▼                          │
 │                                       │                  serve    │
 │                              ◄────────┴──────────────────────────┤
 │   verify state matches                │                          │
 │   POST /api/cli/api-token             │ (server-side WIP)        │
 │     { code, code_verifier }           │                          │
 │   ───────────────────────────►        │                          │
 │                                       │  { token, expires_at }   │
 │   ◄───────────────────────────────────│                          │
 │ save to keyring (service: tastile-cli,│                          │
 │                    user: default)     │                          │
```

When the server-side `/api/cli/api-token` is not yet available, the CLI
prints the structured request body so an operator can complete the
exchange manually with `curl`.

## Server-side follow-up

Until `POST https://app.tastile.app/api/cli/api-token` exists, the auth
flow is structurally complete but stops short of issuing a token.
Required from the web side:

| Endpoint | Method | Auth | Body | Returns |
| --- | --- | --- | --- | --- |
| `/api/cli/api-token` | POST | Better Auth session cookie | `{ code, code_verifier }` | `{ token, expires_at, subject? }` |

`{token}` is the bearer token to embed in the `Authorization: Bearer …`
header against `https://api.tastile.app/v1/...`.

## Boundary guarantees

`tastile-cli` does **not** import:

- `tastile-core` (no `domain`, no `storage`, no `api`).
- A direct PostgreSQL driver.
- A web-bridge secret artifact.
- A core internal secret.

The only "shared" artifact is `openapi/openapi.yaml`.
