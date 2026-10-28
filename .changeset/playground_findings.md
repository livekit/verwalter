---
verwalter: patch
---

A diamond in the dependency graph chains only its own connected component; unrelated packages keep their stacks. Empty changesets are removed by every release branch. The workspace check compares package names normalised (`play_py_ext` matches `play-py-ext`) and accepts a configured package when any of its versioned files exists, so regex-only packages pass.
