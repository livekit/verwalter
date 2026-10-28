---
verwalter: minor
---

`verwalter release` handles packages in dependency order and stops at the first failure, so a dependent is never released before its dependency. The `releases` output lists dependencies first and carries `depends_on` per entry; the example workflow publishes one package at a time, fail-fast. `--package` always reports the named package, so a dispatch can hand one release to the publish job again.
