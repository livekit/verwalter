---
"verwalter": patch
---

The example release workflow checks out the merge commit of the release PR, not the head of `main`. A push that lands right after the merge no longer moves the tags.
