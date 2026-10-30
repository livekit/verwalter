---
verwalter: minor
---

Breaking: a new `[workspace]` table holds the package set and the files outside the packages, with every path relative to the repository root. `[workspace].exclude` replaces `[check].ignore`, and `[versioning].pin_files` moved to `[workspace].pin_files`. The new optional `[workspace].members`, for example `members = ["packages/*"]`, makes every publishable Cargo, pnpm or uv workspace member in those directories a package. A `[packages.<name>]` entry overrides a found package and needs no `path`. `base_branch` moved to `[git].base_branch`.
