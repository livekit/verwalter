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
   - To find the consumers:
     `gh search code --owner <org> "<repo>/releases/download"`.
     Search also for the prefix of each asset name and for the name
     of each download script. A consumer can use a script of the
     producer through a submodule. For each consumer, write down what
     it reads to learn the version, and when that becomes visible. A
     consumer can learn the version from a registry and then download
     an asset.
   - To audit a pull request, read each workflow file whole, not only
     the diff. Get the head commit once, because the head can move
     during the audit: `gh pr view <n> --json headRefOid`. List the
     files:
     `gh api "repos/<repo>/contents/.github/workflows?ref=<sha>" --jq '.[].name'`.
     Read one:
     `gh api "repos/<repo>/contents/<path>?ref=<sha>" --jq .content | base64 -d`.
     Read the PR body too. If it accepts a weakness on purpose, check
     the stated reason against the code.
   - In a migration, also list what the old tooling started: workflows
     on its tags or releases, open drafts, the old release PR, and
     trusted publisher entries that name a workflow file.
2. **Check every rule** of
   [docs/release-workflows.md](https://github.com/livekit/verwalter/blob/main/docs/release-workflows.md)
   against the chain. Each rule has a "Check" line: apply it. For
   each broken rule, write down the file and the line. Read the
   verwalter documents at the version that the repository pins,
   because the behavior of a command can differ between versions.
3. **Check `RELEASING.md`** at the repository root against the chain.
   Each workflow file, input and command that it names must exist.
   Each package, registry and consumer of step 1 must be in it. A
   difference is a finding. If the file is missing, propose it from
   [examples/RELEASING.md](https://github.com/livekit/verwalter/blob/main/examples/RELEASING.md).
4. **Check every weakness** in the table below. The second column
   tells you what a release looks like when the weakness fires.
5. **Ask "what if" at each link of the chain.** What if this step
   runs twice? What if it runs before the step it depends on? What if
   it is skipped? What if it is cancelled halfway? What if someone
   runs it by hand later, from another commit? What if the next
   release starts while this one is incomplete? An answer of "green
   run, incomplete release" is a finding. So is "red, but in a run
   that nobody watches".
6. **Read the history.** List the release runs of the last months and
   read the result per job, not per run. Filter the list with
   `--workflow <file>` and `--branch <release branch>`: a workflow on
   `pull_request: closed` has a skipped run for every other PR. A
   release often spans several workflows. Match their runs by commit
   or by time. A skipped job counts as a success:
   `gh run view <id> --json jobs --jq '.jobs[] | "\(.name) \(.conclusion)"'`.
   A skipped upload or publish job in a green run is a finding that
   already fired. Then replay each bad state from the history through
   the new procedure, for example "public without assets" or "assets
   complete, one registry failed". Start the recovery run in your
   head and follow it job by job. Does it repair the state, or does
   it end green with nothing done?
7. **Rank and report.** Order the findings by what they cost: a
   release that is public and incomplete, or public with content from
   another commit, comes first. Then a release that is stuck, then
   one that needs manual work. Within a class, a
   finding that already fired in the history comes first. For each
   finding, give the file and line, how it fails, and two fixes: the
   smallest one in the current tooling, and the one with verwalter.
   In a pull request that migrates to verwalter, the current tooling
   is the workflows of that PR.
   Give a reason for each. If verwalter has no fix, write "none" and
   report it as a gap, as the `verwalter` skill says. Before you
   propose a fix, run it through every trigger of the workflow and
   through the repair path. A fix for the release run must not break
   the build on a push or the upload to a broken release. The user
   decides. If the user accepts a finding without a fix, keep it in
   the report with the reason.
8. **Prove each fix.** Recreate the failure and show that the new
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
| Shallow checkout: the tags are not there. | An rc number is used twice | `fetch-depth: 0` in the release, release PR and pre-release workflows. |
| A step after the publish, such as a deploy or a notification, can fail the run. | The run is red after the registry has the version | Put such steps in their own job, after `publish`. Its failure is red, and the release jobs stay green. |
| The workflow trusts the branch name `release/*`. | A release ran that nobody approved | The example runs only for a merged PR from the bot with the verwalter marker in its body. A `workflow_dispatch` trigger is a second door: anyone with write access can run it from any branch. Name who may do that. |
| One version goes to several registries and to release assets. | One registry fails after the release is public. A repair run ends green with nothing done, or fails on the registries that have the version. | `publish.registry` is one per package, and it decides when the draft becomes public. Each further registry is a job of the workflow that asks the registry for the version first. |

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
