# verwalter

Changeset-based versioning for single packages and monorepos, in any language.
It bumps versions, writes changelogs, opens stacked release PRs, tags and
creates GitHub releases. Publishing to a registry stays in your workflow.

The behaviour is specified in [versioning.allium](versioning.allium).

## How it works

1. A contributor adds a `.changeset/<name>.md` file to their PR
   (the [changesets](https://github.com/changesets/changesets) format):

   ```md
   ---
   livekit-protocol: minor
   ---

   Negotiated data channels.
   ```

   If a versioned package changed without one, `verwalter check` fails the PR.
   The `internal` label exempts a PR.

2. On every push to `main`, `verwalter pr` opens or updates one release PR per
   package with pending changesets. Dependents get a `patch` bump. In a
   monorepo the PRs stack: the package others depend on is at the bottom, its
   dependents target its branch. Branches are regenerated on every run.

3. A maintainer merges a release PR. The workflow builds, then
   `verwalter release --asset <file>...` tags the version and creates the GitHub
   release with the assets, then the workflow publishes to the registry. The
   step that cannot be undone comes last. Its `releases` output tells the
   workflow what to publish. If the build turns out broken,
   `verwalter release --abandon <tag>` removes the release and the tag.

## Configuration

`verwalter.toml` at the repository root:

```toml
base_branch = "main"            # default
exempt_label = "internal"       # default
release_branch_prefix = "release/"
release_label = "release"
dependent_bump_level = "patch"
pin_files = ["Cargo.toml", "Cargo.lock"]   # workspace manifest and lock files

[packages.livekit-protocol]
path = "livekit-protocol"
versioned_files = ["livekit-protocol/Cargo.toml"]
changelog = "livekit-protocol/CHANGELOG.md"
tag_template = "{package}-v{version}"      # default in a monorepo, v{version} with one package
release = "tag_and_release"                # none | tag | tag_and_release
draft = false
```

Dependencies between packages are read from the manifests, not configured.
Versioned files are handled by
[knope-versioning](https://crates.io/crates/knope-versioning): `Cargo.toml`,
`Cargo.lock`, `package.json`, `package-lock.json`, `pyproject.toml`,
`pubspec.yaml`, `go.mod`, `pom.xml` and more.

## GitHub Actions

See [examples/workflows](examples/workflows) for the three workflows:
changeset check on PRs, release PRs on push, release after merge.

```yaml
- uses: lukasIO/verwalter@v0.1.0
  with:
    command: check   # check | plan | pr | release
```

## Commands

| Command | What it does |
|---|---|
| `verwalter plan` | Print the release plan: next versions and stack order. |
| `verwalter branches [--base <ref>]` | Create the release branches locally. |
| `verwalter pr [--remote origin]` | Push the branches and open or update the PRs. |
| `verwalter release [--asset <file>]...` | Tag and create GitHub releases for every untagged version, with assets. |
| `verwalter release --abandon <tag>` | Delete a GitHub release and its tag. |
| `verwalter check --pr <n>` | Changeset check for a contributor PR, with comment. |

All commands take `--config <file>` and an optional checkout path.
