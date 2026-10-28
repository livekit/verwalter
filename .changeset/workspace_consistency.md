---
verwalter: minor
---

`verwalter check` fails when a publishable Cargo, pnpm or uv workspace member is not in `verwalter.toml`, when a configured package is private, or when its directory has no manifest; other commands warn. New `ignore` config key for members to skip.
