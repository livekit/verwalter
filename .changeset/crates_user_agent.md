---
verwalter: patch
---

The registry check sends a `verwalter/<version>` User-Agent. crates.io answers 403 to curl's default one, so every crates.io existence check failed.
