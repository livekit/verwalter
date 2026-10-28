---
verwalter: patch
---

`pr` no longer re-sends an unchanged base when updating a release PR, which GitHub refuses for PRs in a stack; a changed base on a stacked PR is corrected through the stack instead.
