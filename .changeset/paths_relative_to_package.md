---
verwalter: minor
---

`versioned_files` and `changelog` are relative to the package's `path`, with `..` for a sibling directory, and `changelog` defaults to `CHANGELOG.md`, with `changelog = false` to keep none. Repo-relative paths in an existing config must drop the package prefix.
