//! Reads a checkout: `verwalter.toml`, the version and in-repo dependencies
//! of every package, and the pending changesets in `.changeset/`.

use std::{
    fs,
    path::{Path, PathBuf},
};

use crate::{Version, changeset::Changeset, config::Config, handler, plan::PackageState, Error};

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
    pub fn load(root: &Path) -> Result<Self, Error> {
        Self::load_with_config(root, &root.join(CONFIG_FILE))
    }

    pub fn load_with_config(root: &Path, config_path: &Path) -> Result<Self, Error> {
        let config = Config::parse(
            &fs::read_to_string(config_path)
                .map_err(|e| format!("{}: {e}", config_path.display()))?,
        )?;
        let names: Vec<&str> = config.packages.keys().map(String::as_str).collect();

        let mut packages = Vec::new();
        let mut warnings = Vec::new();
        for (name, package) in &config.packages {
            let mut version: Option<Version> = None;
            let mut dependencies = Vec::new();
            for file in &package.versioned_files {
                let (file, regex) = (file.path(), file.regex());
                let content =
                    fs::read_to_string(root.join(file)).map_err(|e| format!("{file}: {e}"))?;
                match (&version, handler::read_version(file, regex, &content).ok()) {
                    (None, found) => version = found,
                    (Some(first), Some(other)) if *first != other => warnings.push(format!(
                        "{name}: {file} says {other}, {} says {first}; the next release aligns them",
                        package.versioned_files[0].path()
                    )),
                    _ => {}
                }
                let others: Vec<&str> = names.iter().copied().filter(|n| n != name).collect();
                for dep in handler::internal_dependencies(file, &content, &others)? {
                    if !dependencies.contains(&dep) {
                        dependencies.push(dep);
                    }
                }
            }
            let version =
                version.ok_or_else(|| format!("{name}: no versioned file carries a version"))?;
            packages.push(PackageState {
                name: name.clone(),
                version,
                dependencies,
                max_bump: package.max_bump,
                fixed_with: config.fixed_with(name),
            });
        }

        let changesets = read_changesets(&root.join(CHANGESET_DIR), &names)?;
        Ok(Self {
            root: root.to_path_buf(),
            config,
            packages,
            changesets,
            warnings,
        })
    }
}

/// Every `*.md` in the changeset directory except `README.md`. A bump line
/// naming a package that is not configured is an error, so a typo cannot
/// sit in `.changeset/` unnoticed.
fn read_changesets(dir: &Path, known: &[&str]) -> Result<Vec<Changeset>, Error> {
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
        let changeset =
            Changeset::parse(name, &content).map_err(|e| format!("{}: {e}", path.display()))?;
        let unknown = changeset.unknown_packages(known);
        if !unknown.is_empty() {
            return Err(format!(
                "{}: names unknown package {}",
                path.display(),
                unknown.join(", ")
            )
            .into());
        }
        out.push(changeset);
    }
    out.sort_by(|a, b| a.file.cmp(&b.file));
    Ok(out)
}
