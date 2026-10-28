---
verwalter: minor
---

A release branch removes its package's line from every changeset that names it and deletes the file with its last line, instead of inferring released packages from tags. A repository that adopts verwalter with tags behind its versions no longer loses its pending changesets on the first release, and a package with `release = "none"` is no longer bumped twice. A release run that races another on the same tag treats the other run's GitHub release as done.
