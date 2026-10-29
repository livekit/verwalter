# The GitHub Action

```yaml
- uses: lukasIO/verwalter@v0.14.1
  with:
    command: pr        # check | changeset | plan | pr | release | publish | pre-release | status
    token: ${{ secrets.RELEASE_TOKEN || github.token }}
    dry-run: "false"   # "true" prints every push, tag and GitHub write instead
    args: ""           # extra arguments, for example "--asset dist/x.tar.gz"
```

| Input | Meaning |
|---|---|
| `command` | The command to run. Required. |
| `pr` | PR number for `check`. Defaults to the event's PR. |
| `token` | Token with `contents` and `pull-requests` write. `github.token` by default. |
| `version` | verwalter release to download. Defaults to the version the action was checked out at. |
| `dry-run` | `"true"` passes `--dry-run`. Reads still run, writes are printed. |
| `args` | Extra arguments passed to the command. |

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
| `release.yml` | `pull_request` closed, merged, head `release/*`, author `github-actions[bot]`, body with the verwalter marker | `release`, then `publish` |

An rc workflow is one `workflow_dispatch` job. It runs
`pre-release <package> --level <l> --target ${{ github.ref_name }}` with
`fetch-depth: 0`, because the tags number the rc.

Repository settings the workflows need:

- **Allow GitHub Actions to create and approve pull requests** (Settings →
  Actions → General), or a token of a GitHub App or PAT in `token`. That
  token also lets the release PR trigger CI. `github.token` cannot.
- A label named `internal` (config: `exempt_label`) for PRs that need no
  release.
- "Automatically delete head branches" is fine. The next push regenerates
  the release branch.
