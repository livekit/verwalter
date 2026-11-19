---
verwalter: minor
---

`publish` fails a package whose files changed since its tag, before its publish command runs. The reason names `ref=<tag>`: a run by hand from a later commit no longer publishes newer source under the tagged version. `publish --allow-changed-source` turns the check off. Versions the registry has are skipped before the check.
