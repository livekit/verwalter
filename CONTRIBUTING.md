# Contributing

Thanks for your interest in contributing!

## 1. Reporting Bugs

- For suspected or confirmed bugs, please open an issue.
- If possible, provide steps on how to reproduce.

> [!IMPORTANT]
> Bug reports must use the "Bug Report" template; issues that don't will be closed automatically.

## 2. Pull Requests

### 2.1. Before Opening

Start a discussion about your proposed change in one of the following channels:
- **For bugs**: open an issue.
- **For feature requests**: post in the [LiveKit Developer Community](https://community.livekit.io/c/client-sdk). This helps keep changes aligned with the LiveKit roadmap and avoids duplicated or unnecessary work.

> [!IMPORTANT]
> Unsolicited PRs will not be accepted.

### 2.2. Changeset

All code changes must have a chageset in order to get released. This project uses [_verwalter_](https://github.com/livekit/verwalter) for release management. Commit a changeset to your branch with required bump level and a brief summary of what changed:

```sh
verwalter changeset \
    --level [patch|minor|major] \ # set based on nature of change
    --summary "Improve performance of status check"
```

Summary should be suitable for a changelog entry, extended description belongs in the PR description.

### 2.3. Required Checks

Please make sure your change passes all of the following:

```sh
cargo fmt --all -- --check                  # formatting
cargo clippy --all-targets -- -D warnings   # lints
cargo test                                  # tests
```

### 2.4. PR Description

- Link discussion: an issue (e.g. `Closes #123`) or a LiveKit Developer Community thread
- Describe *what* the change does and *why*
- Keep description concise and relevant to PR reviewers
- Note potential areas for improvement or areas you would like reviewers to scrutinize
- When referring to specific parts of the code, include a [permalink](https://docs.github.com/en/get-started/writing-on-github/working-with-advanced-formatting/creating-a-permanent-link-to-a-code-snippet#linking-to-code) with line number

## 3. License

By contributing, you agree that your contributions will be licensed under the same terms as this project (see the `LICENSE` file)
