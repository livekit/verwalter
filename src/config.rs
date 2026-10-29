//! Spec: config block and the per-package fields of entity Package.
//! Loaded from `verwalter.toml` at the repository root.

use std::collections::BTreeMap;

use serde::Deserialize;

use crate::version::BumpLevel;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
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
    /// Ceiling on the bump level: a `major` changeset on a package with
    /// `max_bump = "minor"` bumps minor. In a fixed group the lowest
    /// ceiling applies to the group.
    pub max_bump: Option<BumpLevel>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
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
    pub packages: BTreeMap<String, Package>,
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
        let c = Config::parse(&format!("{MONO}\nmax_bump = \"minor\"\n")).unwrap();
        assert_eq!(c.packages["livekit-ffi"].max_bump, Some(BumpLevel::Minor));
    }

    #[test]
    fn unknown_keys_are_errors() {
        assert!(Config::parse(&format!("{MONO}\npublish = \"cargo publish\"\n")).is_err());
    }
}
