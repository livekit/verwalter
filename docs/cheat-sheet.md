# Cheat sheet

The command for each situation, with a link to the full document.
[commands.md](commands.md) has every command and flag.

## Before you run a command locally

```sh
gh auth login          # once
git fetch --tags       # each time: the tags number the releases
```

`plan` works offline. Every other command needs the `origin` remote.
`pr`, `release`, `pre-release`, `publish` and `check` take `--dry-run`:
each push, tag and GitHub write is printed and not done.

## You changed a package

| Situation | Command |
|---|---|
| Add a changeset for the packages that the branch changes | `verwalter changeset --level patch --summary "Fix the reconnect timer"` |
| Add a changeset for named packages only | `verwalter changeset --level minor --package <name> --summary "<text>"` |
| The PR needs no release | Add the `internal` label to the PR. |
| See what the check says about a PR | `verwalter check --pr <n> --dry-run` |

The level follows the public API: `patch` for a fix, `minor` for an
addition, `major` for a break. On a 0.x package, a break is `minor`
and an addition is `patch`. A dependent needs no changeset.

## You want to know the state

| Situation | Command |
|---|---|
| Version, tag, GitHub release, registry and next step per package | `verwalter status` |
| What the next release contains | `verwalter plan` |
| What a command writes, without the writes | `verwalter <command> --dry-run` |

## You release

| Situation | Command |
|---|---|
| Release | Merge the release PR. The release workflow does the rest. |
| The release PR is missing or stale | `verwalter pr` |
| A release PR into a maintenance branch | Set `[git] base_branch` on that branch. See [action.md](action.md#the-workflows). |
| An rc of everything with a pending changeset | `gh workflow run pre-release.yml --ref <branch>`, or locally `verwalter pre-release --target <branch>` |

Never edit a version or the release PR by hand. A fix goes to `main`
with its own changeset.

## A release failed

First find the cause. It decides the path. The paths are in
[release-workflows.md](release-workflows.md#recovery).

| Cause | Command |
|---|---|
| Transient: a runner, the network, a registry | `gh run rerun <run-id> --failed` |
| The workflow file was wrong, and the fix is on `main` | `gh workflow run release.yml -f ref=<tag>` |
| One package must go through the publish job again | `gh workflow run release.yml -f package=<name> -f ref=<tag>` |
| The released commit is wrong, and no registry has the version | `verwalter release --abandon <tag>`, then a changeset and the next version |

One step failed and you repair it locally:

| What failed | Command |
|---|---|
| Tag push | `verwalter release --package <name>` |
| GitHub release creation | `verwalter release --package <name>` |
| Asset upload | `verwalter release --package <name> --asset <file-or-dir>` |
| A draft never became public | `verwalter release --package <name> --publish` |
| Registry publish | Re-run the publish job. A version that the registry has is skipped. |

Each step is in [features.md](features.md#when-a-step-fails).

## You look at a run

| Situation | Command |
|---|---|
| The runs of the release workflow | `gh run list --workflow release.yml` |
| The result per job. A skipped job counts as a success. | `gh run view <run-id> --json jobs --jq '.jobs[] \| "\(.name) \(.conclusion)"'` |
| The log of the failed steps | `gh run view <run-id> --log-failed` |
| The release, its draft flag and its assets | `gh release view <tag> --json isDraft,assets` |
