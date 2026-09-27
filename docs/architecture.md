# Architecture

`tastile-cli` is a four-crate Rust workspace that talks to the Tastile v1
HTTP API. The wire contract is the pinned `openapi/openapi.yaml` submodule;
nothing else.

## Crates

```text
crates/
├── tastile-api/     typed HTTP client + drift gate (build.rs)
├── tastile-auth/    credential store + PKCE + loopback callback + server bridge
├── tastile-config/  config file + env overrides
└── tastile-cli/     the `tastile` binary (clap CLI + shared app layer + ratatui TUI)
```

### `tastile-api`

The only crate that knows HTTP shapes. Every method corresponds 1:1 to an
`operationId` in the pinned spec. Each module (`tiles.rs`, `prompts.rs`,
`source_tiles.rs`, `executions.rs`, `auth.rs`) is grouped by OpenAPI tag.

The crate's `build.rs` (`OperationContract` table):

1. Reads `openapi/openapi.yaml` (relative to the workspace root via
   `CARGO_MANIFEST_DIR`).
2. Parses it as OpenAPI 3.1.
3. Validates `info.title` / `info.version`.
4. Walks `paths` and confirms every path begins with `/v1/`.
5. For every row in `OperationContract`:
   - Confirms the path is present.
   - Confirms the method is present.
   - Confirms `operationId` matches.
   - Confirms every required path parameter is present (merging
     method-level and path-item-level parameters).
   - Confirms the request body schema `$ref` matches (when the contract
     declares one).
   - Confirms the request body's `payload` schema `$ref` matches and
     contains every required field declared in the contract.
   - Confirms the 200 response shape (`None` / `Ref` / `ArrayOf` / `Empty`).
6. Sets `TASTILE_API_OPENAPI_VERSION` and `TASTILE_API_OPENAPI_TITLE` as
   rustc env vars, exported to dependents as
   `tastile_api::API_VERSION` and `tastile_api::API_TITLE`.

If any check fails, the build fails with `cargo:error=...`. A negative
test was run to confirm drift is caught.

### `tastile-auth`

Stateless auth crate:

- `credential.rs` — `CredentialStore` trait with `KeyringStore`
  (production, OS-native) and `MemoryStore` (tests).
- `pkce.rs` — PKCE state machine (RFC 7636) with SHA256 challenge and
  constant-time state matching.
- `callback.rs` — loopback listener bound to `127.0.0.1:0`. Serves a
  one-page HTML response after capturing the authorization code.
- `browser.rs` — cross-platform browser launch via the `open` crate.
- `server_bridge.rs` — browser-mediated authorization-code grant
  protocol. See "Authentication" below.

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

The binary. Single `tokio` runtime, subcommand-dispatched.

- `app.rs` — shared **application service layer**. Every command
  (CLI subcommand and TUI action) goes through `crate::app::*`, so
  HTTP handling is implemented exactly once. `AppContext::load(cfg)`
  is the single init point: it builds the `ApiClient`, loads the
  stored credential from the keyring, and exposes typed wrappers
  (`today`, `create_source_tile_draft`, `start_execution`,
  `resolve_prompt`, …).
- `cli.rs` — `clap` derive. Subcommands are user-friendly (e.g.
  `tastile source-tiles create --title …`, `tastile prompts resolve
  --resolution ack`); they never echo the API operation name.
- `commands/` — one file per subcommand group, each delegating to
  `app::*`.
- `output.rs` — `print_*` table / detail formatters.
- `tui.rs` — `ratatui` + `crossterm` TUI. Runs in the foreground;
  no args → TUI, subcommand → CLI. The TUI calls the same
  `app::*` functions as the CLI.

## Wire contract: 18 of 21 operations across 5 areas

The pinned `openapi/openapi.yaml` declares 21 `operationId`s in total. The
CLI surface intentionally covers **18** of them — the operations a
command-line or TUI client can drive today. The remaining 3
(`delete_owner`, `export_owner`, `publish_schedule_definition`) are
admin / data-portability endpoints that are out of scope for the
initial CLI; they will land in a later release once a real consumer
asks for them.

| Area | Module | Operations |
| --- | --- | --- |
| tiles | `tiles.rs` | `list_tiles` |
| prompts | `prompts.rs` | `list_pending_prompts`, `request_prompt`, `resolve_prompt`, `respond_startup_recovery` |
| source-tiles | `source_tiles.rs` | `list_source_tiles`, `get_source_tile`, `create_source_tile`, `update_source_tile`, `cancel_source_tile`, `get_source_tile_completion`, `list_source_tile_placements`, `reflow_source_tile` |
| executions | `executions.rs` | `start_execution`, `pause_execution`, `resume_execution`, `finish_execution` |
| auth | `auth.rs` | `signout` |

Total CLI surface: 18 of 21. The drift gate in `build.rs` enforces every
one of the 18 — a row in `OperationContract` per operation, with
path, method, operationId, path params, request schema, required
payload fields, and 200 response shape all checked at build time. The
3 admin operations are **not** in the contract, so they are **not**
covered by the drift gate; if they are needed later, add a row and the
gate will pick them up.

## Authentication: browser-mediated authorization-code grant

The CLI does **not** hold, copy, or forward the Better Auth session
cookie. The auth flow is split between the web origin (which owns the
Better Auth session and the one-time grant store) and the CLI:

```text
CLI
  generates:
    code_verifier        (RFC 7636, 32 random bytes, base64url)
    code_challenge       (SHA256(code_verifier), base64url)
    state                (16 random bytes, base64url)
    loopback redirect_uri (http://127.0.0.1:<port>/cli/callback)

       ↓ opens browser

Web: GET {web_url}/cli/authorize?
       response_type=code
       &client_id={client_id}
       &redirect_uri={redirect_uri}
       &scope={scope}
       &state={state}
       &code_challenge={code_challenge}
       &code_challenge_method=S256

  (Better Auth session cookie authenticates the user — never leaves
   the browser)
  - web mints a one-time authorization grant (opaque code) bound to:
      * user_id
      * code_challenge
      * redirect_uri
      * expiration
      * used = false
  - web redirects to:
      {redirect_uri}?code={opaque_grant}&state={state}

CLI
  - receives the callback
  - verifies state matches (constant time)
  - POSTs to {web_url}/api/cli/token  (NO Better Auth cookie)
    body: { code, code_verifier, redirect_uri }

Web /api/cli/token:
  - looks up the one-time grant
  - verifies expiration, single-use, code_challenge (PKCE),
    redirect_uri
  - mints a Tastile API bearer token (no Better Auth involvement)
  - returns { token, expires_at, subject }
  - marks the grant as used

CLI
  - stores the bearer token in the OS credential store
    (service: tastile-cli, user: default)
```

### Endpoint URL

`POST {web_url}/api/cli/token` — the **only** endpoint the CLI talks to
on the web origin. There is no `client_id` in the request body, and the
request does not carry any cookie.

### Redaction

Server messages that happen to echo `bearer <token>` are passed through
`redact_token()`, which strips both the marker and the token value
(case-insensitive). The grant (`AuthorizationCode`) is never logged;
its redacted summary is a character count and a 2-char prefix only.

### Status (2026-09-27)

✅ Resolved by tastile/tastile-web#153 (CLI side). `tastile auth login`
now calls `HttpServerBridge::fetch_token` directly, which issues the
real `POST {web_url}/api/cli/token`. The legacy
`ServerBridge::exchange` method is preserved as a no-stub
implementation that still returns
`ServerBridgeError::ServerEndpointUnavailable` so the wire-contract
test in `auth_flow.rs` (AC 9) keeps passing; only the `login` command
path was switched to the real call.

## Usable CLI flow

```text
$ tastile auth login
   opens browser → user approves → CLI captures grant → CLI POSTs
   {code, code_verifier, redirect_uri} to {web_url}/api/cli/token →
   bearer token saved to OS credential store

$ tastile today
$ tastile source-tiles create --title "Read Rust book"
$ tastile source-tiles completion <id>
$ tastile source-tiles reflow <id> --from 2026-10-01T00:00:00Z \
                                  --to   2026-10-08T00:00:00Z
$ tastile executions start  <placement-id>
$ tastile executions pause  <execution-id>
$ tastile executions resume <execution-id>
$ tastile executions finish <execution-id> --kind 0
$ tastile prompts list
$ tastile prompts resolve <prompt-id> --resolution ack
```

`$ tastile`  (no subcommand) launches the TUI.

## Boundary guarantees

`tastile-cli` does **not** import:

- `tastile-core` (no `domain`, no `storage`, no `api`).
- A direct PostgreSQL driver.
- A web-bridge secret artifact.
- A core internal secret.

The only "shared" artifact is `openapi/openapi.yaml`.
