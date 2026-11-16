---
name: verwalter-release-debug
description: Debug a release that failed or came out incomplete, in any repository and with any release tooling, and fix the workflow with verwalter. Use this skill whenever a release run failed, a release run was green but something is missing (assets, a tag, a package on npm, crates.io, PyPI or pub.dev), a GitHub release stayed a draft, a downstream repository cannot download a release artifact, a publish stopped halfway, a workflow on a tag or release event never started, or the user asks how to structure release workflows, reusable workflows or workflow calls so that these failures cannot happen, even when they do not name verwalter.
---

# Debug a release, fix it with verwalter

A release fails in one of two ways. A step fails and the run is red.
Or a step is skipped, the run is green, and the release is incomplete.
The second way is found later, by a downstream repository or a user.
Always check for both.

The reference is the verwalter documentation:

- [docs/features.md](https://github.com/livekit/verwalter/blob/main/docs/features.md):
  what to run when a step fails, and what the tool does not cover.
- [docs/release-pitfalls.md](https://github.com/livekit/verwalter/blob/main/docs/release-pitfalls.md):
  the history of release failures across the LiveKit SDKs, per pattern.
- [docs/adopt.md](https://github.com/livekit/verwalter/blob/main/docs/adopt.md):
  the migration procedure. The `verwalter` skill covers it.
- [docs/release-workflows.md](https://github.com/livekit/verwalter/blob/main/docs/release-workflows.md):
  the rules for a release with several jobs, workflows or repositories.
- [examples/workflows/release.yml](https://github.com/livekit/verwalter/blob/main/examples/workflows/release.yml):
  the workflow shape that follows those rules.

## The procedure

1. **Find every run of the release.** One release often has several
   workflows. List them:
   `gh run list --repo <repo> --created <date> --json databaseId,workflowName,event,conclusion`.
2. **Read the jobs, not the run result.** A skipped job counts as a
   success:
   `gh run view <id> --json jobs --jq '.jobs[] | "\(.name) \(.conclusion)"'`.
   For a red run, read `gh run view <id> --log-failed`. For a skipped
   job, read the log of the step that produced its `if:` input.
3. **Compare the result with the intent.** Check each of these and
   write down what is missing:
   - the tag: `git ls-remote --tags origin <tag>`
   - the GitHub release, its draft flag and its assets:
     `gh release view <tag> --json isDraft,assets`
   - each registry: does it have the version?
   - in a verwalter repository: `verwalter status`
4. **Find the cause with evidence.** Name the step, the log line and
   the time. Match it against the table below. If two workflows touch
   the same release, compare their timestamps: a race shows there.
5. **Repair the release first.** Use what exists: re-run the failed
   job, or upload the built artifacts. Never retag a published version.
   Each repair writes to a registry or a public release, so show the
   command to the user and ask before you run it.
6. **Then fix the workflow.** Propose the change from the table and
   the rules. If the repository does not use verwalter, name the
   smallest fix in the current tooling and the verwalter fix, with a
   reason each. The user decides.
7. **Report.** Give the cause with its evidence, the repair, the
   workflow fix, and what you did not verify.

## Symptom, cause, fix

| Symptom | Usual cause | With verwalter |
|---|---|---|
| Run is green, the release has no assets or stays a draft | A separate workflow looked the release up before it existed. The lookup returned nothing and the upload job was skipped. | Build, `release --asset <dir>` and `publish` are jobs of one run, linked with `needs`. `publish` flips the draft and fails the job when it cannot. Repair: `verwalter release --package <name> --asset <dir> --publish`. |
| A downstream repository cannot download an asset | The release was public before its assets were complete. | A package that `publish` handles is a draft until the registry has the version. Assets go on when the release is created. |
| A workflow on a tag or on `release: published` never starts | The tag or release was made with `github.token`. GitHub starts no workflow for it. | Do the next step in the same workflow, or give the action a GitHub App. |
| A publish stopped halfway. Dependents on the registry point at versions that do not exist. | No dependency order, or one matrix leg failed. | `publish` goes in dependency order and blocks only the transitive dependents. Re-run the job. |
| A re-run fails with "version already exists" | The publish command is not safe to run twice. | `publish` asks the registry first and skips the versions it has. |
| The first release of a new package fails | Trusted publishing cannot create a package. | The release PR names a first release, and the command sees `VERWALTER_FIRST_RELEASE=true`. The first upload is by hand. |
| A package is missing from the release | It is not in the release configuration, or a changeset file did not parse. | `check` fails on both, on the pull request. |
| Tag and version file disagree | A version was edited by hand. | The tag comes from the files. A hand edit with no changeset is left alone with a warning. |
| An rc number is used twice | Shallow checkout: the tags are not there. | `fetch-depth: 0` in every workflow. |
| The run is red after the registry has the version | A step after the publish failed, such as a deploy or a notification. | Put such steps in their own job, after `publish`, with `continue-on-error`. |
| A release ran that nobody approved | The workflow trusts the branch name `release/*`. | The example runs only for a merged PR from the bot with the verwalter marker in its body. |

If the symptom is not in the table, read
[docs/release-pitfalls.md](https://github.com/livekit/verwalter/blob/main/docs/release-pitfalls.md).
If it is not there either, it is a gap. Report it as the `verwalter`
skill says.

## Rules for release workflows

When you fix a workflow or review one, apply every rule of
[docs/release-workflows.md](https://github.com/livekit/verwalter/blob/main/docs/release-workflows.md).
It covers job order, reusable workflows and workflow calls, drafts,
re-runs, and repositories that consume a release of another one. Name
each rule that the workflow breaks, with the line.
