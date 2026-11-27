# API check

The changeset level is the contributor's claim. An `api` command is the
oracle: a tool that compares the public API of a package against a
baseline. `verwalter check` runs it for every package that the PR changes
and a changeset covers, and fails the PR when the claim is too low.

```toml
[check]
api = "cargo semver-checks --baseline-rev $(git merge-base origin/${GITHUB_BASE_REF:-main} HEAD) 2>&1 | tee /dev/stderr | sed -n 's/.*requires new \\([a-z]*\\) version.*/\\1/p'"

[packages.livekit-rtc]
check.api = "griffe check livekit.rtc -a $(git merge-base origin/${GITHUB_BASE_REF:-main} HEAD) >/dev/null || echo major"   # overrides [check].api

[packages.livekit-ffi]
check.api = false   # no API check for this package
```

## The contract

The command runs in the package's path. The environment carries
`VERWALTER_PACKAGE`, `VERWALTER_VERSION`, `VERWALTER_BASELINE_TAG`,
`VERWALTER_LEVEL` and `VERWALTER_ROOT`. The command prints the lowest kind
of change it can prove the diff needs, `major`, `minor` or `patch`, or
nothing when it cannot tell. A tool that only knows "breaking or not"
prints `major` or nothing. If the claimed level is below what the answer
needs, the check fails and the comment names the level to use.

## The baseline

Compare against the base of the PR, not against the last release tag.
This command prints that commit:

```sh
git merge-base origin/${GITHUB_BASE_REF:-main} HEAD
```

A changeset makes a claim for the changes of its own PR. Against the
tag, a breaking change that is on `main` and not released yet counts
against each later PR for the same package. Each of those PRs then needs
a `major` changeset for a change that it did not make. The merge base
holds only the changes of the PR itself.

GitHub sets `GITHUB_BASE_REF` to the base branch of the PR, so a PR to a
release branch compares against that branch. Outside a `pull_request`
run, the command falls back to `main`. `VERWALTER_BASELINE_TAG` holds the
tag of the current version, for a tool that can only compare against a
release.

## Commands that work today

What goes to stderr ends up in the PR comment. The `tee` keeps the
tool's own report there.

| Tool | Command | Answers |
|---|---|---|
| cargo-semver-checks | `cargo semver-checks --baseline-rev $(git merge-base origin/${GITHUB_BASE_REF:-main} HEAD) 2>&1 \| tee /dev/stderr \| sed -n 's/.*requires new \([a-z]*\) version.*/\1/p'` | `major`, `minor` |
| griffe | `griffe check <module> -a $(git merge-base origin/${GITHUB_BASE_REF:-main} HEAD) >/dev/null \|\| echo major` | `major` |
| break-check | `break-check --tagPattern="refs/tags/$VERWALTER_PACKAGE@" --pattern='tests/public/**/*' --testCommand='node --test tests/public/**/*.test.js' --certifyCommand=false 1>&2 \|\| echo major` | `major` |

break-check compares behaviour, not types: it restores the files that
match `--pattern` from the newest release tag that matches `--tagPattern`
and runs `--testCommand` against the current source. A restored test that
fails is a break. Its baseline is a release tag, not the merge base, so a
break on `main` that is not released counts against each later PR. The
tag pattern is a regular expression, so `@` after the package name keeps
`@play/core@` from matching `@play/core-extra@`.
When the baseline release has no public tests yet, break-check exits 2 and
`|| echo major` would claim a break. Skip it in that case:

```sh
git ls-tree -r --name-only "$VERWALTER_BASELINE_TAG" -- tests/public | grep -q . || exit 0
```

The tool must be on the runner before the action runs. The checkout needs
the full history, for the merge base and the tags:

```yaml
- uses: actions/checkout@v4
  with:
    fetch-depth: 0
- uses: taiki-e/install-action@cargo-semver-checks
- uses: livekit/verwalter@v0.32.0
  with:
    command: check
```

## Kinds and digits

A tool answers with a kind of change. A changeset names the digit that
moves. On a 1.x package the two agree. On a 0.x package the first non-zero
digit is the breaking one, so a breaking change needs `minor` and an
additive one `patch`. On 0.0.x everything is `patch`. verwalter does that
translation, so the tool's `major` never has to be rewritten in the
command. The comment then says, for example:

> `play-core`: the changeset declares `patch`, the API check requires
> `minor`. Run `verwalter changeset --level minor --package play-core` on
> the branch, or raise the level in the changeset.
