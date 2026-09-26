# Release process

`tastile-cli` releases follow the Tastile project-init policy.

## Branching model

```text
main (protected)
  ↑
  └── release-X-Y-Z
       └── <issue-number>
```

- `main` is the integration branch.
- `release-X-Y-Z` is the only branch allowed to land directly into `main`.
  CI enforces this via `.github/workflows/release-source-check.yml`.
- `<issue-number>` is a working branch off the release branch.

## Pinning policy for the `openapi/` submodule

For every CLI release, the `openapi/` submodule pointer must point at
a **concrete, immutable revision** — a tag (`tastile-openapi@vX.Y.Z`)
or a full SHA — and that revision must be recorded in the merge
commit's body (e.g. `openapi: b0c781dc18111645324e2abc38621b1564d4c518 (v1.0.0)`).

| Path | Pin source | Reproducibility |
| --- | --- | --- |
| Release commit | tag or SHA from `tastile-openapi` | exact |
| Day-to-day dev | `scripts/sync-openapi.sh` (HEAD) is **allowed** for working branches only | mutable until pinned |

`scripts/sync-openapi.sh` without arguments bumps to upstream HEAD —
this is fine for feature / fix branches but **must not** be the final
state on a release branch. The release PR is responsible for re-pinning
to a tag or SHA and verifying `scripts/check-openapi-drift.sh` is still
green before merge.

## Cutting a release

1. Confirm `main` is green.
2. Confirm the `openapi/` submodule is pinned to a tag or SHA (not
   upstream HEAD). If not, `scripts/sync-openapi.sh vX.Y.Z` (or
   `scripts/sync-openapi.sh <sha>`).
3. Cut `release-X-Y-Z` from `main`.
4. Update `CHANGELOG.md` — move `[Unreleased]` items to a versioned
   heading; record the OpenAPI pin (`openapi: <sha> (<tag>)`) in the
   release notes.
5. Open a PR from `release-X-Y-Z` → `main`. CI must be green; the
   `release-source-check` workflow confirms the head branch pattern.
6. Merge (linear history — squash or rebase; merge commit is not
   permitted).
7. Tag `main` HEAD as `vX.Y.Z`.

## Compatibility

`tastile-cli` follows the API revision pinned via the `openapi/` git
submodule. The revision is recorded in `tastile-cli`'s own git history.
Two CLI revisions can target the same API revision and remain
interoperable; bumping the pinned API revision is the only way the CLI's
wire contract can change.

## Emergency hotfix

1. Cut `release-X-Y-Z` from the most recent `main` tag.
2. Cherry-pick the fix commit.
3. Re-pin the `openapi/` submodule to the same tag/SHA as the
   previous release (do **not** pick up a newer revision in a hotfix).
4. Cut a `vX.Y.Z+1` tag on the merge commit.

## Versioning

- Major bump: breaking CLI surface change (e.g. removing a subcommand).
- Minor bump: new subcommand or new typed operation.
- Patch bump: bug fix, dependency bump, or docs-only change.
