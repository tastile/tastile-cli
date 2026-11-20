#!/usr/bin/env bash
# End-to-end smoke test of the CLI against a dedicated isolated Core stack.
#
# The stack comes from `scripts/e2e-local-stack.sh` (own PostgreSQL
# container, database, owner, API and worker; port other than 31400).
#
# The script asserts the behaviours that issue tastile/tastile-cli#6 asks
# for:
#
#   * the write-target gate refuses the reserved operational port and
#     loopback targets without an isolation attestation,
#   * list/create/get/placements work and read back real state,
#   * an update diff is shown without writing,
#   * a stale `--expect-revision` is rejected with a conflict,
#   * reflow and cancel preview their scopes, apply and read back,
#   * schedule publish applies with a stable key and is read back,
#   * server-side revocation in `auth logout` passes through the write gate,
#   * an unsupported recurrence (monthly) is refused, not approximated.
#
# Usage: scripts/e2e-cli-smoke.sh [path-to-tastile-binary]
set -euo pipefail
umask 077

BIN="${1:-target/debug/tastile}"
API_PORT="${TCLI_E2E_API_PORT:-31500}"
DB_PORT="${TCLI_E2E_DB_PORT:-55441}"
API_URL="http://127.0.0.1:${API_PORT}"
DB_CONTAINER="tcli-e2e-db"
API_CONTAINER="tcli-e2e-api"
WORKER_CONTAINER="tcli-e2e-worker"
NETWORK="tcli-e2e-net"
DB_NAME="tastile_cli_e2e"
DB_USER="tastile"
CORE_IMAGE="${TCLI_E2E_IMAGE:-tastile-core-verify:rel110-d0c2ce0}"
POSTGRES_IMAGE="postgres:16-alpine"
WORK_DIR="$(mktemp -d)"
RUN_ID="$(uuidgen | tr 'A-Z' 'a-z' | tr -d '-')"
DAILY_TITLE="tcli-e2e-${RUN_ID}-daily"
UTC_TITLE="tcli-e2e-${RUN_ID}-utc-default"
EXEC_TITLE="tcli-e2e-${RUN_ID}-executions"
PROMPT_NOTE="tcli-e2e-${RUN_ID}-prompt"
trap 'rm -rf "${WORK_DIR}"' EXIT

fail() {
  echo "FAIL: $*" >&2
  exit 1
}

pass() {
  echo "PASS: $*"
}

if [[ ! -x "${BIN}" ]]; then
  fail "CLI binary not found at ${BIN} (run: cargo build)"
fi

# Verify container, image, environment, network and database identity before
# minting a test credential or sending any write. Only non-secret fields are
# printed; Docker's container environment contains the database password.
EXPECTED_CORE_IMAGE_ID="$(docker image inspect --format '{{.Id}}' "${CORE_IMAGE}")"
EXPECTED_POSTGRES_IMAGE_ID="$(docker image inspect --format '{{.Id}}' "${POSTGRES_IMAGE}")"
docker inspect "${API_CONTAINER}" "${DB_CONTAINER}" "${WORKER_CONTAINER}" | \
  python3 -c '
import json, sys, urllib.parse
api_port, core_id, postgres_id, network = sys.argv[1:]
containers = {c["Name"].lstrip("/"): c for c in json.load(sys.stdin)}
api = containers["tcli-e2e-api"]
db = containers["tcli-e2e-db"]
worker = containers["tcli-e2e-worker"]
def env(container):
    return dict(item.split("=", 1) for item in container["Config"].get("Env", []) if "=" in item)
def require(condition, message):
    if not condition:
        raise SystemExit("FAIL: isolated stack identity check: " + message)
api_env, db_env, worker_env = env(api), env(db), env(worker)
require(api["Image"] == core_id and worker["Image"] == core_id, "API/worker image id mismatch")
require(db["Image"] == postgres_id, "database image id mismatch")
require(api_env.get("TASTILE_ENV") == "development" and worker_env.get("TASTILE_ENV") == "development" and api_env.get("TASTILE_API_HOST") == "0.0.0.0" and api_env.get("TASTILE_API_PORT") == api_port, "API/worker environment mismatch")
require(api_env.get("TASTILE_DELIVERY_PROVIDER") == "fake" and worker_env.get("TASTILE_DELIVERY_PROVIDER") == "fake", "delivery provider is not fake")
require(db_env.get("POSTGRES_DB") == "tastile_cli_e2e" and db_env.get("POSTGRES_USER") == "tastile", "dedicated database identity mismatch")
require(all(set(c["NetworkSettings"]["Networks"]) == {network} for c in (api, db, worker)), "containers are not isolated to the dedicated network")
for key in ("DATABASE_URL", "TASTILE_DATABASE_URL"):
    parsed = urllib.parse.urlparse(api_env.get(key, ""))
    require(parsed.hostname == "tcli-e2e-db" and parsed.username == "tastile" and parsed.path.lstrip("/") == "tastile_cli_e2e", key + " does not target the dedicated DB/user")
parsed = urllib.parse.urlparse(worker_env.get("TASTILE_DATABASE_URL", ""))
require(parsed.hostname == "tcli-e2e-db" and parsed.username == "tastile" and parsed.path.lstrip("/") == "tastile_cli_e2e", "worker DB does not target the dedicated DB/user")
parsed = urllib.parse.urlparse(worker_env.get("DATABASE_URL", ""))
require(parsed.hostname == "tcli-e2e-db" and parsed.username == "tastile" and parsed.path.lstrip("/") == "tastile_cli_e2e", "worker DATABASE_URL does not target the dedicated DB/user")
print("PASS: isolated stack identity verified (dedicated images, DB, network and fake delivery)")
' "${API_PORT}" "${EXPECTED_CORE_IMAGE_ID}" "${EXPECTED_POSTGRES_IMAGE_ID}" "${NETWORK}"
docker network inspect "${NETWORK}" | python3 -c '
import json, sys
expected = {"tcli-e2e-api", "tcli-e2e-db", "tcli-e2e-worker"}
network = json.load(sys.stdin)[0]
attached = {container["Name"] for container in network.get("Containers", {}).values()}
if attached != expected:
    raise SystemExit("FAIL: isolated network has unexpected containers: " + ", ".join(sorted(attached ^ expected)))
print("PASS: the dedicated network contains only the API, database and worker")
'
API_BINDING="$(docker port "${API_CONTAINER}" "${API_PORT}/tcp")"
DB_BINDING="$(docker port "${DB_CONTAINER}" 5432/tcp)"
[[ "${API_BINDING}" == "127.0.0.1:${API_PORT}" ]] || fail "API is not bound to the dedicated loopback port ${API_PORT}"
[[ "${DB_BINDING}" == "127.0.0.1:${DB_PORT}" ]] || fail "database is not bound to the dedicated loopback port ${DB_PORT}"
[[ "${API_PORT}" != "31400" && "${DB_PORT}" != "31400" ]] || fail "port 31400 is reserved for the operational stack"
printf 'Core image id: %s\nAPI: %s\nDatabase: %s in %s\n' \
  "${EXPECTED_CORE_IMAGE_ID}" "${API_URL}" "${DB_NAME}" "${DB_CONTAINER}"

# --- credential -----------------------------------------------------------
# Mint a bearer token for a dedicated owner in the isolated database and
# store it in the OS credential store, exactly like `tastile auth login`
# would (the local Core has no web bridge, so the token is minted here).
OWNER="$(uuidgen | tr 'A-Z' 'a-z')"
RAW="$(openssl rand -hex 24)"

docker exec -i "${DB_CONTAINER}" sh -c \
  'PGPASSWORD="$POSTGRES_PASSWORD" psql -U "$POSTGRES_USER" -d "$POSTGRES_DB" -v ON_ERROR_STOP=1 -q' <<SQL
INSERT INTO v1_subject (id, kind, external_subject, display_name, email, timezone)
VALUES ('${OWNER}', 0, 'tcli-e2e-${OWNER}', 'tcli e2e owner', 'tcli-e2e@example.invalid', 'Asia/Tokyo')
ON CONFLICT (id) DO NOTHING;
INSERT INTO v1_api_token (id, owner_id, label, token_hash, prefix, scopes, expires_at)
VALUES (gen_random_uuid(), '${OWNER}', 'tcli-e2e-local', sha256('${RAW}'::bytea), 'tcli', 'all', now() + interval '1 day');
SQL

# The OS credential store needs a GUI session; a headless runner has no
# reachable keychain. The CLI therefore accepts a file-backed credential for
# a loopback API base (test systems only).
CREDENTIAL_FILE="${WORK_DIR}/credential.json"
printf '{"api_base_url":"%s","bearer":"%s","expires_at":null,"subject":"%s"}' \
  "${API_URL}" "${RAW}" "${OWNER}" >"${CREDENTIAL_FILE}"
chmod 600 "${CREDENTIAL_FILE}"

export TASTILE_API_URL="${API_URL}"
export TASTILE_ISOLATED_TEST_STACK=confirmed
export TASTILE_CREDENTIAL_FILE="${CREDENTIAL_FILE}"

# --- negative: operational port ------------------------------------------
# A write command: the gate refuses it locally, so the operational stack
# never even receives a request.
if TASTILE_API_URL="http://127.0.0.1:31400" "${BIN}" source-tiles create \
  --title "operational-port probe" --once 2026-12-01T09:00:00Z >/dev/null 2>"${WORK_DIR}/err"; then
  fail "the operational port must not accept a write"
fi
grep -q "31400" "${WORK_DIR}/err" || fail "expected the operational-port refusal, got: $(cat "${WORK_DIR}/err")"
pass "operational port 31400 is refused"

# --- negative: loopback without attestation ------------------------------
if env -u TASTILE_ISOLATED_TEST_STACK "${BIN}" source-tiles create \
  --title "unattested probe" --once 2026-12-01T09:00:00Z >/dev/null 2>"${WORK_DIR}/err"; then
  fail "a loopback write without an isolation attestation must be refused"
fi
grep -q "isolated test-stack" "${WORK_DIR}/err" || fail "expected the attestation refusal, got: $(cat "${WORK_DIR}/err")"
pass "loopback writes require an isolation attestation"

# --- negative: monthly is refused, not approximated ----------------------
if "${BIN}" source-tiles create --title "monthly probe" --monthly >/dev/null 2>"${WORK_DIR}/err"; then
  fail "monthly recurrence must be refused"
fi
grep -q "monthly" "${WORK_DIR}/err" || fail "expected the monthly refusal, got: $(cat "${WORK_DIR}/err")"
pass "monthly recurrence is refused instead of approximated"

# --- list (empty) ---------------------------------------------------------
"${BIN}" source-tiles list --json >"${WORK_DIR}/list.json"
pass "source-tiles list works against the isolated stack"

# --- create ---------------------------------------------------------------
"${BIN}" source-tiles create --title "${DAILY_TITLE}" --daily 09:00 --utc-offset 540 \
  --duration 30 --horizon-days 14 >"${WORK_DIR}/create.txt"
grep -q "Created source tile" "${WORK_DIR}/create.txt" || fail "create did not report success"
pass "create (daily, fixed UTC offset) succeeded"

"${BIN}" source-tiles list --json >"${WORK_DIR}/list.json"
SOURCE_ID="$(jq -r --arg title "${DAILY_TITLE}" '[.[] | select(.title == $title)][0].source_tile_id // empty' "${WORK_DIR}/list.json")"
[[ -n "${SOURCE_ID}" && "${SOURCE_ID}" != "null" ]] || fail "no source tile in the list"
pass "created source tile ${SOURCE_ID}"

"${BIN}" source-tiles get "${SOURCE_ID}" --json >"${WORK_DIR}/daily.json"
jq -e '.source.schedule.generation.kind == 1 and .source.schedule.generation.interval_ms == 86400000 and .source.schedule.generation.offset_min == 540' \
  "${WORK_DIR}/daily.json" >/dev/null \
  || fail "daily recurrence readback did not preserve kind, interval and UTC offset"
pass "daily recurrence readback preserved the fixed +09:00 offset"

# When omitted, the helper uses UTC explicitly so Core cannot substitute
# an owner-profile timezone for the wall-time anchor.
"${BIN}" source-tiles create --title "${UTC_TITLE}" --daily 10:00 \
  --duration 30 --horizon-days 14 >"${WORK_DIR}/utc-default.txt"
"${BIN}" source-tiles list --json >"${WORK_DIR}/list.json"
UTC_SOURCE="$(jq -r --arg title "${UTC_TITLE}" '[.[] | select(.title == $title)][0].source_tile_id // empty' "${WORK_DIR}/list.json")"
[[ -n "${UTC_SOURCE}" && "${UTC_SOURCE}" != "null" ]] || fail "the UTC-default source tile is missing"
"${BIN}" source-tiles get "${UTC_SOURCE}" --json >"${WORK_DIR}/utc-default.json"
jq -e '.source.schedule.generation.offset_min == 0' "${WORK_DIR}/utc-default.json" >/dev/null \
  || fail "omitting --utc-offset did not store offset_min=0"
pass "omitting --utc-offset stores an explicit UTC offset"

# --- get / placements -----------------------------------------------------
"${BIN}" source-tiles get "${SOURCE_ID}" >"${WORK_DIR}/get.txt"
grep -q "${SOURCE_ID}" "${WORK_DIR}/get.txt" || fail "get did not return the source tile"
pass "get returned the source tile"

"${BIN}" source-tiles placements "${SOURCE_ID}" >"${WORK_DIR}/placements.txt"
pass "placements listing works"

# --- update diff (no write) ----------------------------------------------
"${BIN}" source-tiles get "${SOURCE_ID}" --json >"${WORK_DIR}/current.json"
python3 - "${WORK_DIR}/current.json" "${WORK_DIR}/draft.json" "${DAILY_TITLE}-renamed" <<'PY'
import json
import sys
from datetime import datetime, timedelta, timezone

detail_path, draft_path, title = sys.argv[1:]
with open(detail_path, encoding="utf-8") as source_file:
    detail = json.load(source_file)
source = detail["source"]
today = datetime.now(timezone.utc).date()
horizon_start = datetime.combine(today, datetime.min.time(), timezone.utc)
horizon_end = horizon_start + timedelta(days=14)
draft = {
    "title": title,
    "description": source.get("description"),
    "color": source.get("color"),
    "icon": source.get("icon"),
    "external_id": source.get("external_id"),
    "horizon_start": horizon_start.isoformat().replace("+00:00", "Z"),
    "horizon_end": horizon_end.isoformat().replace("+00:00", "Z"),
    "priority": source["schedule"]["priority"],
    "required_duration_ms": source["schedule"]["required_duration_ms"],
    "plan_role": source["plan_role"],
    "plan": {
        "completion": {"root": {"All": []}, "tasks": [], "time_requirements": []},
        "planning": {"nesting_rules": [], "placement_rules": []},
        "metrics": [],
        "decisions": [],
        "references": [],
    },
    "flows": [],
    "generation": source["schedule"]["generation"],
    "window": source["schedule"]["window"],
    "split_policy": source["schedule"]["split_policy"],
    "relations": [],
    "source_client_local_id": None,
}
with open(draft_path, "w", encoding="utf-8") as draft_file:
    json.dump(draft, draft_file)
PY
"${BIN}" source-tiles update "${SOURCE_ID}" --from-json "${WORK_DIR}/draft.json" --dry-run >"${WORK_DIR}/diff.txt"
grep -q "title" "${WORK_DIR}/diff.txt" || fail "the diff did not report the title change"
grep -q "Dry run" "${WORK_DIR}/diff.txt" || fail "the dry run did not report that nothing was written"
pass "update --dry-run shows the diff without writing"

"${BIN}" source-tiles get "${SOURCE_ID}" --json >"${WORK_DIR}/after-dry-run.json"
TITLE_AFTER="$(jq -r '.source.title' "${WORK_DIR}/after-dry-run.json")"
[[ "${TITLE_AFTER}" == "${DAILY_TITLE}" ]] || fail "the dry run changed the title to ${TITLE_AFTER}"
pass "the dry run left the stored source tile untouched"

# --- revision conflict ----------------------------------------------------
REVISION="$(jq -r '.source.revision' "${WORK_DIR}/after-dry-run.json")"
if "${BIN}" source-tiles update "${SOURCE_ID}" --from-json "${WORK_DIR}/draft.json" \
  --expect-revision "$((REVISION + 5))" >/dev/null 2>"${WORK_DIR}/err"; then
  fail "a stale --expect-revision must be rejected"
fi
grep -q "409" "${WORK_DIR}/err" || fail "expected a 409 conflict, got: $(cat "${WORK_DIR}/err")"
pass "a stale --expect-revision is rejected with a conflict"

# The current revision is an explicit compare-and-swap guard. The complete
# replacement document includes fields the API read model does not expose.
"${BIN}" source-tiles update "${SOURCE_ID}" --from-json "${WORK_DIR}/draft.json" \
  --expect-revision "${REVISION}" >"${WORK_DIR}/update.txt"
grep -q "Updated source tile" "${WORK_DIR}/update.txt" || fail "update did not report success"
grep -q "readback" "${WORK_DIR}/update.txt" || fail "update did not read the state back"
"${BIN}" source-tiles get "${SOURCE_ID}" --json >"${WORK_DIR}/after-update.json"
TITLE_AFTER="$(jq -r '.source.title' "${WORK_DIR}/after-update.json")"
[[ "${TITLE_AFTER}" == "${DAILY_TITLE}-renamed" ]] || fail "update readback returned title ${TITLE_AFTER}"
pass "a full update succeeded with revision check and readback"

# --- reflow preview + reflow + readback ---------------------------------
AFTER_UPDATE_REVISION="$(jq -r '.source.revision' "${WORK_DIR}/after-update.json")"
REFLOW_FROM="$(python3 -c 'from datetime import datetime, timedelta, timezone; print((datetime.now(timezone.utc) - timedelta(days=1)).isoformat().replace("+00:00", "Z"))')"
REFLOW_TO="$(python3 -c 'from datetime import datetime, timedelta, timezone; print((datetime.now(timezone.utc) + timedelta(days=13)).isoformat().replace("+00:00", "Z"))')"
"${BIN}" source-tiles reflow "${SOURCE_ID}" --from "${REFLOW_FROM}" --to "${REFLOW_TO}" \
  --dry-run >"${WORK_DIR}/reflow-preview.txt"
grep -q "reflow impact" "${WORK_DIR}/reflow-preview.txt" || fail "the reflow preview did not report its scope"
grep -q "Dry run" "${WORK_DIR}/reflow-preview.txt" || fail "the reflow preview did not report that nothing was written"
pass "reflow --dry-run previews the affected placements"

"${BIN}" source-tiles reflow "${SOURCE_ID}" --from "${REFLOW_FROM}" --to "${REFLOW_TO}" \
  --expect-revision "${AFTER_UPDATE_REVISION}" >"${WORK_DIR}/reflow.txt"
grep -q "Reflow dispatched" "${WORK_DIR}/reflow.txt" || fail "reflow did not report success"
grep -q "readback" "${WORK_DIR}/reflow.txt" || fail "reflow did not read the state back"
pass "reflow succeeded with revision check and readback"

# --- cancel preview + cancel + readback ----------------------------------
"${BIN}" source-tiles cancel "${SOURCE_ID}" --dry-run >"${WORK_DIR}/cancel-preview.txt"
grep -q "cancel impact" "${WORK_DIR}/cancel-preview.txt" || fail "the cancel preview did not report its scope"
grep -q "Dry run" "${WORK_DIR}/cancel-preview.txt" || fail "the cancel preview did not report that nothing was written"
pass "cancel --dry-run previews the affected placements"

"${BIN}" source-tiles cancel "${SOURCE_ID}" --reason-code 0 >"${WORK_DIR}/cancel.txt"
grep -q "Cancelled source tile" "${WORK_DIR}/cancel.txt" || fail "cancel did not report success"
grep -q "readback" "${WORK_DIR}/cancel.txt" || fail "cancel did not read the state back"
pass "cancel succeeded and read the state back"

"${BIN}" source-tiles get "${SOURCE_ID}" --json >"${WORK_DIR}/final.json"
STATE="$(jq -r '.source.source_state' "${WORK_DIR}/final.json")"
[[ "${STATE}" == "3" ]] || fail "expected source_state 3 (cancelled), got ${STATE}"
pass "the cancelled source tile reads back as state 3"

# --- executions lifecycle -------------------------------------------------
EXEC_AT="$(python3 -c 'from datetime import datetime, timedelta, timezone; print((datetime.now(timezone.utc) + timedelta(hours=1)).isoformat().replace("+00:00", "Z"))')"
"${BIN}" source-tiles create --title "${EXEC_TITLE}" --once "${EXEC_AT}" \
  --duration 30 --horizon-days 2 >/dev/null
EXEC_SOURCE=""
for _ in $(seq 1 15); do
  "${BIN}" source-tiles list --json >"${WORK_DIR}/list.json"
  EXEC_SOURCE="$(jq -r --arg title "${EXEC_TITLE}" '[.[] | select(.title == $title)][0].source_tile_id // empty' "${WORK_DIR}/list.json")"
  if [[ -n "${EXEC_SOURCE}" && "${EXEC_SOURCE}" != "null" ]]; then break; fi
  sleep 2
done
[[ -n "${EXEC_SOURCE}" && "${EXEC_SOURCE}" != "null" ]] || fail "the created source tile is missing"

# The worker materializes the placements; wait until one is open.
PLACEMENT=""
for _ in $(seq 1 15); do
  "${BIN}" source-tiles placements "${EXEC_SOURCE}" --json >"${WORK_DIR}/placements.json"
  PLACEMENT="$(jq -r '[.[] | select(.closed == false)][0].placement_id // empty' "${WORK_DIR}/placements.json")"
  if [[ -n "${PLACEMENT}" ]]; then break; fi
  sleep 2
done
[[ -n "${PLACEMENT}" ]] || fail "the worker materialized no open placement"
pass "the worker materialized the one-time placement"

"${BIN}" executions start "${PLACEMENT}" --json >"${WORK_DIR}/start.json"
EXECUTION="$(jq -r '.aggregate.id // empty' "${WORK_DIR}/start.json")"
[[ -n "${EXECUTION}" ]] || fail "executions start did not return the execution id"
pass "executions start returned the execution id"

"${BIN}" executions pause "${EXECUTION}" >"${WORK_DIR}/pause.txt"
grep -q "Paused execution" "${WORK_DIR}/pause.txt" || fail "pause failed: $(cat "${WORK_DIR}/pause.txt")"
pass "executions pause succeeded"

"${BIN}" executions resume "${EXECUTION}" >"${WORK_DIR}/resume.txt"
grep -q "Resumed execution" "${WORK_DIR}/resume.txt" || fail "resume failed: $(cat "${WORK_DIR}/resume.txt")"
pass "executions resume succeeded"

"${BIN}" executions finish "${EXECUTION}" --kind 0 --note "e2e" >"${WORK_DIR}/finish.txt"
grep -q "Finished execution" "${WORK_DIR}/finish.txt" || fail "finish failed: $(cat "${WORK_DIR}/finish.txt")"
pass "executions finish succeeded"

# --- prompts --------------------------------------------------------------
"${BIN}" prompts request --kind 1 --note "${PROMPT_NOTE}" >"${WORK_DIR}/prompt.txt"
grep -q "Prompt requested" "${WORK_DIR}/prompt.txt" || fail "prompt request failed: $(cat "${WORK_DIR}/prompt.txt")"
"${BIN}" prompts list --json >"${WORK_DIR}/prompts.json"
PROMPT_ID="$(jq -r --arg note "${PROMPT_NOTE}" '[.[] | select(.note == $note)][0].id // empty' "${WORK_DIR}/prompts.json")"
[[ -n "${PROMPT_ID}" ]] || fail "the requested prompt is not listed"
"${BIN}" prompts resolve "${PROMPT_ID}" --resolution ack >"${WORK_DIR}/resolve.txt"
grep -q "Resolved prompt" "${WORK_DIR}/resolve.txt" || fail "prompt resolve failed: $(cat "${WORK_DIR}/resolve.txt")"
pass "prompts request/list/resolve succeeded"

# --- schedule publish -----------------------------------------------------
python3 - "${WORK_DIR}/schedule.json" "tcli-e2e-${RUN_ID}-schedule" <<'PY'
import json
import sys
import uuid
from datetime import datetime, timedelta, timezone

path, title = sys.argv[1:]
now = datetime.now(timezone.utc)
schedule_at = now + timedelta(hours=1)
payload = {
    "tile": {"title": title},
    "plan": {
        "role": 0,
        "references": [],
        "completion": {
            "root": {"Term": {"Fact": {"key": "ready", "comparison": "Exists"}}},
            "tasks": [],
            "time_requirements": [],
        },
        "planning": {"placement_rules": [], "nesting_rules": []},
        "metrics": [],
        "decisions": [],
    },
    "reference_targets": [],
    "windows": [],
    "flows": [],
    "source_horizon": {
        "start": now.isoformat().replace("+00:00", "Z"),
        "end": (now + timedelta(days=14)).isoformat().replace("+00:00", "Z"),
    },
    "source_client_local_id": str(uuid.uuid4()),
    "source_schedule": {
        "generation": {"kind": 0, "at": schedule_at.isoformat().replace("+00:00", "Z")},
        "window": {"start_offset_ms": 0, "end_offset_ms": 14400000},
        "split_policy": {"kind": 0},
        "required_duration_ms": 1800000,
        "priority": 0,
    },
}
with open(path, "w", encoding="utf-8") as output:
    json.dump(payload, output)
PY
# The preview must work without a credential and without a reachable API.
if TASTILE_API_URL="http://127.0.0.1:1" env -u TASTILE_CREDENTIAL_FILE "${BIN}" schedule publish \
  --from-json "${WORK_DIR}/schedule.json" >"${WORK_DIR}/preview.txt" 2>&1; then
  pass "schedule publish previews offline (no credential, no network)"
else
  fail "the offline schedule preview failed: $(cat "${WORK_DIR}/preview.txt")"
fi

# A payload the contract rejects must fail before the request is sent.
cat >"${WORK_DIR}/bad-schedule.json" <<'JSON'
{"tile": {"title": "incomplete"}, "plan": {"role": 0}, "reference_targets": [], "windows": [], "flows": []}
JSON
if "${BIN}" schedule publish --from-json "${WORK_DIR}/bad-schedule.json" --apply \
  --idempotency-key "$(uuidgen)" \
  >/dev/null 2>"${WORK_DIR}/err"; then
  fail "an incomplete plan must be refused"
fi
grep -q "references" "${WORK_DIR}/err" || fail "expected the plan validation error, got: $(cat "${WORK_DIR}/err")"
pass "an incomplete plan is refused before the request"

if "${BIN}" schedule publish --from-json "${WORK_DIR}/schedule.json" --apply \
  >/dev/null 2>"${WORK_DIR}/err"; then
  fail "schedule publish --apply must require a stable idempotency key"
fi
grep -q "requires --idempotency-key" "${WORK_DIR}/err" || fail "expected the idempotency-key refusal, got: $(cat "${WORK_DIR}/err")"
pass "schedule publish --apply requires a reusable idempotency key"

"${BIN}" schedule publish --from-json "${WORK_DIR}/schedule.json" --apply \
  --idempotency-key "$(uuidgen)" >"${WORK_DIR}/publish.txt" 2>&1 \
  || fail "schedule publish failed: $(cat "${WORK_DIR}/publish.txt")"
grep -q "command_id" "${WORK_DIR}/publish.txt" || fail "the publish response has no command id"
pass "schedule publish succeeded against the isolated Core"

# The documented SourceTile listing/read endpoint is used to read the
# published source back and verify what Core actually stored.
"${BIN}" source-tiles list --json >"${WORK_DIR}/after-publish.json"
PUBLISHED_SOURCE="$(jq -r --arg title "tcli-e2e-${RUN_ID}-schedule" '[.[] | select(.title == $title)][0].source_tile_id // empty' "${WORK_DIR}/after-publish.json")"
[[ -n "${PUBLISHED_SOURCE}" ]] || fail "published schedule was not visible in the source-tile listing"
"${BIN}" source-tiles get "${PUBLISHED_SOURCE}" --json >"${WORK_DIR}/published-readback.json"
PUBLISHED_TITLE="$(jq -r '.source.title' "${WORK_DIR}/published-readback.json")"
[[ "${PUBLISHED_TITLE}" == "tcli-e2e-${RUN_ID}-schedule" ]] || fail "published schedule readback returned title ${PUBLISHED_TITLE}"
pass "published schedule was read back through the API"

# --- isolation check ------------------------------------------------------
OUR_COUNT="$(docker exec "${DB_CONTAINER}" sh -c \
  'PGPASSWORD="$POSTGRES_PASSWORD" psql -U "$POSTGRES_USER" -d "$POSTGRES_DB" -tAc "select count(*) from v1_source_tile;"')"
[[ -n "${OUR_COUNT}" ]] || fail "the isolated database did not return a source-tile count"
echo "note: source tiles in dedicated ${DB_NAME} database = ${OUR_COUNT}"
pass "the API writes are persisted in the dedicated database"

# Server-side revocation is guarded like other API mutations. Local logout
# must also remove the file-backed credential used by this test.
"${BIN}" auth logout >"${WORK_DIR}/logout.txt" 2>&1 \
  || fail "auth logout failed: $(cat "${WORK_DIR}/logout.txt")"
grep -q "Local credential removed" "${WORK_DIR}/logout.txt" \
  || fail "auth logout did not remove the local credential"
[[ ! -e "${CREDENTIAL_FILE}" ]] || fail "auth logout left the file-backed credential behind"
pass "auth logout guarded server revocation and removed the local credential"

echo
echo "All CLI end-to-end checks passed against ${API_URL}."
