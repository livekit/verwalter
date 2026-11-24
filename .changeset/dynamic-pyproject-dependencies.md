---
verwalter: minor
---

A Python package with `dynamic = ["version"]` is a dependent of the packages its `pyproject.toml` requires, and those requirements move with each release, in `uv.lock` too. An upper bound (`<` or `<=`) that would exclude the new version moves to the next version at its own level, and `~=` keeps its number of parts
