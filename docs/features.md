# Features and failure paths

What the tool does, what it catches, and what to do when a step fails.

## Features

| Feature | What it does | Where |
|---|---|---|
| Changeset check | Fails a PR that changes a package without a changeset. It posts one comment with a prefilled new-file link and updates it in place. On a passing PR the comment lists the bump each changeset claims, per package, and the dependents and fixed-group members that are released with them, so the reviewer sees the release the merge causes. It is deleted when the PR claims nothing. An invalid changeset is reported in the same comment. | `check`, `exempt_label` |
| API check | Runs a configured command (cargo-semver-checks, griffe, break-check) for every changed package that a changeset covers, and fails the PR when the changeset level is below what the command proves. 0.x versions are translated: a breaking change needs `minor`. | `[check].api`, `api` |
| Changeset command | Writes a changeset for the packages the branch changes and no pending changeset covers, or for named packages. | `changeset --level --package --summary` |
| One release PR | Every package with a pending bump goes in one PR. Each push regenerates the branch from `main` and updates the PR in place. With nothing pending, the PR is closed. | `pr`, `release_branch_prefix` |
| Dependent bumps | A package whose in-repo dependency is released gets a bump too, transitively. Its dependency floor moves in its manifest and the lock files. | `dependent_bump_level`, `pin_files` |
| Dependency order | Releases, tags and the handoff list go dependencies first. A failure stops before any dependent. | `release` |
| Publish scheduler | Runs each package's publish command once per tagged package, dependencies first, independent packages in parallel. A failure blocks its transitive dependents and nothing else. A version the registry has is skipped before the command runs. Nothing is remembered: a re-run repeats the command. | `publish`, `[publish]`, `registry`, `publish` per package |
| Workspace publish | One recursive command for every package on the default registry, run once from the root, with per-package reporting from the registry afterwards. | `[publish].workspace` |
| Registry check | One GET per registry answers whether a version exists. 200 skips, 404 publishes, anything else fails. `status` shows the answer. | `registry`, `exists_url` |
| Fixed groups | Members release at one version: the highest in the group, bumped by the highest level any member needs. A member without a changeset gets a "Released with" line. | `fixed` |
| 0.x rule | `major` on `0.x` gives `1.0.0`, as changesets does. | |
| Pre-releases | An rc PR for one package, or its fixed group, from any branch. Numbering continues from the tags. The final release follows the pending level. It is never lower than the rc. | `pre-release`, `prerelease_label` |
| Draft releases | The release is created as a draft. A later job uploads the assets and flips it with `--publish`. It is reported until published. | `draft`, `release --publish` |
| Changelog | One entry per release with PR number and author, read from the commit that added each changeset. Multi-line summaries stay bullets. | `changelog` |
| Workspace check | A Cargo, pnpm or uv member without a private marker must be in the config. A missing one fails `check` and warns elsewhere. `publish.workspace = true` is resolved. | `ignore` |
| First release | A package with no tag, or no version on its registry, is named in the PR body and flagged in the handoff. Trusted publishing cannot create a new package. | `first_release` |
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
| A changeset claims `patch` for a breaking change | With an `api` command configured, the check fails and the comment names the level to use and the fix command. |
| Someone merges a branch named `release/*` by hand | The example release workflow runs only for a PR that the bot authored, with the verwalter marker in its body. |
| Two pushes to `main` in quick succession | The concurrency group cancels the first run. One PR results. |
| The release PR is closed without merging, or its branch deleted | The next push regenerates the branch and opens a new PR. |
| An rc is requested twice, or by another member of the group | One branch per package or group, updated in place. The number continues. |
| A GitHub release is deleted but the tag remains | The next release run recreates it from the changelog. |
| Two release runs race on the same tag | The run that loses the create sees the existing release and reports it. |
| A publish fails for one package | Its transitive dependents are blocked and reported with the cause. Every other package publishes. The run exits non-zero. |
| A publish is re-run | Versions the registry has are skipped before the command runs. The rest publish. |
| The registry is down during a publish | A 503 is an error, not "not there yet". The package fails, its dependents are blocked. |

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
| The registry publish itself | Your command from `verwalter.toml`, run by `publish`. The tool checks existence first, so the command never greps its own errors: a conflict and a bad token look alike. Commands per registry are in [examples/publish](../examples/publish/README.md). |
| First upload of a new package under trusted publishing | No registry allows it. Upload the first version by hand. The PR body and `first_release` tell you when. |
| Snapshot or nightly releases from any commit | Not built. Use an rc PR, or a workflow that publishes under a dist tag with a version the tool never sees. |
| Cross-repo version coupling | A workflow precondition, not a tool concern. |
| Hotfix from a release branch | `pr --base <branch>` works. The changelog entry is not carried back to `main`. Backport it by hand. |
| rc numbering in a shallow checkout | Tags number the rc. Check out with `fetch-depth: 0`. |
| Downstream workflows triggered by tags or releases | Tags and releases made with `github.token` fire no `push` or `release` event. Use an App or PAT token. Or do the next step in the same workflow. |
