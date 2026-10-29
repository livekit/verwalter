---
verwalter: minor
---

An rc carries the in-repo dependencies of the requested package that have a pending changeset, transitively, each at its own rc version, and pins the carried packages to each other. Dependents outside the rc are untouched. The rc's releases are ordered dependencies first.
