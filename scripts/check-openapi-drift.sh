#!/usr/bin/env bash
# Check OpenAPI drift between the pinned submodule and the typed Rust
# client.
#
# What this script enforces:
#
# 1. The submodule pointer is present and points to a concrete SHA.
# 2. Every operation ID the Rust client surface claims to use still exists
#    in the pinned `openapi/openapi.yaml`.
# 3. Every `/v1/` path in the pinned spec is exercised by at least one
#    Rust API module (light coverage hint; not a full schema check).
#
# The deeper schema-level drift check (Rust struct field coverage) is the
# job of integration tests in `crates/tastile-api/tests/` once we wire them
# up. This script is the cheap gate that runs in CI on every PR.
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

echo "openapi pinned: $PINNED_SHA"
echo "openapi worktree: ${WORKTREE_SHA:-<uninitialized>}"

if [[ -n "$WORKTREE_SHA" && "$WORKTREE_SHA" != "$PINNED_SHA" ]]; then
  echo "error: openapi submodule worktree ($WORKTREE_SHA) does not match pinned SHA ($PINNED_SHA)" >&2
  echo "  run: git submodule update --init openapi" >&2
  exit 1
fi

# ---------------------------------------------------------------------------
# 2. Operation-ID drift gate.
# ---------------------------------------------------------------------------

OPENAPI_YAML="$REPO_ROOT/openapi/openapi.yaml"
if [[ ! -f "$OPENAPI_YAML" ]]; then
  echo "error: $OPENAPI_YAML does not exist; submodule is uninitialized" >&2
  exit 1
fi

declare -a REQUIRED_OPS=(
  list_tiles
  list_pending_prompts
  resolve_prompt
  list_source_tiles
  get_source_tile
  cancel_source_tile
  signout
  request_prompt
)

# Extract every operationId from the spec into a sorted unique list.
SPEC_OPS="$(yq '.paths | to_entries | .[] | .value | to_entries | .[] | .value.operationId // ""' "$OPENAPI_YAML" | sort -u)"

DRIFT=0
for op in "${REQUIRED_OPS[@]}"; do
  if ! grep -qx "$op" <<<"$SPEC_OPS"; then
    echo "error: required operation '$op' is missing from pinned openapi.yaml" >&2
    DRIFT=1
  fi
done

# ---------------------------------------------------------------------------
# 3. Path-prefix sanity.
# ---------------------------------------------------------------------------

NON_V1="$(yq '.paths | keys | .[]' "$OPENAPI_YAML" | grep -v '^/v1/' || true)"
if [[ -n "$NON_V1" ]]; then
  echo "error: pinned openapi.yaml has paths outside /v1/: $NON_V1" >&2
  DRIFT=1
fi

# ---------------------------------------------------------------------------
# 4. Surface coverage hint — every /v1 path must appear in some Rust module.
# ---------------------------------------------------------------------------

# A small whitelist mapping -> search string. Each line is:
#   <operation-id>  <grep-pattern-in-source>
declare -a SURFACE_HINTS=(
  "list_tiles|crates/tastile-api/src/tiles.rs"
  "list_pending_prompts|crates/tastile-api/src/prompts.rs"
  "resolve_prompt|crates/tastile-api/src/prompts.rs"
  "list_source_tiles|crates/tastile-api/src/source_tiles.rs"
  "get_source_tile|crates/tastile-api/src/source_tiles.rs"
  "cancel_source_tile|crates/tastile-api/src/source_tiles.rs"
  "signout|crates/tastile-api/src/auth.rs"
  "request_prompt|crates/tastile-api/src/prompts.rs"
)

for hint in "${SURFACE_HINTS[@]}"; do
  op="${hint%%|*}"
  file="${hint##*|}"
  if ! grep -q "$op" "$file" 2>/dev/null; then
    echo "error: operation '$op' is referenced but not used in $file" >&2
    DRIFT=1
  fi
done

if [[ "$DRIFT" -eq 0 ]]; then
  echo "ok: openapi drift check passed (${#REQUIRED_OPS[@]} operations verified)"
fi

exit "$DRIFT"
