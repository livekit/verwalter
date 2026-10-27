//! Spec: contract VersionedFileHandler, implemented on top of
//! knope-versioning. The file format is picked by file name; see
//! `knope_versioning::versioned_file::Config` for the supported names.
//! Functions take content, not paths, so they stay pure.

use knope_versioning::{
    package::Name, release_notes::ReleaseNotes, versioned_file::cargo, Action, GoVersioning,
    Package, VersionedFile, VersionedFileConfig,
};
use relative_path::RelativePathBuf;
use toml_edit::DocumentMut;

use crate::Version;

pub type Error = Box<dyn std::error::Error + Send + Sync>;

const NO_TAGS: &[&str] = &[];

fn is_pyproject(path: &str) -> bool {
    path.rsplit('/').next() == Some("pyproject.toml")
}

/// PEP 440 pre-release spelling for pyproject.toml: `1.6.0-rc.1` is
/// written as `1.6.0rc1`, `alpha`/`beta` as `a`/`b`. Other labels keep the
/// semver form, which Python tools accept and normalise.
pub fn to_pep440(version: &Version) -> String {
    let s = version.to_string();
    let Some((stable, pre)) = s.split_once('-') else { return s };
    let Some((label, n)) = pre.rsplit_once('.') else { return s };
    let label = match label {
        "alpha" | "a" => "a",
        "beta" | "b" => "b",
        "rc" => "rc",
        _ => return s,
    };
    format!("{stable}{label}{n}")
}

/// The inverse, for reading: `1.6.0rc1` back to `1.6.0-rc.1`.
fn from_pep440(s: &str) -> String {
    for label in ["rc", "a", "b"] {
        if let Some((stable, n)) = s.split_once(label) {
            if !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()) && stable.split('.').all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit())) {
                let semver_label = match label { "a" => "alpha", "b" => "beta", _ => "rc" };
                return format!("{stable}-{semver_label}.{n}");
            }
        }
    }
    s.to_string()
}

/// Rewrite the `version = "..."` line of a pyproject.toml to semver form so
/// knope can parse it.
fn normalise_pyproject(content: &str) -> String {
    content
        .lines()
        .map(|line| {
            let trimmed = line.trim_start();
            if let Some(rest) = trimmed.strip_prefix("version") {
                if let Some(value) = rest.trim_start().strip_prefix('=') {
                    let value = value.trim();
                    if let Some(inner) = value.strip_prefix('"').and_then(|v| v.strip_suffix('"')) {
                        let indent = &line[..line.len() - trimmed.len()];
                        return format!("{indent}version = \"{}\"", from_pep440(inner));
                    }
                }
            }
            line.to_string()
        })
        .collect::<Vec<_>>()
        .join("\n")
        + if content.ends_with('\n') { "\n" } else { "" }
}

fn load(path: &str, dependency: Option<&str>, content: &str) -> Result<(VersionedFileConfig, VersionedFile), Error> {
    let config = VersionedFileConfig::new(RelativePathBuf::from(path), dependency.map(String::from), None)?;
    let content = if is_pyproject(path) { normalise_pyproject(content) } else { content.to_string() };
    let file = VersionedFile::new(&config, content, NO_TAGS)?;
    Ok((config, file))
}

/// The package version in a manifest. Lock files have none.
pub fn read_version(path: &str, content: &str) -> Result<Version, Error> {
    Ok(load(path, None, content)?.1.version()?)
}

/// Spec: write_version (`dependency` = None) and write_dependency_version
/// (`dependency` = Some). Lock files always need the dependency, which is
/// the package itself for its own lock file. Content without a matching
/// entry comes back unchanged.
///
/// Invariant MinimalEdit holds for TOML (toml_edit). package.json is
/// re-serialized with two-space indentation, which is what prettier
/// produces in node-sdks.
pub fn write_version(path: &str, content: &str, dependency: Option<&str>, version: &Version) -> Result<String, Error> {
    let (config, file) = load(path, dependency, content)?;
    let mut package = Package::new(
        Name::from(dependency.unwrap_or("package")),
        NO_TAGS,
        vec![config],
        std::slice::from_ref(&file),
        ReleaseNotes::default(),
        None,
    )?;
    let files = package.set_version(version.clone(), GoVersioning::default(), vec![file])?;
    let written = files.into_iter().next().and_then(VersionedFile::write).and_then(|actions| {
        actions.into_iter().find_map(|a| match a {
            Action::WriteToFile { content, .. } => Some(content),
            _ => None,
        })
    });
    let written = written.unwrap_or_else(|| content.to_string());
    if is_pyproject(path) && version.is_prerelease() {
        return Ok(written.replace(&format!("\"{version}\""), &format!("\"{}\"", to_pep440(version))));
    }
    Ok(written)
}

/// Spec: internal_dependencies. Which of `candidates` (the other package
/// names in the repo) this manifest depends on.
pub fn internal_dependencies(path: &str, content: &str, candidates: &[&str]) -> Result<Vec<String>, Error> {
    let file_name = path.rsplit('/').next().unwrap_or(path);
    let found: Vec<String> = match file_name {
        "Cargo.toml" => {
            let doc: DocumentMut = content.parse()?;
            candidates.iter().filter(|c| cargo::contains_dependency(&doc, c)).map(|c| c.to_string()).collect()
        }
        "package.json" => {
            let json: serde_json::Value = serde_json::from_str(content)?;
            let sections = ["dependencies", "devDependencies", "peerDependencies", "optionalDependencies"];
            candidates
                .iter()
                .filter(|c| sections.iter().any(|s| json.get(s).and_then(|d| d.get(**c)).is_some()))
                .map(|c| c.to_string())
                .collect()
        }
        // ponytail: pyproject.toml and pubspec.yaml report no dependencies
        // yet; add a branch when a monorepo in that language needs stacking.
        _ => Vec::new(),
    };
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(s: &str) -> Version {
        s.parse().unwrap()
    }

    // Obligation contract-signature.VersionedFileHandler.*: Roundtrip and
    // MinimalEdit for one sample file.
    fn check(path: &str, sample: &str, next: &str) -> String {
        let next = v(next);
        assert_ne!(read_version(path, sample).unwrap(), next);
        let written = write_version(path, sample, None, &next).unwrap();
        assert_eq!(read_version(path, &written).unwrap(), next, "Roundtrip");
        written
    }

    fn assert_one_line_changed(before: &str, after: &str) {
        assert_eq!(before.lines().count(), after.lines().count());
        let changed = before.lines().zip(after.lines()).filter(|(a, b)| a != b).count();
        assert_eq!(changed, 1, "MinimalEdit: exactly one line changes");
    }

    const CARGO: &str = r#"[package]
name = "livekit-api"
version = "0.4.3"
edition = "2021"

[dependencies]
livekit-protocol = { version = "0.3.1", path = "../livekit-protocol" }
serde = "1"
"#;

    #[test]
    fn cargo_toml() {
        let written = check("livekit-api/Cargo.toml", CARGO, "0.5.0");
        assert_one_line_changed(CARGO, &written);
        // Known knope-versioning gap: an inline comment on the version line
        // itself is dropped (`*version = value(..)` replaces the decor).
        // Worth an upstream fix; every other line is untouched.

        let deps = internal_dependencies("livekit-api/Cargo.toml", CARGO, &["livekit-protocol", "livekit", "serde"]).unwrap();
        assert_eq!(deps, vec!["livekit-protocol", "serde"]);

        let pinned = write_version("livekit-api/Cargo.toml", CARGO, Some("livekit-protocol"), &v("0.4.0")).unwrap();
        assert_one_line_changed(CARGO, &pinned);
        assert!(pinned.contains(r#"livekit-protocol = { version = "0.4.0""#));
    }

    const CARGO_LOCK: &str = r#"# This file is automatically @generated by Cargo.
version = 4

[[package]]
name = "livekit-api"
version = "0.4.3"

[[package]]
name = "serde"
version = "1.0.228"
"#;

    #[test]
    fn cargo_lock_needs_the_package_as_dependency() {
        let written = write_version("Cargo.lock", CARGO_LOCK, Some("livekit-api"), &v("0.5.0")).unwrap();
        assert_one_line_changed(CARGO_LOCK, &written);
        assert!(written.contains("version = \"0.5.0\""));
        assert!(written.contains("version = \"1.0.228\""));
    }

    const PACKAGE_JSON: &str = r#"{
  "name": "@livekit/rtc-node",
  "version": "0.13.2",
  "dependencies": {
    "@livekit/protocol": "1.30.0"
  },
  "devDependencies": {
    "vitest": "^2"
  }
}"#;

    #[test]
    fn package_json() {
        let written = check("packages/livekit-rtc/package.json", PACKAGE_JSON, "0.14.0");
        assert_one_line_changed(PACKAGE_JSON, &written);

        let deps = internal_dependencies("package.json", PACKAGE_JSON, &["@livekit/protocol", "vitest", "nope"]).unwrap();
        assert_eq!(deps, vec!["@livekit/protocol", "vitest"]);
    }

    const PYPROJECT: &str = r#"[project]
name = "livekit-api"
version = "1.1.1"
dependencies = ["livekit-protocol>=1.1.0"]
"#;

    #[test]
    fn pyproject_toml() {
        let written = check("livekit-api/pyproject.toml", PYPROJECT, "1.2.0");
        assert_one_line_changed(PYPROJECT, &written);
    }

    #[test]
    fn pyproject_prerelease_uses_pep440_spelling() {
        let rc = write_version("pyproject.toml", PYPROJECT, None, &v("1.2.0-rc.1")).unwrap();
        assert!(rc.contains("version = \"1.2.0rc1\""), "{rc}");
        assert_eq!(read_version("pyproject.toml", &rc).unwrap(), v("1.2.0-rc.1"));
        let beta = write_version("pyproject.toml", &rc, None, &v("1.2.0-beta.3")).unwrap();
        assert!(beta.contains("version = \"1.2.0b3\""));
        assert_eq!(read_version("pyproject.toml", &beta).unwrap(), v("1.2.0-beta.3"));
        let final_ = write_version("pyproject.toml", &beta, None, &v("1.2.0")).unwrap();
        assert!(final_.contains("version = \"1.2.0\""));
        assert_eq!(to_pep440(&v("1.2.0-next.1")), "1.2.0-next.1", "unknown labels keep semver");
    }

    #[test]
    fn unknown_file_name_is_an_error() {
        assert!(read_version("Package.swift", "// swift").is_err());
    }
}
