# Rules for release workflows

These rules keep a release with several jobs, workflows or repositories
from ending incomplete. They hold for any release tooling.
[examples/workflows/release.yml](../examples/workflows/release.yml)
follows them. The failures behind them are in
[release-pitfalls.md](release-pitfalls.md).

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
  upload on an empty tag turns a failure into a green run.
- **Call, do not chain by event.** Inside one repository, use a
  reusable workflow (`workflow_call`) for a shared build. The caller
  waits, gets outputs, and fails when the callee fails. A workflow
  started by a tag, by `release: published` or by `workflow_run` runs
  apart: nothing waits for it and nothing checks its result.
- **A reusable workflow gets nothing by default.** Pass each secret,
  or use `secrets: inherit`. Give the calling job `id-token: write`
  for trusted publishing. Registries match the workflow file name in
  the trusted publisher configuration, so read the registry's rule for
  reusable workflows before you move a publish step into one.
- **Public comes last.** The release stays a draft until every asset
  is uploaded and every registry has the version. If several jobs
  upload assets, one job after them checks the list of required assets
  and publishes. No job publishes on its own.
- **Each job is safe to run again.** Upload with `--clobber`, skip a
  version the registry has, and keep a `workflow_dispatch` trigger
  with a `package` input for recovery.
- **Never cancel a publish.** A concurrency group on the release
  workflow sets `cancel-in-progress: false`. A cancelled publish is a
  half publish.
- **Pin the commit.** Every job checks out the merge commit of the
  release PR, not the branch head. A push after the merge must not
  move the tag.
- **A consumer checks before it builds.** A repository that downloads
  a release asset of another repository fails early, with the tag and
  the asset name in the message. The producer notifies consumers only
  after the release is public.
- **Steps after the publish cannot fail the release.** Deploys, docs
  and notifications run in a later job with `continue-on-error`.
