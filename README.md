# verwalter

Changeset-based versioning for single packages and monorepos, in any
language. Contributors describe their change in a changeset file. The tool
keeps one release PR open. Merging it bumps versions, writes changelogs,
tags and creates GitHub releases. Publishing to a registry stays in your
workflow.

| verwalter does | verwalter does not |
|---|---|
| Read changesets, plan versions, dependencies first | Publish to crates.io, npm, PyPI, Maven or pub.dev |
| Bump versions in manifests, lock files and any text file by regex | Build anything |
| Write changelogs with PR and author links | Decide what a change is: the changeset says |
| Open one release PR and one rc PR per package on demand | Keep state outside git, tags and GitHub |
| Tag, create GitHub releases, upload assets, flip drafts | Touch a registry when a release is abandoned |
| Hand the workflow a list of what to publish, in order | Run on a forge other than GitHub |

The behaviour is specified in [versioning.allium](versioning.allium). The
release history of the LiveKit SDKs, and how each pitfall maps to the
tool, is in [docs/release-pitfalls.md](docs/release-pitfalls.md).

## The release cycle

```
PR with .changeset/x.md ──merge──▶ main ──verwalter pr──▶ release PR
                                                              │ merge
             your publish job ◀── releases output ◀── verwalter release
```

1. **Changeset.** A contributor adds `.changeset/<name>.md` to their PR, in
   the [changesets](https://github.com/changesets/changesets) format:

   ```md
   ---
   livekit-protocol: minor
   ---

   Negotiated data channels.
   ```

   `verwalter check` fails a PR that changes a versioned package without
   one. It comments with a prefilled link. `verwalter changeset` writes the
   file locally. The `internal` label exempts a PR.

2. **Release PR.** On every push to `main`, `verwalter pr` opens or updates
   one PR with every package that has a pending changeset. Dependents get a
   `patch` bump and ride along. Every run regenerates the branch
   `release/main` from `main`. It is never stale and never conflicts.

3. **Release.** A maintainer merges the PR. The workflow builds, then
   `verwalter release` tags each version and creates the GitHub release with
   the assets, dependencies first. Its `releases` output lists what to
   publish. The registry step comes last, because it cannot be undone.

4. **Pre-release, on demand.** `verwalter pre-release <package>` opens an rc
   PR from any branch, apart from the release PR. Merging it tags
   `1.6.0-rc.1`. The next request numbers `rc.2`. Changesets stay pending.
   The next final release promotes the rc and follows the pending level.

## Setup

### Install

Download a binary from the
[releases page](https://github.com/lukasIO/verwalter/releases), or build
from source:

```bash
cargo install --git https://github.com/lukasIO/verwalter --tag v0.13.0
```

### The action

```yaml
- uses: lukasIO/verwalter@v0.13.0
  with:
    command: pr        # check | changeset | plan | pr | release | pre-release | status
    token: ${{ secrets.RELEASE_TOKEN || github.token }}
    dry-run: "false"   # "true" prints every push, tag and GitHub write instead
    args: ""           # extra arguments, for example "--asset dist/x.tar.gz"
```

| Input | Meaning |
|---|---|
| `command` | The command to run. Required. |
| `pr` | PR number for `check`. Defaults to the event's PR. |
| `token` | Token with `contents` and `pull-requests` write. `github.token` by default. |
| `version` | verwalter release to download. Defaults to the version the action was checked out at. |
| `dry-run` | `"true"` passes `--dry-run`. Reads still run, writes are printed. |
| `args` | Extra arguments passed to the command. |

| Output (`release` only) | Meaning |
|---|---|
| `released` | `true` if at least one release was tagged, drafted or published. |
| `releases` | JSON list of `{package, previous_version, version, tag, prerelease, first_release, status, depends_on}`, dependencies first. |

### The workflows

[examples/workflows](examples/workflows) has the three workflows to copy,
and [examples/publish](examples/publish/README.md) has publish steps that
are safe to re-run:

| Workflow | Trigger | Command |
|---|---|---|
| `changeset-check.yml` | `pull_request` on `main` | `check` |
| `release-pr.yml` | `push` to `main` | `pr` |
| `release.yml` | `pull_request` closed, merged, head `release/*`, author `github-actions[bot]`, body with the verwalter marker | `release`, then your publish jobs |

An rc workflow is one `workflow_dispatch` job. It runs
`pre-release <package> --level <l> --target ${{ github.ref_name }}` with
`fetch-depth: 0`, because the tags number the rc.

Repository settings the workflows need:

- **Allow GitHub Actions to create and approve pull requests** (Settings →
  Actions → General), or a token of a GitHub App or PAT in `token`. That
  token also lets the release PR trigger CI. `github.token` cannot.
- A label named `internal` (config: `exempt_label`) for PRs that need no
  release.
- "Automatically delete head branches" is fine. The next push regenerates
  the release branch.

### Configuration

`verwalter.toml` at the repository root:

```toml
base_branch = "main"            # default
exempt_label = "internal"       # default
release_branch_prefix = "release/"
release_label = "release"
dependent_bump_level = "patch"
prerelease_label = "rc"          # 1.6.0-rc.1, written as 1.6.0rc1 in pyproject.toml
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

Dependencies between packages are read from the manifests, not configured.

**Versioned files.** Formats come from
[knope-versioning](https://crates.io/crates/knope-versioning): `Cargo.toml`,
`Cargo.lock`, `package.json`, `package-lock.json`, `pyproject.toml`,
`pubspec.yaml`, `go.mod`, `pom.xml` and more. `uv.lock` is handled by the
tool itself. `pnpm-lock.yaml` and `yarn.lock` need no edit. A `workspace:*`
or `catalog:` range stores no version of an internal package. Such
ranges are left alone. Any other text file takes a regex with a `version`
capture group. The regex wins over the file name, and every match moves:

```toml
versioned_files = [
  "pubspec.yaml",
  { path = "lib/src/livekit.dart", regex = "version = '(?<version>[^']+)'" },
  { path = "gradle.properties", regex = "VERSION_NAME=(?<version>.+)" },
  { path = "Cargo.lock", regex = 'name = "my-crate"\nversion = "(?<version>[^"]+)"' },
]
```

The first file of a package is its version.

## Features

| Feature | What it does | Where |
|---|---|---|
| Changeset check | Fails a PR that changes a package without a changeset. It posts one comment with a prefilled new-file link, updates it in place, and deletes it when the check passes. An invalid changeset is reported in the same comment. | `check`, `exempt_label` |
| Changeset command | Writes a changeset for the packages the branch changes and no pending changeset covers, or for named packages. | `changeset --level --package --summary` |
| One release PR | Every package with a pending bump goes in one PR. Each push regenerates the branch from `main` and updates the PR in place. With nothing pending, the PR is closed. | `pr`, `release_branch_prefix` |
| Dependent bumps | A package whose in-repo dependency is released gets a bump too, transitively. Its dependency floor moves in its manifest and the lock files. | `dependent_bump_level`, `pin_files` |
| Dependency order | Releases, tags and the handoff list go dependencies first. A failure stops before any dependent. | `release`, `depends_on` |
| Fixed groups | Members release at one version: the highest in the group, bumped by the highest level any member needs. A member without a changeset gets a "Released with" line. | `fixed` |
| Bump ceiling | Caps what a changeset can do to a package. A group takes the lowest cap. | `max_bump` |
| 0.x rule | `major` on `0.x` gives `1.0.0`, as changesets does. | |
| Pre-releases | An rc PR for one package, or its fixed group, from any branch. Numbering continues from the tags. The final release follows the pending level. It is never lower than the rc. | `pre-release`, `prerelease_label` |
| Draft releases | The release is created as a draft. A later job uploads the assets and flips it with `--publish`. It is reported until published. | `draft`, `release --publish` |
| Changelog | One entry per release with PR number and author, read from the commit that added each changeset. Multi-line summaries stay bullets. | `changelog` |
| Workspace check | A Cargo, pnpm or uv member without a private marker must be in the config. A missing one fails `check` and warns elsewhere. `publish.workspace = true` is resolved. | `ignore` |
| First release | A package with no tag is named in the PR body and flagged in the handoff. Trusted publishing cannot create a new package. | `first_release` |
| Abandon | Removes a GitHub release and its tag, local and remote. The registry is not touched. | `release --abandon <tag>` |
| Dry run | Every push, tag and GitHub write is printed instead of done. | `--dry-run`, action `dry-run` |
| Status | Per package: version, tag, release, pending changesets, open PRs, next step. | `status` |

## Failure paths

### Mistakes the tool catches

| Situation | What happens |
|---|---|
| Changeset names a package not in the config, or does not parse | `check` fails with the error in the PR comment. `pr` refuses to run until the file is fixed. |
| A package is added to the workspace but not to the config | `check` fails, every other command warns. |
| A version is edited by hand, no changeset | The release run leaves it alone with a warning. No tag and no changelog entry means no release. `release --package <name>` overrides. |
| Files of one package disagree on the version | Every command warns and names the files. The first file wins. The next release aligns them. |
| A fixed group drifts | Every command warns. The next release aligns the group. |
| A PR changes a package without a changeset | The check fails with the comment and link. The `internal` label exempts the PR. |
| Someone merges a branch named `release/*` by hand | The example release workflow runs only for a PR that the bot authored, with the verwalter marker in its body. |
| Two pushes to `main` in quick succession | The concurrency group cancels the first run. One PR results. |
| The release PR is closed without merging, or its branch deleted | The next push regenerates the branch and opens a new PR. |
| An rc is requested twice, or by another member of the group | One branch per package or group, updated in place. The number continues. |
| A GitHub release is deleted but the tag remains | The next release run recreates it from the changelog. |
| Two release runs race on the same tag | The run that loses the create sees the existing release and reports it. |

### When a step fails

Nothing is remembered between runs. The tool reads tags, releases and PRs
and finishes what is missing. Start with `verwalter status`. Add
`--dry-run` to any command to see its steps first.

| What failed | What to do |
|---|---|
| Build or test job before the tag | Fix, merge, re-run the workflow (`workflow_dispatch`). Nothing was written. |
| Tag push | `verwalter release --package <name>` tags and pushes. |
| GitHub release creation | `verwalter release --package <name> [--asset <file>...]` creates it from the existing tag. |
| Asset upload | `verwalter release --package <name> --asset <file>... [--publish]` uploads with `--clobber`. |
| Draft never published | `verwalter release --package <name> --publish`. |
| Registry publish | Re-run the publish job. The tag and release stay. The tool does not touch registries. The publish runs in dependency order and stops at the first failure, so dependents were not published. The re-run picks them up. Or dispatch the workflow with `package=<dependent>` once the dependency is on the registry. |
| The commit itself is broken | `verwalter release --abandon <tag>` removes the release and the tag. Add a changeset and release the next version. |
| Release PR missing or stale | `verwalter pr`. The branch is regenerated, the PR updated in place. |
| Push rejected with "directory file conflict" | A branch named like the prefix (`release`) exists. Rename it or set `release_branch_prefix`. |
| rc PR missing or stale | `verwalter pre-release <name> --target <branch>`. |

Running locally needs push rights and `gh auth login`. Fetch first so tags
and `origin/<base>` are current.

## Not covered

| Not covered | Why, and what to do instead |
|---|---|
| Registry publish and re-runs | The workflow's job. Ask the registry whether the version exists before publishing, and skip only then. Every other error must fail. Do not grep the error output: a conflict and a bad token look alike. Snippets per registry are in [examples/publish](examples/publish/README.md). |
| First upload of a new package under trusted publishing | No registry allows it. Upload the first version by hand. The PR body and `first_release` tell you when. |
| Snapshot or nightly releases from any commit | Not built. Use an rc PR, or a workflow that publishes under a dist tag with a version the tool never sees. |
| Cross-repo version coupling | A workflow precondition, not a tool concern. |
| Hotfix from a release branch | `pr --base <branch>` works. The changelog entry is not carried back to `main`. Backport it by hand. |
| rc numbering in a shallow checkout | Tags number the rc. Check out with `fetch-depth: 0`. |
| Downstream workflows triggered by tags or releases | Tags and releases made with `github.token` fire no `push` or `release` event. Use an App or PAT token. Or do the next step in the same workflow. |

## Commands

| Command | What it does |
|---|---|
| `verwalter status` | Per package: version, tag, GitHub release, pending changesets, open PRs, next step. |
| `verwalter plan` | Print the release plan: next versions, dependencies first. |
| `verwalter changeset [--level <l>] [--package <name>]... [--summary <text>]` | Write a changeset for the packages the branch changes and no pending changeset covers. With `--package`, exactly the named packages. The summary defaults to the last commit subject. |
| `verwalter check --pr <n>` | Changeset check for a contributor PR, with comment. |
| `verwalter branches [--base <ref>]` | Create the release branch locally. |
| `verwalter pr [--remote origin]` | Push the branch and open or update the PR. |
| `verwalter release [--asset <file>]... [--package <name>] [--publish]` | Tag and create GitHub releases for every untagged version that has a changelog entry. Uploads assets to drafts or a named package. `--publish` flips a draft. |
| `verwalter release --abandon <tag>` | Delete a GitHub release and its tag. |
| `verwalter pre-release <package> [--target <branch>] [--level <l>]` | Open an rc PR for one package and its fixed group. Changesets stay pending. |

All commands take `--config <file>` and an optional checkout path. `pr`,
`release`, `pre-release` and `check` take `--dry-run`. Every push, tag and
GitHub write is then printed instead of done. Local release branches are
still regenerated.
