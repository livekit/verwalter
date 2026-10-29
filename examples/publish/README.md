# Publish commands for `verwalter.toml`

verwalter never publishes. `verwalter publish` runs each package's publish
command once per tagged package, dependencies first, and owns the order
and the stop. The command comes from `verwalter.toml`: `publish` on the
package, else `[publish].publish` for packages on the default registry,
else the built-in command for the registry. Before each run the tool asks
the registry whether the version exists. If it does, the package is
skipped and the command does not run. So the command only publishes, and
every error it returns is a real failure. Do not grep its output for
"already exists". A revoked token, a missing `repository.url` for
provenance and a conflict all come back as a 403 or a 422 from some
registries.

```toml
[publish]
registry = "crates"
publish = "cargo publish --no-verify"

[packages."@livekit/rtc-node"]
registry = "npm"                                   # built-in: npm publish --provenance --access public

[packages.livekit-rtc]
registry = "pypi"
publish = "python -m twine upload dist/livekit_rtc-*"
```

The command runs in the package's `path` and sees `VERWALTER_PACKAGE`,
`VERWALTER_VERSION`, `VERWALTER_TAG`, `VERWALTER_REGISTRY`,
`VERWALTER_PRERELEASE`, `VERWALTER_FIRST_RELEASE` and `VERWALTER_ROOT`.
`{package}`, `{version}` and `{tag}` in the command are replaced. Anything
longer than a line is a script the command points at:
`publish = "scripts/publish-wheels.sh"`.

`VERWALTER_FIRST_RELEASE` is `true` when the registry has no version of
the package at all. Trusted publishing cannot create a new package on
crates.io, npm or PyPI, so the first upload is done by hand. A command
stops early with a clear message:

```toml
publish = "[ \"$VERWALTER_FIRST_RELEASE\" != true ] || { echo 'first release: publish by hand' >&2; exit 1; }; cargo publish"
```

## Workspaces

If the ecosystem's tool walks the workspace itself, let it. `pnpm -r publish`
rewrites `workspace:*` ranges and skips published versions.
`cargo publish --workspace` (cargo 1.90 and later) orders the crates and
waits for the index between them. One line replaces every per-package
command on the default registry:

```toml
[publish]
registry = "npm"
workspace = "pnpm -r publish --access public --no-git-checks"
```

The command runs once from the root. verwalter asks the registry before
it runs, and skips it if every package is there. Afterwards it asks per
package and reports each one as published or failed. A package with its
own `publish`, or on another registry, is published by the tool as
before and waits for the workspace packages it depends on.

Without a `registry`, the existence check is the command's job. The
checks below are what the tool does for each registry, for a command that
needs its own.

## crates.io

```yaml
- name: Publish ${{ matrix.release.package }} ${{ matrix.release.version }}
  env:
    PKG: ${{ matrix.release.package }}
    VER: ${{ matrix.release.version }}
  run: |
    set -euo pipefail
    if curl -fsS "https://crates.io/api/v1/crates/$PKG/$VER" -o /dev/null; then
      echo "$PKG $VER is on crates.io, skipping"
      exit 0
    fi
    cargo publish -p "$PKG" --no-verify
```

`cargo publish` has no skip flag. The crates.io API answers 404 for a
version that does not exist and 200 for one that does. curl sends the
`User-Agent` that the API requires.

## npm

```yaml
- name: Publish ${{ matrix.release.package }} ${{ matrix.release.version }}
  working-directory: packages/${{ matrix.release.package }}
  env:
    PKG: ${{ matrix.release.package }}
    VER: ${{ matrix.release.version }}
  run: |
    set -euo pipefail
    if [ "$(npm view "$PKG@$VER" version 2>/dev/null)" = "$VER" ]; then
      echo "$PKG $VER is on npm, skipping"
      exit 0
    fi
    npm publish --provenance --access public
```

If the version exists, `npm view` prints it. If not, it prints nothing.
Its exit code differs between npm versions, so the output is compared. `pnpm publish` works the same way after the check, with
`--no-git-checks`.

## PyPI

```yaml
- name: Publish ${{ matrix.release.package }} ${{ matrix.release.version }}
  env:
    PKG: ${{ matrix.release.package }}
    VER: ${{ matrix.release.version }}
  run: |
    set -euo pipefail
    if curl -fsS "https://pypi.org/pypi/$PKG/$VER/json" -o /dev/null; then
      echo "$PKG $VER is on PyPI, skipping"
      exit 0
    fi
    echo "publish=true" >> "$GITHUB_OUTPUT"
  id: preflight
- uses: pypa/gh-action-pypi-publish@release/v1
  if: steps.preflight.outputs.publish == 'true'
  with:
    packages-dir: dist/${{ matrix.release.package }}
```

`pypa/gh-action-pypi-publish` has `skip-existing: true`, which does the
same check inside twine. It is fine for a single package. A package with several
wheels can fail halfway through the upload. The preflight above then skips
the whole version, but `skip-existing` uploads the missing files. Prefer
`skip-existing` there. For a pre-release, the PyPI URL
takes the PEP 440 spelling (`1.6.0rc1`). That is what the version file
carries. The `version` in the handoff is the semver spelling. Convert with
`VER=$(echo "$VER" | sed -E 's/-rc\.([0-9]+)$/rc\1/')`.

## Maven Central, pub.dev, CocoaPods

Maven Central: `curl -fsS https://repo1.maven.org/maven2/<group path>/<artifact>/$VER/`
answers 200 for a released version. pub.dev:
`curl -fsS https://pub.dev/api/packages/$PKG/versions/$VER`. CocoaPods has
no stable API. `pod trunk info $PKG` lists the versions.
