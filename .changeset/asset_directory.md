---
verwalter: patch
---

`--asset` takes a directory and attaches every file in it. An extra argument, such as a glob that the shell expanded after `--asset`, now fails with the fix instead of being read as the checkout.
