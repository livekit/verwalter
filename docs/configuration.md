# Configuration

Every key of `verwalter.toml`, with its default or an example. The [README](../README.md) has the minimum to start.

For completion and hover docs in editors that use Taplo or Tombi (the
Even Better TOML extension for VS Code, Zed, Helix, Neovim), put this line
first in the file:

```toml
#:schema https://raw.githubusercontent.com/lukasIO/verwalter/main/schemas/v1.json
```

The schema is generated from the config types by a test, which fails when
[schemas/v1.json](../schemas/v1.json) is stale. To regenerate it, run
`UPDATE_SCHEMA=1 cargo test schema_is_current`. An additive change, such as a
new optional key, updates `v1.json` in place. A breaking change, such as a
removed or renamed key or a new required key, freezes `v1.json` and adds
`v2.json`.

The keys are grouped in tables by concept. A key of `[versioning]` or
`[release]` that a package can carry is the default for every package. A
package overrides it with a dotted key of the same name, for example
`release.draft = true`. Keys that apply to the whole repository, such as
`pin_files` or `jobs`, are not accepted on a package.

```toml
base_branch = "main"            # default
members = ["packages/*"]        # optional: every publishable workspace member there is a package
exclude = ["examples/*", "packages/internal-tools"]  # by name or path: not a package, not reported

[check]                          # contributor PRs, for `verwalter check`
exempt_label = "internal"       # default: PRs with this label need no changeset
api = "cargo semver-checks --baseline-rev $VERWALTER_BASELINE_TAG 2>&1 | tee /dev/stderr | sed -n 's/.*requires new \\([a-z]*\\) version.*/\\1/p'"

[versioning]                     # what a release changes
files = ["Cargo.toml"]           # default for every package; relative to its path
changelog = "CHANGELOG.md"       # default; relative to the package's path. `false` keeps no changelog
pin_files = ["Cargo.toml", "Cargo.lock"]   # workspace manifest and lock files
fixed = [["livekit-ffi", "livekit-ffi-node"]]  # groups that release at one shared version
dependent_bump_level = "patch"   # default
prerelease_label = "rc"          # 1.6.0-rc.1, written as 1.6.0rc1 in pyproject.toml

[release]                        # the release PR, the tag and the GitHub release
branch_prefix = "release/"       # default
label = "release"                # default
tag = "{package}/v{version}"     # default: v{version} with one package, {package}-v{version} otherwise
mode = "tag_and_release"         # default. none | tag | tag_and_release
draft = false                    # default

[publish]                        # for `verwalter publish`
jobs = 4                         # independent packages published at once
registry = "crates"              # none | crates | npm | pypi | pubdev | custom
command = "cargo publish --no-verify"   # for packages on that registry, run in their path
workspace = "cargo publish --workspace" # or: one command from the root for all of them (pnpm -r publish)
exists_url = "https://r/{name}/{version}"  # for custom: 200 when the version exists, 404 when not

[packages.livekit-protocol]
path = "livekit-protocol"        # the only required key, unless `members` finds the package

[packages.livekit-ffi]
path = "livekit-ffi"
versioning.files = ["Cargo.toml", "../livekit-ffi-node/package.json"]   # the first file carries the version
versioning.changelog = false
release.tag = "ffi/v{version}"
release.mode = "tag"
release.draft = true
publish.registry = "npm"         # "none" with no command: released on GitHub only
publish.command = "pnpm publish --no-git-checks"   # else the built-in one for the registry
publish.exists_url = "https://r/{name}/{version}"
check.api = "griffe check pkg -a $VERWALTER_BASELINE_TAG >/dev/null || echo major"
```

`members` takes directories, with `*` as a whole path component. Each
publishable member of a Cargo, pnpm or uv workspace in them is a package,
named by its manifest. A private member is left out: list it under
`[packages]` to release it. A directory that is no workspace member is an
error unless `exclude` lists it. A `[packages.<name>]` entry for a found
package overrides its keys and needs no `path`. Without `members`, the
`[packages]` entries are the whole list.

`exclude` entries match a workspace member's name or its path, and a
trailing `*` matches a prefix. `members` skips them, and the workspace
consistency check, which reports a publishable member that no package
covers, does not report them.

A package sets each key in any TOML form: dotted keys as above, an inline
table (`release = { draft = true }`) or a `[packages.livekit-ffi.release]`
header.

The API check is described in [api-check.md](api-check.md).

The publish command for a package is its own `publish.command`, else
`[publish].command` when the package is on the default registry, else the
built-in command for its registry: `cargo publish`,
`npm publish --provenance --access public`, `python -m twine upload dist/*`,
`dart pub publish --force`. A package with none fails `publish` before
anything runs. The command runs in the package's `path` with
`VERWALTER_PACKAGE`, `VERWALTER_VERSION`, `VERWALTER_TAG`,
`VERWALTER_REGISTRY`, `VERWALTER_PRERELEASE`, `VERWALTER_FIRST_RELEASE` and
`VERWALTER_ROOT` set. `{package}`, `{version}` and `{tag}` in the command
are replaced.

`[publish].workspace` hands the packages on the default registry to the
ecosystem's recursive publish instead: `pnpm -r publish`,
`cargo publish --workspace`. It runs once, from the root, and owns the
order and the skipping among those packages. The tool still asks the
registry before (all there: the command is skipped) and after (per
package: there, or failed), and a package with its own `publish.command` or
another registry keeps the per-package path and waits for the members it
depends on.

Dependencies between packages are read from the manifests, not configured.

**Versioned files.** Formats come from
[knope-versioning](https://crates.io/crates/knope-versioning): `Cargo.toml`,
`Cargo.lock`, `package.json`, `package-lock.json`, `pyproject.toml`,
`pubspec.yaml`, `go.mod`, `pom.xml` and more. `uv.lock` is handled by the
tool itself. `pnpm-lock.yaml` and `yarn.lock` need no edit. A `workspace:*`
or `catalog:` range stores no version of an internal package. Such
ranges are left alone. Any other text file takes a regex with a `version`
capture group. The regex wins over the file name, and every match moves:

```toml
versioning.files = [
  "pubspec.yaml",
  { path = "lib/src/livekit.dart", regex = "version = '(?<version>[^']+)'" },
  { path = "gradle.properties", regex = "VERSION_NAME=(?<version>.+)" },
  { path = "Cargo.lock", regex = 'name = "my-crate"\nversion = "(?<version>[^"]+)"' },
]
```

The first file of a package is its version.
