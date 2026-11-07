---
verwalter: patch
---

`publish --releases '[]'` publishes every tagged package the registries lack, like no list at all: a re-run by hand after a run that released nothing no longer passes as a success with nothing published
