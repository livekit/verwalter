---
"verwalter": patch
---

`check` reads only the config, so a changeset that the tool cannot read or that names an unknown package no longer fails the run before the check starts. The error goes into the PR comment and the check fails.
