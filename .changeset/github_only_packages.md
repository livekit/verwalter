---
verwalter: patch
---

A package with `registry = "none"` set on itself and no `publish` command is released on GitHub only: `publish` leaves it out instead of failing on a missing command.
