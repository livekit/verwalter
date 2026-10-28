---
verwalter: minor
---

`verwalter check` fails when a publishable Cargo, pnpm or uv workspace member is not in `verwalter.toml`, or when a configured package's directory has no manifest; other commands warn. New `ignore` config key for members to skip. A private package may still be configured.
