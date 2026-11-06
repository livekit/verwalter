---
verwalter: minor
---

A package that `publish` handles is released as a draft by default, and `publish` flips it once the registry has the version: no public GitHub release before the package is there. A package with nothing to publish is released on creation. `release.draft = false` restores the old behaviour
