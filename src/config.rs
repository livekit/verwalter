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

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Package {
    pub path: String,
    pub versioned_files: Vec<String>,
    pub changelog: String,
    /// Default: "v{version}" with one package, "{package}-v{version}" otherwise.
    pub tag_template: Option<String>,
    #[serde(default)]
    pub release: ReleaseMode,
    #[serde(default)]
    pub draft: bool,
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

impl Config {
    pub fn parse(text: &str) -> Result<Self, toml::de::Error> {
        toml::from_str(text)
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
}

#[cfg(test)]
mod tests {
    use super::*;

    const MONO: &str = r#"
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
    }

    #[test]
    fn config_overrides() {
        let c = Config::parse(&format!("base_branch = \"next\"\ndependent_bump_level = \"minor\"\n{MONO}")).unwrap();
        assert_eq!(c.base_branch, "next");
        assert_eq!(c.dependent_bump_level, BumpLevel::Minor);
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
        assert_eq!(mono.tag_for("livekit-protocol", &v), "livekit-protocol-v1.2.0");
        assert_eq!(mono.tag_for("livekit-ffi", &v), "ffi/v1.2.0");

        let single = Config::parse(SINGLE).unwrap();
        assert_eq!(single.tag_for("livekit-client", &v), "v1.2.0");
        assert_eq!(single.packages["livekit-client"].release, ReleaseMode::Tag);
    }

    #[test]
    fn unknown_keys_are_errors() {
        assert!(Config::parse(&format!("{MONO}\npublish = \"cargo publish\"\n")).is_err());
    }
}
