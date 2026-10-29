//! Spec: the file edits behind OpenOrUpdateReleasePullRequests for one
//! package: its versioned files, the pins on it in dependents and pin
//! files, and its changelog entry (ChangelogUpdated). One call is the
//! content of one release PR branch.

use std::{
    fs,
    path::{Path, PathBuf},
};

use crate::{
    changeset::Changeset,
    config::Config,
    handler,
    plan::{PackageState, PlannedRelease},
    version::BumpLevel,
    Error,
};

const LOCK_FILES: &[&str] = &["Cargo.lock", "package-lock.json", "deno.lock"];

fn is_lock_file(path: &str) -> bool {
    LOCK_FILES.contains(&path.rsplit('/').next().unwrap_or(path))
}

/// Which packages depend on `package` (for the pins).
fn dependents<'a>(packages: &'a [PackageState], package: &str) -> Vec<&'a str> {
    packages
        .iter()
        .filter(|p| p.dependencies.iter().any(|d| d == package))
        .map(|p| p.name.as_str())
        .collect()
}

/// Write one release into the working tree under `root`. `packages` is
/// every package in the repo, for the pins in dependents. `pins = false`
/// leaves dependents alone: a pre-release of one package is not something
/// its dependents should pin to. Returns the paths whose content changed.
pub fn apply(
    root: &Path,
    config: &Config,
    packages: &[PackageState],
    changesets: &[Changeset],
    plan: &[PlannedRelease],
    release: &PlannedRelease,
    pins: bool,
) -> Result<Vec<PathBuf>, Error> {
    let name = release.package.as_str();
    let package = config
        .packages
        .get(name)
        .ok_or_else(|| format!("package {name} is not in verwalter.toml"))?;
    let mut written = Vec::new();

    let mut rewrite =
        |rel: &str, regex: Option<&str>, dependency: Option<&str>| -> Result<(), Error> {
            let path = root.join(rel);
            if !path.exists() {
                return Ok(());
            }
            let before = fs::read_to_string(&path)?;
            let after = handler::write_version(rel, regex, &before, dependency, &release.to)?;
            if after != before {
                fs::write(&path, after)?;
                written.push(path);
            }
            Ok(())
        };

    // Own version. A lock file holds it under the package's own entry.
    for file in &package.versioned_files {
        rewrite(
            file.path(),
            file.regex(),
            is_lock_file(file.path()).then_some(name),
        )?;
    }
    // Pins: repo-level pin files, then every dependent's manifests.
    for file in &config.pin_files {
        rewrite(file, None, Some(name))?;
    }
    for dependent in dependents(packages, name).into_iter().filter(|_| pins) {
        if let Some(d) = config.packages.get(dependent) {
            for file in d.versioned_files.iter().filter(|f| !is_lock_file(f.path())) {
                rewrite(file.path(), file.regex(), Some(name))?;
            }
        }
    }

    // Changelog.
    let path = root.join(&package.changelog);
    let before = fs::read_to_string(&path).unwrap_or_else(|_| "# Changelog\n".to_string());
    let after = insert_entry(&before, &render_entry(release, changesets, plan));
    if after != before {
        fs::write(&path, after)?;
        written.push(path);
    }

    Ok(written)
}

/// changesets-github style: a heading per version, a section per bump
/// level, one line per changeset, and an "Updated dependencies" line for
/// bumps the tool added itself.
pub fn render_entry(
    release: &PlannedRelease,
    changesets: &[Changeset],
    plan: &[PlannedRelease],
) -> String {
    let mut out = format!("## {}\n", release.to);
    for (level, title) in [
        (BumpLevel::Major, "Major Changes"),
        (BumpLevel::Minor, "Minor Changes"),
        (BumpLevel::Patch, "Patch Changes"),
    ] {
        let mut lines: Vec<String> = changesets
            .iter()
            .filter(|c| c.unreleased_for(&release.package) == Some(level))
            .map(|c| bullet(&c.summary, c.attribution.as_deref()))
            .collect();
        if level == BumpLevel::Patch && !release.bumped_dependencies.is_empty() {
            let deps: Vec<String> = release
                .bumped_dependencies
                .iter()
                .filter_map(|d| plan.iter().find(|r| &r.package == d))
                .map(|r| format!("{}@{}", r.package, r.to))
                .collect();
            lines.push(format!("- Updated dependencies: {}", deps.join(", ")));
        }
        if !lines.is_empty() {
            out.push_str(&format!("\n### {title}\n\n{}\n", lines.join("\n")));
        }
    }
    out
}

fn bullet(summary: &str, attribution: Option<&str>) -> String {
    let mut lines = summary.lines();
    let first = lines.next().unwrap_or("");
    let rest: String = lines.map(|l| format!("\n  {l}")).collect();
    match attribution {
        Some(a) => format!("- {first} {a}{rest}"),
        None => format!("- {first}{rest}"),
    }
}

/// Put the entry under the top-level heading, above older entries.
fn insert_entry(changelog: &str, entry: &str) -> String {
    let at = changelog
        .find("\n## ")
        .map(|i| i + 1)
        .or_else(|| {
            changelog
                .strip_prefix("# ")
                .map(|_| changelog.find('\n').map_or(changelog.len(), |i| i + 1))
        })
        .unwrap_or(0);
    let (head, tail) = changelog.split_at(at);
    let head = if head.is_empty() || head.ends_with("\n\n") {
        head.to_string()
    } else {
        format!("{}\n", head.trim_end_matches('\n')) + "\n"
    };
    let sep = if tail.is_empty() { "" } else { "\n" };
    format!("{head}{entry}{sep}{tail}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::plan;

    const CONFIG: &str = r#"
pin_files = ["Cargo.toml", "Cargo.lock"]

[packages.livekit-protocol]
path = "livekit-protocol"
versioned_files = ["livekit-protocol/Cargo.toml"]
changelog = "livekit-protocol/CHANGELOG.md"

[packages.livekit-api]
path = "livekit-api"
versioned_files = ["livekit-api/Cargo.toml"]
changelog = "livekit-api/CHANGELOG.md"
"#;

    struct Repo(PathBuf);

    impl Repo {
        fn new(test: &str) -> Self {
            let root =
                std::env::temp_dir().join(format!("verwalter-{}-{test}", std::process::id()));
            let _ = fs::remove_dir_all(&root);
            let files = [
                ("Cargo.toml", "[workspace]\nmembers = [\"livekit-protocol\", \"livekit-api\"]\n\n[workspace.dependencies]\nlivekit-protocol = { version = \"0.3.1\", path = \"livekit-protocol\" }\n"),
                ("Cargo.lock", "version = 4\n\n[[package]]\nname = \"livekit-api\"\nversion = \"0.4.3\"\n\n[[package]]\nname = \"livekit-protocol\"\nversion = \"0.3.1\"\n"),
                ("livekit-protocol/Cargo.toml", "[package]\nname = \"livekit-protocol\"\nversion = \"0.3.1\"\n"),
                ("livekit-protocol/CHANGELOG.md", "# Changelog\n\n## 0.3.1\n\n### Patch Changes\n\n- old fix\n"),
                ("livekit-api/Cargo.toml", "[package]\nname = \"livekit-api\"\nversion = \"0.4.3\"\n\n[dependencies]\nlivekit-protocol = { workspace = true }\nlivekit-protocol-direct = { version = \"0.3.1\" }\n"),
            ];
            for (rel, content) in files {
                let p = root.join(rel);
                fs::create_dir_all(p.parent().unwrap()).unwrap();
                fs::write(p, content).unwrap();
            }
            Self(root)
        }
        fn read(&self, rel: &str) -> String {
            fs::read_to_string(self.0.join(rel)).unwrap()
        }
    }

    impl Drop for Repo {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn state() -> Vec<PackageState> {
        vec![
            PackageState {
                name: "livekit-protocol".into(),
                version: "0.3.1".parse().unwrap(),
                dependencies: vec![],
            },
            PackageState {
                name: "livekit-api".into(),
                version: "0.4.3".parse().unwrap(),
                dependencies: vec!["livekit-protocol".into()],
            },
        ]
    }

    #[test]
    fn protocol_release_updates_files_pins_and_changelog() {
        let repo = Repo::new("protocol");
        let config = Config::parse(CONFIG).unwrap();
        let changesets = [Changeset::parse(
            "a.md",
            "---\nlivekit-protocol: minor\n---\nNegotiated data channels\nwith details\n",
        )
        .unwrap()];
        let p = plan(&state(), &changesets, BumpLevel::Patch);

        let written = apply(&repo.0, &config, &state(), &changesets, &p, &p[0], true).unwrap();
        let names: Vec<String> = written
            .iter()
            .map(|w| w.strip_prefix(&repo.0).unwrap().display().to_string())
            .collect();
        // livekit-api inherits the pin via `workspace = true`, so its manifest stays as is.
        assert_eq!(
            names,
            [
                "livekit-protocol/Cargo.toml",
                "Cargo.toml",
                "Cargo.lock",
                "livekit-protocol/CHANGELOG.md"
            ]
        );

        assert!(repo
            .read("livekit-protocol/Cargo.toml")
            .contains("version = \"0.4.0\""));
        assert!(repo
            .read("Cargo.toml")
            .contains("livekit-protocol = { version = \"0.4.0\""));
        let lock = repo.read("Cargo.lock");
        assert!(lock.contains("name = \"livekit-protocol\"\nversion = \"0.4.0\""));
        assert!(
            lock.contains("name = \"livekit-api\"\nversion = \"0.4.3\""),
            "api untouched in the lock"
        );
        assert_eq!(
            repo.read("livekit-protocol/CHANGELOG.md"),
            "# Changelog\n\n## 0.4.0\n\n### Minor Changes\n\n- Negotiated data channels\n  with details\n\n## 0.3.1\n\n### Patch Changes\n\n- old fix\n"
        );
    }

    #[test]
    fn dependent_release_writes_updated_dependencies_line() {
        let repo = Repo::new("api");
        let config = Config::parse(CONFIG).unwrap();
        let changesets =
            [Changeset::parse("a.md", "---\nlivekit-protocol: minor\n---\nx\n").unwrap()];
        let p = plan(&state(), &changesets, BumpLevel::Patch);
        let api = p.iter().find(|r| r.package == "livekit-api").unwrap();

        apply(&repo.0, &config, &state(), &changesets, &p, api, true).unwrap();
        assert!(repo
            .read("livekit-api/Cargo.toml")
            .contains("version = \"0.4.4\""));
        // Created from scratch: no changelog existed.
        assert_eq!(
            repo.read("livekit-api/CHANGELOG.md"),
            "# Changelog\n\n## 0.4.4\n\n### Patch Changes\n\n- Updated dependencies: livekit-protocol@0.4.0\n"
        );
    }

    #[test]
    fn insert_entry_handles_missing_heading() {
        assert_eq!(insert_entry("", "## 1.0.0\n"), "## 1.0.0\n");
        assert_eq!(
            insert_entry("## 0.9.0\n", "## 1.0.0\n"),
            "## 1.0.0\n\n## 0.9.0\n"
        );
    }
}
