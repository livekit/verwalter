---
name: verwalter
description: Release packages with verwalter (changeset files, one release PR, tags, GitHub releases, publish commands) and adopt it in a repository that releases another way today, such as changesets/action, knope, release-plz, a hand-written bump script, or tags pushed by hand. Use this skill whenever the user mentions verwalter, verwalter.toml, changesets, a release PR, version bumps, tagging, publishing to npm, crates.io, PyPI or pub.dev, release workflows, rc or pre-releases, or asks to set up, migrate, switch, fix or debug how a repository releases, even when they do not name the tool.
---

# verwalter

verwalter keeps one release PR open from the changeset files in a
repository. Merging it bumps versions, writes changelogs, tags, creates
GitHub releases and runs the publish commands, dependencies first. Every
command reads the tags, releases and PRs and finishes what is missing.

The repository's documentation is the reference. Its
[README](https://github.com/livekit/verwalter/blob/main/README.md) has
the quick start and a table of every document with what it answers.
Read the document that matches the question before you guess. Outside
a checkout of the repository, fetch the raw file from GitHub.

## Which job

- **Operate**: the repository has `verwalter.toml`. Follow the rules
  below. Commands and flags are in
  [docs/commands.md](https://github.com/livekit/verwalter/blob/main/docs/commands.md),
  what to do when a step fails is in
  [docs/features.md](https://github.com/livekit/verwalter/blob/main/docs/features.md).
- **Adopt**: the repository releases another way, or not at all. Follow
  [docs/adopt.md](https://github.com/livekit/verwalter/blob/main/docs/adopt.md)
  step by step. It holds the procedure, the mapping per tooling, the
  rules for a minimal `verwalter.toml`, and the handoff list. A
  repository that releases nothing yet takes the README quick start.

## Rules for operating

- **Read before you write.** `verwalter status` and `verwalter plan`
  are read-only. Run them first. Every writing command takes
  `--dry-run`. Use it before a real run when you are not sure.
- **A change needs a changeset, on the branch, before review.** Choose
  the level by the public API: `patch` for a fix, `minor` for an
  addition, `major` for a break. On a 0.x package, a break is `minor`
  and an addition is `patch`. The summary is one sentence for the user
  of the package, without the PR number: the tool adds it.
- **A dependent needs no changeset.** It gets a `patch` bump and rides
  along. Name it only when its own behavior changed.
- **Never edit a version by hand, and nothing on the release PR by
  hand.** Each push to `main` regenerates the branch. A fix goes to
  `main` with its own changeset.
- **A failed step is re-run.** `verwalter status` names the next step
  per package. The tool never touches a registry, so a failed publish
  is re-run and a broken release is abandoned and followed by the next
  version, never retagged.
- **Choices are the user's.** Where the docs offer two ways, such as
  one recursive publish or one command per package, name both with a
  reason each and ask.
