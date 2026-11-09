---
verwalter: minor
---

A custom `publish.command` for an npm package gets the dist-tag the built-in command would use, `next` for a pre-release and `latest-1` for a maintenance release, as `VERWALTER_NPM_TAG` and `{npm_tag}`. Before, only the built-in npm command knew it, and a custom command published every version under `latest`.
