# Publish steps that are safe to re-run

verwalter never touches a registry. The publish job is yours. If anything
after the tag failed, the release run is repeated. The job must then skip
a version that is already on the registry. It must still fail on every
other error. Do not grep the error output of the publish command for "already
exists". A revoked token, a missing `repository.url` for provenance and a
conflict all come back as a 403 or a 422 from some registries.

Ask the registry first. Each snippet below reads `package` and `version`
from one entry of verwalter's `releases` output, a matrix over
`fromJSON(needs.release.outputs.releases)`. It checks whether that version
exists. If it does not, the snippet publishes. Every error of the publish
command itself fails the job.

The `first_release` field of the same entry says that the package has no
earlier tag. Trusted publishing cannot create a new package on crates.io,
npm or PyPI, so the first upload is done by hand. A step stops early with
a clear message:

```yaml
- if: matrix.release.first_release
  run: |
    echo "::error::${{ matrix.release.package }} has no release yet. Publish the first version by hand, then re-run."
    exit 1
```

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
