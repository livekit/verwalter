//! Spec: config block and the per-package fields of entity Package.
//! Loaded from `verwalter.toml` at the repository root.
//!
//! The tables group the keys by concept. A package overrides a table key
//! with a dotted key of the same name: `release.draft = true`.

use std::{collections::BTreeMap, path::Path};

use serde::Deserialize;

use crate::version::BumpLevel;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[cfg_attr(test, derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum ReleaseMode {
    None,
    Tag,
    #[default]
    TagAndRelease,
}

/// A versioned file: a bare path for a format knope-versioning knows by
/// file name, or `{ path, regex }` for any other text file. The regex must
/// have a `(?<version>...)` capture group; only that group is rewritten.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[cfg_attr(test, derive(schemars::JsonSchema))]
#[serde(untagged)]
pub enum VersionedFile {
    Path(String),
    Regex { path: String, regex: String },
}

impl VersionedFile {
    pub fn path(&self) -> &str {
        match self {
            Self::Path(p) | Self::Regex { path: p, .. } => p,
        }
    }

    pub fn regex(&self) -> Option<&str> {
        match self {
            Self::Path(_) => None,
            Self::Regex { regex, .. } => Some(regex),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[cfg_attr(test, derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Package {
    /// Required, unless `members` finds the package.
    #[serde(default)]
    pub path: String,
    /// Overrides `[versioning]`.
    #[serde(default)]
    pub versioning: PackageVersioning,
    /// Overrides `[release]`.
    #[serde(default)]
    pub release: PackageRelease,
    /// Overrides `[publish]`.
    #[serde(default)]
    pub publish: PackagePublish,
    /// Overrides `[check]`.
    #[serde(default)]
    pub check: PackageCheck,
    /// The module path when the first versioned file is a go.mod: Go
    /// names dependencies and proxy entries by it. Set by `Config::load`.
    #[serde(skip)]
    pub go_module: Option<String>,
}

/// The package keys of `[versioning]`. `Config::parse` fills in the
/// defaults and makes the paths repo-relative.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[cfg_attr(test, derive(schemars::JsonSchema))]
#[serde(default, deny_unknown_fields)]
pub struct PackageVersioning {
    /// Relative to the package's `path`. The first file carries the version.
    pub files: Vec<VersionedFile>,
    /// Relative to the package's `path`. Default: `CHANGELOG.md`.
    /// `changelog = false` keeps no changelog: the release notes are
    /// empty, and a version without a tag is released as is, because
    /// there is no entry to tell a hand edit from a release.
    #[serde(deserialize_with = "changelog_or_off")]
    pub changelog: Option<String>,
}

impl Default for PackageVersioning {
    fn default() -> Self {
        Self {
            files: Vec::new(),
            changelog: Some("CHANGELOG.md".into()),
        }
    }
}

/// The package keys of `[release]`. `Config::parse` fills in the defaults.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Default)]
#[cfg_attr(test, derive(schemars::JsonSchema))]
#[serde(default, deny_unknown_fields)]
pub struct PackageRelease {
    /// Tag name with `{package}` and `{version}`. Default: "v{version}"
    /// with one package, "{package}-v{version}" otherwise, and the Go
    /// module tag "<path>/v{version}" when the first file is a go.mod.
    pub tag: Option<String>,
    pub mode: ReleaseMode,
    pub draft: bool,
}

/// The package keys of `[publish]`. Unlike the other tables, these hold
/// the package's own values only: whether the default command applies
/// depends on whether the package is on the default registry.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Default)]
#[cfg_attr(test, derive(schemars::JsonSchema))]
#[serde(default, deny_unknown_fields)]
pub struct PackagePublish {
    pub registry: Option<crate::registry::Registry>,
    pub exists_url: Option<String>,
    /// Run in the package's `path`. Default: `[publish].command` when on
    /// the default registry, else the built-in command for the registry.
    pub command: Option<String>,
}

/// The package keys of `[check]`, the package's own values only.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Default)]
#[cfg_attr(test, derive(schemars::JsonSchema))]
#[serde(default, deny_unknown_fields)]
pub struct PackageCheck {
    /// Spec: Package.api_command.
    pub api: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[cfg_attr(test, derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Spec: config.git.
    #[serde(default)]
    pub git: Git,
    /// Spec: config.workspace.
    #[serde(default)]
    pub workspace: Workspace,
    /// Spec: config.check.
    #[serde(default)]
    pub check: Check,
    #[serde(default)]
    pub versioning: Versioning,
    #[serde(default)]
    pub release: Release,
    /// Spec: config.publish.
    #[serde(default)]
    pub publish: Publish,
    #[serde(default)]
    pub packages: BTreeMap<String, Package>,
}

/// Spec: config.git.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[cfg_attr(test, derive(schemars::JsonSchema))]
#[serde(default, deny_unknown_fields)]
pub struct Git {
    /// Release PRs target this branch, and pre-releases start from it.
    pub base_branch: String,
}

impl Default for Git {
    fn default() -> Self {
        Self {
            base_branch: "main".into(),
        }
    }
}

/// Spec: config.workspace. Which packages exist, and the files outside
/// them. Every path here is relative to the repository root, and no
/// package overrides these keys.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Default)]
#[cfg_attr(test, derive(schemars::JsonSchema))]
#[serde(default, deny_unknown_fields)]
pub struct Workspace {
    /// Directories of packages, `*` as a whole path component
    /// (`packages/*`). Each publishable Cargo, pnpm or uv workspace member
    /// there is a package, named by its manifest. A `[packages]` entry of
    /// that name overrides it and needs no `path`.
    pub members: Vec<String>,
    /// Workspace members that are no package, by name or path; a trailing
    /// `*` matches a prefix (`examples/*`). `members` skips them, and the
    /// consistency check does not report them.
    pub exclude: Vec<String>,
    /// Files outside any package that pin every package's version: a
    /// workspace Cargo.toml, Cargo.lock, package-lock.json. Each release
    /// updates its own entry in each of them.
    pub pin_files: Vec<String>,
}

/// Spec: config.check. Contributor PRs: the changeset check and the API
/// check.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[cfg_attr(test, derive(schemars::JsonSchema))]
#[serde(default, deny_unknown_fields)]
pub struct Check {
    /// PRs carrying this label need no changeset.
    pub exempt_label: String,
    /// Shell command, run in the package's path with the claimed level
    /// and the baseline tag in the environment. It prints the lowest
    /// change kind it can prove the diff needs, `major`, `minor` or
    /// `patch`, or nothing when it cannot tell. A package's `check.api`
    /// overrides it.
    pub api: Option<String>,
}

impl Default for Check {
    fn default() -> Self {
        Self {
            exempt_label: "internal".into(),
            api: None,
        }
    }
}

/// What a release changes. `files` and `changelog` are defaults for
/// every package, relative to each package's own `path`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[cfg_attr(test, derive(schemars::JsonSchema))]
#[serde(default, deny_unknown_fields)]
pub struct Versioning {
    /// Default for `versioning.files` of every package.
    pub files: Option<Vec<VersionedFile>>,
    /// Default for `versioning.changelog` of every package. `false` keeps
    /// no changelog. Default: `CHANGELOG.md`.
    #[serde(deserialize_with = "changelog_or_off")]
    pub changelog: Option<String>,
    /// Groups of packages that always release at one shared version: the
    /// highest version in the group, bumped by the highest level any
    /// member needs. `fixed = [["play-core", "play-ffi"]]`.
    pub fixed: Vec<Vec<String>>,
    pub dependent_bump_level: BumpLevel,
    /// Pre-release label: `1.6.0-rc.1`. Written as PEP 440 in pyproject.toml
    /// (`1.6.0rc1`; `alpha` and `beta` become `a` and `b`).
    pub prerelease_label: String,
}

impl Default for Versioning {
    fn default() -> Self {
        Self {
            files: None,
            changelog: None,
            fixed: Vec::new(),
            dependent_bump_level: BumpLevel::Patch,
            prerelease_label: "rc".into(),
        }
    }
}

/// The release PR, the tag and the GitHub release. `tag`, `mode` and
/// `draft` are defaults for every package.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[cfg_attr(test, derive(schemars::JsonSchema))]
#[serde(default, deny_unknown_fields)]
pub struct Release {
    /// Release PR branches start with this.
    pub branch_prefix: String,
    /// Release PRs carry this label.
    pub label: String,
    /// Default for `release.tag` of every package.
    pub tag: Option<String>,
    /// Default for `release.mode` of every package: none | tag |
    /// tag_and_release. Default: tag_and_release.
    pub mode: Option<ReleaseMode>,
    /// Default for `release.draft` of every package. Default: false.
    pub draft: Option<bool>,
}

impl Default for Release {
    fn default() -> Self {
        Self {
            branch_prefix: "release/".into(),
            label: "release".into(),
            tag: None,
            mode: None,
            draft: None,
        }
    }
}

/// Spec: config.publish. The default registry and command for every
/// package; `publish.registry` and `publish.command` on a package
/// override them.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[cfg_attr(test, derive(schemars::JsonSchema))]
#[serde(default, deny_unknown_fields)]
pub struct Publish {
    /// Packages run at once, independent ones only.
    pub jobs: usize,
    pub registry: crate::registry::Registry,
    /// For `registry = "custom"`: a URL with `{name}` and `{version}` that
    /// answers 200 when the version exists and 404 when it does not.
    pub exists_url: Option<String>,
    /// Shell command, run in the package's path. Applies to packages on
    /// the default registry; others take the built-in default for theirs.
    pub command: Option<String>,
    /// One command, run once from the root, that publishes every package
    /// on the default registry without a `publish.command` of its own:
    /// the ecosystem's recursive publish (`pnpm -r publish`,
    /// `cargo publish --workspace`). Order and skipping are its job; the
    /// tool checks the registry before and after and reports per package.
    pub workspace: Option<String>,
}

impl Default for Publish {
    fn default() -> Self {
        Self {
            jobs: 4,
            registry: Default::default(),
            exists_url: None,
            command: None,
            workspace: None,
        }
    }
}

/// `changelog = "path"` or `changelog = false`.
fn changelog_or_off<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Raw {
        Path(String),
        Off(bool),
    }
    match Raw::deserialize(d)? {
        Raw::Path(p) => Ok(Some(p)),
        Raw::Off(false) => Ok(None),
        Raw::Off(true) => Err(serde::de::Error::custom(
            "changelog: a path, or false for none",
        )),
    }
}

/// Spec: config.versioning and config.release. Their package keys are
/// defaults: each one a package does not set is copied into it, before
/// paths are made relative to the package.
fn inherit(table: &mut toml::Table) {
    let shared: [(&str, &[&str]); 2] = [
        ("versioning", &["files", "changelog"]),
        ("release", &["tag", "mode", "draft"]),
    ];
    for (section, keys) in shared {
        let Some(defaults) = table.get(section).and_then(toml::Value::as_table).cloned() else {
            continue;
        };
        let Some(packages) = table
            .get_mut("packages")
            .and_then(toml::Value::as_table_mut)
        else {
            return;
        };
        for package in packages.iter_mut().filter_map(|(_, p)| p.as_table_mut()) {
            let own = package
                .entry(section)
                .or_insert_with(|| toml::Table::new().into());
            let Some(own) = own.as_table_mut() else {
                continue;
            };
            for key in keys {
                if let Some(value) = defaults.get(*key) {
                    own.entry(*key).or_insert_with(|| value.clone());
                }
            }
        }
    }
}

/// Spec: config.workspace.members. Each publishable workspace member in
/// a `members` directory gets a package entry, or the `path` of the entry
/// of its name.
fn discover(
    table: &mut toml::Table,
    root: &Path,
    workspace: &Workspace,
) -> Result<(), crate::Error> {
    let Workspace {
        members, exclude, ..
    } = workspace;
    if members.is_empty() {
        return Ok(());
    }
    let known = crate::workspace::members(root);
    let packages = table
        .entry("packages")
        .or_insert_with(|| toml::Table::new().into())
        .as_table_mut()
        .ok_or("packages: a table")?;
    for dir in members
        .iter()
        .flat_map(|m| crate::workspace::expand(root, m))
    {
        let path = crate::workspace::relative(root, &dir);
        let Some(m) = known.iter().find(|m| m.path == path) else {
            if crate::workspace::excluded(exclude, "", &path) {
                continue;
            }
            return Err(format!(
                "members: {path} is no Cargo, pnpm, uv or Go workspace member: add it to the workspace, or to exclude"
            )
            .into());
        };
        if m.private || crate::workspace::excluded(exclude, &m.name, &path) {
            continue;
        }
        if let Some(entry) = packages
            .entry(m.name.clone())
            .or_insert_with(|| toml::Table::new().into())
            .as_table_mut()
        {
            entry.entry("path").or_insert_with(|| path.into());
        }
    }
    Ok(())
}

/// `file` relative to a package `path`, as a repo-relative path. `..`
/// is resolved, so a sibling directory's file is `../sibling/file`.
fn under(path: &str, file: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for c in path.split('/').chain(file.split('/')) {
        match c {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            c => parts.push(c),
        }
    }
    parts.join("/")
}

impl Config {
    /// Reads the config at `path` and finds the `members` packages under
    /// `root`.
    pub fn load(root: &Path, path: &Path) -> Result<Self, crate::Error> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        Self::parse_in(&text, Some(root))
    }

    /// The config text alone: `members` finds nothing without a checkout.
    pub fn parse(text: &str) -> Result<Self, crate::Error> {
        Self::parse_in(text, None)
    }

    fn parse_in(text: &str, root: Option<&Path>) -> Result<Self, crate::Error> {
        // The file as written first, so an error carries its line.
        let written = toml::from_str::<Self>(text)?;
        let mut table: toml::Table = toml::from_str(text)?;
        if let Some(root) = root {
            discover(&mut table, root, &written.workspace)?;
        }
        inherit(&mut table);
        let mut config: Self = table.try_into()?;
        if config.packages.is_empty() {
            return Err("no packages: add [packages.<name>] or members".into());
        }
        let inheritors = root
            .map(crate::workspace::cargo_version_inheritors)
            .unwrap_or_default();
        // Spec: Package.versioned_files and changelog_file are relative to
        // the package's path. Everything after this sees repo-relative paths.
        for (name, p) in &mut config.packages {
            if p.path.is_empty() {
                return Err(format!("{name}: no path, and members does not find it").into());
            }
            if p.versioning.files.is_empty() {
                return Err(format!(
                    "{name}: no versioning.files, on the package or in [versioning]"
                )
                .into());
            }
            for f in &mut p.versioning.files {
                match f {
                    VersionedFile::Path(path) | VersionedFile::Regex { path, .. } => {
                        *path = under(&p.path, path)
                    }
                }
            }
            p.versioning.changelog = p.versioning.changelog.as_deref().map(|c| under(&p.path, c));
            // A crate with `version.workspace = true` has no version of its
            // own: the root Cargo.toml's [workspace.package] has it, so that
            // is the crate's first file. Its own manifest stays, for the
            // pins on its dependencies.
            if let Some(VersionedFile::Path(file)) = p.versioning.files.first()
                && *file == under(&p.path, "Cargo.toml")
                && inheritors.iter().any(|m| m.path == p.path)
            {
                p.versioning
                    .files
                    .insert(0, VersionedFile::Path("Cargo.toml".into()));
            }
            if let (Some(root), Some(VersionedFile::Path(file))) =
                (root, p.versioning.files.first())
                && file.rsplit('/').next() == Some("go.mod") {
                    p.go_module = std::fs::read_to_string(root.join(file))
                        .ok()
                        .and_then(|c| crate::handler::go_module(&c))
                        .map(|(module, _)| module);
                }
            if p.publish.registry.unwrap_or(config.publish.registry)
                == crate::registry::Registry::Goproxy
                && root.is_some()
                && p.go_module.is_none()
            {
                return Err(format!(
                    "{name}: publish.registry = \"goproxy\" needs go.mod as the first versioned file"
                )
                .into());
            }
            // A module in `sdk/v2/` is tagged `sdk/v2.x.y`, which the
            // `<path>/v{version}` default cannot express.
            if let Some(module) = &p.go_module {
                let last = p.path.rsplit('/').next().unwrap_or_default();
                if p.release.tag.is_none()
                    && last.len() > 1
                    && last.starts_with('v')
                    && last[1..].parse::<u64>().is_ok()
                    && module.ends_with(&format!("/{last}"))
                {
                    return Err(format!(
                        "{name}: {module} is in a major-version directory ({}), which is not supported: keep the module in its directory and change the module path instead",
                        p.path
                    )
                    .into());
                }
            }
        }
        // Spec: config.versioning.fixed. Packages whose version is the
        // root Cargo.toml's cannot differ: they form one group, joined to
        // a configured group that holds any of them.
        let shared: Vec<String> = config
            .packages
            .iter()
            .filter(|(_, p)| {
                matches!(p.versioning.files.first(), Some(VersionedFile::Path(f)) if f == "Cargo.toml")
            })
            .map(|(name, _)| name.clone())
            .collect();
        if shared.len() > 1 {
            let fixed = &mut config.versioning.fixed;
            match fixed
                .iter_mut()
                .find(|g| g.iter().any(|n| shared.contains(n)))
            {
                Some(group) => {
                    let missing: Vec<String> = shared
                        .iter()
                        .filter(|n| !group.contains(n))
                        .cloned()
                        .collect();
                    group.extend(missing);
                }
                None => fixed.push(shared),
            }
        }
        let mut seen = std::collections::BTreeSet::new();
        for group in &config.versioning.fixed {
            for name in group {
                if !config.packages.contains_key(name) {
                    return Err(format!("fixed group names unknown package {name}").into());
                }
                if !seen.insert(name) {
                    return Err(format!("{name} is in two fixed groups").into());
                }
            }
        }
        Ok(config)
    }

    /// Spec: Package.api_command. The package's own, else `[check].api`.
    pub fn api_command(&self, package: &str) -> Option<&str> {
        self.packages[package]
            .check
            .api
            .as_deref()
            .or(self.check.api.as_deref())
    }

    /// Spec: Package.registry. The package's own, else the default.
    pub fn registry_of(&self, package: &str) -> crate::registry::Registry {
        self.packages[package]
            .publish
            .registry
            .unwrap_or(self.publish.registry)
    }

    /// Spec: Package.exists_url, with the same fallback.
    pub fn exists_url_of(&self, package: &str) -> Option<&str> {
        self.packages[package]
            .publish
            .exists_url
            .as_deref()
            .or(self.publish.exists_url.as_deref())
    }

    /// Spec: Package.in_workspace_publish. Covered by `[publish].workspace`.
    pub fn in_workspace_publish(&self, package: &str) -> bool {
        let p = &self.packages[package].publish;
        self.publish.workspace.is_some()
            && p.command.is_none()
            && p.registry.is_none_or(|r| r == self.publish.registry)
    }

    /// Spec: Package.publish_command. The package's own, else the default
    /// when the package is on the default registry, else the built-in
    /// command for its registry. None when nothing applies.
    /// `pnpm` is whether the repo is a pnpm workspace: `npm publish`
    /// ships a `workspace:` range as is, `pnpm publish` resolves it to
    /// the dependency's version. `npm_tag` keeps a pre-release or a
    /// maintenance release off npm's `latest` dist-tag
    /// (`version::npm_dist_tag`). Other registries resolve to the highest
    /// version by the version alone.
    pub fn publish_command(
        &self,
        package: &str,
        pnpm: bool,
        npm_tag: Option<&str>,
    ) -> Option<String> {
        use crate::registry::Registry;
        let p = &self.packages[package].publish;
        if let Some(c) = &p.command {
            return Some(c.clone());
        }
        let registry = self.registry_of(package);
        if p.registry.is_none_or(|r| r == self.publish.registry)
            && let Some(c) = &self.publish.command {
                return Some(c.clone());
            }
        let tag = npm_tag.map(|t| format!(" --tag {t}")).unwrap_or_default();
        Some(match registry {
            Registry::Crates => "cargo publish".into(),
            Registry::Npm if pnpm => {
                format!("pnpm publish --provenance --access public --no-git-checks{tag}")
            }
            Registry::Npm => format!("npm publish --provenance --access public{tag}"),
            Registry::Pypi => "python -m twine upload dist/*".into(),
            Registry::Pubdev => "dart pub publish --force".into(),
            Registry::None | Registry::Custom | Registry::Goproxy => return None,
        })
    }

    /// The other members of `package`'s fixed group, if it is in one.
    pub fn fixed_with(&self, package: &str) -> Vec<String> {
        self.versioning
            .fixed
            .iter()
            .find(|g| g.iter().any(|n| n == package))
            .map(|g| g.iter().filter(|n| *n != package).cloned().collect())
            .unwrap_or_default()
    }

    /// Spec: Package.tag_template default.
    pub fn tag_template(&self, package: &str) -> String {
        let p = &self.packages[package];
        match p.release.tag.clone() {
            Some(t) => t,
            // Go reads `<dir>/vX.Y.Z` for a module in a subdirectory.
            None if self.is_go(package) && p.path != "." => format!("{}/v{{version}}", p.path),
            None if self.is_go(package) || self.packages.len() == 1 => "v{version}".into(),
            None => "{package}-v{version}".into(),
        }
    }

    /// Whether the package's version lives in a go.mod.
    pub fn is_go(&self, package: &str) -> bool {
        matches!(
            self.packages[package].versioning.files.first(),
            Some(VersionedFile::Path(f)) if f.rsplit('/').next() == Some("go.mod")
        )
    }

    /// The name dependents use for `package`: its Go module path, else its
    /// name.
    pub fn dependency_key<'a>(&'a self, package: &'a str) -> &'a str {
        self.packages[package]
            .go_module
            .as_deref()
            .unwrap_or(package)
    }

    /// Spec: black box tag_for.
    pub fn tag_for(&self, package: &str, version: &crate::Version) -> String {
        self.tag_template(package)
            .replace("{package}", package)
            .replace("{version}", &version.to_string())
    }

    /// The inverse of `tag_for`: the version a tag of this package carries.
    pub fn version_from_tag(&self, package: &str, tag: &str) -> Option<crate::Version> {
        let template = self.tag_template(package).replace("{package}", package);
        let (prefix, suffix) = template.split_once("{version}")?;
        tag.strip_prefix(prefix)?.strip_suffix(suffix)?.parse().ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MONO: &str = r#"
[workspace]
pin_files = ["Cargo.toml", "Cargo.lock"]

[packages.livekit-protocol]
path = "livekit-protocol"
versioning.files = ["Cargo.toml", "Cargo.lock"]

[packages.livekit-ffi]
path = "livekit-ffi"
versioning.files = ["Cargo.toml", "livekit-ffi-node-bindings/package.json"]
release.tag = "ffi/v{version}"
release.mode = "tag_and_release"
release.draft = true
"#;

    const SINGLE: &str = r#"
[packages.livekit-client]
path = "."
versioning.files = ["package.json"]
release.mode = "tag"
"#;

    // obligations: config-default.* (all four)
    #[test]
    fn config_defaults() {
        let c = Config::parse(MONO).unwrap();
        assert_eq!(c.git.base_branch, "main");
        assert_eq!(c.check.exempt_label, "internal");
        assert_eq!(c.release.branch_prefix, "release/");
        assert_eq!(c.release.label, "release");
        assert_eq!(c.versioning.dependent_bump_level, BumpLevel::Patch);
        assert_eq!(c.versioning.prerelease_label, "rc");
        assert_eq!(c.workspace.pin_files, vec!["Cargo.toml", "Cargo.lock"]);
        assert_eq!(c.publish.jobs, 4);
        assert!(
            Config::parse(SINGLE)
                .unwrap()
                .workspace
                .pin_files
                .is_empty()
        );
    }

    #[test]
    fn config_overrides() {
        let c = Config::parse(&format!(
            "{MONO}\n[git]\nbase_branch = \"next\"\n[versioning]\ndependent_bump_level = \"minor\"\n"
        ))
        .unwrap();
        assert_eq!(c.git.base_branch, "next");
        assert_eq!(c.versioning.dependent_bump_level, BumpLevel::Minor);
    }

    #[test]
    fn versioned_files_accept_paths_and_regex_entries() {
        let c = Config::parse(
            "[packages.sdk]\npath = \".\"\nversioning.files = [\"pubspec.yaml\", { path = \"lib/src/livekit.dart\", regex = \"version = '(?<version>[^']+)'\" }]\n",
        )
        .unwrap();
        let files = &c.packages["sdk"].versioning.files;
        assert_eq!((files[0].path(), files[0].regex()), ("pubspec.yaml", None));
        assert_eq!(
            (files[1].path(), files[1].regex()),
            (
                "lib/src/livekit.dart",
                Some("version = '(?<version>[^']+)'")
            )
        );
    }

    // obligation: entity-fields.Package (release_mode, release_as_draft, tag_template)
    #[test]
    fn package_fields_and_defaults() {
        let c = Config::parse(MONO).unwrap();
        let protocol = &c.packages["livekit-protocol"];
        assert_eq!(protocol.release.mode, ReleaseMode::TagAndRelease);
        assert!(!protocol.release.draft);
        let ffi = &c.packages["livekit-ffi"];
        assert!(ffi.release.draft);
        assert_eq!(ffi.versioning.files.len(), 2);
    }

    // Spec: config.versioning and config.release hold defaults for every
    // package; a package's own dotted key wins.
    #[test]
    fn table_keys_are_package_defaults() {
        let c = Config::parse(
            r#"
[versioning]
files = ["Cargo.toml"]
changelog = false

[release]
tag = "{package}/v{version}"
mode = "tag"

[packages.a]
path = "crates/a"

[packages.b]
path = "crates/b"
versioning.files = ["Cargo.toml", "node/package.json"]
versioning.changelog = "CHANGES.md"
release.mode = "tag_and_release"
"#,
        )
        .unwrap();
        let v: crate::Version = "1.0.0".parse().unwrap();
        let a = &c.packages["a"];
        assert_eq!(a.versioning.files[0].path(), "crates/a/Cargo.toml");
        assert_eq!(a.versioning.changelog, None);
        assert_eq!(a.release.mode, ReleaseMode::Tag);
        assert_eq!(c.tag_for("a", &v), "a/v1.0.0");
        let b = &c.packages["b"];
        assert_eq!(b.versioning.files.len(), 2);
        assert_eq!(
            b.versioning.changelog.as_deref(),
            Some("crates/b/CHANGES.md")
        );
        assert_eq!(b.release.mode, ReleaseMode::TagAndRelease);
        assert_eq!(c.tag_for("b", &v), "b/v1.0.0");

        let e = Config::parse("[packages.a]\npath = \"a\"\n").unwrap_err();
        assert!(e.to_string().contains("no versioning.files"));
    }

    #[test]
    fn tag_template_defaults_depend_on_package_count() {
        let mono = Config::parse(MONO).unwrap();
        let v: crate::Version = "1.2.0".parse().unwrap();
        assert_eq!(
            mono.tag_for("livekit-protocol", &v),
            "livekit-protocol-v1.2.0"
        );
        assert_eq!(mono.tag_for("livekit-ffi", &v), "ffi/v1.2.0");

        assert_eq!(
            mono.version_from_tag("livekit-ffi", "ffi/v1.2.0-rc.1")
                .unwrap()
                .to_string(),
            "1.2.0-rc.1"
        );
        assert_eq!(
            mono.version_from_tag("livekit-ffi", "livekit-protocol-v1.2.0"),
            None
        );
        assert_eq!(
            mono.version_from_tag("livekit-protocol", "livekit-protocol-v1.2.0"),
            Some(v.clone())
        );

        let single = Config::parse(SINGLE).unwrap();
        assert_eq!(single.tag_for("livekit-client", &v), "v1.2.0");
        assert_eq!(
            single.packages["livekit-client"].release.mode,
            ReleaseMode::Tag
        );
    }

    #[test]
    fn fixed_groups_are_validated() {
        let with = |fixed: &str| Config::parse(&format!("{MONO}\n[versioning]\nfixed = {fixed}\n"));
        let c = with("[[\"livekit-protocol\", \"livekit-ffi\"]]").unwrap();
        assert_eq!(c.fixed_with("livekit-ffi"), ["livekit-protocol"]);
        assert!(c.fixed_with("nobody").is_empty());
        let e = with("[[\"livekit-protocol\", \"x\"]]").unwrap_err();
        assert!(e.to_string().contains("unknown package x"));
        let e = with("[[\"livekit-protocol\"], [\"livekit-protocol\"]]").unwrap_err();
        assert!(e.to_string().contains("two fixed groups"));
    }

    #[test]
    fn unknown_keys_are_errors() {
        assert!(Config::parse(&format!("{MONO}\npublsh = \"cargo publish\"\n")).is_err());
        assert!(Config::parse(&format!("[publish]\njob = 2\n{MONO}\n")).is_err());
        // A repo-wide key on a package, and a key of the old layout.
        assert!(Config::parse(&format!("{MONO}\nversioning.fixed = []\n")).is_err());
        assert!(Config::parse(&format!("{MONO}\nworkspace.pin_files = []\n")).is_err());
        assert!(Config::parse(&format!("{MONO}\nversioned_files = []\n")).is_err());
    }

    /// The editor schema. An additive change updates v1.json in place. A
    /// breaking change freezes v1.json and starts v2.json.
    // Spec: Package.versioned_files and changelog_file are relative to path.
    #[test]
    fn files_are_relative_to_the_package_path() {
        let c = Config::parse("[packages.a]\npath = \"crates/a\"\nversioning.files = [\"Cargo.toml\", { path = \"../bindings/package.json\", regex = \"v(?<version>.+)\" }]\n[packages.b]\npath = \".\"\nversioning.files = [\"package.json\"]\nversioning.changelog = \"docs/CHANGES.md\"\n").unwrap();
        let a = &c.packages["a"];
        assert_eq!(a.versioning.files[0].path(), "crates/a/Cargo.toml");
        assert_eq!(a.versioning.files[1].path(), "crates/bindings/package.json");
        assert_eq!(
            a.versioning.changelog.as_deref(),
            Some("crates/a/CHANGELOG.md")
        );
        let b = &c.packages["b"];
        assert_eq!(b.versioning.files[0].path(), "package.json");
        assert_eq!(b.versioning.changelog.as_deref(), Some("docs/CHANGES.md"));
        // Opt out.
        let c = Config::parse(
            "[packages.a]\npath = \"a\"\nversioning.files = [\"Cargo.toml\"]\nversioning.changelog = false\n",
        )
        .unwrap();
        assert_eq!(c.packages["a"].versioning.changelog, None);
        assert!(Config::parse(
            "[packages.a]\npath = \"a\"\nversioning.files = [\"Cargo.toml\"]\nversioning.changelog = true\n"
        )
        .is_err());
    }

    #[test]
    fn schema_is_current() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/schemas/v1.json");
        let schema = serde_json::to_string_pretty(&schemars::schema_for!(Config)).unwrap() + "\n";
        if std::env::var_os("UPDATE_SCHEMA").is_some() {
            std::fs::write(path, &schema).unwrap();
        }
        assert!(
            std::fs::read_to_string(path).is_ok_and(|s| s == schema),
            "schemas/v1.json is stale: run UPDATE_SCHEMA=1 cargo test schema_is_current"
        );
    }
}
