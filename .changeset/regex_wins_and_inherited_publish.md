---
"verwalter": patch
---

A `{ path, regex }` entry is handled by the tool's own regex, whatever the file name, and every match moves. A `Cargo.lock` entry can now be addressed by regex, for a workspace whose crates inherit one version. A crate with `publish.workspace = true` takes its publish flag from `[workspace.package]`, so the workspace check no longer reports it as publishable.
