---
verwalter: minor
---

`check` on a release PR runs the dry run of the publish command for each package that the merge releases, with `[check].publish_dry_run = true` or a command. A failed dry run fails the check and shows its output in the PR comment. The built-in cargo dry run covers every crate of the release in one run, so a crate can depend on a version that is not on crates.io yet
