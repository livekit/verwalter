---
verwalter: minor
---

Breaking: `[check].ignore` is now the top-level `exclude`. The new optional top-level `members`, for example `members = ["packages/*"]`, makes every publishable Cargo, pnpm or uv workspace member in those directories a package. A `[packages.<name>]` entry overrides a found package and needs no `path`.
