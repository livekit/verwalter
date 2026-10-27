---
verwalter: minor
---

`verwalter pre-release <package>` opens an rc PR for one package on demand, against any branch. The rc bumps the package's own files and changelog, keeps the changesets pending, and is tagged as a pre-release after the merge. Pre-release tags no longer count as releases for changesets.
