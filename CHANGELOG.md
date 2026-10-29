# Changelog

## 0.8.1

### Patch Changes

- Point examples and README at v0.8.0 - [`0e3faaf`](https://github.com/lukasIO/verwalter/commit/0e3faafaf2cf3f8029b7f361e252337fdbb5b135) ([@lukasIO](https://github.com/lukasIO))

## 0.8.0

### Minor Changes

- A release branch leaves a changeset for the dependents it pins but does not release. Merging only the bottom of a stack no longer loses their bumps. - [`0ba7146`](https://github.com/lukasIO/verwalter/commit/0ba714660511a662bf050d86d7ed729744a1fe8e) ([@lukasIO](https://github.com/lukasIO))

### Patch Changes

- Point examples and README at v0.7.0 - [`cb8bdc0`](https://github.com/lukasIO/verwalter/commit/cb8bdc072b1e921ad61a29f0362f548c48aed197) ([@lukasIO](https://github.com/lukasIO))

## 0.7.0

### Minor Changes

- A release branch removes its package's line from every changeset that names it and deletes the file with its last line, instead of inferring released packages from tags. A repository that adopts verwalter with tags behind its versions no longer loses its pending changesets on the first release, and a package with `release = "none"` is no longer bumped twice. A release run that races another on the same tag treats the other run's GitHub release as done. - [`fa8bfc2`](https://github.com/lukasIO/verwalter/commit/fa8bfc275d89450a5ce607c85bb0346db682d962) ([@lukasIO](https://github.com/lukasIO))

### Patch Changes

- A changeset whose commit has no GitHub user behind its email now still links the commit; the empty author field was trimmed away and the whole attribution dropped. - [`698366c`](https://github.com/lukasIO/verwalter/commit/698366cd779a5ad0f5bfac541ba15552521336cf) ([@lukasIO](https://github.com/lukasIO))
- README and example workflows reference `lukasIO/verwalter@v0.6.0`. - [`a02f183`](https://github.com/lukasIO/verwalter/commit/a02f18376602ea74b1329f6458aa697c4c8dcbfd) ([@lukasIO](https://github.com/lukasIO))

## 0.6.0

### Minor Changes

- Changelog lines and release PR bodies end with the PR and author that added the changeset, in the livekit changesets-changelog-github form: `summary - [#12](…) ([@user](…))`. A changeset pushed without a PR links its commit instead. - [`1f6105f`](https://github.com/lukasIO/verwalter/commit/1f6105fe729df421d7de45f463221e12e011dbe4) ([@lukasIO](https://github.com/lukasIO))
- Release PRs that form one chain are linked as a GitHub stack (`gh stack link`); the action installs the gh-stack extension. `gh` is pinned to the repository behind the remote, so a clone with an `upstream` remote never touches the parent repository. - [`73179ea`](https://github.com/lukasIO/verwalter/commit/73179eacaea6923cd3920f21e6e35c0de658145b) ([@lukasIO](https://github.com/lukasIO))
- Versioned files can be any text file: `{ path, regex }` entries name a regex with a `version` capture group. This covers gradle.properties, podspecs, `.version` files and version constants in Go, Swift or Dart source. - [`be49e35`](https://github.com/lukasIO/verwalter/commit/be49e35359bb1c91fe33b26c771cee26b7eb85d4) ([@lukasIO](https://github.com/lukasIO))
- `verwalter check` fails when a publishable Cargo, pnpm or uv workspace member is not in `verwalter.toml`, or when a configured package's directory has no manifest; other commands warn. New `ignore` config key for members to skip. A private package may still be configured. - [`0bcc832`](https://github.com/lukasIO/verwalter/commit/0bcc83247806758033497814e86c00f131481004) ([@lukasIO](https://github.com/lukasIO))

### Patch Changes

- A Cargo dependency declared with `workspace = true` is no longer given a `version` when its package is released; the pin stays in the root `[workspace.dependencies]`. - [`8b457da`](https://github.com/lukasIO/verwalter/commit/8b457da0d30ead71dd3328c06dbd9985197a6473) ([@lukasIO](https://github.com/lukasIO))
- README and example workflows reference `lukasIO/verwalter@v0.5.0`. - [`b8363d9`](https://github.com/lukasIO/verwalter/commit/b8363d978379f7a457b1d1c75a02400dacdb58c8) ([@lukasIO](https://github.com/lukasIO))
- verwalter is no longer published to crates.io. Install from the GitHub release binaries, the action, or `cargo install --git`. - [`fd98df4`](https://github.com/lukasIO/verwalter/commit/fd98df45baba0851a8b395e24c12bb2944732d0f) ([@lukasIO](https://github.com/lukasIO))
- A diamond in the dependency graph chains only its own connected component; unrelated packages keep their stacks. Empty changesets are removed by every release branch. The workspace check compares package names normalised (`play_py_ext` matches `play-py-ext`) and accepts a configured package when any of its versioned files exists, so regex-only packages pass. - [`64a2ee7`](https://github.com/lukasIO/verwalter/commit/64a2ee7d2b4bcb5eb2e7df4d915522ba6967d0d8) ([@lukasIO](https://github.com/lukasIO))
- A push rejected because a branch named like the release prefix exists now says so and names the fix. - [`b9f2d8e`](https://github.com/lukasIO/verwalter/commit/b9f2d8eb27fa8d0d47779ad2c551f991050d99f6) ([@lukasIO](https://github.com/lukasIO))
- `pr` no longer re-sends an unchanged base when updating a release PR, which GitHub refuses for PRs in a stack; a changed base on a stacked PR is corrected through the stack instead. - [`cb29a4d`](https://github.com/lukasIO/verwalter/commit/cb29a4decebdc3560eb42dd461594bcdbc886a25) ([@lukasIO](https://github.com/lukasIO))

## 0.5.0

### Minor Changes

- `pyproject.toml` dependencies are discovered (`[project] dependencies` and optional groups, PEP 503 name matching) and a dependency's lower bound (`>=`, `==`, `~=`) is moved to the released version in PEP 440 spelling. `package.json` devDependencies no longer make a package a dependent, matching changesets.

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
