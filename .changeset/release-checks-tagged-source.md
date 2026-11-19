---
verwalter: minor
---

`release` refuses to attach or upload assets for a tag from an earlier run when the package's files changed since the tag, and names `ref=<tag>`. A release without assets is still created. `release --allow-changed-source` turns the check off.
