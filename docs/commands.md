# Commands

| Command | What it does |
|---|---|
| `verwalter status [--check] [--json] [--package <name>]` | Per package: version, tag, GitHub release, pending changesets, open PRs, next step. `--check` prints the same, then exits non-zero for a package whose current version is not completely released. The tag must be on the remote. The GitHub release must be public and have its required assets. The registry must have the version, and so must each entry in `distributions` that is not optional. Pending changesets and an open release PR do not fail it. `--json` prints one JSON list in place of the table, with one object per package: `package`, `version`, `tag`, `tag_state`, `release`, `assets`, `missing_assets`, `on_registry`, `pending_changesets`, `next_version`, `release_pr`, `rc_pr`, `next_step`, `incomplete`. `incomplete` is `null` when the current version is completely released. `--package` limits the output and the check to one package. |
| `verwalter plan` | Print the release plan: next versions, dependencies first. |
| `verwalter changeset [--level <l>] [--package <name>]... [--summary <text>]` | Write a changeset for the packages the branch changes and no pending changeset covers. With `--package`, exactly the named packages. The summary defaults to the last commit subject. |
| `verwalter check --pr <n>` | Changeset and API check for a contributor PR, with comment. |
| `verwalter branches [--base <ref>]` | Create the release branch locally. |
| `verwalter pr [--remote origin]` | Push the branch and open or update the PR. |
| `verwalter release [--asset <file\|dir>]... [--package <name>] [--publish] [--allow-changed-source]` | Tag and create GitHub releases for every untagged version that has a changelog entry. Uploads assets to drafts or a named package. A directory stands for every file directly in it. `--publish` flips a draft. Assets for a tag from an earlier run are refused when the package's files changed since the tag, unless `--allow-changed-source` is set. |
| `verwalter release --abandon <tag>` | Delete a GitHub release and its tag. |
| `verwalter publish [--releases <json>] [--jobs <n>] [--allow-changed-source]` | Run each package's publish command for the packages in `--releases` (the `releases` output of `release`), or every tagged package without it or with an empty list, dependencies first, up to `n` at once. A failure blocks dependents. A version on the registry is skipped. A package whose files changed since its tag fails, unless `--allow-changed-source` is set. |
| `verwalter pre-release [--target <branch>]` | Open an rc PR for every package with a pending changeset and the fixed groups they are in, from `target`. Changesets stay pending. |

All commands take `--config <file>` and an optional checkout path
(default: the current git repository's root). `pr`,
`release`, `pre-release`, `publish` and `check` take `--dry-run`. Every push, tag and
GitHub write is then printed instead of done. Local release branches are
still regenerated.

There is no `--help` and no `--version`. The usage line prints on a
wrong argument.

## Running locally

Every command reads `verwalter.toml` in the checkout. `plan` works
offline. Every other command needs the `origin` remote, `gh auth login`
and a fresh `git fetch --tags`, because the tags number the releases
and the rcs. Without them, `status` reports `tag:local` and
`registry:no` for every package. That is the offline case, not drift.
