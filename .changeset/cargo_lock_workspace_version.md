---
verwalter: minor
---

A package that versions `[workspace.package].version` in the root `Cargo.toml` also moves the `Cargo.lock` entry of every crate with `version.workspace = true`, private crates included. A product named like no crate, such as one version for several `publish = false` crates, no longer needs a `Cargo.lock` regex.
