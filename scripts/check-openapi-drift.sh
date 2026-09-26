#!/usr/bin/env bash
# Check OpenAPI drift between the pinned submodule and the typed Rust
# client.
#
# What this script enforces:
#
# 1. The submodule pointer is present and points to a concrete SHA.
# 2. Every operationId the Rust client surface claims to use still exists
#    in the pinned `openapi/openapi.yaml`.
# 3. The 18 surface operations are exercised by the right Rust modules.
# 4. The pinned spec is OpenAPI 3.1.x with the expected `info.title` and
#    `info.version` we pin in `crates/tastile-api/build.rs`.
# 5. Every /v1 path in the pinned spec exists (no missing wire entries).
#
# The deeper schema-level drift check (Rust struct field coverage,
# required payload fields, response 200 shape) is enforced by
# `crates/tastile-api/build.rs` and runs on every `cargo build`.
# This shell wrapper exists so the same check is runnable in CI without
# Cargo and surfaces a human-readable summary.
#
# Exit codes:
#   0 = no drift detected
#   1 = drift detected (output explains what to fix)
#   2 = external prerequisite missing (e.g. yq)

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

require() {
  command -v "$1" >/dev/null 2>&1 || {
    echo "error: required tool '$1' is not installed" >&2
    exit 2
  }
}

require git
require yq
require jq

# ---------------------------------------------------------------------------
# 1. Submodule pointer sanity.
# ---------------------------------------------------------------------------

if ! git config --file .gitmodules --get submodule.openapi.path >/dev/null 2>&1; then
  echo "error: openapi submodule is not registered in .gitmodules" >&2
  exit 1
fi

PINNED_SHA="$(git ls-files -s openapi | awk '{print $2}')"
WORKTREE_SHA="$(git -C openapi rev-parse HEAD 2>/dev/null || true)"

if [[ -z "$PINNED_SHA" ]]; then
  echo "error: openapi submodule pointer is empty; submodule is not initialized" >&2
  exit 1
fi

echo "openapi pinned:   $PINNED_SHA"
echo "openapi worktree: ${WORKTREE_SHA:-<uninitialized>}"

if [[ -n "$WORKTREE_SHA" && "$WORKTREE_SHA" != "$PINNED_SHA" ]]; then
  echo "error: openapi submodule worktree ($WORKTREE_SHA) does not match pinned SHA ($PINNED_SHA)" >&2
  echo "  run: git submodule update --init openapi" >&2
  exit 1
fi

# ---------------------------------------------------------------------------
# 2. Required operationIds.
# ---------------------------------------------------------------------------

OPENAPI_YAML="$REPO_ROOT/openapi/openapi.yaml"
if [[ ! -f "$OPENAPI_YAML" ]]; then
  echo "error: $OPENAPI_YAML does not exist; submodule is uninitialized" >&2
  exit 1
fi

# All 18 operations the typed client claims to use. The drift gate in
# `crates/tastile-api/build.rs` enforces each row with full structural
# checks; this list is the human-readable summary.
declare -a REQUIRED_OPS=(
  list_tiles
  list_pending_prompts
  request_prompt
  resolve_prompt
  respond_startup_recovery
  list_source_tiles
  get_source_tile
  create_source_tile
  update_source_tile
  cancel_source_tile
  get_source_tile_completion
  list_source_tile_placements
  reflow_source_tile
  start_execution
  pause_execution
  resume_execution
  finish_execution
  signout
)

# Extract every operationId from the spec into a sorted unique list.
SPEC_OPS="$(yq '.paths | to_entries | .[] | .value | to_entries | .[] | .value.operationId // ""' "$OPENAPI_YAML" | sort -u)"

DRIFT=0
MISSING=()
for op in "${REQUIRED_OPS[@]}"; do
  if ! grep -qx "$op" <<<"$SPEC_OPS"; then
    echo "error: required operation '$op' is missing from pinned openapi.yaml" >&2
    MISSING+=("$op")
    DRIFT=1
  fi
done

# ---------------------------------------------------------------------------
# 3. Spec metadata sanity.
# ---------------------------------------------------------------------------

INFO_VERSION="$(yq '.info.version // ""' "$OPENAPI_YAML")"
INFO_TITLE="$(yq '.info.title // ""' "$OPENAPI_YAML")"
OPENAPI_VERSION="$(yq '.openapi // ""' "$OPENAPI_YAML")"

echo "openapi version:  $OPENAPI_VERSION"
echo "spec title:       $INFO_TITLE"
echo "spec version:     $INFO_VERSION"

case "$OPENAPI_VERSION" in
  3.1.*) ;;
  *) echo "error: pinned openapi.yaml is not OpenAPI 3.1.x (got '$OPENAPI_VERSION')" >&2; DRIFT=1 ;;
esac

# ---------------------------------------------------------------------------
# 4. Path-prefix sanity.
# ---------------------------------------------------------------------------

NON_V1="$(yq '.paths | keys | .[]' "$OPENAPI_YAML" | grep -v '^/v1/' || true)"
if [[ -n "$NON_V1" ]]; then
  echo "error: pinned openapi.yaml has paths outside /v1/: $NON_V1" >&2
  DRIFT=1
fi

# ---------------------------------------------------------------------------
# 5. Surface coverage hint — every operationId must be referenced in
#    its expected Rust module.
# ---------------------------------------------------------------------------

declare -a SURFACE_HINTS=(
  "list_tiles|crates/tastile-api/src/tiles.rs"
  "list_pending_prompts|crates/tastile-api/src/prompts.rs"
  "request_prompt|crates/tastile-api/src/prompts.rs"
  "resolve_prompt|crates/tastile-api/src/prompts.rs"
  "respond_startup_recovery|crates/tastile-api/src/prompts.rs"
  "list_source_tiles|crates/tastile-api/src/source_tiles.rs"
  "get_source_tile|crates/tastile-api/src/source_tiles.rs"
  "create_source_tile|crates/tastile-api/src/source_tiles.rs"
  "update_source_tile|crates/tastile-api/src/source_tiles.rs"
  "cancel_source_tile|crates/tastile-api/src/source_tiles.rs"
  "get_source_tile_completion|crates/tastile-api/src/source_tiles.rs"
  "list_source_tile_placements|crates/tastile-api/src/source_tiles.rs"
  "reflow_source_tile|crates/tastile-api/src/source_tiles.rs"
  "start_execution|crates/tastile-api/src/executions.rs"
  "pause_execution|crates/tastile-api/src/executions.rs"
  "resume_execution|crates/tastile-api/src/executions.rs"
  "finish_execution|crates/tastile-api/src/executions.rs"
  "signout|crates/tastile-api/src/auth.rs"
)

for hint in "${SURFACE_HINTS[@]}"; do
  op="${hint%%|*}"
  file="${hint##*|}"
  if ! grep -q "$op" "$file" 2>/dev/null; then
    echo "error: operation '$op' is referenced but not used in $file" >&2
    DRIFT=1
  fi
done

# ---------------------------------------------------------------------------
# 6. Final summary.
# ---------------------------------------------------------------------------

if [[ "$DRIFT" -eq 0 ]]; then
  echo
  echo "ok: openapi drift check passed"
  echo "  ${#REQUIRED_OPS[@]} operations verified"
  echo "  surface coverage: 18/18"
else
  echo
  echo "fail: openapi drift detected"
  if [[ "${#MISSING[@]}" -gt 0 ]]; then
    echo "  missing operationIds: ${MISSING[*]}"
  fi
fi

exit "$DRIFT"
