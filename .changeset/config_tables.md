---
verwalter: minor
---

Breaking: `verwalter.toml` groups its keys in `[check]`, `[versioning]`, `[release]` and `[publish]`. Keys in `[versioning]` and `[release]` are defaults for every package, and a package overrides any table key with a dotted key such as `release.draft = true`. Renamed keys: `versioned_files` is `versioning.files`, `tag_template` is `release.tag`, `release` is `release.mode`, `release_branch_prefix` is `[release].branch_prefix`, `release_label` is `[release].label`, and `publish` is `publish.command`. See docs/configuration.md for every key.
