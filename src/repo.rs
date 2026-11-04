//! Reads a checkout: `verwalter.toml`, the version and in-repo dependencies
//! of every package, and the pending changesets in `.changeset/`.

use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};

use crate::{Version, changeset::Changeset, config::Config, handler, plan::PackageState};

pub const CONFIG_FILE: &str = "verwalter.toml";
pub const CHANGESET_DIR: &str = ".changeset";

pub struct Repo {
    pub root: PathBuf,
    pub config: Config,
    pub packages: Vec<PackageState>,
    pub changesets: Vec<Changeset>,
    /// Spec: VersionedFilesAgree. Files of one package that carry
    /// different versions; the first file wins, the rest is reported.
    pub warnings: Vec<String>,
}

impl Repo {
    /// Load with the config at `<root>/verwalter.toml`.
    pub fn load(root: &Path) -> Result<Self> {
        Self::load_with_config(root, &root.join(CONFIG_FILE))
    }

    pub fn load_with_config(root: &Path, config_path: &Path) -> Result<Self> {
        let config = Config::load(root, config_path)?;
        let (packages, warnings) = Self::load_packages(root, &config)?;
        let names: Vec<&str> = config.packages.keys().map(String::as_str).collect();
        let changesets = read_changesets(&root.join(CHANGESET_DIR), &names)?;
        Ok(Self {
            root: root.to_path_buf(),
            config,
            packages,
            changesets,
            warnings,
        })
    }

    /// The packages alone: versions and in-repo dependencies from the
    /// manifests, with the file-drift warnings. `check` uses this without
    /// the changesets, which it reads from the PR itself.
    pub fn load_packages(root: &Path, config: &Config) -> Result<(Vec<PackageState>, Vec<String>)> {
        let names: Vec<&str> = config.packages.keys().map(String::as_str).collect();
        // Dependents name a package by its key: the module path for Go.
        let key_of = |n: &str| config.dependency_key(n).to_string();
        let mut packages = Vec::new();
        let mut warnings = Vec::new();
        for (name, package) in &config.packages {
            let mut version: Option<Version> = None;
            // Why the first file carries no version, for the error below.
            let mut unread: Option<String> = None;
            let mut dependencies = Vec::new();
            for file in &package.versioning.files {
                let (file, regex) = (file.path(), file.regex());
                let content =
                    fs::read_to_string(root.join(file)).with_context(|| file.to_string())?;
                let read = handler::read_version(file, regex, &content);
                if let (None, Err(e)) = (&unread, &read) {
                    unread = Some(format!("{e:#}"));
                }
                match (&version, read.ok()) {
                    (None, found) => version = found,
                    (Some(first), Some(other)) if *first != other => warnings.push(format!(
                        "{name}: {file} says {other}, {} says {first}; the next release aligns them",
                        package.versioning.files[0].path()
                    )),
                    _ => {}
                }
                let others: Vec<String> = names
                    .iter()
                    .filter(|n| *n != name)
                    .map(|n| key_of(n))
                    .collect();
                let others: Vec<&str> = others.iter().map(String::as_str).collect();
                for key in handler::internal_dependencies(file, &content, &others)? {
                    let dep = names
                        .iter()
                        .find(|n| key_of(n) == key)
                        .map_or(key, |n| n.to_string());
                    if !dependencies.contains(&dep) {
                        dependencies.push(dep);
                    }
                }
            }
            // A go.mod before its first release with the tool has no version
            // comment: the latest tag of the module, else 0.0.0.
            if version.is_none() && config.is_go(name) {
                version = Some(go_tag_version(root, config, name));
            }
            let version = version.with_context(|| match &unread {
                Some(e) => format!("{name}: no versioned file carries a version ({e})"),
                None => format!("{name}: no versioned file carries a version"),
            })?;
            packages.push(PackageState {
                name: name.clone(),
                version,
                dependencies,
                fixed_with: config.fixed_with(name),
            });
        }
        Ok((packages, warnings))
    }
}

/// The highest stable version among the package's tags, or 0.0.0. A tag
/// that git cannot list counts as none: `plan` works without a checkout's
/// history.
fn go_tag_version(root: &Path, config: &Config, name: &str) -> Version {
    crate::git::run(root, &["tag", "--list"])
        .unwrap_or_default()
        .lines()
        .filter_map(|t| config.version_from_tag(name, t))
        .filter(|v| !v.is_prerelease())
        .max()
        .unwrap_or_else(|| Version::new(0, 0, 0, None))
}

/// Every `*.md` in the changeset directory except `README.md`. A bump line
/// naming a package that is not configured is an error, so a typo cannot
/// sit in `.changeset/` unnoticed.
fn read_changesets(dir: &Path, known: &[&str]) -> Result<Vec<Changeset>> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Ok(Vec::new());
    };
    let mut out = Vec::new();
    for entry in entries {
        let path = entry?.path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if !name.ends_with(".md") || name.eq_ignore_ascii_case("README.md") {
            continue;
        }
        let content = fs::read_to_string(&path)?;
        out.push(
            Changeset::parse_known(name, &content, known)
                .with_context(|| path.display().to_string())?,
        );
    }
    out.sort_by(|a, b| a.file.cmp(&b.file));
    Ok(out)
}
