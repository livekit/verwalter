# Contributing

Thanks for your interest in contributing!

## Before writing code

If you'd like to contribute code, it's recommended to first discuss your idea on the
[LiveKit Developer Community](https://community.livekit.io/c/client-sdk). This helps keep changes aligned with the LiveKit roadmap and avoids duplicated or unnecessary work.

## Checks that must pass

Please make sure your change passes all of the following:

```sh
cargo fmt --all -- --check                  # formatting
cargo clippy --all-targets -- -D warnings   # lints
cargo test                                  # tests
```

## Changeset

This repo itself uses verwalter for release management. Create a changeset with required bump level and a brief summary of what changed:

```sh
verwalter changeset \
    --level [patch|minor|major] \ # set based on nature of change
    --summary "Improve performance of status check"
```

Summary should be suitable for a changelog entry, extended description belongs in the PR.

## Pull requests

- Open PRs against the `main` branch
- Describe *what* the change does and *why*
- Clearly state any breaking changes and choose changeset level accordingly
- Keep description concise and relevant to PR reviewers
- Link any related issues (e.g. `Closes #123`)

## Reporting bugs

The issue tracker is for bugs or suspected bugs only. Bug reports must use the "Bug Report" template; issues that don't will be closed automatically.

## License

By contributing, you agree that your contributions will be licensed under the same terms as this project (see the `LICENSE` file)
