---
verwalter: minor
---

New command `verwalter gate`: it says whether a workflow run is a release run. With `command: gate`, the action has the output `release`. The example `release.yml` uses it in a first job in place of the long `if:` condition. New config key `[release] author`, the login that opens the release PR, default `verwalter-release[bot]`. A repository without the GitHub App sets `[release] author = "github-actions[bot]"`.
