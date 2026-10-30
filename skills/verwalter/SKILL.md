---
name: verwalter
description: Release packages with verwalter (changeset files, one release PR, tags, GitHub releases, publish commands) and adopt it in a repository that releases another way today, such as changesets/action, knope, release-plz, a hand-written bump script, or tags pushed by hand. Use this skill whenever the user mentions verwalter, verwalter.toml, changesets, a release PR, version bumps, tagging, publishing to npm, crates.io, PyPI or pub.dev, release workflows, rc or pre-releases, or asks to set up, migrate, switch, fix or debug how a repository releases, even when they do not name the tool.
---

# verwalter

verwalter is one binary and one GitHub Action. Contributors describe each
change in a changeset file. The tool keeps one release PR open. Merging it
bumps versions, writes changelogs, tags, creates GitHub releases and runs
the publish commands, dependencies first. Nothing is remembered between
runs: every command reads the tags, releases and PRs and finishes what is
missing, so a failed step is re-run, not repaired by hand.

The documentation in the verwalter repository is the reference. This
skill says how to use the tool and which document answers which
question. Read the document before you guess.

| Document | Read it for |
|---|---|
| [README.md](https://github.com/livekit/verwalter/blob/main/README.md) | The quick start for a repository that releases nothing yet, and the feature list. |
| [docs/commands.md](https://github.com/livekit/verwalter/blob/main/docs/commands.md) | Every command and flag. |
| [docs/configuration.md](https://github.com/livekit/verwalter/blob/main/docs/configuration.md) | Every key of `verwalter.toml`, the versioned file formats and regex entries, and the defaults in one table at the end. |
| [docs/action.md](https://github.com/livekit/verwalter/blob/main/docs/action.md) | The action inputs and outputs, the four workflows, repository settings, maintenance branches. |
| [docs/features.md](https://github.com/livekit/verwalter/blob/main/docs/features.md) | What the tool catches, what to do when a step fails, what is not covered. |
| [docs/adopt.md](https://github.com/livekit/verwalter/blob/main/docs/adopt.md) | Adopting verwalter in a repository that releases another way: the procedure, the mapping per tool, the traps. |
| [examples/workflows](https://github.com/livekit/verwalter/tree/main/examples/workflows) | The four workflows to copy. |
| [examples/publish/README.md](https://github.com/livekit/verwalter/blob/main/examples/publish/README.md) | Publish commands and existence checks per registry. |
| [docs/api-check.md](https://github.com/livekit/verwalter/blob/main/docs/api-check.md) | Checking the changeset level with cargo-semver-checks, griffe or break-check. |
| [docs/release-pitfalls.md](https://github.com/livekit/verwalter/blob/main/docs/release-pitfalls.md) | The release history of the LiveKit SDKs and how each pitfall maps to the tool. |

In a checkout of the verwalter repository, the files are under `docs/`
and `examples/`. Elsewhere, fetch the raw file from GitHub.

## Which job

- **Operate**: the repository has `verwalter.toml`. The user wants a
  changeset, a release, an rc, or a failed run fixed. The rules are
  below.
- **Adopt**: the repository releases another way, or not at all. Follow
  [docs/adopt.md](https://github.com/livekit/verwalter/blob/main/docs/adopt.md)
  step by step, or the README quick start when nothing releases yet.
  The result is one PR with a minimal `verwalter.toml`, the four
  workflows, the old tooling removed, and a handoff list for the
  maintainer.

## The binary

Install it from the
[releases page](https://github.com/livekit/verwalter/releases), or build
it: `cargo install --git https://github.com/livekit/verwalter --tag <tag>`.
Find the newest tag with
`gh release view --repo livekit/verwalter --json tagName -q .tagName`,
and its commit with `gh api repos/livekit/verwalter/commits/<tag> -q .sha`.
The workflows pin that commit, with the tag as the comment:
`uses: livekit/verwalter@<sha> # <tag>`. The example workflows carry a
tag pin of the release they shipped with: replace it. There is no
`--help` and no `--version`: the usage line prints on a wrong argument.

Every command reads `verwalter.toml`. `plan` works offline. `status`,
`pr` and every other command need the `origin` remote, `gh auth login`
and a fresh `git fetch --tags`, because the tags number the releases and
the rcs. Without them, `status` reports `tag:local` and `registry:no`
for every package. That is the offline case, not drift.

## Using it

**Read before you write.** `verwalter status` and `verwalter plan` are
read-only. Run them first. Every writing command takes `--dry-run`,
which prints each push, tag and GitHub write instead of doing it. Use
it before a real run when you are not sure.

**A change needs a changeset.** Write it on the branch before the PR is
reviewed, so that the check passes and the reviewer sees the bump it
claims. `verwalter changeset --level minor --package <name> --summary
"<text>"` writes it. Without `--package`, it covers the packages the
branch changed. Choose the level by the public API: `patch` for a fix,
`minor` for an addition, `major` for a break. On a 0.x package, a break
is `minor` and an addition is `patch`. The summary is the changelog
line: one sentence for the user of the package, without the PR number,
because the tool adds the number and the author.

**A dependent needs no changeset.** A package whose in-repo dependency
is released gets a `patch` bump and rides along. Name it only when its
own behavior changed.

**Never edit a version by hand.** The release run leaves a hand-edited
version alone, with a warning, and no tag or changelog entry results.
If a version must move, write a changeset.

**Nothing on the release PR by hand.** Each push to `main` regenerates
the branch. A fix goes to `main` with its own changeset.

**A failed step is re-run.** Start with `verwalter status`: its last
line per package is the next step. The table "When a step fails" in
[docs/features.md](https://github.com/livekit/verwalter/blob/main/docs/features.md)
names the command for each failure. The registry is never touched by
the tool: a failed publish is re-run, and versions the registry has are
skipped. A broken release is removed with `release --abandon <tag>` and
followed by the next version, never retagged.

**An rc** comes from `verwalter pre-release --target <branch>` or the
`pre-release.yml` workflow. The pending changesets decide the packages
and the levels, the tags decide the rc number. The final release follows
later from the same changesets.

**A maintenance branch** carries its own `verwalter.toml` with
`[git] base_branch`, and its name in the `branches` filter of the
workflows on that branch. [docs/action.md](https://github.com/livekit/verwalter/blob/main/docs/action.md)
has the three steps.

## Rules the configuration and the workflows follow

- **Minimal configuration.** Two keys are required: the package list
  (`[workspace] members` or `[packages.<name>] path`) and
  `versioning.files`. Write another key only when the repository
  differs from its default. Never drop a key the repository needs: the
  tag format, every file that carries a version, a fixed group, a
  registry, pin files.
- **The tag format matches the existing tags.** verwalter finds the
  last release by tag. A format change makes the next release look like
  the first.
- **The version in the files equals the last tag.** No `-SNAPSHOT`, no
  pre-bumped version.
- **The publish command only publishes.** The tool asks the registry
  first and skips a version that exists. A command that greps for
  "already exists" hides real failures.
- **Build first, tag second, publish last.** The step that cannot be
  undone comes last. `release.draft = true` only when the assets need
  the tag to exist.
- **Every workflow checks out with `fetch-depth: 0`.** The tags number
  the rcs, and the history gives each changelog line its PR and author.
- **`release.yml` runs only for a PR the bot authored** with the
  verwalter marker in its body. Keep that condition from the example.
- **A PAT or App token in `token`** when the release PR must trigger CI,
  or when a tag or a release must trigger another workflow.
  `github.token` fires no events.
- **Pin the action by commit.** `uses: livekit/verwalter@<sha> # <tag>`,
  the newest release, the same pin in every workflow, as the LiveKit
  repositories pin every action.
- **One recursive publish or one command per package is the user's
  call.** When the ecosystem has a recursive publish (`pnpm -r publish`,
  `cargo publish --workspace`), name both with a one-line reason each
  and ask which one the migration uses. Do not pick.
- **The API check is a follow-up, not part of the migration.** The
  handoff names the tool for the language and
  [docs/api-check.md](https://github.com/livekit/verwalter/blob/main/docs/api-check.md).
