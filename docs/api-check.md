# API check

The changeset level is the contributor's claim. An `api` command is the
oracle: a tool that compares the public API of a package against its last
release. `verwalter check` runs it for every package that the PR changes
and a changeset covers, and fails the PR when the claim is too low.

```toml
[check]
api = "cargo semver-checks --baseline-rev $VERWALTER_BASELINE_TAG 2>&1 | tee /dev/stderr | sed -n 's/.*requires new \\([a-z]*\\) version.*/\\1/p'"

[packages.livekit-rtc]
api = "griffe check livekit.rtc -a $VERWALTER_BASELINE_TAG >/dev/null || echo major"   # overrides [check].api
```

## The contract

The command runs in the package's path. The environment carries
`VERWALTER_PACKAGE`, `VERWALTER_VERSION`, `VERWALTER_BASELINE_TAG`,
`VERWALTER_LEVEL` and `VERWALTER_ROOT`. The command prints the lowest kind
of change it can prove the diff needs, `major`, `minor` or `patch`, or
nothing when it cannot tell. A tool that only knows "breaking or not"
prints `major` or nothing. If the claimed level is below what the answer
needs, the check fails and the comment names the level to use.

## Commands that work today

What goes to stderr ends up in the PR comment. The `tee` keeps the
tool's own report there.

| Tool | Command | Answers |
|---|---|---|
| cargo-semver-checks | `cargo semver-checks --baseline-rev $VERWALTER_BASELINE_TAG 2>&1 \| tee /dev/stderr \| sed -n 's/.*requires new \([a-z]*\) version.*/\1/p'` | `major`, `minor` |
| griffe | `griffe check <module> -a $VERWALTER_BASELINE_TAG >/dev/null \|\| echo major` | `major` |
| break-check | `break-check detect --baseline <dir> --format json \| jq -r .recommendedBump` | `major`, `minor`, `patch` |

The tool must be on the runner before the action runs, and the checkout
needs the baseline tag:

```yaml
- uses: actions/checkout@v4
  with:
    fetch-depth: 0
- uses: taiki-e/install-action@cargo-semver-checks
- uses: lukasIO/verwalter@v0.16.0
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
