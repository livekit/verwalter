---
name: release-hardening
description: Harden how a repository releases, with any release tooling, so that a release cannot end failed or incomplete, and fix the weaknesses with verwalter. Use this skill whenever the user asks to review, audit, harden or restructure release or publish workflows, reusable workflows or workflow calls, asks whether a release process is reliable, or plans a release across several jobs, workflows or repositories. Use it also after a release went wrong (a red run, a green run with missing assets, a draft that stayed a draft, a publish that stopped halfway, a downstream repository that cannot download an artifact, a workflow on a tag that never started), because a failed release is a weakness found late, even when the user does not name verwalter.
---

# Harden a release procedure

A release procedure is hard when every failure is loud, every step is
safe to run again, and nothing is public before it is complete. Your
job is to find where a procedure is not hard, before a release finds
it. A failed release is the same job, started from the other end: one
weakness is known, and the others are still there.

The reference is the verwalter documentation:

- [docs/release-workflows.md](https://github.com/livekit/verwalter/blob/main/docs/release-workflows.md):
  the rules for a release with several jobs, workflows or
  repositories. They hold for any tooling.
- [examples/workflows/release.yml](https://github.com/livekit/verwalter/blob/main/examples/workflows/release.yml):
  a workflow that follows those rules.
- [docs/features.md](https://github.com/livekit/verwalter/blob/main/docs/features.md):
  the mistakes the tool catches, what to run when a step fails, and
  what the tool does not cover.
- [docs/release-pitfalls.md](https://github.com/livekit/verwalter/blob/main/docs/release-pitfalls.md):
  the history of release failures across the LiveKit SDKs.
- [docs/adopt.md](https://github.com/livekit/verwalter/blob/main/docs/adopt.md):
  the migration procedure. The `verwalter` skill covers it.

## The audit

1. **Map the release.** Read every workflow, script and written
   instruction that takes part in a release. Include the repositories
   that consume its artifacts. Draw the chain: what starts each step,
   what each step reads, what it writes, and who waits for it.
2. **Check every rule** of
   [docs/release-workflows.md](https://github.com/livekit/verwalter/blob/main/docs/release-workflows.md)
   against the chain. For each broken rule, write down the file and
   the line.
3. **Check every weakness** in the table below. The second column
   tells you what a release looks like when the weakness fires.
4. **Ask "what if" at each link of the chain.** What if this step
   runs twice? What if it runs before the step it depends on? What if
   it is skipped? What if it is cancelled halfway? An answer of
   "green run, incomplete release" is a finding.
5. **Read the history.** List the release runs of the last months and
   read the result per job, not per run. A skipped job counts as a
   success:
   `gh run view <id> --json jobs --jq '.jobs[] | "\(.name) \(.conclusion)"'`.
   A skipped upload or publish job in a green run is a finding that
   already fired.
6. **Rank and report.** Order the findings by what they cost: a
   release that is public and incomplete comes first, then a release
   that is stuck, then one that needs manual work. For each finding,
   give the file and line, how it fails, and two fixes: the smallest
   one in the current tooling, and the one with verwalter. Give a
   reason for each. The user decides.
7. **Prove each fix.** Recreate the failure and show that the new
   workflow handles it. Use `--dry-run`, a fork, or a test package. If
   you did not prove a fix, say so in the report.

## Weaknesses

| Weakness in the workflow | How it fails | With verwalter |
|---|---|---|
| A separate workflow looked the release up before it existed. The lookup returned nothing and the upload job was skipped. | Run is green, the release has no assets or stays a draft | Build, `release --asset <dir>` and `publish` are jobs of one run, linked with `needs`. `publish` flips the draft and fails the job when it cannot. Repair: `verwalter release --package <name> --asset <dir> --publish`. |
| The release was public before its assets were complete. | A downstream repository cannot download an asset | A package that `publish` handles is a draft until the registry has the version. Assets go on when the release is created. |
| The tag or release was made with `github.token`. GitHub starts no workflow for it. | A workflow on a tag or on `release: published` never starts | Do the next step in the same workflow, or give the action a GitHub App. |
| No dependency order, or one matrix leg failed. | A publish stopped halfway. Dependents on the registry point at versions that do not exist. | `publish` goes in dependency order and blocks only the transitive dependents. Re-run the job. |
| The publish command is not safe to run twice. | A re-run fails with "version already exists" | `publish` asks the registry first and skips the versions it has. |
| Trusted publishing cannot create a package. | The first release of a new package fails | The release PR names a first release, and the command sees `VERWALTER_FIRST_RELEASE=true`. The first upload is by hand. |
| A package is not in the release configuration, or a changeset file does not parse. | A package is missing from the release | `check` fails on both, on the pull request. |
| A version can be edited by hand. | Tag and version file disagree | The tag comes from the files. A hand edit with no changeset is left alone with a warning. |
| Shallow checkout: the tags are not there. | An rc number is used twice | `fetch-depth: 0` in every workflow. |
| A step after the publish, such as a deploy or a notification, can fail the run. | The run is red after the registry has the version | Put such steps in their own job, after `publish`, with `continue-on-error`. |
| The workflow trusts the branch name `release/*`. | A release ran that nobody approved | The example runs only for a merged PR from the bot with the verwalter marker in its body. |

If a weakness is not in the table, read
[docs/release-pitfalls.md](https://github.com/livekit/verwalter/blob/main/docs/release-pitfalls.md).
If it is not there either, it is a gap. Report it as the `verwalter`
skill says.

## After a failed release

1. **Find every run of the release.** One release often has several
   workflows:
   `gh run list --repo <repo> --created <date> --json databaseId,workflowName,event,conclusion`.
2. **Compare the result with the intent**, and write down what is
   missing:
   - the tag: `git ls-remote --tags origin <tag>`
   - the GitHub release, its draft flag and its assets:
     `gh release view <tag> --json isDraft,assets`
   - each registry: does it have the version?
   - in a verwalter repository: `verwalter status`
3. **Find the cause with evidence.** Name the step, the log line and
   the time. For a red run, read `gh run view <id> --log-failed`. For
   a skipped job, read the log of the step that produced its `if:`
   input. If two workflows touch the same release, compare their
   timestamps: a race shows there.
4. **Repair the release.** Use what exists: re-run the failed job, or
   upload the built artifacts. Never retag a published version. Each
   repair writes to a registry or a public release, so show the
   command to the user and ask before you run it.
5. **Run the audit.** The failure names one weakness. Start the audit
   from it and do not stop there. Say in the report which rule the
   failure broke.
