---
verwalter: minor
---

Each release updates its own entry in the root lock file of its manifest format (`Cargo.lock`, `package-lock.json`, `uv.lock`) without a `pin_files` entry. Before, a lock file that no package and no `pin_files` listed stayed at the old version, and `cargo build --locked` failed on the release PR.
