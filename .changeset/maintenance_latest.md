---
verwalter: minor
---

A release below a version already tagged, such as 1.5.1 from a `v1` branch after 2.0.0, is now a maintenance release. Its GitHub release is created with `--latest=false`, and the built-in npm command publishes it under `latest-1` (`latest-0.4` for 0.4.x) instead of `latest`. docs/action.md describes how to set up a maintenance branch.
