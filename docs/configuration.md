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

```toml
base_branch = "main"            # default
exempt_label = "internal"       # default
release_branch_prefix = "release/"
release_label = "release"
dependent_bump_level = "patch"
prerelease_label = "rc"          # 1.6.0-rc.1, written as 1.6.0rc1 in pyproject.toml
pin_files = ["Cargo.toml", "Cargo.lock"]   # workspace manifest and lock files
ignore = ["examples/*"]         # workspace members the consistency check skips
fixed = [["livekit-ffi", "livekit-ffi-node"]]  # groups that release at one shared version

[publish]                        # defaults for `verwalter publish`
jobs = 4                         # independent packages published at once
registry = "crates"              # none | crates | npm | pypi | pubdev | custom
publish = "cargo publish --no-verify"   # command for packages on that registry, run in their path
workspace = "cargo publish --workspace" # or: one command from the root for all of them (pnpm -r publish)

[check]                          # for `verwalter check`
api = "cargo semver-checks --baseline-rev $VERWALTER_BASELINE_TAG 2>&1 | tee /dev/stderr | sed -n 's/.*requires new \\([a-z]*\\) version.*/\\1/p'"

[packages.livekit-protocol]
path = "livekit-protocol"
versioned_files = ["livekit-protocol/Cargo.toml"]
changelog = "livekit-protocol/CHANGELOG.md"
tag_template = "{package}-v{version}"      # default in a monorepo, v{version} with one package
release = "tag_and_release"                # none | tag | tag_and_release
draft = false
registry = "npm"                           # overrides [publish].registry
publish = "pnpm publish --no-git-checks"   # overrides the command; else the built-in one for the registry
exists_url = "https://r/{name}/{version}"  # for custom: 200 when the version exists, 404 when not
api = "griffe check pkg -a $VERWALTER_BASELINE_TAG >/dev/null || echo major"   # overrides [check].api
```

The API check is described in [api-check.md](api-check.md).

The publish command for a package is its own `publish`, else
`[publish].publish` when the package is on the default registry, else the
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
package: there, or failed), and a package with its own `publish` or
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
versioned_files = [
  "pubspec.yaml",
  { path = "lib/src/livekit.dart", regex = "version = '(?<version>[^']+)'" },
  { path = "gradle.properties", regex = "VERSION_NAME=(?<version>.+)" },
  { path = "Cargo.lock", regex = 'name = "my-crate"\nversion = "(?<version>[^"]+)"' },
]
```

The first file of a package is its version.
