#!/usr/bin/env bash
# Pin the openapi submodule to a new revision.
#
# Usage:
#   scripts/sync-openapi.sh           # bump to the upstream main HEAD
#   scripts/sync-openapi.sh <sha>     # pin to a specific commit SHA
#   scripts/sync-openapi.sh <tag>     # pin to a specific tag
#
# What this does:
# 1. `git submodule update --remote openapi` (or checks out the requested SHA).
# 2. `cargo build` to verify the build-script drift gate still passes.
# 3. `cargo test` to verify the typed client still compiles + tests pass.
# 4. `scripts/check-openapi-drift.sh` for the semantic drift check.
#
# Exit code 0 = ready to commit; non-zero = stop and resolve.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

if ! command -v git >/dev/null 2>&1; then
  echo "error: git is required" >&2
  exit 2
fi

if ! git config --file .gitmodules --get submodule.openapi.path >/dev/null 2>&1; then
  echo "error: openapi submodule is not registered" >&2
  exit 1
fi

# Capture pre-bump SHA for the log.
PRE_SHA="$(git ls-files -s openapi | awk '{print $2}')"
echo "openapi pinned before: $PRE_SHA"

if [[ $# -ge 1 ]]; then
  TARGET="$1"
  echo "checking out openapi to: $TARGET"
  git -C openapi checkout --quiet "$TARGET"
else
  echo "fetching openapi from origin and bumping to upstream HEAD"
  git submodule update --remote --merge openapi
fi

POST_SHA="$(git -C openapi rev-parse HEAD)"
echo "openapi pinned after:  $POST_SHA"

if [[ "$PRE_SHA" == "$POST_SHA" ]]; then
  echo "no change — nothing to do"
  exit 0
fi

echo
echo "Running drift check..."
"$(dirname "${BASH_SOURCE[0]}")/check-openapi-drift.sh"

echo
echo "Running cargo build (drift gate via build.rs)..."
cargo build --quiet

echo
echo "Running cargo test..."
cargo test --quiet --all-features

echo
echo "All checks passed. To commit:"
echo "  git add openapi"
echo "  git commit -m 'chore(openapi): bump pinned revision to ${POST_SHA:0:12}'"
