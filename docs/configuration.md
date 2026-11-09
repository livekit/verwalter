# Configuration

Every key of `verwalter.toml`, with its default or an example. The [README](../README.md) has the minimum to start.

For completion and hover docs in editors that use Taplo or Tombi (the
Even Better TOML extension for VS Code, Zed, Helix, Neovim), put this line
first in the file:

```toml
#:schema https://raw.githubusercontent.com/livekit/verwalter/main/schemas/v1.json
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
`fixed` or `jobs`, are not accepted on a package. A package overrides no
key of `[git]` or `[workspace]`.

Paths in `[workspace]` (`members`, `exclude`, `pin_files`) are relative to
the repository root. Paths in `[versioning]` or a package (`files`,
`changelog`) are relative to each package's `path`.

```toml
[git]
base_branch = "main"             # default

[workspace]                      # which packages exist; paths relative to the root
members = ["packages/*"]         # optional: every publishable workspace member there is a package
exclude = ["examples/*", "packages/internal-tools"]  # by name or path: not a package, not reported
pin_files = ["Cargo.toml"]       # workspace manifests; each release updates its own entry. Root lock files need no entry

[check]                          # contributor PRs, for `verwalter check`
exempt_label = "internal"       # default: PRs with this label need no changeset
api = "cargo semver-checks --baseline-rev $VERWALTER_BASELINE_TAG 2>&1 | tee /dev/stderr | sed -n 's/.*requires new \\([a-z]*\\) version.*/\\1/p'"

[versioning]                     # what a release changes
files = ["Cargo.toml"]           # default for every package; relative to its path. Unset: a found member takes its own manifest
changelog = "CHANGELOG.md"       # default; relative to the package's path. `false` keeps no changelog
fixed = [["livekit-ffi", "livekit-ffi-node"]]  # groups that release at one shared version
dependent_bump_level = "patch"   # default
prerelease_label = "rc"          # 1.6.0-rc.1, written as 1.6.0rc1 in pyproject.toml

[release]                        # the release PR, the tag and the GitHub release
branch_prefix = "release/"       # default
label = "release"                # default
tag = "{package}/v{version}"     # default: v{version} with one package, {package}-v{version} otherwise, <path>/v{version} for a go.mod
mode = "tag_and_release"         # default. none | tag | tag_and_release
draft = true                     # default: a package `publish` handles is a draft until the registry has it; a package with nothing to publish is released on creation

[publish]                        # for `verwalter publish`
jobs = 4                         # default: independent packages published at once
registry = "crates"              # none (default) | crates | npm | pypi | pubdev | goproxy | custom
command = "cargo publish --no-verify"   # for packages on that registry, run in their path
workspace = "cargo publish --workspace" # or: one command from the root for all of them (pnpm -r publish)
exists_url = "https://r/{name}/{version}"  # for custom: 200 when the version exists, 404 when not

[packages.livekit-protocol]
path = "livekit-protocol"        # the only required key, unless `members` finds the package

[packages.livekit-ffi]
path = "livekit-ffi"
dependencies = ["livekit-protocol"]   # in-repo packages its manifests do not name; added to the ones they do
versioning.files = ["Cargo.toml", "../livekit-ffi-node/package.json"]   # the first file carries the version
versioning.changelog = false
release.tag = "ffi/v{version}"
release.mode = "tag"
release.draft = false            # public on creation, before the publish
publish.registry = "npm"         # "none" with no command: released on GitHub only
publish.command = "pnpm publish --no-git-checks"   # else the built-in one for the registry
publish.exists_url = "https://r/{name}/{version}"
check.api = "griffe check pkg -a $VERWALTER_BASELINE_TAG >/dev/null || echo major"
```

`members` takes directories, with `*` as a whole path component. Each
publishable member of a Cargo, pnpm, uv or Go workspace in them is a
package, named by its manifest. A module in a `go.work` is named by its
directory, and the module at the root by the last element of its module
path. A private member is left out: list it under
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
`dart pub publish --force`. On `goproxy`, the registry request is the
publish: verwalter asks the Go module proxy for the version, which makes
the proxy fetch the tag, and no command runs. The built-in npm command adds `--tag next` for
a pre-release and `--tag latest-1` for a maintenance release, a final
version below one already tagged (`latest-0.4` for 0.4.x). A custom npm command
reads the same tag from `VERWALTER_NPM_TAG` or `{npm_tag}`, `latest` when
neither applies. The variable is set on every registry and means nothing
outside npm. A package with none fails `publish` before anything runs. The command runs in the package's `path` with
`VERWALTER_PACKAGE`, `VERWALTER_VERSION`, `VERWALTER_TAG`,
`VERWALTER_REGISTRY`, `VERWALTER_NPM_TAG`, `VERWALTER_PRERELEASE`,
`VERWALTER_FIRST_RELEASE` and `VERWALTER_ROOT` set. `{package}`, `{version}`,
`{tag}` and `{npm_tag}` in the command are replaced.

`[publish].workspace` hands the packages on the default registry to the
ecosystem's recursive publish instead: `pnpm -r publish`,
`cargo publish --workspace`. It runs once, from the root, and owns the
order and the skipping among those packages. The tool still asks the
registry before (all there: the command is skipped) and after (per
package: there, or failed), and a package with its own `publish.command` or
another registry keeps the per-package path and waits for the members it
depends on.

Dependencies between packages are read from the manifests. A package
whose manifests do not name one, for example a Python package that links
an in-repo crate at build time, lists it in `dependencies`. A release of
that crate then bumps the package too, by `dependent_bump_level`. A crate
that is never published is still a package, with `release.mode = "none"`
and `publish.registry = "none"`: its changesets bump its `Cargo.toml` and
fan out to the packages that list it.

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

The first file of a package is its version. A `pyproject.toml` with
`dynamic = ["version"]` holds no version. List the file that holds it
instead, for example the `Cargo.toml` next to it for maturin.

**Lock files.** Each release also updates its own entry in the root lock
file of its manifest format: `Cargo.lock` for `Cargo.toml`,
`package-lock.json` for `package.json`, and `uv.lock` for
`pyproject.toml`. No configuration lists them. A lock file of another
format stays as it is, because a Python package can have the name of a
crate from crates.io.

A package can version `[workspace.package].version` in the root
`Cargo.toml`, for example one product made of several `publish = false`
crates. Then `Cargo.lock` needs no regex. Each release also updates the
`Cargo.lock` entry of every crate with `version.workspace = true`:

```toml
[packages.my-product]
path = "."
versioning.files = ["Cargo.toml", "Cargo.lock"]
```

Such a package answers to its own name only. Another package that
depends on one of its crates is not a dependent of it and gets no bump.
To release them together, make the crates packages of their own (below),
or list both packages in `fixed`.

A publishable crate with `version.workspace = true` can also be a package
of its own. Its version is in the root `Cargo.toml`, so verwalter reads and
writes it there, and the crate's own `Cargo.toml` keeps the pins on its
dependencies. All packages on `[workspace.package].version` release
together as one fixed group, with no `fixed` entry:

```toml
[workspace]
members = ["crates/*"]

[versioning]
files = ["Cargo.toml"]
```

**Go modules.** With `go.mod` as the first file, the version is the
`// vX.Y.Z` comment on the module line. The first release writes it.
Until then, the latest tag of the module is the version, else 0.0.0. The
`require` lines that name other packages are the dependencies, and each
release moves them. `go.sum` is not edited: a build of a dependent with
`GOWORK=off` fails until the new tag exists and `go mod tidy` runs. The
default tag is `<path>/v{version}`, or `v{version}` at the root, which is
the format Go reads.

Go finds v2 and later only under a module path that ends in `/vN`.
`plan` and the release branch refuse a major release without it. Change
the module path and every import in the PR that adds the major
changeset. A module in a major-version directory, such as `lib/v2`, is
not supported.

```toml
[workspace]
members = ["sdk/*"]              # the modules that go.work uses

[versioning]
files = ["go.mod"]

[publish]
registry = "goproxy"             # public modules only; a private module stays on none
```
