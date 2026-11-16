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

## Report every gap

A gap is anything that made you leave the documented path:

- the old flow did something that verwalter cannot do
- a document was wrong, unclear, or silent on your case
- a command failed, or its output did not match the documents
- you wrote a workaround, such as a wrapper script or an extra
  workflow step

A workaround that stays in one repository fixes one repository. The
next repository meets the same gap, and the maintainers never learn
about it. A reported gap is fixed for all of them. While a gap you
met is unreported, the job is not finished.

1. When you meet a gap, write it down. Do not wait for the end.
2. Search before you file:
   `gh issue list --repo livekit/verwalter --state all --search "<words>"`.
   If an issue exists, add your case to it as a comment.
3. Draft one issue per gap for the "Bug Report" template. A missing
   feature is a gap and takes the same template. Give the
   verwalter version, what you tried, what you expected, what
   happened, and the workaround you used. Reduce the case to a minimal
   `verwalter.toml` and command. Leave out names and contents of a
   private repository.
4. Show the drafts to the user. Before you post, ask. The issue is
   public and goes out under the user's account.
5. If the fix is small and you know it, offer a pull request that
   closes the issue. A wrong sentence in a document is one example.
   Follow
   [CONTRIBUTING.md](https://github.com/livekit/verwalter/blob/main/CONTRIBUTING.md).
6. List each gap in your report with its issue link, or with the
   reason it was not filed. Put the issue link in a comment next to
   the workaround, so that someone removes the workaround later.

If you met no gap, say so in the report in one sentence.

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
