# Adopting verwalter in a repository that releases another way

The goal is one PR that replaces the current release tooling with
`verwalter.toml`, the four workflows and the repository settings, with
the version numbers, tag names and changelogs continuing without a gap.
The [README](../README.md) has the quick start for a repository that
releases nothing yet. This document is for a repository that releases
with changesets/action, knope, release-plz, a hand-written bump script
or tags pushed by hand.

Contents:

- [The procedure](#the-procedure): inventory, configuration, validation, workflows, handoff.
- [Per tooling](#per-tooling): the mapping from each tool's configuration to `verwalter.toml`, what to delete, what changes afterwards.
- [Traps during adoption](#traps-during-adoption).

## The procedure

Work in this order. Each step builds on the facts the step before
collected.

### Step 1. Inventory what the repository does today

Look before you write. The tag format, the version files and the publish
command are facts in the repository, not choices to make. Collect:

```bash
git tag --sort=-v:refname | head -20          # the tag format and the last version per package
ls .changeset knope.toml release-plz.toml .version version.go gradle.properties 2>/dev/null
ls .github/workflows && grep -l "release\|publish\|version\|tag" .github/workflows/*
grep -rn '"version"\|^version\|Version = \|VERSION' --include=package.json --include=Cargo.toml --include=pyproject.toml --include=pubspec.yaml --include=*.go --include=*.properties --include=*.podspec --include=*.py . | grep -v node_modules | head -40
```

Write down, per package: name, path, every file that carries its version,
its changelog file, its tag format, its registry and the command that
publishes it, and whether it is private. Note the fixed groups (packages
that always share a version) and what the old tool ignored. A version
file that matches no package and no tag is not listed: name it in the
report and ask what it versions. Then read the matching section of
[Per tooling](#per-tooling) below: it maps each tool's config to
`verwalter.toml` and lists what to delete.

### Step 2. Write `verwalter.toml`

Write the smallest file that describes the repository. Two things are
required: the package list, and the files that carry the versions.
Everything else has a default, and a key that repeats a default is
noise that hides the keys that matter. Write a key only when the
repository differs from the default, and never drop a key the
repository needs: the tag format, a second version file, a fixed group
or a registry are facts of the repository, not optional polish.

A pnpm monorepo that releases with changesets/action today needs four
keys:

```toml
[workspace]
members = ["packages/*"]           # every publishable workspace member is a package

[versioning]
files = ["package.json"]           # relative to each package's path. The first carries the version

[release]
tag = "{package}@{version}"        # the existing tags. The default is {package}-v{version}

[publish]
registry = "npm"                   # the built-in publish command and the existence check
```

A single package in another language lists itself:

```toml
[packages.server-sdk-go]
path = "."
versioning.files = [{ path = "version.go", regex = 'Version = "(?<version>[^"]+)"' }]
```

Every default is marked `# default` in
[configuration.md](configuration.md).

Keys that are written when the repository needs them:

- **`release.tag`** must match the existing tags, or the next release
  looks like the first. Set it per package when the formats differ.
- **Every file that carries a version** is listed, the manifest first.
  The version in the files must equal the last tag: a `-SNAPSHOT` or a
  pre-bumped version is set to the released version in the migration PR.
  Any text file takes a regex with a `version` capture group.
- **`[workspace] pin_files`** for a root `Cargo.toml` with
  `[workspace.dependencies]`, `Cargo.lock` or `uv.lock`, so that the
  pins of in-repo dependencies move with each release.
- **`[versioning] fixed`** for packages that always share a version.
- **`publish.registry`** per package on another registry, and
  `publish.command` only when the built-in command does not fit.
  A package with `registry = "none"` and no command is released on
  GitHub only. Go modules and Swift packages are that case.
- **`[publish] workspace`** when the repository chooses one recursive
  publish. Where the ecosystem has one (`pnpm -r publish`,
  `cargo publish --workspace`), ask the user which of the two the
  migration uses, with one line per option: the workspace command is
  one run that owns the order and the skipping, the per-package
  commands give one result per package and let a failure block only
  its dependents.
- **`[workspace] exclude`** for a publishable member that is not
  released, else every command warns about it.

Dependencies between packages are read from the manifests, never
configured. The changelog continues: an existing `CHANGELOG.md` is
kept, new entries go on top. Every key is in
[configuration.md](configuration.md).

### Step 3. Validate locally before any workflow runs

```bash
verwalter plan            # offline: every package found, no warnings about drift or missing config
verwalter status          # needs origin and gh: each package, version equals its tag, registry:yes
verwalter pr --dry-run    # needs origin: the branch and the PR that the next push to main produces
```

In a checkout without the `origin` remote, `plan` is the check. See
"Running locally" in [commands.md](commands.md).

A warning names a real problem: files of one package that disagree, a
workspace member the config does not cover, a changeset for an unknown
package. Fix the config or the files until the output is clean. A
pending changeset from the old tool parses as it is, as long as the
package names in `verwalter.toml` are the names the changesets use.

### Step 4. Replace the workflows

Copy the four workflows from the verwalter repository's
`examples/workflows`. Pin the newest release by commit, with the tag as
the comment, in every workflow:

```bash
TAG=$(gh release view --repo livekit/verwalter --json tagName -q .tagName)
SHA=$(gh api "repos/livekit/verwalter/commits/$TAG" -q .sha)
perl -pi -e "s|livekit/verwalter\@v[0-9.]+|livekit/verwalter\@$SHA # $TAG|" .github/workflows/*.yml
```

Then:

- In `release.yml`, replace the placeholder build job with the real one
  (wheels, binaries, xcframeworks, docs). Keep the order: build, then
  `release`, then `publish`. The step that cannot be undone comes last.
  A package with nothing to attach, such as an npm package, needs no
  build job before `release`: drop the `--asset` argument and the
  `upload-artifact` and `download-artifact` steps.
- In the publish job, set up the registry tooling the commands need
  (`setup-node` with `registry-url`, `cargo login`, `uv`, `setup-dart`),
  and `id-token: write` for trusted publishing. The command publishes
  what is on disk, so the job installs and builds first, or downloads
  the build job's artifact.
- In `changeset-check.yml`, drop the `taiki-e/install-action` step and
  `fetch-depth: 0` unless `[check].api` is set.
- Keep what ran after the old publish (docs upload, downstream
  dispatch) as steps at the end of the publish job, with
  `continue-on-error: true`, because the installed workspace is there.
  Move the secrets or the OIDC role those steps used to that job.
- Delete the old tooling: the old workflows, the config file of the old
  tool, bump scripts, release-note scripts, and the `devDependency` or
  install step of the old tool. A removed `devDependency` changes the
  lock file: run the install once and commit it, or
  `--frozen-lockfile` fails in CI. Keep the `.changeset/*.md` files:
  the format is the same and pending ones carry over.
- If another workflow triggers on the tag or on `release: published`,
  it will not fire from `github.token`. Give the action a PAT or App
  token in `token`, or move that job into `release.yml`.

### Step 5. Settings and the handoff

The migration PR cannot do these. List them in its description for the
maintainer:

- Settings, Actions, General: allow GitHub Actions to create and approve
  pull requests. Or provide a token in `RELEASE_TOKEN`.
- Create the `internal` label.
- Trusted publishing on npm, PyPI or crates.io for the workflow, if the
  old flow used a token secret. Keep the secret until the first release
  went through.
- A package the registry has never seen is a first release. Trusted
  publishing cannot create it: the first upload is by hand, and the PR
  body and `VERWALTER_FIRST_RELEASE` say when.
- Add the `internal` label to the migration PR itself, or write a
  changeset for it if it touched a package's files.
- The API check as a follow-up: `[check].api` with cargo-semver-checks
  for Rust, griffe for Python, break-check for TypeScript, as described
  in [api-check.md](api-check.md). The migration PR leaves it off.
- Every item of the tool's "Different afterwards" list under
  [Per tooling](#per-tooling): what contributors do differently, and
  what the old flow had that the new one has not, such as snapshot
  releases.

End your report with the output of `verwalter plan`. That is the
first release PR the maintainer will see.

## Per tooling

One section per release tooling. Each one lists what the repository has,
how its configuration maps to `verwalter.toml`, what to delete, and what
stays different. A repository that matches none is "Tags pushed by hand"
plus the registry it publishes to.

Every section ends in the same place: `verwalter plan` and
`verwalter status` run clean, and the four workflows are in
`.github/workflows`. Each example configuration is the smallest one for
its case. The defaults it leaves out are marked `# default` in
[configuration.md](configuration.md). Publish commands per registry are
in [examples/publish/README.md](../examples/publish/README.md).

### changesets/action

**What is there.** `.changeset/config.json`, `.changeset/*.md`, a
`release.yml` that runs `changesets/action` with `publish: pnpm ci:publish`
on every push to `main`, often a `snapshot` job that runs
`changeset version --snapshot` and publishes under a dist-tag, and
`@changesets/cli` plus a changelog plugin in `devDependencies`. Tags are
`<name>@<version>`, one GitHub release per package.

**Mapping.**

| `.changeset/config.json` | `verwalter.toml` |
|---|---|
| `baseBranch` | `[git] base_branch` |
| `fixed` | `[versioning] fixed`. Globs such as `@scope/plugin-*` are not supported: list the names. |
| `linked` | No equivalent. Use `fixed`, or leave the packages independent. |
| `ignore` | `[workspace] exclude`. A `private: true` package is left out on its own. |
| `updateInternalDependencies` | `[versioning] dependent_bump_level` (default `patch`). |
| `access` | Part of the publish command: the built-in npm command passes `--access public`. |
| `changelog` plugin | Built in. The entry carries the PR number and the author. |
| `commit` | No equivalent. The tool commits on the release branch only. |

```toml
[workspace]
members = ["packages/*"]           # every non-private pnpm workspace member

[versioning]
files = ["package.json"]

[release]
tag = "{package}@{version}"        # changesets/action's format

[publish]
registry = "npm"                   # built-in: pnpm publish in a pnpm workspace, with --access public
```

Add `[workspace] exclude` only for a publishable member the old
`ignore` list left out. Ask the user whether the migration publishes
per package (the built-in `pnpm publish` in each package, one result
per package) or with one recursive run
(`[publish] workspace = "pnpm -r publish --access public --no-git-checks"`,
which owns the order and the skipping). Write the answer.

**Delete.** `.changeset/config.json`, the `changesets/action` step and
the snapshot job, `@changesets/cli` and the changelog plugin from
`devDependencies`, the `changeset`, `ci:version` and `ci:publish`
scripts. A `ci:publish` that builds and then runs `changeset publish`
splits: the build goes into the publish job, the publish is the
built-in command. Run `pnpm install` once after the removal and commit
`pnpm-lock.yaml`. Keep `.changeset/*.md` and `.changeset/README.md`:
pending changesets carry over as they are. Keep every `CHANGELOG.md`.

**Different afterwards.**

- The release PR is regenerated from `main` on every push instead of
  being committed to. Nobody pushes fixes onto it.
- `publish` runs in `release.yml` after the merge, not on the same push
  that opened the PR. The old flow tagged after the publish. The new one
  tags first, then publishes. A publish failure leaves the tag, and the
  re-run skips what the registry has.
- Snapshot releases have no equivalent. `pre-release.yml` opens an rc PR
  with the versions `1.6.0-rc.1` under the `next` dist-tag. For a true
  snapshot from any commit, keep a separate workflow that writes
  `0.0.0-<label>-<sha>` and publishes under a dist-tag, without a tag.
- A job that verified every package exists on npm before the publish
  is covered: `status` shows `registry:no`, and `VERWALTER_FIRST_RELEASE`
  is set for the command.

### knope

**What is there.** `knope.toml` with `[packages.<name>]` entries,
`versioned_files` that include the root `Cargo.toml` with a `dependency`
key and `Cargo.lock`, `changelog` per package, `scopes` for conventional
commits, and `[github]`. Changesets in `.changeset/*.md`, same format.
Workflows run `knope prepare-release` on push and `knope release` after
the merge. Tags are `<name>/v<version>`.

**Mapping.**

| `knope.toml` | `verwalter.toml` |
|---|---|
| `[packages.<name>] versioned_files = ["pkg/Cargo.toml"]` | `[packages.<name>] path = "pkg"`, and `[versioning] files = ["Cargo.toml"]` once for all. Paths are relative to the package. |
| `{ path = "Cargo.toml", dependency = "x" }` and `"Cargo.lock"` on every package | `[workspace] pin_files = ["Cargo.toml", "Cargo.lock"]` once. Each release updates its own entry. |
| `changelog = "pkg/CHANGELOG.md"` | Default. Leave it out. |
| `scopes` | Dropped. Conventional commits are not read. |
| `[github]` | Read from the `origin` remote. |
| `[bot.releases]` | `[release] mode = "tag_and_release"`, the default. |
| `[changes] ignore_conventional_commits` | Always the case. |

```toml
[workspace]
members = ["crates/*"]              # `*` matches one whole path component; crates at the root are listed under [packages] with a path
pin_files = ["Cargo.toml", "Cargo.lock"]

[versioning]
files = ["Cargo.toml"]

[release]
tag = "{package}/v{version}"        # knope's format. The default is {package}-v{version}

[publish]
registry = "crates"
```

The built-in command is `cargo publish` per crate, dependencies first,
with one result per crate. On cargo 1.90 and later,
`[publish] workspace = "cargo publish --workspace"` is one run from the
root that owns the order and the waits for the index. Ask the user
which one the migration uses, and write the answer.

A `members` entry that is a Cargo workspace lists every publishable
member. A crate with `publish = false` is left out. List a crate under
`[packages]` to override its keys, such as `release.mode = "tag"` for a
crate that is tagged but not released on GitHub.

**Delete.** `knope.toml`, the knope install and run steps, and any
`knope` references in contributor docs. Keep `.changeset/*.md`.

**Different afterwards.** `cargo publish` refuses a crate whose
dependency's version is not on the index yet. The tool orders the crates
and waits, or `cargo publish --workspace` does. A crate that failed
blocks its dependents only. The rest publish.

### release-plz and cargo-release

**What is there.** `release-plz.toml` or `[workspace.metadata.release]`
in `Cargo.toml`, a changelog made by git-cliff from conventional
commits, `release-plz release-pr` on push and `release-plz release` after
the merge, or a `cargo release` run by a maintainer. Tags are
`<name>-v<version>` in a workspace and `v<version>` for one crate.

**Mapping.**

| release-plz | `verwalter.toml` |
|---|---|
| `[workspace] publish = false` on a package | `publish.registry = "none"` on the package. |
| `git_release_enable = false` | `release.mode = "tag"`. |
| `git_tag_name` | `release.tag`. The default already matches release-plz's. |
| `changelog_update = false` | `versioning.changelog = false`. |
| `git_release_draft = true` | `release.draft = true`. |
| `semver_check` | `[check] api` with cargo-semver-checks. See the API check doc. |
| `dependencies_update`, `allow_dirty`, `pr_labels` | No equivalent. The PR gets `[release].label`. |

**Delete.** `release-plz.toml`, `cliff.toml`, the release-plz or
cargo-release workflow steps.

**Different afterwards.** The level no longer comes from commit prefixes.
Every PR that changes a crate needs a changeset, and `changeset-check.yml`
enforces it. Tell contributors in `CONTRIBUTING.md`, and give them
`verwalter changeset --level minor`. Existing changelogs are kept; new
entries are one bullet per changeset, not one per commit.

### A custom changeset or bump script

**What is there.** A script such as `.github/changesets.py` or
`.github/update_versions.py` that reads `.changeset/*.md` or a
`workflow_dispatch` choice (`patch`, `minor-rc`, `next-rc`, `promote`),
writes the versions into `pyproject.toml` or a `version.py`, opens a
`release/...` PR, and a second job that publishes when that PR merges. A
`manual-version-bump.yml` as the fallback. Release notes written by a
script or an LLM. Plugins that pin a floor on the core package.

**Mapping.**

| Script feature | verwalter |
|---|---|
| `.changeset/*.md` in the changesets format | Read as they are. |
| `patch`, `minor`, `major` choice | The changeset level. Nothing is chosen at dispatch time. |
| `minor-rc`, `next-rc` | `pre-release.yml`. The level comes from the changesets, the number from the tags. |
| `promote` | Merging the release PR after an rc. The final version is never below the rc. |
| `republish` | Re-run the publish job. Versions on the registry are skipped. |
| `--rc-only` on non-main branches | `pre-release --target <branch>`. A final release from a branch is a maintenance branch, with its own `verwalter.toml`. |
| A version in `version.py` | `versioning.files = ["pyproject.toml", { path = "livekit/agents/version.py", regex = '__version__ = "(?<version>[^"]+)"' }]` |
| Plugins pinned to the core at one version | `[versioning] fixed = [["core", "plugin-a", "plugin-b"]]` |
| Plugins with a floor on the core | Nothing. The floor in `pyproject.toml` moves on each release of the core. |
| `uv.lock` | Updated by the tool. |
| Release notes script | Dropped. The GitHub release body is the changelog entry. |
| `release-gate.yml` (bot author plus approvals) | The example `release.yml` checks the bot author and the body marker. Keep a branch protection rule for the reviews. |

```toml
[workspace]
members = ["packages/*"]            # `*` matches one whole path component; members at the root are listed under [packages] with a path
pin_files = ["uv.lock"]

[versioning]
files = ["pyproject.toml"]

[publish]
registry = "pypi"                   # built-in: python -m twine upload dist/*, so the publish job builds into dist/ first
```

The rc label `rc` is the default and is written as `1.6.0rc1` in
`pyproject.toml`. A package built on a matrix (wheels per platform) is
better uploaded with `pypa/gh-action-pypi-publish` and
`skip-existing: true` in the publish job, driven by the `releases`
output, so that a half-uploaded version completes on re-run.

**Delete.** The scripts, `manual-version-bump.yml`, the `pip install
click packaging` steps, and the `release-gate.yml` if the branch
protection rule covers the approvals.

**Different afterwards.** The tag format. Check `git tag` first: a
Python monorepo often tags `<dist-name>@<version>` or `<name>-v<version>`.
Set `release.tag` to match, per package if they differ. The package name
in `verwalter.toml` is the manifest name (`livekit-agents`), and the
changesets must use the same name.

### Tags pushed by hand

**What is there.** The version in one or more files: `.version`,
`version.go`, `gradle.properties` (`VERSION_NAME=1.2.3-SNAPSHOT`),
`pubspec.yaml`, a `.podspec`, `Package.swift`, a README. A maintainer
edits the files, commits, tags `v1.2.3` and pushes. A workflow on
`push: tags: ["v*"]` builds and publishes, and checks that the tag
matches the file. A `release-instructions.md`. Sometimes goreleaser.

**Mapping.** One package at `path = "."`, every file listed:

```toml
[packages.client-sdk-swift]
path = "."
versioning.files = [
  { path = ".version", regex = "(?<version>.+)" },
  { path = "LiveKitClient.podspec", regex = 'spec.version = "(?<version>[^"]+)"' },
  { path = "Sources/LiveKit/LiveKit.swift", regex = 'static let version = "(?<version>[^"]+)"' },
  { path = "README.md", regex = 'LiveKitClient", from: "(?<version>[^"]+)"' },
]
publish.command = "pod trunk push LiveKitClient.podspec"   # no registry: the command is the whole publish
```

The tag `v{version}` is the single-package default, so `release.tag`
stays out. Every file that carries the version stays in: a file left
out drifts on the next release.

- `.version` and other files that hold only the version: a regex entry
  that captures the line, `{ path = ".version", regex = "(?<version>.+)" }`.
  A bare path works for the formats the tool knows (`Cargo.toml`,
  `package.json`, `pyproject.toml`, `pubspec.yaml`, `go.mod`, `pom.xml`).
- `-SNAPSHOT` in `gradle.properties`: set the released version in the
  migration PR. The next release moves it. There is no snapshot state
  between releases any more.
- `version.go` (`const Version = "2.19.0"`): a regex entry. Go modules
  have no registry and no publish command: `[publish]` stays out, which
  is the default `none`. The tag `v{version}` is the release, which is
  the single-package default.
- A nested `/v2` module path is not managed. Say so in the report.

**The build workflow.** Two options:

1. Move the build into `release.yml` (preferred). The build job produces
   the assets, `release` attaches them, `publish` runs the registry
   command. The tag-triggered workflow is deleted.
2. Keep the tag-triggered workflow and give the action a PAT or App
   token in `token`, because a tag pushed with `github.token` fires no
   `push` event. Then `release.mode = "tag"` when the old workflow
   creates the GitHub release itself.

goreleaser creates the GitHub release on its own. Either run it with
`--skip=publish` in the build job and attach `dist/*` with `--asset`, or
choose option 2 and let goreleaser own the release.

**Delete.** `release-instructions.md` or the release section of the
README, the "tag matches file" check (the tool derives the tag from the
file), and the `-SNAPSHOT` bump step. Keep the cross-repository version
check if there is one, as a step before `release`.

**Different afterwards.** Contributors write changesets. The maintainer
merges the release PR instead of pushing a tag. The changelog starts with
this release unless `CHANGELOG.md` already exists.

### Registry notes

| Registry | Built-in command | Notes |
|---|---|---|
| npm | `npm publish --provenance --access public`, or `pnpm publish` in a pnpm workspace | `--tag next` for an rc, `--tag latest-1` for a maintenance release. `setup-node` with `registry-url` and `id-token: write` for provenance. |
| crates.io | `cargo publish` | `[publish] workspace = "cargo publish --workspace"` on cargo 1.90 and later. Else per crate, dependencies first, which the tool orders. |
| PyPI | `python -m twine upload dist/*` | Build first. `pypa/gh-action-pypi-publish` with `skip-existing: true` is the alternative for multi-wheel packages. |
| pub.dev | `dart pub publish --force` | `setup-dart` mints the token from OIDC. |
| Maven Central | none: `publish.registry = "custom"` with `exists_url`, and the gradle command | `exists_url = "https://repo1.maven.org/maven2/<group path>/{name}/{version}/"` |
| CocoaPods | none: `publish.command = "pod trunk push ..."` without a registry | No stable existence API. The command fails on a duplicate, which is a real failure to the tool. |
| Go modules | none: `[publish]` stays out | The tag is the release. |
| GitHub only | none: `[publish]` stays out | Binaries go on the release as `--asset`. |

Trusted publishing cannot create a new package on npm, crates.io or
PyPI. `status` shows `registry:no` for such a package, and the publish
command sees `VERWALTER_FIRST_RELEASE=true`. The first upload is by hand,
and the PR body names it.

## Traps during adoption

- **Version drift across files.** Swift, Flutter and Android carry the
  version in up to five files. List them all. The first file wins and the
  next release aligns the rest.
- **`workspace:*` ranges in npm.** `npm publish` ships the literal range.
  The built-in command is `pnpm publish` in a pnpm workspace, which
  rewrites it. Run `pnpm install` in the publish job first.
- **PEP 440.** `1.6.0-rc.1` is written as `1.6.0rc1` in `pyproject.toml`.
  The tool converts. A `uv.lock` is updated too.
- **Shallow checkouts.** rc numbering and changelog attribution read the
  tags and the history. Every workflow checks out with `fetch-depth: 0`.
- **Hand merges of `release/*`.** The example `release.yml` runs only for
  a PR the bot authored with the verwalter marker in its body. Keep that
  condition.
- **Hotfix from a release branch.** `pr --base <branch>` works. The
  changelog entry is not carried back to `main`. Backport it by hand.
- **Snapshot releases from any commit** have no equivalent. Offer an rc
  PR, or a separate workflow that publishes under a dist-tag with a
  version the tool never sees.
- **Cross-repository version coupling** (a CLI that must match an SDK's
  major.minor) stays a workflow precondition. Keep that check as a step
  before the tag.

The release history behind these is in
[release-pitfalls.md](release-pitfalls.md).
