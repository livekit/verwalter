---
verwalter: minor
---

A Python package with `dynamic = ["version"]` is a dependent of the packages its `pyproject.toml` requires, and those requirements move with each release, in `uv.lock` too. A `<` bound that would exclude a new major moves to the next major
