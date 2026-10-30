# Release pitfalls across LiveKit SDKs, mapped to verwalter

Source: the git history, pull requests, issues and workflow runs of rust-sdks, python-sdks, protocol, agents, agents-js, node-sdks, client-sdk-js, components-js, client-sdk-swift, client-sdk-android, client-sdk-flutter, server-sdk-go and livekit-cli, read on 2026-10-29. Run logs older than about four months are gone, so some causes come from PR text only.

## Patterns

| # | Pattern | Where it bit | verwalter today | Gap |
|---|---|---|---|---|
| 1 | A publish dies halfway. Dependents on the registry point at versions that do not exist yet. | rust-sdks #1216 (cargo died after `webrtc-sys`), agents 1.8.1 (one matrix leg failed), agents-js #906 and #1071 (missing releases) | Tags and GitHub releases are idempotent and resumable. Packages go in dependency order and the run stops at the first failure. The handoff lists `depends_on`. Registry publish is the workflow's job. | The example workflows have no `skip-existing` or "already on the registry" preflight. Document the pattern: publish job per package in handoff order, idempotent on re-run. |
| 2 | Trusted publishing cannot create a new package. The first release of a new crate, npm package or PyPI project fails. | rust-sdks #1321, agents 1.7.0 (bland plugin), agents-js #906 | The handoff has `previous_version: null` for a first release. | Nothing names a first release in the release PR. Add a "first release" line to the PR body and a `first_release` field to the handoff, so the workflow can stop or warn. |
| 3 | A package the tool does not know is never released. | rust-sdks #1303 (`livekit-common` dropped from knope.toml), node-sdks #614 (stale `fixed` group) | Workspace consistency check fails `check` and warns elsewhere. Unknown names in `fixed` are a config error. | None. |
| 4 | An invalid changeset silently drops packages from the release. | rust-sdks #1464 (duplicate key), #1263 (quoted names) | Quoted names parse. An unparsable file or an unknown name fails the load, and `check` posts the error on the PR. | None. |
| 5 | Dependents must be bumped too, transitively. | rust-sdks #979, #1461 | Dependents get `[versioning].dependent_bump_level` to a fixpoint, with pins rewritten in `[versioning].pin_files` and manifests. | None. |
| 6 | 0.x: `major` must give 1.0.0. | rust-sdks #1410 | Done. | None. |
| 7 | Many files carry the version and drift. | swift (4 files), flutter (5 files), android README | Any number of `versioning.files` per package with regex entries. The group drift warning covers fixed groups. | The version is read from the first file only. Two files of one package that disagree are not detected. Add a per-package drift warning. |
| 8 | A hand-edited version fights the bot. | python-sdks #701 and #703, agents #7456, server-sdk-go #792, livekit-cli (four tags before the bump merged) | The tag is derived from the files, so tag and files cannot disagree. | `release` tags every package whose file version has no tag. A hand bump with no changeset gets tagged on the next release run. Consider tagging only what the merged release PR lists. |
| 9 | Core and plugins drift, or a floor on the core must move with every release. | agents (plugins pin `livekit-agents>=X`), agents-js #900 (fixed group restored), python-sdks (`livekit-api` floor on protocol) | Fixed groups. In-repo dependency floors are rewritten for Cargo, package.json and pyproject. | `uv.lock` is not in the lock file list. A bumped uv workspace leaves a stale lock. |
| 10 | Pre-release versions in PEP 440. | python-sdks, agents (`1.8.7.rc1`) | `to_pep440` writes `1.6.0rc1` into pyproject and version files. | Verified in unit tests only. Not exercised on the python fork yet. |
| 11 | rc from a branch, promote later, numbering continues. | python-sdks and agents (`next-rc`, `promote`, rc only off main) | `pre-release --target`, rc numbering from tags, promotion follows the pending level. | rc numbers come from local tags. A shallow checkout reuses rc.1. Document `fetch-depth: 0`, or fetch tags inside the command. |
| 12 | Snapshot or dev releases from any commit. | client-sdk-js and agents-js (`changeset version --snapshot`, npm tag `next`), agents (`.devN`) | Not supported. rc PRs are the only pre-release path. | A `snapshot` command that writes `0.0.0-<label>-<sha>` to the files without a PR or tag, for a workflow to publish under a dist tag. Decided out of scope so far. |
| 13 | A bot token does not trigger the next workflow. | rust-sdks #670 (release-plz tags), uniffi #1256 (`release: published`) | Documented for the release PR (`token` input). The example release workflow builds, tags and publishes in one run. | Document that tags and releases made with `github.token` fire no `push` or `release` events. A PAT or App token is needed for a separate build-on-tag workflow. |
| 14 | A draft release stays a draft forever. | rust-sdks uniffi #1256 (marker drafts, lookup race) | A draft is reported on every run until `--publish` flips it. `status` shows it. | None. |
| 15 | Anyone can merge a `release/*` branch and trigger a publish. | python-sdks #901, agents release-gate (bot author plus two approvals) | The example triggers on the branch prefix only. | Add to the example: require `pull_request.user.login == 'github-actions[bot]'` and the body marker, plus a review rule. |
| 16 | Hotfix from a release branch leaves main without the changelog entry, and a stale changeset. | flutter #1259 | Release PRs can target another base. Tags drive rc numbering. | Nothing carries the hotfix changelog entry back to main. Known limitation; document the backport step. |
| 17 | Changelog generation fails on the GitHub API (timeouts, batching). | node-sdks run 28447364484, components run 39620382235, agents-js 1.5.0, changelog fork PRs #1 and #2 | Attribution is one `gh api` call per changeset and degrades to no attribution on failure. | A repo with hundreds of pending changesets makes hundreds of calls. Fine for now. |
| 18 | Multi-line summaries render as headings. | rust-sdks #1411 | Continuation lines are indented under the bullet. | None. |
| 19 | Private or unpublished packages break the run. | node-sdks #498 | `release.mode = "none"` and `release.mode = "tag"`. | None. |
| 20 | A dependency change bumps a major. | agents-js #186 (peer deps) | Dependents get patch by default. | None. |
| 21 | Post-publish jobs fail the run after the registry step succeeded. | agents (deploy-examples in four releases), android (notify dispatch) | Workflow design. | Note in the example: steps after publish use `continue-on-error`. |
| 22 | Release notes drafted by hand or by an LLM. | agents `release_notes.py` | Notes come from changesets with PR and author links. | None. |
| 23 | Cross-repo version coupling. | livekit-cli needs server-sdk-go major.minor, flutter pins `flutter_webrtc` to the WebRTC pod, python-sdks pins the FFI by submodule | Out of scope. | A workflow precondition; nothing for the tool. |

## Gaps, ranked

Items 1 to 5 were closed on 2026-10-29 (verwalter 0.12.0).

1. **uv.lock** is not updated on a bump. Python workspaces release with a stale lock.
2. **First release** is invisible. Name it in the PR body and the handoff.
3. **Per-package version drift** between a package's own files goes unnoticed.
4. **A hand bump gets tagged** by the next release run. Tag only what the merged PR lists, or warn.
5. **Example release workflow** trusts the branch name. Gate on the bot author and the body marker.
6. **rc numbering needs tags** in the checkout. Fetch tags inside `pre-release`.
7. **Token events**: document that `github.token` fires no downstream workflow.
8. **Snapshot releases** have no path. Decided out of scope, keep it on the list.
9. **PEP 440 rc** is untested in CI. Run one rc cycle on the python fork.
10. **Hotfix branches** need a documented backport of the changelog entry.
