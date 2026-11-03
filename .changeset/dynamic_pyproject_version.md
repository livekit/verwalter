---
verwalter: patch
---

A package whose `pyproject.toml` has `dynamic = ["version"]` fails with the cause and the file to list instead: the `Cargo.toml` next to it for maturin. Before, the error only said "no versioned file carries a version".
