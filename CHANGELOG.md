# Changelog

## 0.4.0

### Minor Changes

- `--dry-run` for `pr`, `release`, `pre-release` and `check` (action input `dry-run: true`): pushes, tags and GitHub writes are printed instead of done.
- `verwalter release` handles packages in dependency order and stops at the first failure, so a dependent is never released before its dependency. The `releases` output lists dependencies first and carries `depends_on` per entry; the example workflow publishes one package at a time, fail-fast. `--package` always reports the named package, so a dispatch can hand one release to the publish job again.
- `verwalter status` shows per package where the release stands and what finishes it. `verwalter release` takes `--package <name>` and `--publish`, uploads assets to an existing draft or named release, and every failure names the command that resumes it. The README has a runbook for failed releases.

### Patch Changes

- Published to crates.io: `cargo install verwalter` and `cargo binstall verwalter` work. The release workflow publishes the crate after the GitHub release.

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
