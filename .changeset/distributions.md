---
verwalter: minor
---

`distributions` on a package lists the other forms in which its version is distributed, such as the same version on a second registry: each entry has a name and a `registry` with a `name`, or an `exists_url`. `status` asks each one, `status --json` reports the answers, and `status --check` fails while an entry that is not optional lacks the version.
