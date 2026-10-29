---
"verwalter": minor
---

`verwalter publish [--jobs <n>]` runs each package's publish command once per tagged package, dependencies first, independent packages in parallel. A failure blocks the transitive dependents and nothing else, and the run exits non-zero. The command comes from `verwalter.toml`: `[publish]` holds the default `registry`, `publish` command and `jobs`, and a package overrides `registry` and `publish`. A package on another registry than the default takes the built-in command for it. The tool asks the registry whether the version exists before the command runs, skips it when it does, and fails on any answer other than 200 or 404. `status` shows the answer, and `first_release` means the registry has no version at all. The `depends_on` field leaves the release handoff.
