# verwalter

Changeset-based versioning for single packages and monorepos, in any language.
It bumps versions, writes changelogs, opens stacked release PRs, tags and
creates GitHub releases. Publishing to a registry stays in your workflow.

The behaviour is specified in [versioning.allium](versioning.allium).

## Install

Download a binary from the
[releases page](https://github.com/lukasIO/verwalter/releases), or build
from source:

```bash
cargo install --git https://github.com/lukasIO/verwalter --tag v0.5.0
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

2. On every push to `main`, `verwalter pr` opens or updates one release PR per
   package with pending changesets. Dependents get a `patch` bump. In a
   monorepo the PRs stack: the package others depend on is at the bottom, its
   dependents target its branch. Branches are regenerated on every run.

3. A maintainer merges a release PR. The workflow builds, then
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

[packages.livekit-protocol]
path = "livekit-protocol"
versioned_files = ["livekit-protocol/Cargo.toml"]
changelog = "livekit-protocol/CHANGELOG.md"
tag_template = "{package}-v{version}"      # default in a monorepo, v{version} with one package
release = "tag_and_release"                # none | tag | tag_and_release
draft = false
```

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
`pubspec.yaml`, `go.mod`, `pom.xml` and more.

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
- uses: lukasIO/verwalter@v0.5.0
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
| Release PR missing or stale | `verwalter pr`. Branches are regenerated, PRs updated in place. |
| Push rejected with "directory file conflict" | A branch named like the prefix (`release`) exists. Rename it or set `release_branch_prefix`. |
| rc PR missing or stale | `verwalter pre-release <name> --target <branch>`. |

Running locally needs push rights and `gh auth login`. Fetch first so tags
and `origin/<base>` are current.

## Commands

| Command | What it does |
|---|---|
| `verwalter status` | Per package: version, tag, GitHub release, pending changesets, open PRs, next step. |
| `verwalter plan` | Print the release plan: next versions and stack order. |
| `verwalter branches [--base <ref>]` | Create the release branches locally. |
| `verwalter pr [--remote origin]` | Push the branches and open or update the PRs. |
| `verwalter release [--asset <file>]... [--package <name>] [--publish]` | Tag and create GitHub releases for every untagged version. Uploads assets to drafts or a named package, `--publish` flips a draft. |
| `verwalter release --abandon <tag>` | Delete a GitHub release and its tag. |
| `verwalter check --pr <n>` | Changeset check for a contributor PR, with comment. |
| `verwalter pre-release <package> [--target <branch>] [--level <l>]` | Open an rc PR for one package. Changesets stay pending. |

All commands take `--config <file>` and an optional checkout path. `pr`,
`release`, `pre-release` and `check` take `--dry-run`: every push, tag and
GitHub write is printed instead of done (the action input is `dry-run: true`).
Local release branches are still regenerated.
