# verwalter

Changeset-based versioning for single packages and monorepos, in any language.
It bumps versions, writes changelogs, opens one release PR, tags and
creates GitHub releases. Publishing to a registry stays in your workflow.

The behaviour is specified in [versioning.allium](versioning.allium).

## Install

Download a binary from the
[releases page](https://github.com/lukasIO/verwalter/releases), or build
from source:

```bash
cargo install --git https://github.com/lukasIO/verwalter --tag v0.9.0
```

In GitHub Actions use the action below.

## How it works

1. A contributor adds a `.changeset/<name>.md` file to their PR
   (the [changesets](https://github.com/changesets/changesets) format):

   ```md
   ---
   livekit-protocol: minor
   ---

   Negotiated data channels.
   ```

   If a versioned package changed without one, `verwalter check` fails the PR.
   The `internal` label exempts a PR.

2. On every push to `main`, `verwalter pr` opens or updates one release PR
   with every package that has pending changesets. Dependents get a `patch`
   bump and ride in the same PR. The branch (`release/main`) is regenerated
   from `main` on every run and the PR is updated in place.

3. A maintainer merges the release PR. The workflow builds, then
   `verwalter release --asset <file>...` tags the version and creates the GitHub
   release with the assets, then the workflow publishes to the registry. The
   step that cannot be undone comes last. Its `releases` output tells the
   workflow what to publish. If the build turns out broken,
   `verwalter release --abandon <tag>` removes the release and the tag.

## Configuration

`verwalter.toml` at the repository root:

```toml
base_branch = "main"            # default
exempt_label = "internal"       # default
release_branch_prefix = "release/"
release_label = "release"
dependent_bump_level = "patch"
prerelease_label = "rc"          # 1.6.0-rc.1; written as 1.6.0rc1 in pyproject.toml
pin_files = ["Cargo.toml", "Cargo.lock"]   # workspace manifest and lock files
ignore = ["examples/*"]         # workspace members the consistency check skips
fixed = [["livekit-ffi", "livekit-ffi-node"]]  # groups that release at one shared version

[packages.livekit-protocol]
path = "livekit-protocol"
versioned_files = ["livekit-protocol/Cargo.toml"]
changelog = "livekit-protocol/CHANGELOG.md"
tag_template = "{package}-v{version}"      # default in a monorepo, v{version} with one package
release = "tag_and_release"                # none | tag | tag_and_release
draft = false
max_bump = "minor"                         # optional ceiling on the bump level
```

A fixed group releases at one version: the highest version in the group,
bumped by the highest level any member needs. A member without a changeset
of its own comes along with a "Released with" changelog line. A pre-release
of a member takes the whole group. If the committed versions of a group
differ, every command warns until the next release aligns them.

`max_bump` caps what a changeset can do to a package: with `minor`, a
`major` changeset bumps the minor version. In a fixed group the lowest
cap applies to the group.

Every publishable workspace member must be listed, or named in `ignore`.
`verwalter check` fails when a Cargo, pnpm or uv workspace member without a
private marker (`publish = false`, `"private": true`, the
`Private :: Do Not Upload` classifier, `[tool.uv] package = false`) is
missing from the config, or when a configured package's directory has no
manifest. A private package may still be configured: it gets versions, tags
and GitHub releases, and the workflow simply does not publish it. The other
commands print the same as warnings.

Dependencies between packages are read from the manifests, not configured.
Versioned files are handled by
[knope-versioning](https://crates.io/crates/knope-versioning): `Cargo.toml`,
`Cargo.lock`, `package.json`, `package-lock.json`, `pyproject.toml`,
`pubspec.yaml`, `go.mod`, `pom.xml` and more. `uv.lock` is handled by the
tool itself. The first file of a package is its version. Files of one
package that disagree are reported as a warning. `pnpm-lock.yaml` and
`yarn.lock` need no edit: a `workspace:*` or `catalog:` range stores no
version of an internal package, and the tool leaves such ranges alone.

Any other text file takes a regex with a `version` capture group. Only the
captured text changes. This covers `gradle.properties`, a `.podspec`, a
`.version` file, or a version constant in source:

```toml
versioned_files = [
  "pubspec.yaml",
  { path = "lib/src/livekit.dart", regex = "version = '(?<version>[^']+)'" },
  { path = "gradle.properties", regex = "VERSION_NAME=(?<version>.+)" },
]
```

## GitHub Actions

See [examples/workflows](examples/workflows) for the three workflows:
changeset check on PRs, release PRs on push, release after merge.

```yaml
- uses: lukasIO/verwalter@v0.12.0
  with:
    command: check   # check | plan | pr | release
```

For `pr`, the repository setting "Allow GitHub Actions to create and approve
pull requests" (Settings → Actions → General) must be on, or the job must use
a token of a GitHub App or a PAT. The App or PAT token also lets the release
PRs trigger CI, which the default `github.token` does not.

## When a release fails

Nothing is remembered between runs: the tool reads tags, releases and PRs
and finishes what is missing. Start with `verwalter status`. It prints,
per package, the tag state, the GitHub release state and the next step.
Add `--dry-run` to any command to see its steps first.

| What failed | What to do |
|---|---|
| Build or test job before the tag | Fix, merge, re-run the workflow (`workflow_dispatch`). Nothing was written. |
| Tag push | `verwalter release --package <name>` tags and pushes. |
| GitHub release creation | `verwalter release --package <name> [--asset <file>...]` creates it from the existing tag. |
| Asset upload | `verwalter release --package <name> --asset <file>... [--publish]` uploads with `--clobber`. |
| Draft never published | `verwalter release --package <name> --publish`. |
| Registry publish | Re-run the publish job. The tag and release stay; the tool does not touch registries. Dependents were not published (the publish runs in dependency order and stops at the first failure); the re-run picks them up, or dispatch the workflow with `package=<dependent>` once the dependency is on the registry. |
| The commit itself is broken | `verwalter release --abandon <tag>` removes the release and the tag. Add a changeset and release the next version. |
| Release PR missing or stale | `verwalter pr`. The branch is regenerated, the PR updated in place. |
| Push rejected with "directory file conflict" | A branch named like the prefix (`release`) exists. Rename it or set `release_branch_prefix`. |
| rc PR missing or stale | `verwalter pre-release <name> --target <branch>`. |

Running locally needs push rights and `gh auth login`. Fetch first so tags
and `origin/<base>` are current.

## Commands

| Command | What it does |
|---|---|
| `verwalter status` | Per package: version, tag, GitHub release, pending changesets, open PRs, next step. |
| `verwalter plan` | Print the release plan: next versions, dependencies first. |
| `verwalter branches [--base <ref>]` | Create the release branch locally. |
| `verwalter pr [--remote origin]` | Push the branch and open or update the PR. |
| `verwalter release [--asset <file>]... [--package <name>] [--publish]` | Tag and create GitHub releases for every untagged version. Uploads assets to drafts or a named package, `--publish` flips a draft. |
| `verwalter release --abandon <tag>` | Delete a GitHub release and its tag. |
| `verwalter check --pr <n>` | Changeset check for a contributor PR, with comment. |
| `verwalter changeset [--level <l>] [--summary <text>]` | Write a changeset for the packages the branch changes and no pending changeset covers. The summary defaults to the last commit subject. |
| `verwalter pre-release <package> [--target <branch>] [--level <l>]` | Open an rc PR for one package. Changesets stay pending. |

All commands take `--config <file>` and an optional checkout path. `pr`,
`release`, `pre-release` and `check` take `--dry-run`: every push, tag and
GitHub write is printed instead of done (the action input is `dry-run: true`).
Local release branches are still regenerated.
