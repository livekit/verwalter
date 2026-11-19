# Rules for release workflows

These rules keep a release with several jobs, workflows or repositories
from ending incomplete. They hold for any release tooling. Each rule
has a reason and a check. The failures behind them are in
[release-pitfalls.md](release-pitfalls.md).

[examples/workflows/release.yml](../examples/workflows/release.yml) is
the starting point.

Three terms:

- **Publish**: upload a version to a registry.
- **Make public**: change a GitHub release from draft to public.
- **Release run**: the workflow run that does both for one release.

## The structure of the run

- **One run owns one release.** Build, tag, release and publish are
  jobs of one workflow run, linked with `needs`.
  - Why: a failure in one job stops the next, and the run is red.
  - The trigger covers every branch that can release. An rc PR or a
    maintenance release can target another branch than `main`. A
    `branches` filter on the trigger then starts no run for it: the
    branch has the version, but no tag and no publish. Let the
    condition on the first job decide which PRs release.
  - Check: can you reach every job of the release from one run page?

- **Call, do not chain by event.** A shared build is a reusable
  workflow (`workflow_call`), called as a job.
  - Why: the caller waits, gets outputs, and fails when the callee
    fails. A workflow started by a tag, by `release: published`, by
    `workflow_run` or by `gh workflow run` runs apart. Its failure is
    red in a run that nobody watches.
  - Exception: a registry that needs a run of its own. pub.dev accepts
    only a run on a tag. The job that starts such a run also waits for
    it. See [Start a run and wait for it](#start-a-run-and-wait-for-it).
  - Check: search the workflows for `gh workflow run`, `workflow_run`,
    and `on:` with `release` or tags.

- **Pass the identity. Never look it up.** A later job gets the tag
  and version from a job output or a `workflow_call` input. In
  verwalter, that is the `releases` output of the `release` step.
  - Why: a job that searches for "the latest draft" or "the newest
    tag" races with the job that creates it.
  - Check: search for `gh release list`, `git describe` and
    `git tag --sort` in release jobs.

- **A lookup that finds nothing is an error.** A step that resolves a
  tag, a release or a run and gets nothing exits non-zero.
  - Why: an `if:` that skips the upload on an empty value turns a
    failure into a green run.
  - Not an error: the producer says that there is nothing to do, with
    an explicit output such as `released == 'false'`.
  - If one workflow both builds on a push and releases on a call, the
    tag input decides the mode. With a tag, every later lookup must
    succeed.
  - Check: for each `if:` on a release job, name who sets the value
    and what an empty value means.

- **A reusable workflow gets nothing by default.** Pass each secret,
  or use `secrets: inherit`. Give the calling job the permissions that
  the called jobs need, such as `id-token: write`.
  - Trusted publishing matches a workflow file, and each registry has
    its own rule for a called workflow:
    - PyPI does not accept a reusable workflow as the trusted
      publisher. The publish job stays in the top workflow.
    - npm checks the name of the calling workflow, not of the file
      that runs `npm publish`. Both need `id-token: write`.
    - For any other registry, read its rule before you move a publish
      step.
  - Check: after a move, does the trusted publisher entry still name
    the right file?

## What becomes public, and when

- **Public comes last.** The release stays a draft until every asset
  is uploaded and every registry has the version. Exactly one job
  makes the release public, after all jobs that upload.
  - Why: a consumer that sees a public release expects all of it.
  - If the assets are built after the tag, the order with verwalter
    is: the `release` job makes the draft, the build jobs follow, and
    a last job runs
    `verwalter release --package <name> --asset <dir> --publish`.
  - verwalter enforces the asset part with `release.required_assets`
    on the package. `release --publish` and `publish` do not make the
    release public while a listed name is missing. They fail and list
    the missing names, and the release stays a draft.
  - A version number on the default branch is public too. If a
    consumer reads the version from a file there, the merge announces
    the release before the assets exist.
  - Check: list everything that a consumer reads. For each item, when
    does it become visible, and is the release complete then?

- **Check the end state.** The last job of the run asserts the result:
  the tag exists, the release is public, each required asset is there,
  and each registry has the version.
  - Why: this job fails on every silent skip, whatever caused it.
  - With verwalter, the last job is one step: `verwalter status --check`.
    The tag must be on the remote. The release must be public and
    have each name in `release.required_assets`. The registry must
    have the version. A further place that gets the version, such as
    a second registry, is an entry in `distributions` of the package:
    the same step then asks it too.
    See [Check the end state](#check-the-end-state).
  - Check: delete one upload step in your head. Does the run go red?

- **A consumer checks before it builds.** A repository that downloads
  a release asset of another repository fails early, with the tag and
  the asset name in the message.
  - Why: a build that fails late on a missing download looks like a
    build problem.
  - The producer notifies consumers only after the release is public.

- **Steps after the publish get their own job.** Deploys, docs and
  notifications run in a job after the publish job.
  - Why: their failure makes the run red, but the release jobs stay
    green, so it is clear that the release is complete.
  - Do not use `continue-on-error` for them: it hides the failure.
  - A step that writes to one shared place, such as a docs site with
    one version, checks three facts first: this run released the
    package that the place belongs to, the version is not a
    pre-release, and the run is for the main line. Read these from the
    fields of the handoff, such as `package` and `prerelease` in
    `releases`. Do not search the handoff for a substring: after an
    rc, `previous_version` of the stable release contains `-rc.`.
  - A publish to a second registry is not such a step. It is part of
    the release.

## Runs that repeat

- **Each job is safe to run again.** Upload with `--clobber`, and
  skip a version that the registry has.
  - Why: every recovery path runs a job a second time.
  - Skip a step only when its result is complete, never on a flag that
    stands for it. A release that is public but has no assets still
    needs its upload.
  - Complete is a fact per target: the assets, and each registry.
    Before each publish, ask that registry for the version. One flag
    for a chain of publishes makes a repair run green with nothing
    done.
  - Check: for each job, what happens on the second run?

- **One writer per release.** A job that writes to a release or to a
  registry has a concurrency group that names the release, with
  `cancel-in-progress: false`.
  - Why: a second run replaces assets on a release that is already
    public. A cancelled publish is a half publish.
  - Put the group on the job, not on the workflow. A job group can use
    `needs` and `inputs`, so it can name the tag. A workflow group is
    evaluated when the run starts and can use only `github`, `inputs`
    and `vars`.
  - Never use a constant group, or a group per branch, on a workflow
    that also starts for other events. By default, a group holds one
    running and one waiting run. A third run cancels the waiting one,
    also with `cancel-in-progress: false`. A closed pull request or a
    push can then cancel a waiting release. `queue: max` keeps every
    waiting run.
  - A registry with a moving pointer needs one group for all
    versions. npm moves `latest` to the version that was published
    last. With a group per tag, the publish jobs of two versions
    overlap, and the older one can finish last and take `latest`. Give
    the publish job one group per registry, with `queue: max`. A
    waiting job checks out after the running one ends, so it sees the
    newer tag. verwalter then gives the older version another
    dist-tag. See [One publish at a time](#one-publish-at-a-time).
  - A run without a tag, such as a build on each push, gets a group of
    its own: `inputs.tag_name || github.run_id`.
  - Check: start the workflow twice for one tag in your head. Does the
    second run wait?

- **Pin the commit.** Every job checks out the merge commit of the
  release PR, not the branch head.
  - Why: a push after the merge must not change what is released.
  - A run by hand checks out the branch head. Give it a `ref` input,
    as the example workflow does, and set it to the merge commit or to
    any tag of that release.
  - `verwalter publish` fails a package whose files changed since its
    tag, and the reason names `ref=<tag>`. `verwalter release --asset`
    does the same before it uploads to a tag from an earlier run.
    Neither compares files outside the package path, such as a root
    lock file, so pin the commit anyway.
  - A later release run can finish an older release: verwalter reports
    a draft on every run until it is public. A job that finishes such
    a draft checks out the tag of that release, not the commit of the
    run.
  - Check: for each checkout in a release job, which commit does it
    use on a merge, on a re-run, and on a run by hand?

- **Guard the run by hand.** A `workflow_dispatch` trigger on a
  release workflow lets anyone with write access publish from any
  branch.
  - Why: the guard on the release PR does not apply to this trigger.
  - Name who can do it. An environment with required reviewers on the
    publish job is one guard. It then asks for approval on every
    release.
  - Check: who can start the workflow, and from which refs?

- **Rehearse the whole chain.** A dry run goes through every job and
  every workflow of the release, and skips only the writes.
  - Why: the link that a dry run skips is the link that fails on the
    release.
  - With verwalter, `dry-run: "true"` on the action prints each write.
  - Check: which jobs does a dry run skip?

## Patterns

These examples show the shape. Test them in your repository.

### Start a run and wait for it

For a registry that needs a run of its own. The job needs
`actions: write`.

```yaml
- name: Start the publish run on the tag and wait for it
  timeout-minutes: 60
  env:
    GH_TOKEN: ${{ github.token }}
    TAG: ${{ needs.release.outputs.tag }}
  run: |
    set -euo pipefail
    running() {
      gh run list --workflow publish-on-tag.yml --branch "$TAG" \
        --json databaseId,status \
        --jq 'map(select(.status != "completed")) | .[0].databaseId // empty'
    }
    # A run for this tag that is still going: wait for it, start no second one.
    run_id=$(running)
    if [ -z "$run_id" ]; then
      gh workflow run publish-on-tag.yml --ref "$TAG" -f tag_name="$TAG"
      for _ in $(seq 1 30); do
        sleep 5
        run_id=$(running)
        [ -n "$run_id" ] && break
      done
    fi
    if [ -z "$run_id" ]; then
      echo "::error::the run for $TAG did not start"
      exit 1
    fi
    # The second call covers one failed poll.
    gh run watch "$run_id" --exit-status || gh run watch "$run_id" --exit-status
```

`gh workflow run` prints no run id. The lookup by workflow and tag is
the one lookup that these rules permit. The started workflow has its
own group for the tag, and it skips a version that the registry has.

### One writer per release

```yaml
jobs:
  upload:
    needs: build
    concurrency:
      group: release-assets-${{ inputs.tag_name || github.run_id }}
      cancel-in-progress: false
```

### One publish at a time

For a registry with a moving pointer, such as `latest` on npm.

```yaml
jobs:
  publish:
    needs: release
    concurrency:
      group: npm-publish
      cancel-in-progress: false
      queue: max
    steps:
      # The checkout is a step of this job. It runs after the wait,
      # so it sees the tags of a release that finished in between.
      - uses: actions/checkout@v7
        with:
          ref: ${{ github.event.pull_request.merge_commit_sha || inputs.ref || github.sha }}
          fetch-depth: 0
```

### Pin a run by hand

```yaml
on:
  workflow_dispatch:
    inputs:
      ref:
        description: Commit or tag to build and publish. Empty is the branch head.
        default: ""
jobs:
  release:
    steps:
      - uses: actions/checkout@v7
        with:
          ref: ${{ github.event.pull_request.merge_commit_sha || inputs.ref || github.sha }}
```

### Check the end state

```yaml
verify:
  needs: publish
  runs-on: ubuntu-latest
  permissions:
    contents: write # a draft release is visible only with write access
  steps:
    - uses: actions/checkout@v7
      with:
        ref: ${{ github.event.pull_request.merge_commit_sha || inputs.ref || github.sha }}
        fetch-depth: 0
    - uses: livekit/verwalter@v0.22.0
      with:
        command: status
        args: --check
    # A registry that verwalter does not handle gets one request of its
    # own here, such as: npm view "my-package@<version>" version
```

The required assets are the names in `release.required_assets` of each
package. The step prints the status of every package, then one line
for each package that is not completely released.

## Recovery

A recovery run can change how the release is done. It never changes
what is released. The content of a version comes from the commit of its tag.
First find the cause of the failure. It decides the path.

| Cause | Path |
|---|---|
| Transient: a runner, the network, a registry that was down | Re-run the failed jobs of the same run: `gh run rerun <run-id> --failed`. The run keeps its event, its inputs and its commit. Only the failed jobs and the jobs after them run again. GitHub permits this for 30 days after the first run. The build artifacts of the first run must still exist. |
| The workflow file was wrong: a missing secret, a wrong setup step | A re-run uses the old workflow file. Merge the fix, then start a new run by hand with the tag: `gh workflow run <file> -f ref=<tag>`. The workflow file comes from the branch, and the source comes from the tag. Every job builds again. |
| The release configuration was wrong: a publish command in `verwalter.toml` | A run with `ref=<tag>` reads the old configuration from the tag. Merge the fix, then start a run by hand from the branch head. verwalter compares the package's files with the tag. If they changed, `publish` and `release --asset` fail and name the tag. Then release the next version. |
| The released commit is wrong: the source does not build or the package is broken | Do not publish a fixed copy of that version. If no registry has the version, remove the release and the tag (`verwalter release --abandon <tag>`). Then merge the fix with a changeset and release the next version. |

A workflow supports these paths when it follows three rules above.
"Each job is safe to run again" makes a second run harmless. "Pin the
commit" makes a run by hand build the tag. "One writer per release"
makes a second run for the same tag wait.

What to run for each failed step of a verwalter release is in
[features.md](features.md#when-a-step-fails).
