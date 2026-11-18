---
verwalter: minor
---

Reusable workflows for the changeset check, the release PR and the pre-release PR: a repository calls `livekit/verwalter/.github/workflows/call-check.yml`, `call-pr.yml` or `call-pre-release.yml` with one pin instead of a copy of the steps
