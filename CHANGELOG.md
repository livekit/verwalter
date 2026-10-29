# Changelog

## 0.14.1

### Patch Changes

- `VERWALTER_ROOT` is absolute. A relative checkout path made `$VERWALTER_ROOT/dist` resolve inside the package. - [`1a3e582`](https://github.com/lukasIO/verwalter/commit/1a3e582f17b3ef2bda17bf621961417cd3027aff) ([@lukasIO](https://github.com/lukasIO))
- README and example workflows pin v0.14.0. - [`56be448`](https://github.com/lukasIO/verwalter/commit/56be448fe8f1caf79a3a819c46225d2d838cffc5) ([@lukasIO](https://github.com/lukasIO))

## 0.14.0

### Minor Changes

- `check` runs an API check per covered package: `[check].api` or the package's `api` command prints the kind of change the diff needs, verwalter translates it to the digit for the package's version, and fails the PR when the changeset claims less. The comment names the level to use. - [`cea5758`](https://github.com/lukasIO/verwalter/commit/cea57582571fab94239f64083113e4203f657b81) ([@lukasIO](https://github.com/lukasIO))
- `verwalter publish [--jobs <n>]` runs each package's publish command once per tagged package, dependencies first, independent packages in parallel. A failure blocks the transitive dependents and nothing else, and the run exits non-zero. The command comes from `verwalter.toml`: `[publish]` holds the default `registry`, `publish` command and `jobs`, and a package overrides `registry` and `publish`. A package on another registry than the default takes the built-in command for it. The tool asks the registry whether the version exists before the command runs, skips it when it does, and fails on any answer other than 200 or 404. `status` shows the answer, and `first_release` means the registry has no version at all. The `depends_on` field leaves the release handoff. `[publish].workspace` hands every package on the default registry to one recursive command such as `pnpm -r publish`, run once from the root, with per-package reporting from the registry afterwards. - [`cf11da6`](https://github.com/lukasIO/verwalter/commit/cf11da60207ffa7e573b07f497e03909b608fb90) ([@lukasIO](https://github.com/lukasIO))

### Patch Changes

- The README and the example workflows point at `v0.13.0`. - [`83170da`](https://github.com/lukasIO/verwalter/commit/83170da6a8dfeb447347cd2d47e3fc350d4e7de3) ([@lukasIO](https://github.com/lukasIO))
- The README is restructured: scope table, the release cycle, setup with the action inputs and outputs, a feature table, the mistakes the tool catches, the recovery table, what is not covered, and the commands. `examples/publish` shows publish steps that ask the registry before publishing, so a re-run skips an existing version and still fails on every other error. - [`68d305a`](https://github.com/lukasIO/verwalter/commit/68d305ab218c5dc3e3e437a572442b76c6f10f7e) ([@lukasIO](https://github.com/lukasIO))

## 0.13.0

### Minor Changes

- `verwalter changeset [--level <l>] [--summary <text>]` writes a changeset for the packages that the branch and working tree change against the base branch, minus those a pending changeset covers. `--package <name>`, repeatable, names the packages instead. The summary defaults to the last commit subject. The check comment names the command next to the prefilled link. - [`8b2c3a7`](https://github.com/lukasIO/verwalter/commit/8b2c3a7768af26655a66193923cc466cafed9389) ([@lukasIO](https://github.com/lukasIO))

### Patch Changes

- The README and the example workflows point at `v0.12.0`. A `workspace:*` range in package.json is left alone when a dependency bumps, with a test, and the README says why pnpm and yarn lock files need no edit. - [`90d49f8`](https://github.com/lukasIO/verwalter/commit/90d49f8dab16dcc110d15e473d8e3cafec23a9d0) ([@lukasIO](https://github.com/lukasIO))
- A `{ path, regex }` entry is handled by the tool's own regex, whatever the file name, and every match moves. A `Cargo.lock` entry can now be addressed by regex, for a workspace whose crates inherit one version. A crate with `publish.workspace = true` takes its publish flag from `[workspace.package]`, so the workspace check no longer reports it as publishable. - [`4fcbccf`](https://github.com/lukasIO/verwalter/commit/4fcbccf96896b5a803f19af88443a543e94c8801) ([@lukasIO](https://github.com/lukasIO))

## 0.12.0

### Minor Changes

- Four gaps found in the release history of the LiveKit SDKs are closed. `uv.lock` is a lock file: the package's entry and every `requires-dist` floor on it move with the version. A first release, one with no earlier tag, is named in the release PR and as `first_release` in the handoff, because trusted publishing cannot create a new package. Files of one package that carry different versions are reported as a warning. A version with no tag and no changelog entry is left alone by `release`, with a warning, because it was edited by hand; `--package` releases it anyway. The example release workflow only runs for a PR the tool opened. The research is in `docs/release-pitfalls.md`. - [`0e86710`](https://github.com/lukasIO/verwalter/commit/0e86710991a71f24c32c2fcc57a6e428273e1df3) ([@lukasIO](https://github.com/lukasIO))

### Patch Changes

- The README and the example workflows point at `v0.11.0`. - [`ff30cdb`](https://github.com/lukasIO/verwalter/commit/ff30cdb6720a65bac34d22cf96665f4021d0f3a3) ([@lukasIO](https://github.com/lukasIO))

## 0.11.0

### Minor Changes

- A package left at a pre-release version is in the next release PR even with no pending changeset. The changelog entry says that it promotes the rc with no further changes. - [`5f9a869`](https://github.com/lukasIO/verwalter/commit/5f9a86967195f53ad039e2f3bec5ce4e9503171b) ([@lukasIO](https://github.com/lukasIO))

### Patch Changes

- `check` reads only the config, so a changeset that the tool cannot read or that names an unknown package no longer fails the run before the check starts. The error goes into the PR comment and the check fails. - [`31a3b68`](https://github.com/lukasIO/verwalter/commit/31a3b686923a20436109bda4dba04114cd3669cc) ([@lukasIO](https://github.com/lukasIO))
- The README and the example workflows point at `v0.10.0`. - [`90f8b4c`](https://github.com/lukasIO/verwalter/commit/90f8b4cc9272d7590f97ebefba3253f45422ae4a) ([@lukasIO](https://github.com/lukasIO))
- A pre-release of a fixed group uses one branch, `release/<first member>-rc`, whichever member was named. Two members no longer open two rc PRs. - [`5f9a869`](https://github.com/lukasIO/verwalter/commit/5f9a86967195f53ad039e2f3bec5ce4e9503171b) ([@lukasIO](https://github.com/lukasIO))
- The example release workflow checks out the merge commit of the release PR, not the head of `main`. A push that lands right after the merge no longer moves the tags. - [`f730f24`](https://github.com/lukasIO/verwalter/commit/f730f24505b882e2ed35bab5429c274e3973bac4) ([@lukasIO](https://github.com/lukasIO))

## 0.10.0

### Minor Changes

- The release PR sync only looks at the PR on the release branch. The cleanup of per-package PRs from versions before 0.9 is gone. - [`ad2da31`](https://github.com/lukasIO/verwalter/commit/ad2da316eb8df294089e51dda9aa2ba0eda3cd4f) ([@lukasIO](https://github.com/lukasIO))
- Fixed groups (`fixed = [["a", "b"]]`) release at one shared version, and a pre-release of a member takes the whole group. `max_bump` on a package caps the bump level. - [`4d8d0b6`](https://github.com/lukasIO/verwalter/commit/4d8d0b63b3c99b79746cb64aade4ce478579cfda) ([@lukasIO](https://github.com/lukasIO))
- A changeset that names a package missing from `verwalter.toml` is an error, reported by every command and by `verwalter check`. The release branch deletes changesets outright instead of editing their front matter. - [`01809d9`](https://github.com/lukasIO/verwalter/commit/01809d96e2ca9b81e58a2f8473d56948e8f1ff4e) ([@lukasIO](https://github.com/lukasIO))
- The final release after an rc follows the pending changesets. A major changeset added after `1.6.0-rc.2` releases `2.0.0`, not `1.6.0`. A release is never lower than its rc. - [`a9f5269`](https://github.com/lukasIO/verwalter/commit/a9f526903b61fd8ebe76f7efcdd991423e3bb646) ([@lukasIO](https://github.com/lukasIO))

### Patch Changes

- Point examples and README at v0.9.0 - [`d8da541`](https://github.com/lukasIO/verwalter/commit/d8da5414a9d82ea47843cc16d7316b0edb750902) ([@lukasIO](https://github.com/lukasIO))

## 0.9.0

### Minor Changes

- One release PR for every package with a pending bump, instead of one stacked PR per package. The branch is `release/<base branch>`. Older per-package PRs are closed on the next run. - [`36d3074`](https://github.com/lukasIO/verwalter/commit/36d30745984188b57ba933ce128d63e0ecbd9cc7) ([@lukasIO](https://github.com/lukasIO))

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
