---
verwalter: minor
---

A package that `members` finds takes the manifest it was found by as its versioned file when `[versioning].files` is not set: `Cargo.toml`, `package.json`, `pyproject.toml` or `go.mod`, and the `Cargo.toml` next to a maturin `pyproject.toml` with a dynamic version. Before, it failed with "no versioning.files". A mixed Cargo, pnpm and uv repository now needs no `[versioning]` table.
