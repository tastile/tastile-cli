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
- `safety.rs` — the **write-target gate**. Every mutating command calls
  the shared app write gate before it sends a request (`schedule publish`
  has its stricter loopback-only variant). The gate classifies the
  configured API base (loopback / remote, TLS / plaintext, reserved
  operational port) and then probes the target with a documented read
  endpoint to prove it answers as a Tastile v1 API that accepts the
  stored credential. The classification is a pure function, so it is
  unit-tested without touching the process environment.
- `simple_input.rs` — the convenience recurrence input
  (`--once` / `--daily` / `--weekly` / `--days` / `--exclude`). It only
  produces the shapes the pinned contract describes; anything it cannot
  express exactly (for example a monthly rule) is refused with an
  explicit error instead of being approximated.
- `cli.rs` — `clap` derive. Subcommands are user-friendly (e.g.
  `tastile source-tiles create --title …`, `tastile prompts resolve
  --resolution ack`); they never echo the API operation name.
- `commands/` — one file per subcommand group, each delegating to
  `app::*`.
- `output.rs` — `print_*` table / detail formatters.
- `tui.rs` — `ratatui` + `crossterm` TUI. Runs in the foreground;
  no args → TUI, subcommand → CLI. The TUI calls the same
  `app::*` functions as the CLI.

### Write-target safety gate

A loopback API base is ambiguous: it can be an isolated test stack or the
operational Muse stack that owns the real schedule. `ApiConfig` rejects port
31400 before any API request, including reads. `safety.rs` applies the
remaining target rules to every write:

| Target | Decision |
| --- | --- |
| Loopback, any other port | Allowed only with `TASTILE_ISOLATED_TEST_STACK=confirmed`. |
| Remote, `http://` | Refused — plaintext remote API bases are rejected for reads and writes. |
| Remote, `https://` | Allowed — the operator configured that host explicitly. |

The server-side revocation attempted by `auth logout` uses the same gate;
local credential removal still completes when the API revoke is refused or
unavailable.

The attestation is a safeguard, not proof of isolation: it records that
somebody checked the database, owner, worker and notification sink of the
target stack. The identity probe (a documented read endpoint) proves the
URL points at a Tastile v1 API that accepts the account — it does not
prove isolation.

### Read completeness, diffs and revision conflicts

- **Pagination.** `tastile-api` follows cursor pages for `/v1/tiles`,
  source-tile list and placements; source-tile detail independently pages
  occurrences and placements. Legacy array responses are decoded during
  the transition, but a full legacy page fails explicitly because the
  client cannot prove it is complete. `/v1/tiles` requests the pinned
  1,000-row maximum and rejects a cursorless 500-item response to account
  for the pre-cursor Core cap. No `offset` parameter is sent. The pinned
  pending-prompt list operation has no pagination contract, so its
  completeness cannot be proven if Core adds a result cap.
- **Today.** `tastile today` requests the full tile projection and keeps
  only rows with a `projected_next_start_at` at or after the local current
  instant. This filters stale past projections and entries with no known
  next start before selecting the earliest upcoming tile.
- **Diffs.** `source-tiles update --dry-run` compares the stored source
  tile with the proposed draft and prints changed fields exposed by the
  read model: tile metadata, plan role, duration/priority, generation,
  window and split policy. The API read model omits the full plan body,
  flows, horizon, request-shaped relations and `source_client_local_id`, so
  the CLI warns that those values are not diffed.
- **Full replacement.** `PUT /v1/source-tiles/{id}` replaces the complete
  payload; the pinned contract has no PATCH or diff endpoint. Update
  therefore requires a JSON draft with every replacement field, including
  explicit `null` values for optional tile metadata and explicit empty
  arrays when appropriate. Keep the original request-shaped plan, flows,
  horizon, relations and `source_client_local_id`: the read model cannot
  reconstruct them safely.
- **Revision conflicts.** Update uses the revision from the read shown in
  its diff; cancel and reflow fetch the current revision before mutation.
  `--expect-revision` overrides that default. Each value maps to the
  `expected_revision` envelope field; the server answers `409` when the
  stored revision moved on, and the CLI surfaces that conflict instead of
  overwriting the change. Reflow always sends the field to remain compatible
  with Core PR #302, which makes it mandatory.
- **Impact preview and readback.** `cancel` / `reflow --dry-run` list the
  placements in scope; after a successful write the CLI reads the source
  tile and its placements back and prints the resulting state.
- **Placement.** Simple create flags describe a schedule; Core's worker
  materializes occurrences and placements. The CLI does not invent a
  client-side auto-placement command or promise placements before the
  worker has processed the new source.
- **Numeric cancel reason.** `CancelSourceTilePayloadSchema.reason` is an
  integer registry code in the contract, so the CLI sends
  `--reason-code` (0 = user-requested) instead of free text.

## Wire contract: 19 of 21 operations across 6 areas

The pinned `openapi/openapi.yaml` declares 21 `operationId`s in total. The
CLI surface covers **19** of them, including `publish_schedule_definition`
for offline preview and an explicit loopback-only Core write. The remaining
2 (`delete_owner`, `export_owner`) are administrative endpoints and are not
included in the typed CLI surface.

| Area | Module | Operations |
| --- | --- | --- |
| tiles | `tiles.rs` | `list_tiles` |
| prompts | `prompts.rs` | `list_pending_prompts`, `request_prompt`, `resolve_prompt`, `respond_startup_recovery` |
| source-tiles | `source_tiles.rs` | `list_source_tiles`, `get_source_tile`, `create_source_tile`, `update_source_tile`, `cancel_source_tile`, `get_source_tile_completion`, `list_source_tile_placements`, `reflow_source_tile` |
| executions | `executions.rs` | `start_execution`, `pause_execution`, `resume_execution`, `finish_execution` |
| auth | `auth.rs` | `signout` |
| schedule | `schedule.rs` | `publish_schedule_definition` |

Total CLI surface: 19 of 21. The drift gate in `build.rs` checks all
19 entries for method, path, operationId, request schema, required payload
fields and response. The 2 remaining administrative operations are not yet
covered. The new `schedule publish` command deliberately separates offline
payload preview from explicit `--apply` and allows mutation of loopback Core
only; this is not evidence of Muse migration readiness.

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
