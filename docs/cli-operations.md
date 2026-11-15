# CLI operations guide

How to drive a Tastile Core from the command line: what each command does,
which ones write, what a safe stop/resume looks like, and which errors you
can expect.

## Safety model

Every API mutation (`source-tiles create/update/cancel/reflow`,
`executions start/pause/resume/finish`, `prompts resolve/request/
startup-recovery`, `schedule publish --apply`, and the server-side revoke
attempt in `auth logout`) passes the same write gate before a request leaves
the process. `auth logout` still removes the local credential if revocation
is refused or the API is unavailable:

1. **API-base validation and target classification**
   - API base port **31400** is refused by the HTTP client for every request,
     including reads, before it can connect.
   - A loopback API (`127.0.0.1`, `localhost`, `::1`) is *ambiguous*: it
     can be an isolated test stack or the operational Muse stack. Writes to
     loopback are refused unless `TASTILE_ISOLATED_TEST_STACK=confirmed`
     is set.
   - A remote API base must use **HTTPS**; plaintext remote APIs are refused for reads and writes.
2. **Identity probe** — the target must answer a documented read endpoint
   (`GET /v1/source-tiles`) with the stored credential. The CLI also checks
   that the credential's API-base binding matches the configured base and
   that its expiry has not passed. This proves the URL points at a Tastile
   API that accepts the credential; the pinned contract has no health,
   version or `whoami` operation, so this does **not** prove the binary
   version, owner identity or stack isolation. Only the operator can attest
   isolation by checking the database, owner, worker, secrets and
   notification sink of the target stack. Ordinary writes allow an
   explicitly configured HTTPS API; `schedule publish --apply` is
   additionally restricted to attested loopback Core.

The confirmation flag is a safeguard, not a proof. See
[local-first-acceptance.md](./local-first-acceptance.md) for the full
isolation checklist.

## Reading

```sh
# every source tile of the owner (the CLI follows every cursor page)
tastile source-tiles list

# one source tile with its occurrences and placements
tastile source-tiles get <source-tile-id>

# every placement of a source tile (paginated)
tastile source-tiles placements <source-tile-id>

# today's tiles
tastile today
tastile tiles --lifecycle ready
tastile tiles --search "rust" --json
```

`GET /v1/tiles` supports `owner_ids`, `limit`, `view_mode`, `lifecycle`,
`range`, `granularity`, `search` and `exclude_future`, but has no offset or
cursor. `tastile tiles` and `tastile today` therefore make one request and
cannot guarantee a complete result if Core caps it; the CLI warns when the
500-row cap is reached. `prompts list` likewise has no pagination parameters
in the pinned contract. Source-tile list and placements follow `cursor` pages;
source-tile detail independently follows `occurrence_cursor` and
`placement_cursor`, with `occurrence_limit` and `placement_limit`.

The cursor request/response shape is implemented ahead of the pinned OpenAPI
revision to match Core PRs [#302](https://github.com/tastile/tastile-core/pull/302)
and [#305](https://github.com/tastile/tastile-core/pull/305). The submodule
pin remains unchanged until those contracts are merged and published. The
client accepts legacy array responses while the transition is in progress;
if an old server returns a full 500-row array without a cursor, it reports
that completeness cannot be proven instead of silently returning a truncated
list. No `offset` parameter is sent.

## Writing

### Create a source tile

The convenience flags cover exactly the recurrence shapes the pinned
contract can express:

```sh
# one-time occurrence at an exact instant
tastile source-tiles create --title "Read the Rust book" \
  --once 2026-11-20T09:00:00Z --duration 90

# every day at a local wall time (fixed UTC offset; defaults to UTC)
tastile source-tiles create --title "Standup" \
  --daily 09:30 --utc-offset 540 --duration 15

# selected weekdays at a local wall time
tastile source-tiles create --title "Gym" \
  --weekly 19:00 --days mon,wed,fri --utc-offset 540 --duration 60

# exception days: excluded local calendar dates
tastile source-tiles create --title "Team sync" \
  --weekly 10:00 --days thu --exclude 2026-11-26 --exclude 2026-12-24 \
  --utc-offset 540 --duration 60
```

`--monthly` is refused with an explicit error: the pinned contract
describes `SourceGenerationSchema.kind` as `0 = one-time` and
`1 = recurring`, and the CLI never approximates a monthly rule as a daily
one.

Daily and weekly wall times use a fixed UTC offset in minutes; omission of
`--utc-offset` means UTC (`0`). The CLI does not apply daylight-saving
transitions or named-zone holiday calendars; choose an offset appropriate
for the dates being modelled. `--exclude`
skips explicit local calendar dates. Core's worker performs occurrence
materialization and placement after the definition is accepted; the CLI has
no separate client-side auto-placement switch.

For full control, `--from-json` takes a draft document; see
`docs/architecture.md` for the shape.

### Update, diff and revision conflicts

```sh
# show what would change, without writing
tastile source-tiles update <id> --from-json ./draft.json --dry-run

# write, overriding the revision read by the CLI with revision 12
tastile source-tiles update <id> --from-json ./draft.json --expect-revision 12
```

`--expect-revision` maps to the contract's `expected_revision` envelope
field. By default, update uses the revision from the source-tile read shown
in its diff; cancel and reflow fetch the current revision before sending the
mutation. The optional flag overrides that value. When the stored revision
moved on, the server answers `409` and the CLI reports the conflict instead
of overwriting someone else's change:

```text
error: update_source_tile failed: http 409: {"kind":"Conflict",
"message":"stale revision: have=13, expected=12", ...}
```

Update is a full replacement (`PUT`), not a patch. Its JSON draft must
include `title`, `description`, `color`, `icon`, `external_id`, `priority`,
`required_duration_ms`, `horizon_start`, `horizon_end`, `plan_role`, `plan`,
`flows`, `generation`, `window`, `split_policy`, `relations`, and
`source_client_local_id`. Use explicit `null` for optional tile metadata,
`source_client_local_id`, and `[]` for empty collections. The read model
omits plan details, flows, horizon, the local client id and request-shaped
relations, so preserve those values from the original request when editing.
The dry-run warning also names `source_client_local_id`, which is absent from
the read model and cannot be recovered from `get` output.

```json
{
  "title": "Standup",
  "description": null,
  "color": null,
  "icon": null,
  "external_id": null,
  "priority": 0,
  "required_duration_ms": 900000,
  "horizon_start": "2026-11-20T00:00:00Z",
  "horizon_end": "2026-12-20T00:00:00Z",
  "plan_role": 0,
  "plan": {
    "completion": {"root": {"All": []}, "tasks": [], "time_requirements": []},
    "planning": {"nesting_rules": [], "placement_rules": []},
    "metrics": [], "decisions": [], "references": []
  },
  "flows": [],
  "generation": {"kind": 1, "starts_at": "2026-11-20T00:30:00Z", "interval_ms": 86400000, "offset_min": 540},
  "window": {"start_offset_ms": 0, "end_offset_ms": 14400000},
  "split_policy": {"kind": 0},
  "relations": [],
  "source_client_local_id": null
}
```

### Cancel and reflow

```sh
# show the placements a cancel would touch, without writing
tastile source-tiles cancel <id> --dry-run

# cancel (reason code 0 = user-requested; the wire value is numeric)
tastile source-tiles cancel <id> --reason-code 0

# show the placements inside the reflow window, without writing
tastile source-tiles reflow <id> \
  --from 2026-11-20T00:00:00Z --to 2026-11-27T00:00:00Z --dry-run

tastile source-tiles reflow <id> \
  --from 2026-11-20T00:00:00Z --to 2026-11-27T00:00:00Z
```

After a successful write the CLI reads the source tile and its placements
back from the server and prints the resulting state, so the output shows
what the server actually stored rather than what the request claimed.

### Executions and prompts

```sh
tastile executions start <placement-id>
tastile executions pause <execution-id>
tastile executions resume <execution-id>
tastile executions finish <execution-id> --kind 0 --note "done"

tastile prompts list
tastile prompts resolve <prompt-id> --resolution ack
```

### Schedule publication

```sh
# offline preview: no credential store, no network, no database
tastile schedule publish --from-json ./local-schedule.json

# publish to an isolated loopback Core
TASTILE_API_URL=http://127.0.0.1:31500 \
  TASTILE_ISOLATED_TEST_STACK=confirmed \
  tastile schedule publish --from-json ./local-schedule.json --apply \
    --idempotency-key <stable-uuid>
```

Reuse the same `--idempotency-key` when a response was uncertain.
When publishing a source-backed definition, provide
`source_client_local_id`, `source_schedule` and `source_horizon` together;
Core rejects partial linkage even though the pinned schema marks these
fields individually nullable. Core also validates that completion and flow
conditions are semantically non-empty. Offline preview checks the contract
shape but does not prove these server-owned scheduling semantics.

## Safe stop and resume

```sh
# 1. stop creating new work (writes are refused until the stack is back)
docker stop <core-api> <core-worker>

# 2. read-only inspection still works against a running API
tastile source-tiles list
tastile source-tiles placements <id>

# 3. resume
docker start <core-api> <core-worker>
```

Nothing in the CLI holds a long-lived transaction or a background worker,
so stopping the API and the worker is enough: the schedule state lives in
PostgreSQL, and every command re-reads it from the server.

## Error messages you can expect

| Message | Meaning |
| --- | --- |
| `invalid base url: API base URL may not use reserved operational port 31400` | The operational Muse port was targeted; all requests are refused. Pick another port. |
| `no isolated test-stack confirmation for API …` | Loopback target without `TASTILE_ISOLATED_TEST_STACK=confirmed`. |
| `the write target … rejected the stored credential (HTTP 401)` | The stored token is not valid for that target. |
| `the stored credential belongs to a different API base …` | Re-login for the configured API; the CLI will not reuse the token at a different base. |
| `the stored API credential has expired …` | Re-login before writing. |
| `http 409: stale revision: have=13, expected=12` | Revision conflict. Re-read the source tile and retry. |
| `monthly recurrence is not described by the pinned Tastile OpenAPI …` | Unsupported semantics; the CLI refuses instead of approximating. |
| `schedule publish failed; retry with the same --idempotency-key` | Uncertain write outcome. Retry with the same key. |
