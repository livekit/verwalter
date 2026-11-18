---
verwalter: minor
---

`status --check [--package <name>]` exits non-zero when the current version of a package is not completely released: a tag that is not on the remote, a missing or draft GitHub release, a missing required asset, or a version the registry does not have.
