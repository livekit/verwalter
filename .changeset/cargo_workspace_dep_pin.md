---
verwalter: patch
---

A Cargo dependency declared with `workspace = true` is no longer given a `version` when its package is released; the pin stays in the root `[workspace.dependencies]`.
