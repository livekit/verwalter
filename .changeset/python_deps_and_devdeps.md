---
verwalter: minor
---

`pyproject.toml` dependencies are discovered (`[project] dependencies` and optional groups, PEP 503 name matching) and a dependency's lower bound (`>=`, `==`, `~=`) is moved to the released version in PEP 440 spelling. `package.json` devDependencies no longer make a package a dependent, matching changesets.
