---
verwalter: minor
---

`check` runs an API check per covered package: `[check].api` or the package's `api` command prints the kind of change the diff needs, verwalter translates it to the digit for the package's version, and fails the PR when the changeset claims less. The comment names the level to use.
