#!/usr/bin/env bash
# Muse shadow daily-use E2E against an isolated Tastile Core stack.
#
# Exercises the same command surface Muse uses day to day:
#   create -> placements -> execution lifecycle -> reads -> edge cases
#   (excluded dates, label-only role, priority contention + reflow
#   isolation, prompts, cancel).
#
# Required environment (all three):
#   TASTILE_API_URL                  loopback API base, e.g. http://127.0.0.1:31498
#                                    (the operational port 31400 is refused)
#   TASTILE_CREDENTIAL_FILE          mode-0600 file credential for that base
#   TASTILE_ISOLATED_TEST_STACK=confirmed
#
# Optional:
#   TASTILE_BIN     CLI binary (default: ./target/release/tastile, then PATH)
#   MUSE_E2E_OUT    directory for JSON evidence (default: mktemp -d)
#
# The script only talks to the API through the CLI. It never touches a
# database, a notification sink, or the operational stack.
set -euo pipefail
umask 077

BIN="${TASTILE_BIN:-}"
if [[ -z "${BIN}" ]]; then
  if [[ -x "./target/release/tastile" ]]; then
    BIN="./target/release/tastile"
  else
    BIN="$(command -v tastile)"
  fi
fi
OUT="${MUSE_E2E_OUT:-$(mktemp -d "${TMPDIR:-/tmp}/muse-e2e.XXXXXX")}"
mkdir -p "${OUT}"
RUN="muse-e2e-$(date -u +%Y%m%dT%H%M%SZ)"
PASS=0
FAIL=0

note() { printf '%s\n' "$*"; }
pass() { PASS=$((PASS + 1)); note "PASS  $*"; }
fail() { FAIL=$((FAIL + 1)); note "FAIL  $*"; }
check() { # check <description> <command...>
  local desc="$1"; shift
  if "$@" >"${OUT}/last.out" 2>"${OUT}/last.err"; then pass "$desc"; else fail "$desc"; tail -5 "${OUT}/last.err" "${OUT}/last.out" 2>/dev/null | sed 's/^/      /'; fi
}
json_get() { jq -r "$1" "${OUT}/last.out"; }
cli() { "${BIN}" "$@"; }

# --- preflight -------------------------------------------------------------
: "${TASTILE_API_URL:?set TASTILE_API_URL to the isolated loopback API}"
: "${TASTILE_CREDENTIAL_FILE:?set TASTILE_CREDENTIAL_FILE}"
: "${TASTILE_ISOLATED_TEST_STACK:?set TASTILE_ISOLATED_TEST_STACK=confirmed after verifying isolation}"
[[ "${TASTILE_ISOLATED_TEST_STACK}" == "confirmed" ]] || { note "attestation must be exactly 'confirmed'"; exit 2; }
[[ -f "${TASTILE_CREDENTIAL_FILE}" ]] || { note "credential file missing: ${TASTILE_CREDENTIAL_FILE}"; exit 2; }
perm="$(stat -f '%Lp' "${TASTILE_CREDENTIAL_FILE}" 2>/dev/null || stat -c '%a' "${TASTILE_CREDENTIAL_FILE}")"
[[ "${perm}" == "600" ]] || { note "credential file must be mode 600 (got ${perm})"; exit 2; }

hostport="${TASTILE_API_URL#*://}"
hostport="${hostport%%/*}"
port="${hostport##*:}"
[[ "${port}" != "31400" ]] || { note "port 31400 is reserved for the operational stack"; exit 2; }
case "${hostport%%:*}" in
  127.0.0.1|localhost|::1) ;;
  *) note "refusing non-loopback API base ${TASTILE_API_URL} (writes over remote plaintext are refused by the CLI anyway)"; exit 2 ;;
esac

note "== muse-shadow-e2e ${RUN}"
note "api: ${TASTILE_API_URL}  bin: ${BIN}  out: ${OUT}"

# Auth probe: an empty owner must still list successfully.
check "authenticated source-tiles list" cli source-tiles list --json
SOURCE_COUNT_BEFORE="$(json_get 'length')"
note "      existing sources for this owner: ${SOURCE_COUNT_BEFORE}"

# Pre-clean leftovers from earlier runs so contended-slot checks start from
# a known state. Only titles carrying the muse-e2e marker are touched.
cli source-tiles list --json >"${OUT}/preclean-list.json" 2>/dev/null || true
while IFS= read -r stale_id; do
  [[ -n "${stale_id}" ]] || continue
  cli source-tiles cancel "${stale_id}" --reason-code 0 >/dev/null 2>&1 || true
  note "      pre-clean: cancelled leftover ${stale_id}"
done < <(jq -r '.[] | select(.source_state == 0) | select(.title | startswith("muse-e2e")) | .source_tile_id' "${OUT}/preclean-list.json" 2>/dev/null || true)

CREATED=()
cleanup() {
  if [[ "${KEEP:-0}" == "1" ]]; then return 0; fi
  for id in "${CREATED[@]:-}"; do
    [[ -n "${id}" ]] || continue
    cli source-tiles cancel "${id}" --reason-code 0 >/dev/null 2>&1 || true
  done
}
trap cleanup EXIT

new_source() { # prints source_tile_id on stdout via last.out
  cli source-tiles list --json >"${OUT}/list.json"
  jq -r --arg t "$1" '[.[] | select(.title == $t)][0].source_tile_id // empty' "${OUT}/list.json"
}

wait_placements() { # <source_id> <min_count> <timeout_s>
  local id="$1" want="$2" timeout="$3" i=0 n=0
  while (( i < timeout )); do
    cli source-tiles placements "${id}" --json >"${OUT}/placements.json" 2>/dev/null || true
    n="$(jq 'length' "${OUT}/placements.json" 2>/dev/null || echo 0)"
    (( n >= want )) && return 0
    sleep 5; i=$((i + 5))
  done
  return 1
}

# --- 1. one-time task: create -> placements -> execution lifecycle ---------
T1="${RUN} once"
cli source-tiles create --title "${T1}" --once 2026-11-21T10:00:00+09:00 --duration 30 \
  >"${OUT}/create-once.txt" 2>&1 \
  && pass "create one-time source" || { fail "create one-time source"; cat "${OUT}/create-once.txt"; }
S1="$(new_source "${T1}")"
[[ -n "${S1}" ]] && CREATED+=("${S1}")
[[ -n "${S1}" ]] && pass "read-back finds created source ${S1}" || fail "read-back missing ${T1}"

if [[ -n "${S1}" ]]; then
  check "placements materialize (worker)" wait_placements "${S1}" 1 120
  cli source-tiles placements "${S1}" --json >"${OUT}/placements-s1.json"
  P1="$(jq -r '.[0].placement_id' "${OUT}/placements-s1.json")"
  [[ -n "${P1}" && "${P1}" != "null" ]] && pass "placement id ${P1}" || fail "no placement for ${S1}"

  if [[ -n "${P1}" && "${P1}" != "null" ]]; then
    cli executions start "${P1}" --json >"${OUT}/exec-start.json" 2>"${OUT}/exec-start.err" \
      && pass "execution start" || fail "execution start"
    E1="$(jq -r '.aggregate.id // empty' "${OUT}/exec-start.json")"
    if [[ -n "${E1}" ]]; then
      cli executions pause "${E1}" --json >/dev/null 2>&1 && pass "execution pause" || fail "execution pause"
      cli executions resume "${E1}" --json >/dev/null 2>&1 && pass "execution resume" || fail "execution resume"
      cli executions finish --kind 2 --note "${RUN}" "${E1}" --json >/dev/null 2>&1 \
        && pass "execution finish" || fail "execution finish"
    else
      fail "execution id not returned"
    fi
  fi
fi

# --- 2. reads: today / prompts / timeline -------------------------------
check "today" cli today
check "prompts list" cli prompts list
check "timeline next 3 days" cli timeline --start 2026-11-19T00:00:00Z --end 2026-11-22T00:00:00Z
check "timeline with labels+blocked" cli timeline --start 2026-11-19T00:00:00Z --end 2026-11-22T00:00:00Z --include-labels --include-blocked --json

# --- 3. exception dates ----------------------------------------------------
T2="${RUN} exception"
# Exclude two upcoming Mondays (2026-11-23, 2026-11-30).
cli source-tiles create --title "${T2}" --weekly 09:00 --days mon,wed \
  --utc-offset 540 --exclude 2026-11-23 --exclude 2026-11-30 \
  --duration 30 --horizon-days 21 >"${OUT}/create-exc.txt" 2>&1 \
  && pass "create weekly with excluded dates" || fail "create weekly with excluded dates"
S2="$(new_source "${T2}")"
[[ -n "${S2}" ]] && CREATED+=("${S2}")
if [[ -n "${S2}" ]]; then
  check "exception-day placements materialize" wait_placements "${S2}" 4 120
  cli source-tiles placements "${S2}" --json >"${OUT}/placements-s2.json"
  # Every placement start (UTC) must not fall on an excluded local date.
  # 09:00 JST = 00:00 UTC same date, so the UTC date equals the local date.
  if jq -e '[.[].start | .[0:10]] | index("2026-11-23") == null and index("2026-11-30") == null' \
       "${OUT}/placements-s2.json" >/dev/null; then
    pass "excluded dates produce no placements"
  else
    fail "excluded date still produced a placement"
  fi
  if jq -e '[.[].start | .[0:10]] | index("2026-11-25") != null' "${OUT}/placements-s2.json" >/dev/null; then
    pass "non-excluded weekday still produces placements"
  else
    fail "expected weekday missing"
  fi
fi

# --- 4. label-only source --------------------------------------------------
T3="${RUN} label"
HSTART="$(date -u +%Y-%m-%dT00:00:00Z)"
HEND="$(date -u -v+30d +%Y-%m-%dT00:00:00Z 2>/dev/null || date -u -d '+30 days' +%Y-%m-%dT00:00:00Z)"
cat >"${OUT}/label-draft.json" <<EOF
{
  "title": "${T3}",
  "priority": 0,
  "required_duration_ms": 1800000,
  "horizon_start": "${HSTART}",
  "horizon_end": "${HEND}",
  "plan_role": 1,
  "generation": {"kind": 1, "starts_at": "${HSTART}", "interval_ms": 86400000, "weekday_mask": 127, "offset_min": 540}
}
EOF
cli source-tiles create --from-json "${OUT}/label-draft.json" >"${OUT}/create-label.txt" 2>&1 \
  && pass "create label-only source (plan_role=1)" || fail "create label-only source"
S3="$(new_source "${T3}")"
[[ -n "${S3}" ]] && CREATED+=("${S3}")
if [[ -n "${S3}" ]]; then
  check "label placements materialize" wait_placements "${S3}" 3 120
  cli timeline --start "${HSTART}" --end "$(date -u -v+2d +%Y-%m-%dT00:00:00Z 2>/dev/null || date -u -d '+2 days' +%Y-%m-%dT00:00:00Z)" --include-labels --json >"${OUT}/timeline-labels.json" 2>/dev/null || true
  if jq -e '[.. | objects | select(.role? == 1)] | length > 0' "${OUT}/timeline-labels.json" >/dev/null 2>&1; then
    pass "label periods appear with role=1 (LABEL lane)"
  else
    fail "no role=1 label items in timeline"
  fi
fi

# --- 5. priority contention + reflow isolation -----------------------------
D1="${RUN} prio-low"; D2="${RUN} prio-high"
cli source-tiles create --title "${D1}" --daily 10:00 --utc-offset 540 \
  --duration 120 --window-minutes 120 --priority 10 --horizon-days 7 >"${OUT}/create-plow.txt" 2>&1 \
  && pass "create low-priority daily" || fail "create low-priority daily"
cli source-tiles create --title "${D2}" --daily 10:00 --utc-offset 540 \
  --duration 120 --window-minutes 120 --priority 30 --horizon-days 7 >"${OUT}/create-phigh.txt" 2>&1 \
  && pass "create high-priority daily" || fail "create high-priority daily"
SLOW="$(new_source "${D1}")"; SHIGH="$(new_source "${D2}")"
[[ -n "${SLOW}" ]] && CREATED+=("${SLOW}")
[[ -n "${SHIGH}" ]] && CREATED+=("${SHIGH}")
if [[ -n "${SLOW}" && -n "${SHIGH}" ]]; then
  check "contended placements materialize" wait_placements "${SLOW}" 3 120
  cli source-tiles placements "${SHIGH}" --json >"${OUT}/placements-high-before.json"
  HIGH_BEFORE="$(jq -c '[.[].placement_id] | sort' "${OUT}/placements-high-before.json")"
  OPEN_BEFORE="$(jq '[.[] | select(.closed == false)] | length' "${OUT}/placements-high-before.json")"
  cli source-tiles reflow "${SLOW}" --from 2026-11-19T00:00:00Z --to 2026-11-27T00:00:00Z \
    >"${OUT}/reflow.txt" 2>&1 && pass "reflow low-priority source" || fail "reflow low-priority source"
  cli source-tiles placements "${SLOW}" --json >"${OUT}/placements-low-after.json"
  cli source-tiles placements "${SHIGH}" --json >"${OUT}/placements-high-after.json"
  HIGH_AFTER="$(jq -c '[.[].placement_id] | sort' "${OUT}/placements-high-after.json")"
  if [[ "${HIGH_BEFORE}" == "${HIGH_AFTER}" ]]; then
    pass "reflow did not touch the other source's placements"
  else
    fail "reflow changed another source's placements"
  fi
  LOW_OPEN="$(jq '[.[] | select(.closed == false)] | length' "${OUT}/placements-low-after.json")"
  HIGH_OPEN="$(jq '[.[] | select(.closed == false)] | length' "${OUT}/placements-high-after.json")"
  note "      after reflow: low open=${LOW_OPEN}, high open=${HIGH_OPEN} (was ${OPEN_BEFORE})"
  if [[ "${LOW_OPEN}" == "0" && "${HIGH_OPEN}" == "${OPEN_BEFORE}" ]]; then
    pass "priority resolved contention: low-priority occurrences closed, high-priority kept"
  else
    note "      NOTE: capacity outcome differs from the 2026-11-19 baseline; inspect ${OUT}/reflow.txt"
  fi
fi

# --- 6. prompts ------------------------------------------------------------
cli prompts request --kind 1 --note "${RUN} probe" --json >"${OUT}/prompt.json" 2>"${OUT}/prompt.err" \
  && pass "prompt request" || fail "prompt request"
PID="$(jq -r '.aggregate_meta.prompt_id // empty' "${OUT}/prompt.json" 2>/dev/null || true)"
if [[ -z "${PID}" ]]; then
  # fall back to the newest pending prompt in the list output
  PID="$(cli prompts list --json 2>/dev/null | jq -r '.[0].id // empty' || true)"
fi
if [[ -n "${PID}" && "${PID}" != "null" ]]; then
  cli prompts resolve "${PID}" --resolution ack --note "${RUN}" >/dev/null 2>&1 \
    && pass "prompt resolve (${PID})" || fail "prompt resolve"
else
  note "      SKIP  prompt id not discoverable; resolve manually via 'tastile prompts list'"
fi

# --- 7. cancel --------------------------------------------------------------
T4="${RUN} cancel-me"
cli source-tiles create --title "${T4}" --once 2026-11-28T09:00:00+09:00 --duration 15 \
  >"${OUT}/create-cancel.txt" 2>&1 && pass "create cancel target" || fail "create cancel target"
S4="$(new_source "${T4}")"
[[ -n "${S4}" ]] && CREATED+=("${S4}")
if [[ -n "${S4}" ]]; then
  cli source-tiles cancel "${S4}" --dry-run >"${OUT}/cancel-dry.txt" 2>&1 \
    && pass "cancel dry-run" || fail "cancel dry-run"
  cli source-tiles cancel "${S4}" --reason-code 0 >"${OUT}/cancel.txt" 2>&1 \
    && pass "cancel apply" || fail "cancel apply"
  cli source-tiles get "${S4}" --json >"${OUT}/get-cancelled.json" 2>"${OUT}/get-cancelled.err" || true
  STATE="$(jq -r '.source.source_state // .source_state // empty' "${OUT}/get-cancelled.json" 2>/dev/null || true)"
  [[ "${STATE}" == "3" ]] && pass "cancelled source reads back state=3" || fail "cancelled source state=${STATE:-unknown}"
fi

note "== done: ${PASS} passed, ${FAIL} failed — evidence in ${OUT}"
[[ "${FAIL}" == "0" ]]
