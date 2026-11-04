# The GitHub Action

```yaml
- uses: livekit/verwalter@v0.22.0
  with:
    command: pr        # check | changeset | plan | pr | release | publish | pre-release | status
    app-client-id: ${{ secrets.VERWALTER_RELEASE_CLIENT_ID }}
    app-private-key: ${{ secrets.VERWALTER_RELEASE_APP_KEY }}
    dry-run: "false"   # "true" prints every push, tag and GitHub write instead
    args: ""           # extra arguments, for example "--asset dist/x.tar.gz"
```

| Input | Meaning |
|---|---|
| `command` | The command to run. Required. |
| `pr` | PR number for `check`. Defaults to the event's PR. |
| `token` | Token with `contents` and `pull-requests` write. `github.token` by default. |
| `app-client-id`, `app-private-key` | A GitHub App. If set, the action makes an installation token for the repository and uses it in place of `token`. |
| `version` | verwalter release to download. Defaults to the version the action was checked out at. If the download fails, the action builds that checked-out version with `cargo`. |
| `dry-run` | `"true"` passes `--dry-run`. Reads still run, writes are printed. |
| `args` | Extra arguments passed to the command. |
| `releases` | `publish` only: the `releases` output of the `release` step. Only those packages are published. |

| Output (`release` only) | Meaning |
|---|---|
| `released` | `true` if at least one release was tagged, drafted or published. |
| `releases` | JSON list of `{package, previous_version, version, tag, prerelease, first_release, status}`, dependencies first. |

| Output (`publish` only) | Meaning |
|---|---|
| `published`, `failed`, `blocked` | JSON lists of package names. |
| `outcomes` | JSON list of `{package, version, tag, status, reason}`. |

## The workflows

[examples/workflows](../examples/workflows) has the three workflows to copy,
and [examples/publish](../examples/publish/README.md) has publish commands
per registry for `verwalter.toml`:

| Workflow | Trigger | Command |
|---|---|---|
| `changeset-check.yml` | `pull_request` on `main` | `check` |
| `release-pr.yml` | `push` to `main` | `pr` |
| `release.yml` | `pull_request` closed, merged, head `release/*`, author `github-actions[bot]` or the App (`vars.VERWALTER_RELEASE_BOT`, default `verwalter-release[bot]`), body with the verwalter marker | `release`, then `publish` |
| `pre-release.yml` | `workflow_dispatch` | `pre-release --target ${{ github.ref_name }}`, with `fetch-depth: 0` because the tags number the rc. The pending changesets decide what is in it and how far it moves. |

To ship releases from a maintenance branch such as `v1` next to `main`:

1. On `v1`, set `[git] base_branch = "v1"` in `verwalter.toml`.
2. On `v1`, add `v1` to the `branches` filter of `changeset-check.yml`, `release-pr.yml` and `release.yml`.
3. If a package publishes to npm with its own command, add `--tag latest-1` to that command on `v1`.

verwalter then keeps a `release/v1` PR into `v1`. A release from `v1` below
a version already tagged does not take GitHub's Latest badge, and the
built-in npm command keeps it off npm's `latest` dist-tag.

If the release binary cannot be downloaded, the action builds verwalter
from its own checkout instead. The build needs `cargo`, which GitHub-hosted
Linux and macOS runners have.

Repository settings the workflows need:

- **Allow GitHub Actions to create and approve pull requests** (Settings →
  Actions → General), or a GitHub App (see below) or a PAT in `token`.
- `pull-requests: write` for `check`, so it can comment on the PR. A PR from
  a fork gets a read-only token: the check then shows its result in the
  workflow log and the job summary only, and still passes or fails.
- A label named `internal` (config: `[check].exempt_label`) for PRs that need no
  release.
- "Automatically delete head branches" is fine. The next push regenerates
  the release branch.

## A GitHub App token

GitHub starts no workflows for pushes, tags, PRs and releases made with
`github.token`. So the release PR gets no CI, and a workflow that runs on
a tag or on `release: published` does not start. A GitHub App token
removes this limit. The action uses the token for the GitHub API and for
`git push`.

1. Create an App in the organization settings (Developer settings →
   GitHub Apps → New GitHub App). Name it `verwalter-release`, or set
   the org variable `VERWALTER_RELEASE_BOT` to its login, such as
   `my-app[bot]`: the example `release.yml` publishes only PRs from
   `github-actions[bot]` or that login. Clear **Webhook → Active**. Give it
   these repository permissions: Contents read and write, Pull requests
   read and write.
2. Under **Key pairs**, click **New key**. Keep the downloaded `.pem` file
   for step 4. A client secret is not needed.
3. Install the App on the repositories that release.
4. Add two organization secrets for those repositories: the App's client
   ID as `VERWALTER_RELEASE_CLIENT_ID`, the `.pem` file as
   `VERWALTER_RELEASE_APP_KEY`.
5. If a ruleset protects the base branch or tags, add the App to its
   bypass list.

The example workflows pass both secrets already. A PAT in `token` also
works, but it belongs to one person and expires.
