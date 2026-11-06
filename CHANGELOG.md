# Changelog

## 0.28.1

### Patch Changes

- Fix `verwalter release` and `verwalter status` failing on a tag that starts with `@`, such as `@scope/pkg@1.0.0`. - [#78](https://github.com/livekit/verwalter/pull/78) ([@lukasIO](https://github.com/lukasIO))
- The check comment heads dependent and fixed-group bumps "Also bumped" instead of "Also released": the merge only adds them to the next release PR - [#76](https://github.com/livekit/verwalter/pull/76) ([@lukasIO](https://github.com/lukasIO))

## 0.28.0

### Minor Changes

- Add a `dependencies` key to a package for in-repo dependencies its manifests do not name, such as an unpublished crate a Python or Node package links at build time - [#74](https://github.com/livekit/verwalter/pull/74) ([@lukasIO](https://github.com/lukasIO))

## 0.27.1

### Patch Changes

- Adopt anyhow for error handling - [#64](https://github.com/livekit/verwalter/pull/64) ([@ladvoc](https://github.com/ladvoc))
- Fix changeset silently overwritten - [#71](https://github.com/livekit/verwalter/pull/71) ([@ladvoc](https://github.com/ladvoc))
- Fix `verwalter pr` failing in repositories that use Git LFS. The release worktree no longer downloads LFS objects. - [#73](https://github.com/livekit/verwalter/pull/73) ([@lukasIO](https://github.com/lukasIO))
- Fix release view query swallows errors - [#72](https://github.com/livekit/verwalter/pull/72) ([@ladvoc](https://github.com/ladvoc))

## 0.27.0

### Minor Changes

- The action takes a GitHub App (`app-client-id`, `app-private-key`) in place of `token`, and `git push` now uses the given token, so release PRs, tags and releases start other workflows - [#60](https://github.com/livekit/verwalter/pull/60) ([@lukasIO](https://github.com/lukasIO))

### Patch Changes

- Document installation from source - [#57](https://github.com/livekit/verwalter/pull/57) ([@ladvoc](https://github.com/ladvoc))

## 0.26.0

### Minor Changes

- A package that `members` finds takes the manifest it was found by as its versioned file when `[versioning].files` is not set: `Cargo.toml`, `package.json`, `pyproject.toml` or `go.mod`, and the `Cargo.toml` next to a maturin `pyproject.toml` with a dynamic version. Before, it failed with "no versioning.files". A mixed Cargo, pnpm and uv repository now needs no `[versioning]` table. - [#54](https://github.com/livekit/verwalter/pull/54) ([@lukasIO](https://github.com/lukasIO))
- Each release updates its own entry in the root lock file of its manifest format (`Cargo.lock`, `package-lock.json`, `uv.lock`) without a `pin_files` entry. Before, a lock file that no package and no `pin_files` listed stayed at the old version, and `cargo build --locked` failed on the release PR. - [#51](https://github.com/livekit/verwalter/pull/51) ([@lukasIO](https://github.com/lukasIO))
- A crate with `version.workspace = true` can be a package of its own. Its version is read from and written to the root `Cargo.toml`, the pins on its dependencies still move in its own manifest, and all packages on `[workspace.package].version` release together as one fixed group. Before, such a crate failed with "no versioned file carries a version", and the workaround (the root `Cargo.toml` as its only file) left the pins in dependent crates at the old version. - [#52](https://github.com/livekit/verwalter/pull/52) ([@lukasIO](https://github.com/lukasIO))

### Patch Changes

- A package whose `pyproject.toml` has `dynamic = ["version"]` fails with the cause and the file to list instead: the `Cargo.toml` next to it for maturin. Before, the error only said "no versioned file carries a version". - [#53](https://github.com/livekit/verwalter/pull/53) ([@lukasIO](https://github.com/lukasIO))

## 0.25.0

### Minor Changes

- A package that versions `[workspace.package].version` in the root `Cargo.toml` also moves the `Cargo.lock` entry of every crate with `version.workspace = true`, private crates included. A product named like no crate, such as one version for several `publish = false` crates, no longer needs a `Cargo.lock` regex. - [#49](https://github.com/livekit/verwalter/pull/49) ([@lukasIO](https://github.com/lukasIO))

## 0.24.0

### Minor Changes

- Use Clap for CLI argument parsing & documentation - [#44](https://github.com/livekit/verwalter/pull/44) ([@ladvoc](https://github.com/ladvoc))

### Patch Changes

- `check` builds the link to create a missing changeset from the PR's head repository, so on a PR from a fork it opens the new-file page on the fork, where the branch lives, instead of a 404 on the base repo. - [#48](https://github.com/livekit/verwalter/pull/48) ([@MaxHeimbrock](https://github.com/MaxHeimbrock))
- Upgrade to 2024 edition - [#47](https://github.com/livekit/verwalter/pull/47) ([@ladvoc](https://github.com/ladvoc))

## 0.23.0

### Minor Changes

- Go modules: `go.mod` carries the version in a module-line comment, `go.work` members are packages named by directory, `require` lines move with each release, tags default to `<path>/vX.Y.Z`, and `publish.registry = "goproxy"` publishes through the Go module proxy. - [#39](https://github.com/livekit/verwalter/pull/39) ([@lukasIO](https://github.com/lukasIO))

### Patch Changes

- `--asset` takes a directory and attaches every file in it. An extra argument, such as a glob that the shell expanded after `--asset`, now fails with the fix instead of being read as the checkout. - [#41](https://github.com/livekit/verwalter/pull/41) ([@lukasIO](https://github.com/lukasIO))
- `check` prints its result to the workflow log and the job summary. A PR from a fork, whose token cannot comment, now passes or fails on the check alone instead of failing on the comment write. - [#42](https://github.com/livekit/verwalter/pull/42) ([@lukasIO](https://github.com/lukasIO))
- Cap release and pre-release PR titles at 72 characters: a long package list ends in `and N more`. - [#43](https://github.com/livekit/verwalter/pull/43) ([@lukasIO](https://github.com/lukasIO))

## 0.22.2

### Patch Changes

- An agent skill in `skills/verwalter` teaches a coding agent to operate verwalter and to adopt it in a repository that releases with changesets/action, knope, release-plz, a custom bump script or hand-pushed tags. - [#35](https://github.com/livekit/verwalter/pull/35) ([@lukasIO](https://github.com/lukasIO))

## 0.22.1

### Patch Changes

- The action builds verwalter from its own checkout with `cargo` when the release binary cannot be downloaded. - [#34](https://github.com/livekit/verwalter/pull/34) ([@lukasIO](https://github.com/lukasIO))
- verwalter moved to `livekit/verwalter`. The action, the schema URL, the registry User-Agent and the docs name the new repository, and the README, docs and example workflows pin v0.22.0. - [#32](https://github.com/livekit/verwalter/pull/32) ([@lukasIO](https://github.com/lukasIO))

## 0.22.0

### Minor Changes

- A release below a version already tagged, such as 1.5.1 from a `v1` branch after 2.0.0, is now a maintenance release. Its GitHub release is created with `--latest=false`, and the built-in npm command publishes it under `latest-1` (`latest-0.4` for 0.4.x) instead of `latest`. docs/action.md describes how to set up a maintenance branch. - [`29fcd01`](https://github.com/lukasIO/verwalter/commit/29fcd01a0cdceb166daba1fb183e3c168d5216ae) ([@lukasIO](https://github.com/lukasIO))

## 0.21.0

### Minor Changes

- Breaking: a new `[workspace]` table holds the package set and the files outside the packages, with every path relative to the repository root. `[workspace].exclude` replaces `[check].ignore`, and `[versioning].pin_files` moved to `[workspace].pin_files`. The new optional `[workspace].members`, for example `members = ["packages/*"]`, makes every publishable Cargo, pnpm or uv workspace member in those directories a package. A `[packages.<name>]` entry overrides a found package and needs no `path`. `base_branch` moved to `[git].base_branch`. - [`3f010f8`](https://github.com/lukasIO/verwalter/commit/3f010f86b7e01aa08dfa41be8b480d7b40cbb587) ([@lukasIO](https://github.com/lukasIO))

## 0.20.0

### Minor Changes

- Breaking: `verwalter.toml` groups its keys in `[check]`, `[versioning]`, `[release]` and `[publish]`. Keys in `[versioning]` and `[release]` are defaults for every package, and a package overrides any table key with a dotted key such as `release.draft = true`. Renamed keys: `versioned_files` is `versioning.files`, `tag_template` is `release.tag`, `release` is `release.mode`, `release_branch_prefix` is `[release].branch_prefix`, `release_label` is `[release].label`, and `publish` is `publish.command`. See docs/configuration.md for every key. - [`0946061`](https://github.com/lukasIO/verwalter/commit/09460612d8514fd9444e539310631f6d19f67552) ([@lukasIO](https://github.com/lukasIO))

### Patch Changes

- README, docs and example workflows pin v0.19.0. - [`fdccaee`](https://github.com/lukasIO/verwalter/commit/fdccaeed830f406106d990cf655c9c6f45910d11) ([@lukasIO](https://github.com/lukasIO))

## 0.19.0

### Minor Changes

- `versioned_files` and `changelog` are relative to the package's `path`, with `..` for a sibling directory, and `changelog` defaults to `CHANGELOG.md`, with `changelog = false` to keep none. Repo-relative paths in an existing config must drop the package prefix. - [`26f2470`](https://github.com/lukasIO/verwalter/commit/26f2470d57aaaca22dbaf2ba48b139579f11457e) ([@lukasIO](https://github.com/lukasIO))

### Patch Changes

- README, docs and example workflows pin v0.18.1. - [`09a1dbc`](https://github.com/lukasIO/verwalter/commit/09a1dbccf8683bb7683df024b589a1e94caa01d2) ([@lukasIO](https://github.com/lukasIO))

## 0.18.1

### Patch Changes

- The registry check sends a `verwalter/<version>` User-Agent. crates.io answers 403 to curl's default one, so every crates.io existence check failed. - [`fbf9ebc`](https://github.com/lukasIO/verwalter/commit/fbf9ebcb7e6800381caa6332010211ee7a7748a5) ([@lukasIO](https://github.com/lukasIO))
- README, docs and example workflows pin v0.18.0. - [`6a3ac3d`](https://github.com/lukasIO/verwalter/commit/6a3ac3d7d6af1b5403a787d904dd960df5e14925) ([@lukasIO](https://github.com/lukasIO))
- A package with `registry = "none"` set on itself and no `publish` command is released on GitHub only: `publish` leaves it out instead of failing on a missing command. - [`1667755`](https://github.com/lukasIO/verwalter/commit/166775526d8dda16a8ecb9ec0b791905ba0c57e3) ([@lukasIO](https://github.com/lukasIO))
- An rc is bumped from the higher of the current version and the newest stable tag. A version ahead of its tags was pulled back to the tag before. - [`2182a69`](https://github.com/lukasIO/verwalter/commit/2182a694ff9a8d768eb7c711c3f591aae9b31b0a) ([@lukasIO](https://github.com/lukasIO))

## 0.18.0

### Minor Changes

- `pre-release` takes no package and no level. The rc carries every package with a pending changeset and the fixed groups they are in, each at its own rc version from its changesets, pinned to each other, on `release/<target>-rc`. The example workflow has no inputs. - [`c1f65f6`](https://github.com/lukasIO/verwalter/commit/c1f65f6a2c57cf5be341ac7c55d586a282f80709) ([@lukasIO](https://github.com/lukasIO))

### Patch Changes

- The rc example workflow takes the level from the pending changesets unless one is given. - [`94e89f7`](https://github.com/lukasIO/verwalter/commit/94e89f789c3ec8fbf8dd41994a7cfe7494f6b5f9) ([@lukasIO](https://github.com/lukasIO))
- README, docs and example workflows pin v0.17.0. - [`5bca7be`](https://github.com/lukasIO/verwalter/commit/5bca7be7a4b42a6b228e723c7f8d36bb0045ec34) ([@lukasIO](https://github.com/lukasIO))
- The built-in npm and pnpm publish commands add `--tag next` for a pre-release version. - [`75f2dba`](https://github.com/lukasIO/verwalter/commit/75f2dbabf015ab77ae12b889e10c05b1b957fa47) ([@lukasIO](https://github.com/lukasIO))
- In a pnpm workspace the built-in npm publish command is `pnpm publish`, which resolves `workspace:` ranges to versions. `npm publish` ships them as is. - [`e585556`](https://github.com/lukasIO/verwalter/commit/e5855562a0d7af391f4a66bb1a3f3d387015f12f) ([@lukasIO](https://github.com/lukasIO))

## 0.17.0

### Minor Changes

- `publish` takes `--releases`, the `releases` output of `release`, and runs only those packages. The action passes its new `releases` input through. Without it, every tagged package runs, as before. - [`45e850d`](https://github.com/lukasIO/verwalter/commit/45e850d23b6a4c3edcfe6916eb04d46b36f206d1) ([@lukasIO](https://github.com/lukasIO))
- An rc carries the in-repo dependencies of the requested package that have a pending changeset, transitively, each at its own rc version, and pins the carried packages to each other. Dependents outside the rc are untouched. The rc's releases are ordered dependencies first. - [`45e850d`](https://github.com/lukasIO/verwalter/commit/45e850d23b6a4c3edcfe6916eb04d46b36f206d1) ([@lukasIO](https://github.com/lukasIO))

### Patch Changes

- The example workflows use actions/checkout v7, upload-artifact v7, download-artifact v8 and taiki-e/install-action v2 with a `tool` input. - [`6439bac`](https://github.com/lukasIO/verwalter/commit/6439bacf88003d780e7c9653737e4ec2c8989385) ([@lukasIO](https://github.com/lukasIO))
- A JSON Schema for `verwalter.toml` at `schemas/v1.json` gives completion and hover docs in editors with Taplo or Tombi. Add `#:schema https://raw.githubusercontent.com/lukasIO/verwalter/main/schemas/v1.json` as the first line. - [#24](https://github.com/lukasIO/verwalter/pull/24) ([@lukasIO](https://github.com/lukasIO))
- examples/workflows has the rc workflow, `pre-release.yml`. - [`ac999dd`](https://github.com/lukasIO/verwalter/commit/ac999ddd1cd2d036ed273a3a19b03f71213166d1) ([@lukasIO](https://github.com/lukasIO))
- README, docs and example workflows pin v0.16.0. - [`7177f40`](https://github.com/lukasIO/verwalter/commit/7177f407ff978af25ece3e1da13e9919a5d0be8d) ([@lukasIO](https://github.com/lukasIO))
- The README shows the release cycle as a diagram. - [`ac62e05`](https://github.com/lukasIO/verwalter/commit/ac62e0519b13e646d8eade895fef7eb5f7c6b96e) ([@lukasIO](https://github.com/lukasIO))

## 0.16.0

### Minor Changes

- The check comment lists the bump each changeset claims, per package, on a passing PR too, and the dependents and fixed-group members that ride along. It is deleted only when the PR claims nothing. The `max_bump` config field is removed: a changeset says what a change is. - [`b5a57a7`](https://github.com/lukasIO/verwalter/commit/b5a57a7f8705f9259572560339dcd397448c7c26) ([@lukasIO](https://github.com/lukasIO))

### Patch Changes

- README, docs and example workflows pin v0.15.0. - [`dc18056`](https://github.com/lukasIO/verwalter/commit/dc18056034e189bc56f2fb66d4d4ce7ee4e3f7d4) ([@lukasIO](https://github.com/lukasIO))

## 0.15.0

### Minor Changes

- The check comment lists the bump each changeset claims, per package, on a passing PR too. It is deleted only when the PR claims nothing. - [`45b4d6f`](https://github.com/lukasIO/verwalter/commit/45b4d6f54a3fc913bd27806dfa4072a3c45bb4b9) ([@lukasIO](https://github.com/lukasIO))

### Patch Changes

- README and example workflows pin v0.14.1. - [`1f53e16`](https://github.com/lukasIO/verwalter/commit/1f53e16429b714a94c8d790735c8f49870b3f993) ([@lukasIO](https://github.com/lukasIO))
- Internal cleanup: toml 1 (one toml stack instead of two), shared helpers, dead code removed. No behaviour change. - [#20](https://github.com/lukasIO/verwalter/pull/20) ([@lukasIO](https://github.com/lukasIO))
- The README is a quick start. Configuration, the action, features and failure paths, the API check and the commands moved to `docs/`. - [`6a457da`](https://github.com/lukasIO/verwalter/commit/6a457dab5141d43a0dd40ca4d8748b25e641fbc2) ([@lukasIO](https://github.com/lukasIO))

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
