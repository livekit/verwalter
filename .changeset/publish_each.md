---
"verwalter": minor
---

`verwalter publish --each <command> [--jobs <n>]` runs the workflow's publish command once per tagged package, dependencies first, independent packages in parallel. A failure blocks the transitive dependents and nothing else, and the run exits non-zero. Per package, `registry = crates | npm | pypi | pubdev | custom` names where it is published. The tool asks the registry whether the version exists before the command runs, skips it when it does, and fails on any answer other than 200 or 404. `status` shows the answer, and `first_release` means the registry has no version at all. The `depends_on` field leaves the release handoff.
