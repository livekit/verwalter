//! Reads a checkout: `verwalter.toml`, the version and in-repo dependencies
//! of every package, and the pending changesets in `.changeset/`.

use std::{
    fs,
    path::{Path, PathBuf},
};

use crate::{changeset::Changeset, config::Config, git, handler, plan::PackageState, Error};

pub const CONFIG_FILE: &str = "verwalter.toml";
pub const CHANGESET_DIR: &str = ".changeset";

pub struct Repo {
    pub root: PathBuf,
    pub config: Config,
    pub packages: Vec<PackageState>,
    pub changesets: Vec<Changeset>,
}

impl Repo {
    /// Load with the config at `<root>/verwalter.toml`.
    pub fn load(root: &Path) -> Result<Self, Error> {
        Self::load_with_config(root, &root.join(CONFIG_FILE))
    }

    pub fn load_with_config(root: &Path, config_path: &Path) -> Result<Self, Error> {
        let config = Config::parse(&fs::read_to_string(config_path).map_err(|e| format!("{}: {e}", config_path.display()))?)?;
        let names: Vec<&str> = config.packages.keys().map(String::as_str).collect();

        let mut packages = Vec::new();
        for (name, package) in &config.packages {
            let mut version = None;
            let mut dependencies = Vec::new();
            for file in &package.versioned_files {
                let (file, regex) = (file.path(), file.regex());
                let content = fs::read_to_string(root.join(file)).map_err(|e| format!("{file}: {e}"))?;
                if version.is_none() {
                    version = handler::read_version(file, regex, &content).ok();
                }
                let others: Vec<&str> = names.iter().copied().filter(|n| n != name).collect();
                for dep in handler::internal_dependencies(file, &content, &others)? {
                    if !dependencies.contains(&dep) {
                        dependencies.push(dep);
                    }
                }
            }
            let version = version.ok_or_else(|| format!("{name}: no versioned file carries a version"))?;
            packages.push(PackageState { name: name.clone(), version, dependencies });
        }

        let mut changesets = read_changesets(&root.join(CHANGESET_DIR))?;
        if git::is_repo(root) {
            mark_released(root, &config, &packages, &mut changesets);
        }
        Ok(Self { root: root.to_path_buf(), config, packages, changesets })
    }
}

/// Spec: Changeset.released_packages, derived from git rather than stored.
/// A bump for package P is released when the tag of P's current version
/// contains the commit that added the changeset file. No tag, or a tag
/// elsewhere, means not released. A pre-release never counts: its
/// changesets stay pending for the final release.
fn mark_released(root: &Path, config: &Config, packages: &[PackageState], changesets: &mut [Changeset]) {
    let tags: Vec<(&str, String)> = packages
        .iter()
        .filter(|p| !p.version.is_prerelease())
        .map(|p| (p.name.as_str(), config.tag_for(&p.name, &p.version)))
        .filter(|(_, tag)| git::ref_exists(root, tag))
        .collect();
    for c in changesets {
        let Some(added) = git::added_in(root, "HEAD", &format!("{CHANGESET_DIR}/{}", c.file)) else { continue };
        for (name, tag) in &tags {
            if c.bumps.iter().any(|b| b.package_name == *name) && git::is_ancestor(root, &added, tag) {
                c.released_packages.insert(name.to_string());
            }
        }
    }
}

/// Every `*.md` in the changeset directory except `README.md`.
fn read_changesets(dir: &Path) -> Result<Vec<Changeset>, Error> {
    let Ok(entries) = fs::read_dir(dir) else { return Ok(Vec::new()) };
    let mut out = Vec::new();
    for entry in entries {
        let path = entry?.path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else { continue };
        if !name.ends_with(".md") || name.eq_ignore_ascii_case("README.md") {
            continue;
        }
        let content = fs::read_to_string(&path)?;
        out.push(Changeset::parse(name, &content).map_err(|e| format!("{}: {e}", path.display()))?);
    }
    out.sort_by(|a, b| a.file.cmp(&b.file));
    Ok(out)
}
