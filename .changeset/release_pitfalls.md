---
"verwalter": minor
---

Four gaps found in the release history of the LiveKit SDKs are closed. `uv.lock` is a lock file: the package's entry and every `requires-dist` floor on it move with the version. A first release, one with no earlier tag, is named in the release PR and as `first_release` in the handoff, because trusted publishing cannot create a new package. Files of one package that carry different versions are reported as a warning. A version with no tag and no changelog entry is left alone by `release`, with a warning, because it was edited by hand; `--package` releases it anyway. The example release workflow only runs for a PR the tool opened. The research is in `docs/release-pitfalls.md`.
