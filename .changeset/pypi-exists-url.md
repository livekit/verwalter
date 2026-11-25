---
verwalter: minor
---

A package's own `publish.exists_url` replaces the built-in check of its registry, and `{version}` in it is the PEP 440 form on PyPI. A PyPI package with several files can check for its sdist, so a half upload does not count as published. The built-in PyPI command uploads with `--skip-existing`, so a re-run completes a half upload
