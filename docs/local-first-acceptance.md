# Core + CLI local-first acceptance — Muse parity

Owner issue: [tastile-cli#6](https://github.com/tastile/tastile-cli/issues/6).
Plan: [tastile-root#105](https://github.com/tastile/tastile-root/issues/105).
Evidence baseline: Muse operational inventory, 2026-11-15 10:31 JST,
reported to a private shared document. Metrics below are **Muse's reported
observations**, not independently verified in this repository.

## Contract gaps found while verifying the CLI (2026-11-15)

The pinned `openapi/openapi.yaml` (Tastile OpenAPI 1.0.1) turned out to
disagree with the CLI in several places. The original five discrepancies
below were fixed against the pinned contract. Review follow-up work adds
compatibility for the upcoming cursor and required-revision contract without
changing the OpenAPI submodule pin:

| Finding | Contract | CLI before | Now |
| --- | --- | --- | --- |
| Cancel reason type | `CancelSourceTilePayloadSchema.reason` is `integer` (the server's `domain::source_tile::state::cancel_reason` registry) | free-text string, rejected by the server | `--reason-code` (0 = user-requested) |
| Placement window | `SourceWindowSchema` requires `start_offset_ms` / `end_offset_ms` with `start < end` | `{"window": "anytime"}` | `{"start_offset_ms": 0, "end_offset_ms": 14400000}` |
| One-time generation | `SourceGenerationSchema.kind = 0` requires the `at` instant | `{"kind": 0}` | `{"kind": 0, "at": "<horizon start>"}` |
| Tile list parameters | `GET /v1/tiles` documents `owner_ids`, `limit`, `view_mode`, `lifecycle`, `range`, `granularity`, `search`, `exclude_future` | `window_start`, `window_end`, `plan_id`, `include_closed` (all ignored by the server) | only the documented parameters |
| Listing page size | The pinned contract uses `limit` (1..500) and `offset` | one request, no pagination | the updated client sends cursor requests per Core #305; legacy arrays decode during transition |

## Incoming Core contract compatibility (pin pending)

Core PRs [#302](https://github.com/tastile/tastile-core/pull/302) and
[#305](https://github.com/tastile/tastile-core/pull/305) are still unpublished
contract changes. This branch follows their intended wire behavior while
keeping the `openapi/` gitlink pinned until Core merges the PRs and publishes
the updated spec:

- Update, cancel and reflow send `expected_revision` by default. Update uses
  the revision from the source read displayed for its diff; cancel and reflow
  read the current revision before the write. `--expect-revision` overrides
  the value. Reflow therefore satisfies Core #302's required field.
- Source-tile list and placement-list requests use `cursor`. Source detail
  requests use independent `occurrence_cursor` / `placement_cursor` and
  `occurrence_limit` / `placement_limit`. Responses decode the new
  `{items,count,next_cursor}` page or the pinned legacy array. No `offset`
  parameter is sent.
- A legacy array shorter than 500 items is treated as terminal. A full legacy
  500-item page fails with an explicit pagination error because that response
  cannot establish whether more results exist. The cursor contract is needed
  to prove completeness above that page size.
- The build drift gate accepts the pinned old array response and the named
  cursor response schemas. It also fails on newly required request fields,
  except for explicitly modeled optional fields that this CLI already sends.

Do not bump the submodule to an unpublished Core spec. Track the pin update on
[tastile-cli#6](https://github.com/tastile/tastile-cli/issues/6) and complete
it after both Core contract changes merge and the generated/published OpenAPI
revision is available.

Gaps that the pinned contract does **not** cover, and that the CLI
therefore refuses or works around instead of inventing wire fields:

- **No health / version handshake.** The pinned spec has no `/v1/health`
  path, so the CLI cannot verify the target's binary version through the
  pinned contract. Every authenticated request checks the stored token's
  API-base binding and expiry. Every API mutation, including the server-side
  revoke attempted by `auth logout`, also probes documented
  `GET /v1/source-tiles`; this proves the
  target accepts the credential but cannot independently verify the token
  subject against a Core owner id or prove stack isolation. The contract has
  no `whoami` operation, so that identity check remains a gap.
- **Some list projections cannot be paged.** `GET /v1/tiles` and the pending
  prompt list define no offset or cursor in the pinned spec. The tile command
  warns when its current server cap of 500 rows is reached. Prompt listing
  currently returns all unresolved rows in Core but the contract has no
  pagination semantics if that ever changes. Source-tile list/detail/
  placements use the upcoming Core cursor parameters, with the pin pending.
- **No diff or patch endpoint.** `PUT /v1/source-tiles/{id}` is a full
  replacement. The CLI's `--dry-run` compares fields exposed by the read
  model and explicitly warns that the model omits plan details, flows,
  horizon, request-shaped relations and `source_client_local_id`. Update
  refuses drafts that omit those replacement fields; callers must provide
  the full values. The
  server has no non-destructive patch or preview operation, so dry-run
  cannot compare hidden fields.
- **No monthly recurrence.** `SourceGenerationSchema.kind` documents
  `0 = one-time` and `1 = recurring` only. `source-tiles create --monthly`
  fails with an explicit error; the CLI never approximates a monthly rule
  as a daily one. Daily and weekly wall times use fixed UTC offsets; the
  CLI does not apply daylight-saving transitions or named-zone holiday
  calendars. Core's worker materializes placements, so create acceptance
  does not prove a placement exists until it is read back.
- **Cancel reason registry values are not in the spec.** The schema points
  at the server-side registry; the CLI sends the numeric code the operator
  passes (`--reason-code`) and documents `0 = user-requested`.
- **Schedule-publish linkage is stricter in Core than the schema.** The
  OpenAPI marks `source_client_local_id`, `source_schedule`, and
  `source_horizon` individually optional/nullable. The isolated Core
  returned `422` when only the latter two were supplied. The CLI now
  validates that these three existing fields are either all absent/null
  or all supplied; it does not add wire fields.
- **Core validates completion conditions beyond the schema.** An empty
  `All` condition is structurally representable in OpenAPI but Core refuses
  it as empty/invalid. The successful publish E2E used a non-empty Fact term;
  offline preview does not attempt to prove all Core scheduling semantics.

## CLI end-to-end evidence against an isolated Core (2026-11-15)

### Stack identity

| Item | Value |
| --- | --- |
| Core image | `tastile-core-verify:rel110-d0c2ce0` (`sha256:15929d970bd2777eba203a1af4e8cb494878dca94c8b042cef657c31caaca334`, built 2026-11-14) |
| API | `127.0.0.1:31500` (`TASTILE_API_PORT=31500`, `TASTILE_ENV=development`) |
| Database | own container `tcli-e2e-db` (`127.0.0.1:55441`), own database `tastile_cli_e2e`, own user and password |
| Worker | own container `tcli-e2e-worker` |
| Delivery | `TASTILE_DELIVERY_PROVIDER=fake` — no real notification sink |
| Owner / credential | dedicated owner row and a dedicated API token in the isolated database |
| Shared verify stack | **not accessed by the E2E harness**; its API is never called and its database is never queried |

The stack is created by `scripts/e2e-local-stack.sh`; the CLI run is
`scripts/e2e-cli-smoke.sh`.

### Results

| Check | Result |
| --- | --- |
| Write to the operational port 31400 | refused locally, before any request |
| Loopback write without `TASTILE_ISOLATED_TEST_STACK=confirmed` | refused locally, before any request |
| `--monthly` | refused explicitly, not approximated as daily |
| `source-tiles list` | reads every row (paginated); the mock integration test crosses the 500-row page boundary with 503 rows |
| `source-tiles create --daily 09:00 --utc-offset 540` | accepted; readback preserves `kind=1`, `interval_ms=86400000`, `offset_min=540` |
| `source-tiles create --daily 10:00` | stores explicit `offset_min=0` (UTC), so Core cannot substitute the owner-profile timezone |
| `source-tiles get` / `placements` | decoded against the real API; detail and placement pagination tests walk multiple pages |
| `source-tiles create --once <future instant>` | the dedicated worker materialized an open placement, which the CLI then started as an execution |
| `source-tiles update --dry-run` | printed the field diff and left the stored title unchanged |
| `source-tiles update --expect-revision <stale>` | rejected by the server with `409 stale revision` |
| `source-tiles update --expect-revision <current>` | applied a complete replacement and read the updated title/revision back |
| `source-tiles cancel --dry-run` | printed the placement scope, wrote nothing |
| `source-tiles cancel` | cancelled; the readback shows `source_state = 3` |
| `source-tiles reflow --dry-run` / apply | previewed its scope, applied with `--expect-revision`, and read the source state and placements back |
| `executions start / pause / resume / finish` | all succeeded; the start response carries the execution id |
| `prompts request / list / resolve` | all succeeded |
| `schedule publish --apply` | required an explicit idempotency key, applied to the isolated loopback Core, and was read back through the source-tile API |
| `auth logout` | server-side revocation passed the write gate and the file-backed local credential was removed |

### PR #7 review-fix E2E rerun (2026-11-15)

This bounded rerun exercised the revision-default and cursor-transition code
against Core 1.1.0 with a host-built API and a newly created, task-owned
PostgreSQL container. It did not use the shared `tastile_verify` database or
connect to port 31400.

| Item | Value |
| --- | --- |
| Core source | `origin/release-1-1-0` at `e8bdfef3567f1bb5560acf5ac9f58b91280696d8`; API binary built from that source on the host |
| API | `127.0.0.1:31637`, `TASTILE_ENV=development`; `/v1/health` reported version `1.1.0` and a reachable database (overall `degraded` because this isolated stack has no bridge secret) |
| Database | own container `tcli6-pr7-review-db`, Postgres `16-alpine` image `sha256:f377d590e5474681d4d7838635031a2b50647923b7abfa803ce9b5ff35d5c663`, bound only to `127.0.0.1:55461`; database `tcli6_pr7_e2e`, dedicated user and credential |
| Delivery / worker | `TASTILE_DELIVERY_PROVIDER=fake`; worker not started for this API command-flow rerun |
| CLI auth | dedicated owner and bearer row seeded only in `tcli6_pr7_e2e`; mode-0600 file credential; isolation attestation set; token not printed |

| Check | Result |
| --- | --- |
| Authenticated `source-tiles list` | passed against the dedicated Core API |
| `source-tiles create --daily 09:00 --utc-offset 540` | created and read back; detail returned 14 occurrences and 14 placements, with `kind=1`, daily interval, and `offset_min=540` |
| Update preview and conflict | `--dry-run` passed; a stale explicit revision was rejected with `409` |
| `source-tiles update` without `--expect-revision` | passed using the revision from the diff read; new title and revision were read back |
| `source-tiles reflow` without `--expect-revision` | preview and apply passed; state was read back |
| `source-tiles cancel` without `--expect-revision` | preview and apply passed; readback showed `source_state = 3` |
| `schedule publish --apply` | offline preview passed; apply with an idempotency key succeeded, and the published source was read back through the API |
| Cursor response compatibility | Core 1.1.0 returned the legacy array shape in this run; mock pagination tests cover `{items,count,next_cursor}`, continuation cursors, independent detail cursors, and fail-closed legacy full pages |

This rerun did not exercise worker delivery/execution flows. The upcoming
Core #302/#305 request and cursor response shapes remain covered by CLI code
and mock tests, but are not live-verified until those Core changes are merged
and the published OpenAPI pin is updated.

### Authentication mismatch (test systems only)

The production CLI obtains its bearer token through the browser-mediated
PKCE flow (`{web_url}/cli/authorize` + `{web_url}/api/cli/token`). An
isolated Core stack runs neither the web origin nor a PKCE issuer, so no
token can be minted through that flow. On **test systems** the token is
therefore minted directly in the isolated database and handed to the CLI
  through `TASTILE_CREDENTIAL_FILE`, a file-backed credential store that is
  honoured **only** for a loopback API base and refuses a file readable by
  other users. The test harness keeps the bearer in a mode-0600 temporary
  file and does not print it. The OS credential store remains the production
  path; nothing in this branch weakens the production authentication model.

## Safety boundary

The Muse VM's operational Tastile v1 API is bound to **127.0.0.1:31400**.
Do not use this port for test API, migration, restart, CLI mutation, or
comparison. Localhost is **not synonymous with test**. The operational
PostgreSQL and background hooks remain the only production source of truth.

`ApiConfig` rejects port 31400 for every API request, including reads, before
connecting. Every write path also passes the shared gate: loopback needs
`TASTILE_ISOLATED_TEST_STACK=confirmed`, remote plaintext is refused,
credentials must match the configured API base and be unexpired, and the
target must accept a documented read probe. Ordinary
mutations may use an explicitly configured HTTPS API; `schedule publish
--apply` is additionally loopback-only and requires a stable idempotency key.
These are safeguards against accidental writes, not proof of isolation. An
operator must independently verify the actual DB target, test account, worker,
secrets and notification sink.
Never run a test command using the operational test account or its credentials.

A dry-run preview checks only the documented minimal OpenAPI shape. A successful
preview is **not** evidence of server-side semantic correctness. For uncertain
write retries, reuse the same `--idempotency-key`.

## Actual Muse operational baseline (reported)

- Core v1 debug API, PostgreSQL 16, migration V1_050, CLI 1.0.0; exact
  running binary commit is unverified (GitHub ZIP snapshot).
- 179 source tiles: 140 one-time; 14 daily; 22 weekly; 3 demand-driven.
  12 labels are an independent plan-role classification.
- 402 placements in the previous 17 days; 859 in the following 30 days.
- Excluded dates on 15 sources; 29 distinct time window offset pairs.
- Manual edit via PUT; on tie priorities, missing placements can be silent.
- 30-day timeline read reported about 10s; 2.3-day read 66 results.
  Worker slow statement reported rows=90,512 (not independently checked
  as table cardinality).
- User schedules do not currently use monthly recurrence, dynamic duration,
  explicit dependency rules, executions, or prompts.
- Calendar is an incomplete manual mirror of fixed anchors, not the SoT.
  Notification is driven by external hooks/SwiftBar, not Core delivery.
- Last known backup dated 2026-10-29; current restore procedure unverified.

## Comparison cases (use sanitized, immutable local input)

| Case | Existing behavior to retain | Evidence required |
| --- | --- | --- |
| Single fixed event | Exact start/end, no surprise placement | write + readback + timeline |
| Daily and weekly | Local wall time, weekday mask, valid exclusions | at least two weekly cycles |
| Holiday / exceptions | No occurrence on excluded local dates | schedule edits + nonmatching dates |
| Priority ties | Report conflict explicitly; never silently omit | deterministic negative case |
| Demand-driven pool | Fill around anchors without overlap | utilization + completion lineage |
| Window and fixed duration | Placement respects both, no silent fallback | property tests + DB E2E |
| Label and machine time | Non-blocking semantics and discoverability | read-model / display E2E |
| Multi-stage washing | No accidental overlap / lost phases | dependency vs explicit time chain |
| Reschedule/PUT | Changes affect intended occurrence only | before/after diff |
| Cancel | No unrelated source disappears | isolation regression + rollback |
| Monthly | New capability; no monthly→daily approximation | by_day/by_weekday / leap years |
| Delays and nap room | Explicit rule support or honest unsupported | counterexample + comparison |
| Offline/VM replacement | No duplicate notification or missing data | stop/start + notification isolation |

## Readiness gates

1. Independent read-only runtime inventory, process/port/DB mapping and
   binary identity on the Muse VM. Reconcile the 07:36 read report with
   the observed 09:05 API process start.
2. Fresh **local private backup** of operational data and a restore into
   a completely isolated target; no real data or secrets in GitHub, docs,
   CI artifact or shared fixture.
3. Separate test API port (not 31400), DB, owner, keyring context,
   worker and no real notification hooks. Verify with connection/traffic logs.
4. Authenticated CLI-to-isolated-Core E2E including list/create/update/
   cancel/reflow and schedule-publish with read-back and negative cases.
   Existing local API uses development headers whereas CLI 1.0.0 expects a
   bearer token via browser PKCE; resolve this mismatch **on test systems**
   without weakening production authentication.
5. Compare supported/partially supported/unsupported cases, manual steps,
   error rates, p50/p95 latency, resident resource use, model/inference
   overhead and repeated edits. Record measured vs inferred numbers.
6. Seven **full calendar days** of shadow observation prior to a real
   replacement judgment. Only the current operational instance sends
   notifications or mutates the actual schedule.
7. Any real cutover requires a separate explicit decision and proven rollback.

**Important:** source snapshot sanitization must preserve recurrence
semantics, including weekdays, month lengths, timezone offsets and exception
relationships. A blindly relative-shifted timeline is not adequate to test
holiday, DST or calendar-sensitive scheduling.
