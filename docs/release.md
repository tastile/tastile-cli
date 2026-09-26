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

## Cutting a release

1. Confirm `main` is green.
2. Cut `release-X-Y-Z` from `main`.
3. Update `CHANGELOG.md` — move `[Unreleased]` items to a versioned
   heading.
4. Open a PR from `release-X-Y-Z` → `main`. CI must be green.
5. Merge with a merge commit.
6. Tag `main` HEAD as `vX.Y.Z`.

## Compatibility

`tastile-cli` follows the API revision pinned via the `openapi/` git
submodule. The revision is recorded in `tastile-cli`'s own git history.
Two CLI revisions can target the same API revision and remain
interoperable; bumping the pinned API revision is the only way the CLI's
wire contract can change.

## Emergency hotfix

1. Cut `release-X-Y-Z` from the most recent `main` tag.
2. Cherry-pick the fix commit.
3. Cut a `vX.Y.Z+1` tag on the merge commit.

## Versioning

- Major bump: breaking CLI surface change (e.g. removing a subcommand).
- Minor bump: new subcommand or new typed operation.
- Patch bump: bug fix, dependency bump, or docs-only change.
