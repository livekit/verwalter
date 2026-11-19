<!--
Template for a repository that releases with verwalter. Copy it to the
repository root as RELEASING.md and fill in each <placeholder>. Keep
only what is specific to this repository. The commands that are the
same in every repository are in the verwalter cheat sheet. Delete a
section that does not apply, and delete this comment.

Change this file in the same pull request as the workflow it describes.
-->

# Releasing <repository>

This repository releases with [verwalter](https://github.com/livekit/verwalter).
The commands that are the same in every repository are in the
[verwalter cheat sheet](https://github.com/livekit/verwalter/blob/main/docs/cheat-sheet.md).
This file holds what is specific to this repository.

## Packages

| Package | Tag | Goes to |
|---|---|---|
| `<package>` | `<package>/v<version>` | <crates.io, npm, PyPI, pub.dev, GitHub release assets> |

## Workflows

| File | Starts when | Does |
|---|---|---|
| `changeset-check.yml` | A PR is opened or updated | Fails a PR that changes a package without a changeset. |
| `release-pr.yml` | A push to `main` | Keeps the release PR `release/main` current. |
| `release.yml` | The release PR merges | Tags, creates the GitHub releases, publishes. |
| `<other>.yml` | <called by `release.yml`, or run by hand> | <builds the assets, publishes to a second registry> |

## Release

1. Merge the release PR.
2. Watch the run of `release.yml`: `gh run list --workflow release.yml --limit 1`.
3. <Each step that stays manual, such as the first upload of a new package.>

A release is complete when:

- each tag of the release PR has a public GitHub release
- <each required asset is on the release: list the names>
- <each registry has the version>

## Pre-release

<`gh workflow run pre-release.yml --ref <branch>`, or "not used".>

## A release failed

Find the cause first. The paths by cause are in the
[recovery section](https://github.com/livekit/verwalter/blob/main/docs/release-workflows.md#recovery).

| Situation | Command |
|---|---|
| A job failed for a transient reason | `gh run rerun <run-id> --failed` |
| The workflow file needed a fix, and the fix is on `main` | `gh workflow run release.yml -f ref=<tag>` |
| <A build of this repository failed, such as the binaries> | <`gh workflow run <file> -f tag_name=<tag>`> |
| The released commit is wrong | `verwalter release --abandon <tag>`, then a changeset and the next version |

<What this repository cannot repair, and what to do then.>

## After a release

<The repositories that use this release, and what to do in each. For
example: update the submodule in `<consumer>`, or change the version in
`<consumer>/<file>`.>

## Who to ask

<A team or a channel, not one person.>
