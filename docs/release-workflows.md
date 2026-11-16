# Rules for release workflows

These rules keep a release with several jobs, workflows or repositories
from ending incomplete. They hold for any release tooling.
[examples/workflows/release.yml](../examples/workflows/release.yml)
follows them. The failures behind them are in
[release-pitfalls.md](release-pitfalls.md).

Read the verwalter documents at the version that the repository pins.
The behavior of a command can differ between versions.

- **One run owns one release.** Build, tag, release and publish are
  jobs of one workflow run, linked with `needs`. A failure in one job
  stops the next, and the run is red.
- **Pass the identity. Never look it up.** A later job gets the tag
  and version from a job output or a `workflow_call` input. In
  verwalter, that is the `releases` output of the `release` step. A
  job that searches for "the latest draft" or "the newest tag" races
  with the job that creates it.
- **An empty value is an error, not a skip.** If a step resolves a
  tag and gets nothing, it must exit non-zero. An `if:` that skips the
  upload on an empty tag turns a failure into a green run. If one
  workflow both builds on a push and releases on a call, give it an
  explicit mode input. In release mode, an empty tag fails.
- **Call, do not chain by event.** Inside one repository, use a
  reusable workflow (`workflow_call`) for a shared build. The caller
  waits, gets outputs, and fails when the callee fails. A workflow
  started by a tag, by `release: published`, by `workflow_run` or by
  `gh workflow run` runs apart: nothing waits for it and nothing
  checks its result. Its failure is red in a run that nobody watches.
  Some registries need a run of their own, for example pub.dev needs
  a run on the tag. Then the job that starts the run also waits for
  it: `gh run watch <id> --exit-status`. `gh workflow run` may not
  print the id of the run. The job then finds the run by its ref and
  its start time. Before the job dispatches, it looks for a run that
  is in progress for the same tag, and waits for that one. The wait
  has a timeout and survives one failed poll.
- **A reusable workflow gets nothing by default.** Pass each secret,
  or use `secrets: inherit`. Give the calling job `id-token: write`
  for trusted publishing. Registries match the workflow file name in
  the trusted publisher configuration, so read the registry's rule for
  reusable workflows before you move a publish step into one.
- **Public comes last.** The release stays a draft until every asset
  is uploaded and every registry has the version. If several jobs
  upload assets, one job after them checks the list of required assets
  and publishes. No job publishes on its own. If the assets are built
  after the tag, the order with verwalter is: the `release` job makes
  the draft, the build jobs follow, and a last job runs
  `verwalter release --package <name> --asset <dir> --publish`.
  A version number on the default branch is public too. If a consumer
  reads the version from a file there, the merge announces the release
  before the assets exist.
- **Each job is safe to run again.** Upload with `--clobber`, skip a
  version the registry has, and keep a `workflow_dispatch` trigger
  with a `package` input for recovery. Skip a step only when its
  result is complete, never on a flag that stands for it. A release
  that is public but has no assets still needs its upload. Complete
  is a fact per target: the assets, and each registry. Before each
  publish, ask that registry for the version. One flag for a chain of
  publishes makes a repair run green with nothing done.
- **Never cancel a publish.** A concurrency group on the release
  workflow sets `cancel-in-progress: false`. A cancelled publish is a
  half publish. No group at all also passes. A group holds one
  running and one waiting run. A third run cancels the waiting one,
  also with `cancel-in-progress: false`. So a group for release runs
  has the tag or the PR number in its name.
- **One writer per release.** Each workflow that writes to a release
  has a concurrency group with the tag in its name. Two runs for one
  tag then go one after the other. Without the group, the second run
  replaces assets on a release that is already public. The rule holds
  for a workflow that another one dispatches too. If the workflow
  also runs without a tag, for example as a build on each push, those
  runs get a group of their own: `inputs.tag_name || github.run_id`.
  With `github.ref` as the fallback, all pushes to one branch share a
  group.
- **Pin the commit.** Every job checks out the merge commit of the
  release PR, not the branch head. A push after the merge must not
  move the tag. Recovery follows the same rule. "Re-run failed jobs"
  on the first run keeps the event and the commit. A new
  `workflow_dispatch` run checks out the branch head, and a publish
  from there uploads newer source under the released version. Give a
  dispatch a `ref` input, as the example workflow does. Set it to the
  merge commit or to any tag of that release. A later release run is
  a recovery run too: verwalter reports a draft on every run until it
  is public. A job that finishes such a draft checks out the tag of
  that release, not the commit of the run.
- **A consumer checks before it builds.** A repository that downloads
  a release asset of another repository fails early, with the tag and
  the asset name in the message. The producer notifies consumers only
  after the release is public.
- **Steps after the publish cannot fail the release.** Deploys, docs
  and notifications run in a later job with `continue-on-error`. A
  publish to a second registry is not such a step. It is part of the
  release, and its failure must be red.

## Recovery

A recovery run may change how the release is done, never what is
released. The content of a version comes from the commit of its tag.
First find the cause of the failure. It decides the path.

| Cause | Path |
|---|---|
| Transient: a runner, the network, a registry that was down | Re-run the failed jobs of the same run: `gh run rerun <run-id> --failed`. The run keeps its event, its inputs and its commit. Only the failed jobs and the jobs after them run again. GitHub permits this for 30 days. |
| The workflow file was wrong: a missing secret, a wrong setup step | A re-run uses the old workflow file. Merge the fix, then start a new run by hand with the tag: `gh workflow run <file> -f ref=<tag>`. The workflow file comes from the branch, and the source comes from the tag. |
| The released commit is wrong: the source does not build or the package is broken | Do not publish a fixed copy of that version. If no registry has the version, remove the release and the tag (`verwalter release --abandon <tag>`). Then merge the fix with a changeset and release the next version. |

A workflow supports these paths when it follows three rules above.
"Each job is safe to run again" makes a second run harmless. "Pin the
commit" makes a run by hand build the tag. "One writer per release"
makes a second run for the same tag wait.

Two limits:

- A run by hand with the tag reads the release configuration from the
  tag too. A fix to a publish command in `verwalter.toml` does not
  reach that run.
- A run by hand builds every job again. A re-run builds only what
  failed.

What to run for each failed step of a verwalter release is in
[features.md](features.md#when-a-step-fails).
