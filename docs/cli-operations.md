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

# placements intersecting a required range (window at most 31 days)
tastile timeline --start 2026-11-20T00:00:00Z --end 2026-11-21T00:00:00Z
tastile timeline --start ... --end ... --include-labels --include-blocked --include-nested --json
```

`GET /v1/timeline` returns a bare array ordered by `span.start ASC,
placement_id ASC` — there is no cursor contract, so an omitted `--limit`
returns the complete in-range set. `include_labels` adds the non-blocking
LABEL (`role = 1`) periods; the table marks them `LABEL` and prints a
footer reminding that they never count as busy time. `--parent-placement-id`
requires `--include-nested`; `--month` requests the `summary=month`
collapse; `--min-duration-ms` / `--min-recurring-step-ms` /
`--exclude-source-kinds` map to the same-named query parameters.

`GET /v1/tiles` supports `owner_ids`, `limit`, `cursor`, `view_mode`,
`lifecycle`, `range`, `granularity`, `search` and `exclude_future`.
`tastile tiles`, `tastile today`, source-tile list, source-tile placements,
and source-tile detail collections follow cursor pages. A full legacy array
fails closed because its completeness cannot be established. For `/v1/tiles`,
the CLI requests the contract maximum of 1,000 but rejects a cursorless
500-item legacy response, matching the old Core cap. `prompts list` has no
pagination parameters in the pinned contract. The client sends no `offset`.

`tastile today` requests future projections and filters out rows whose
`projected_next_start_at` is in the past or absent, so a stale projection
cannot become the next actionable tile.

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

Update is a merge-or-replace (`PUT` of a rebuilt full payload). The CLI reads
the source tile detail — including the `definition` read-back (v1/14 §17) —
and applies the patch on top, so fields you omit are preserved rather than
silently reset. The patch is a JSON object; every field is optional:

- `title`, `description`, `color`, `icon`, `external_id` — tile metadata.
  `null` clears the nullable fields; `title` must be a string.
- `plan` — object merged per subfield (`role`, `completion`, `planning`,
  `metrics`, `decisions`, `references`); an absent subfield keeps the stored
  value, an array `[]` clears a collection subfield.
- `plan_role`, `priority`, `required_duration_ms`, `generation`, `window`,
  `split_policy` — schedule/plan scalars; absent keeps the stored value.
- `flows` — array replaces the complete flow set; absent keeps it.
- `relations` — absent keeps the stored relation payloads (read back
  normalized to `existing` per §17-2, so `local` authoring syntax is not
  preserved); `[]` or `null` clears all relations.
- `source_client_local_id` — never persisted, so read-back is always `null`;
  supply a UUID string to re-assign it.
- `horizon` — required on every update because Core never persists it;
  supply `{"start": RFC3339, "end": RFC3339}` or `horizon_start` +
  `end` + `horizon_end`.

```json
{
  "title": "Standup (renamed)",
  "plan": {"metrics": [{"kind": 0}]},
  "priority": 5,
  "horizon": {"start": "2026-11-20T00:00:00Z", "end": "2026-12-20T00:00:00Z"}
}
```

`--dry-run` prints the field-level diff between the stored definition and
the merged payload (tile, plan, schedule, flows, relations) plus the number
of derived occurrences and placements the update will recompute.

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
