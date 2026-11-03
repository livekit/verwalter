---
verwalter: minor
---

A crate with `version.workspace = true` can be a package of its own. Its version is read from and written to the root `Cargo.toml`, the pins on its dependencies still move in its own manifest, and all packages on `[workspace.package].version` release together as one fixed group. Before, such a crate failed with "no versioned file carries a version", and the workaround (the root `Cargo.toml` as its only file) left the pins in dependent crates at the old version.
