# Changelog

## 0.3.0

### Minor Changes

- `verwalter pre-release <package>` opens an rc PR for one package on demand, against any branch. The rc bumps the package's own files and changelog, keeps the changesets pending, and is tagged as a pre-release after the merge. Pre-release tags no longer count as releases for changesets.
- `prerelease_label` configures the pre-release label (default `rc`). `pyproject.toml` gets PEP 440 spelling for pre-releases (`1.6.0rc1`, `1.6.0b3`) and reads both spellings. The example release workflow shows the npm dist-tag for pre-releases.

## 0.3.0-rc.1

### Minor Changes

- `verwalter pre-release <package>` opens an rc PR for one package on demand, against any branch. The rc bumps the package's own files and changelog, keeps the changesets pending, and is tagged as a pre-release after the merge. Pre-release tags no longer count as releases for changesets.

## 0.2.0

### Minor Changes

- `verwalter release --asset <file>` attaches files when the GitHub release is created, so a workflow can build first and needs no draft. `verwalter release --abandon <tag>` deletes a release and its tag.
