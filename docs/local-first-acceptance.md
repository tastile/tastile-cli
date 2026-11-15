# Core + CLI local-first acceptance — Muse parity

Owner issue: [tastile-cli#6](https://github.com/tastile/tastile-cli/issues/6).
Plan: [tastile-root#105](https://github.com/tastile/tastile-root/issues/105).
Evidence baseline: Muse operational inventory, 2026-11-15 10:31 JST,
reported to a private shared document. Metrics below are **Muse's reported
observations**, not independently verified in this repository.

## Safety boundary

The Muse VM's operational Tastile v1 API is bound to **127.0.0.1:31400**.
Do not use this port for test API, migration, restart, CLI mutation, or
comparison. Localhost is **not synonymous with test**. The operational
PostgreSQL and background hooks remain the only production source of truth.

The `tastile schedule publish --apply` command has two deliberately narrow
guardrails: (a) reject non-loopback targets and operational port 31400;
(b) require `TASTILE_ISOLATED_TEST_STACK=confirmed`. These are safeguards
against accidental writes, not proof of isolation. An operator must independently
verify the actual DB target, test account, worker, secrets and notification sink.
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
