//! Spec: config block and the per-package fields of entity Package.
//! Loaded from `verwalter.toml` at the repository root.

use std::collections::BTreeMap;

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
    pub path: String,
    pub versioned_files: Vec<VersionedFile>,
    pub changelog: String,
    /// Default: "v{version}" with one package, "{package}-v{version}" otherwise.
    pub tag_template: Option<String>,
    #[serde(default)]
    pub release: ReleaseMode,
    #[serde(default)]
    pub draft: bool,
    /// Where the package is published. Default: `[publish].registry`.
    pub registry: Option<crate::registry::Registry>,
    /// For `registry = "custom"`: a URL with `{name}` and `{version}` that
    /// answers 200 when the version exists and 404 when it does not.
    pub exists_url: Option<String>,
    /// Shell command that publishes this package, run in `path`. Default:
    /// `[publish].publish` when on the default registry, else the built-in
    /// command for the registry.
    pub publish: Option<String>,
    /// Spec: Package.api_command. Overrides `[check].api` for this package.
    pub api: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[cfg_attr(test, derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde(default = "default_base_branch")]
    pub base_branch: String,
    #[serde(default = "default_exempt_label")]
    pub exempt_label: String,
    #[serde(default = "default_release_branch_prefix")]
    pub release_branch_prefix: String,
    #[serde(default = "default_release_label")]
    pub release_label: String,
    #[serde(default = "default_dependent_bump_level")]
    pub dependent_bump_level: BumpLevel,
    /// Pre-release label: `1.6.0-rc.1`. Written as PEP 440 in pyproject.toml
    /// (`1.6.0rc1`; `alpha` and `beta` become `a` and `b`).
    #[serde(default = "default_prerelease_label")]
    pub prerelease_label: String,
    /// Files outside any package that pin every package's version: a
    /// workspace Cargo.toml, Cargo.lock, package-lock.json. Each release
    /// updates its own entry in each of them.
    #[serde(default)]
    pub pin_files: Vec<String>,
    /// Workspace members the consistency check must not report, by package
    /// name or path; a trailing `*` matches a prefix (`examples/*`).
    #[serde(default)]
    pub ignore: Vec<String>,
    /// Groups of packages that always release at one shared version: the
    /// highest version in the group, bumped by the highest level any
    /// member needs. `fixed = [["play-core", "play-ffi"]]`.
    #[serde(default)]
    pub fixed: Vec<Vec<String>>,
    /// Defaults for `verwalter publish`, overridden per package.
    #[serde(default)]
    pub publish: Publish,
    /// Spec: config.check.
    #[serde(default)]
    pub check: Check,
    pub packages: BTreeMap<String, Package>,
}

/// Spec: config.publish. The default registry and command for every
/// package; `registry` and `publish` on a package override them.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Default)]
#[cfg_attr(test, derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Publish {
    /// Packages run at once, independent ones only.
    #[serde(default = "default_jobs")]
    pub jobs: usize,
    #[serde(default)]
    pub registry: crate::registry::Registry,
    /// For `registry = "custom"`: a URL with `{name}` and `{version}`.
    pub exists_url: Option<String>,
    /// Shell command, run in the package's path. Applies to packages on
    /// the default registry; others take the built-in default for theirs.
    pub publish: Option<String>,
    /// One command, run once from the root, that publishes every package
    /// on the default registry without a `publish` of its own: the
    /// ecosystem's recursive publish (`pnpm -r publish`,
    /// `cargo publish --workspace`). Order and skipping are its job; the
    /// tool checks the registry before and after and reports per package.
    pub workspace: Option<String>,
}

/// Spec: config.check. The API check that `verwalter check` runs for
/// every affected package that a changeset covers.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Default)]
#[cfg_attr(test, derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Check {
    /// Shell command, run in the package's path with the claimed level
    /// and the baseline tag in the environment. It prints the lowest
    /// change kind it can prove the diff needs, `major`, `minor` or
    /// `patch`, or nothing when it cannot tell.
    pub api: Option<String>,
}

fn default_jobs() -> usize {
    4
}

fn default_base_branch() -> String {
    "main".into()
}
fn default_exempt_label() -> String {
    "internal".into()
}
fn default_release_branch_prefix() -> String {
    "release/".into()
}
fn default_release_label() -> String {
    "release".into()
}
fn default_dependent_bump_level() -> BumpLevel {
    BumpLevel::Patch
}
fn default_prerelease_label() -> String {
    "rc".into()
}

impl Config {
    pub fn parse(text: &str) -> Result<Self, crate::Error> {
        let config: Self = toml::from_str(text)?;
        let mut seen = std::collections::BTreeSet::new();
        for group in &config.fixed {
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
            .api
            .as_deref()
            .or(self.check.api.as_deref())
    }

    /// Spec: Package.registry. The package's own, else the default.
    pub fn registry_of(&self, package: &str) -> crate::registry::Registry {
        self.packages[package]
            .registry
            .unwrap_or(self.publish.registry)
    }

    /// Spec: Package.exists_url, with the same fallback.
    pub fn exists_url_of(&self, package: &str) -> Option<&str> {
        self.packages[package]
            .exists_url
            .as_deref()
            .or(self.publish.exists_url.as_deref())
    }

    /// Spec: Package.in_workspace_publish. Covered by `[publish].workspace`.
    pub fn in_workspace_publish(&self, package: &str) -> bool {
        let p = &self.packages[package];
        self.publish.workspace.is_some()
            && p.publish.is_none()
            && p.registry.is_none_or(|r| r == self.publish.registry)
    }

    /// Spec: Package.publish_command. The package's own, else the default
    /// when the package is on the default registry, else the built-in
    /// command for its registry. None when nothing applies.
    /// `pnpm` is whether the repo is a pnpm workspace: `npm publish`
    /// ships a `workspace:` range as is, `pnpm publish` resolves it to
    /// the dependency's version.
    pub fn publish_command(&self, package: &str, pnpm: bool) -> Option<String> {
        use crate::registry::Registry;
        let p = &self.packages[package];
        if let Some(c) = &p.publish {
            return Some(c.clone());
        }
        let registry = self.registry_of(package);
        if p.registry.is_none_or(|r| r == self.publish.registry) {
            if let Some(c) = &self.publish.publish {
                return Some(c.clone());
            }
        }
        Some(
            match registry {
                Registry::Crates => "cargo publish",
                Registry::Npm if pnpm => {
                    "pnpm publish --provenance --access public --no-git-checks"
                }
                Registry::Npm => "npm publish --provenance --access public",
                Registry::Pypi => "python -m twine upload dist/*",
                Registry::Pubdev => "dart pub publish --force",
                Registry::None | Registry::Custom => return None,
            }
            .to_string(),
        )
    }

    /// The other members of `package`'s fixed group, if it is in one.
    pub fn fixed_with(&self, package: &str) -> Vec<String> {
        self.fixed
            .iter()
            .find(|g| g.iter().any(|n| n == package))
            .map(|g| g.iter().filter(|n| *n != package).cloned().collect())
            .unwrap_or_default()
    }

    /// Spec: Package.tag_template default.
    pub fn tag_template(&self, package: &str) -> String {
        match self.packages[package].tag_template.clone() {
            Some(t) => t,
            None if self.packages.len() == 1 => "v{version}".into(),
            None => "{package}-v{version}".into(),
        }
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
pin_files = ["Cargo.toml", "Cargo.lock"]

[packages.livekit-protocol]
path = "livekit-protocol"
versioned_files = ["livekit-protocol/Cargo.toml", "Cargo.lock"]
changelog = "livekit-protocol/CHANGELOG.md"

[packages.livekit-ffi]
path = "livekit-ffi"
versioned_files = ["livekit-ffi/Cargo.toml", "livekit-ffi-node-bindings/package.json"]
changelog = "livekit-ffi/CHANGELOG.md"
tag_template = "ffi/v{version}"
release = "tag_and_release"
draft = true
"#;

    const SINGLE: &str = r#"
[packages.livekit-client]
path = "."
versioned_files = ["package.json"]
changelog = "CHANGELOG.md"
release = "tag"
"#;

    // obligations: config-default.* (all four)
    #[test]
    fn config_defaults() {
        let c = Config::parse(MONO).unwrap();
        assert_eq!(c.base_branch, "main");
        assert_eq!(c.exempt_label, "internal");
        assert_eq!(c.release_branch_prefix, "release/");
        assert_eq!(c.release_label, "release");
        assert_eq!(c.dependent_bump_level, BumpLevel::Patch);
        assert_eq!(c.prerelease_label, "rc");
        assert_eq!(c.pin_files, vec!["Cargo.toml", "Cargo.lock"]);
        assert!(Config::parse(SINGLE).unwrap().pin_files.is_empty());
    }

    #[test]
    fn config_overrides() {
        let c = Config::parse(&format!(
            "base_branch = \"next\"\ndependent_bump_level = \"minor\"\n{MONO}"
        ))
        .unwrap();
        assert_eq!(c.base_branch, "next");
        assert_eq!(c.dependent_bump_level, BumpLevel::Minor);
    }

    #[test]
    fn versioned_files_accept_paths_and_regex_entries() {
        let c = Config::parse(
            "[packages.sdk]\npath = \".\"\nversioned_files = [\"pubspec.yaml\", { path = \"lib/src/livekit.dart\", regex = \"version = '(?<version>[^']+)'\" }]\nchangelog = \"CHANGELOG.md\"\n",
        )
        .unwrap();
        let files = &c.packages["sdk"].versioned_files;
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
        assert_eq!(protocol.release, ReleaseMode::TagAndRelease);
        assert!(!protocol.draft);
        let ffi = &c.packages["livekit-ffi"];
        assert!(ffi.draft);
        assert_eq!(ffi.versioned_files.len(), 2);
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
        assert_eq!(single.packages["livekit-client"].release, ReleaseMode::Tag);
    }

    #[test]
    fn fixed_groups_are_validated() {
        let c = Config::parse(&format!(
            "fixed = [[\"livekit-protocol\", \"livekit-ffi\"]]\n{MONO}"
        ))
        .unwrap();
        assert_eq!(c.fixed_with("livekit-ffi"), ["livekit-protocol"]);
        assert!(c.fixed_with("nobody").is_empty());
        let e =
            Config::parse(&format!("fixed = [[\"livekit-protocol\", \"x\"]]\n{MONO}")).unwrap_err();
        assert!(e.to_string().contains("unknown package x"));
        let e = Config::parse(&format!(
            "fixed = [[\"livekit-protocol\"], [\"livekit-protocol\"]]\n{MONO}"
        ))
        .unwrap_err();
        assert!(e.to_string().contains("two fixed groups"));
    }

    #[test]
    fn unknown_keys_are_errors() {
        assert!(Config::parse(&format!("{MONO}\npublsh = \"cargo publish\"\n")).is_err());
        assert!(Config::parse(&format!("[publish]\njob = 2\n{MONO}\n")).is_err());
    }

    /// The editor schema. An additive change updates v1.json in place. A
    /// breaking change freezes v1.json and starts v2.json.
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
