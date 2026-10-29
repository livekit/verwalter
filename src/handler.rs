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

use crate::{Error, Version};

const NO_TAGS: &[&str] = &[];

fn is_pyproject(path: &str) -> bool {
    path.rsplit('/').next() == Some("pyproject.toml")
}

/// PEP 440 pre-release spelling for pyproject.toml: `1.6.0-rc.1` is
/// written as `1.6.0rc1`, `alpha`/`beta` as `a`/`b`. Other labels keep the
/// semver form, which Python tools accept and normalise.
pub fn to_pep440(version: &Version) -> String {
    let s = version.to_string();
    let Some((stable, pre)) = s.split_once('-') else {
        return s;
    };
    let Some((label, n)) = pre.rsplit_once('.') else {
        return s;
    };
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
            if !n.is_empty()
                && n.bytes().all(|b| b.is_ascii_digit())
                && stable
                    .split('.')
                    .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
            {
                let semver_label = match label {
                    "a" => "alpha",
                    "b" => "beta",
                    _ => "rc",
                };
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

fn load(
    path: &str,
    regex: Option<&str>,
    dependency: Option<&str>,
    content: &str,
) -> Result<(VersionedFileConfig, VersionedFile), Error> {
    let regex = regex.map(|r| vec![r.to_string()]);
    let config = VersionedFileConfig::new(
        RelativePathBuf::from(path),
        dependency.map(String::from),
        regex,
    )?;
    let content = if is_pyproject(path) {
        normalise_pyproject(content)
    } else {
        content.to_string()
    };
    let file = VersionedFile::new(&config, content, NO_TAGS)?;
    Ok((config, file))
}

/// The package version in a manifest. Lock files have none. `regex` picks
/// the version out of a file knope has no format for (config::VersionedFile).
pub fn read_version(path: &str, regex: Option<&str>, content: &str) -> Result<Version, Error> {
    Ok(load(path, regex, None, content)?.1.version()?)
}

/// Spec: write_version (`dependency` = None) and write_dependency_version
/// (`dependency` = Some). Lock files always need the dependency, which is
/// the package itself for its own lock file. Content without a matching
/// entry comes back unchanged.
///
/// Invariant MinimalEdit holds for TOML (toml_edit). package.json is
/// re-serialized with two-space indentation, which is what prettier
/// produces in node-sdks.
pub fn write_version(
    path: &str,
    regex: Option<&str>,
    content: &str,
    dependency: Option<&str>,
    version: &Version,
) -> Result<String, Error> {
    if regex.is_some() && dependency.is_some() {
        // A regex file carries one version, its own. Nothing to pin.
        return Ok(content.to_string());
    }
    if let (true, Some(dep)) = (path.rsplit('/').next() == Some("Cargo.toml"), dependency) {
        // A `workspace = true` dependency takes its version from the root
        // [workspace.dependencies]; cargo rejects a `version` next to it.
        // knope would add one, so leave such entries alone.
        let doc: DocumentMut = content.parse()?;
        let inherits = ["dependencies", "dev-dependencies", "build-dependencies"]
            .iter()
            .any(|section| {
                doc.get(section)
                    .and_then(|d| d.get(dep))
                    .and_then(|e| e.get("workspace"))
                    .and_then(|w| w.as_bool())
                    == Some(true)
            });
        if inherits {
            return Ok(content.to_string());
        }
    }
    if let (true, Some(dep)) = (is_pyproject(path), dependency) {
        // knope's pyproject handler has no dependency mode; it would set the
        // package's own version instead.
        return Ok(pyproject_pin(content, dep, version));
    }
    let (config, file) = load(path, regex, dependency, content)?;
    let mut package = Package::new(
        Name::from(dependency.unwrap_or("package")),
        NO_TAGS,
        vec![config],
        std::slice::from_ref(&file),
        ReleaseNotes::default(),
        None,
    )?;
    let files = package.set_version(version.clone(), GoVersioning::default(), vec![file])?;
    let written = files
        .into_iter()
        .next()
        .and_then(VersionedFile::write)
        .and_then(|actions| {
            actions.into_iter().find_map(|a| match a {
                Action::WriteToFile { content, .. } => Some(content),
                _ => None,
            })
        });
    let written = written.unwrap_or_else(|| content.to_string());
    if is_pyproject(path) && version.is_prerelease() {
        return Ok(written.replace(
            &format!("\"{version}\""),
            &format!("\"{}\"", to_pep440(version)),
        ));
    }
    Ok(written)
}

/// PEP 503 normalised name: lower case, runs of `-`, `_` and `.` as `-`.
fn normalise_py(name: &str) -> String {
    let mut out = String::new();
    let mut dash = false;
    for c in name.chars() {
        if matches!(c, '-' | '_' | '.') {
            dash = true;
        } else {
            if dash && !out.is_empty() {
                out.push('-');
            }
            dash = false;
            out.push(c.to_ascii_lowercase());
        }
    }
    out
}

/// Every requirement string under `[project] dependencies` and
/// `[project.optional-dependencies]`.
fn pyproject_requirements(content: &str) -> Vec<String> {
    let Ok(doc) = content.parse::<toml::Value>() else {
        return Vec::new();
    };
    let Some(project) = doc.get("project") else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let mut take = |v: Option<&toml::Value>| {
        if let Some(arr) = v.and_then(|v| v.as_array()) {
            out.extend(arr.iter().filter_map(|r| r.as_str()).map(String::from));
        }
    };
    take(project.get("dependencies"));
    if let Some(groups) = project
        .get("optional-dependencies")
        .and_then(|g| g.as_table())
    {
        for g in groups.values() {
            take(Some(g));
        }
    }
    out
}

/// The distribution name at the start of a PEP 508 requirement.
fn requirement_name(req: &str) -> &str {
    req.trim_start()
        .split(|c: char| !(c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.')))
        .next()
        .unwrap_or("")
}

/// Spec: write_dependency_version for pyproject.toml. Moves the lower bound
/// (`>=`, `==`, `~=`) of every requirement on `dep` to `version`, in PEP
/// 440 spelling; upper bounds and markers stay. Line-based, so formatting
/// and comments are preserved.
fn pyproject_pin(content: &str, dep: &str, version: &Version) -> String {
    let wanted = normalise_py(dep);
    let new = to_pep440(version);
    let re = regex::Regex::new(r#"(["'])\s*([A-Za-z0-9][A-Za-z0-9._-]*)(\[[^\]]*\])?\s*(>=|==|~=)\s*([0-9][0-9A-Za-z.+!*-]*)"#).expect("static regex");
    re.replace_all(content, |caps: &regex::Captures| {
        let whole = caps.get(0).expect("match");
        if normalise_py(&caps[2]) == wanted {
            // Keep everything up to the old version, spacing included.
            let old_version = caps.get(5).expect("version group");
            format!("{}{new}", &content[whole.start()..old_version.start()])
        } else {
            whole.as_str().to_string()
        }
    })
    .into_owned()
}

/// Spec: internal_dependencies. Which of `candidates` (the other package
/// names in the repo) this manifest depends on. Only runtime dependencies
/// count: a devDependency (package.json) does not make a package a
/// dependent for bumps and ordering, as in changesets.
pub fn internal_dependencies(
    path: &str,
    content: &str,
    candidates: &[&str],
) -> Result<Vec<String>, Error> {
    let file_name = path.rsplit('/').next().unwrap_or(path);
    let found: Vec<String> = match file_name {
        "Cargo.toml" => {
            let doc: DocumentMut = content.parse()?;
            candidates
                .iter()
                .filter(|c| cargo::contains_dependency(&doc, c))
                .map(|c| c.to_string())
                .collect()
        }
        "package.json" => {
            let json: serde_json::Value = serde_json::from_str(content)?;
            let sections = ["dependencies", "peerDependencies", "optionalDependencies"];
            candidates
                .iter()
                .filter(|c| {
                    sections
                        .iter()
                        .any(|s| json.get(s).and_then(|d| d.get(**c)).is_some())
                })
                .map(|c| c.to_string())
                .collect()
        }
        "pyproject.toml" => {
            let names: Vec<String> = pyproject_requirements(content)
                .iter()
                .map(|r| normalise_py(requirement_name(r)))
                .collect();
            candidates
                .iter()
                .filter(|c| names.contains(&normalise_py(c)))
                .map(|c| c.to_string())
                .collect()
        }
        // ponytail: pubspec.yaml and go.mod report no dependencies yet; add
        // a branch when a monorepo in that language needs dependency ordering.
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
        assert_ne!(read_version(path, None, sample).unwrap(), next);
        let written = write_version(path, None, sample, None, &next).unwrap();
        assert_eq!(
            read_version(path, None, &written).unwrap(),
            next,
            "Roundtrip"
        );
        written
    }

    fn assert_one_line_changed(before: &str, after: &str) {
        assert_eq!(before.lines().count(), after.lines().count());
        let changed = before
            .lines()
            .zip(after.lines())
            .filter(|(a, b)| a != b)
            .count();
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

        let deps = internal_dependencies(
            "livekit-api/Cargo.toml",
            CARGO,
            &["livekit-protocol", "livekit", "serde"],
        )
        .unwrap();
        assert_eq!(deps, vec!["livekit-protocol", "serde"]);

        let pinned = write_version(
            "livekit-api/Cargo.toml",
            None,
            CARGO,
            Some("livekit-protocol"),
            &v("0.4.0"),
        )
        .unwrap();
        assert_one_line_changed(CARGO, &pinned);
        assert!(pinned.contains(r#"livekit-protocol = { version = "0.4.0""#));
    }

    const CARGO_WORKSPACE_DEP: &str = r#"[package]
name = "livekit-ffi"
version = "0.13.2"

[dependencies]
webrtc-sys = { workspace = true, optional = true }
livekit = { workspace = true }
serde = "1"
"#;

    #[test]
    fn cargo_workspace_inherited_dependency_is_not_pinned() {
        let out = write_version(
            "livekit-ffi/Cargo.toml",
            None,
            CARGO_WORKSPACE_DEP,
            Some("webrtc-sys"),
            &v("0.3.52"),
        )
        .unwrap();
        assert_eq!(
            out, CARGO_WORKSPACE_DEP,
            "version comes from [workspace.dependencies]"
        );
        let deps = internal_dependencies(
            "livekit-ffi/Cargo.toml",
            CARGO_WORKSPACE_DEP,
            &["webrtc-sys", "livekit"],
        )
        .unwrap();
        assert_eq!(
            deps,
            vec!["webrtc-sys", "livekit"],
            "still a dependency for ordering and bumps"
        );
        // The root manifest is where the pin lives.
        let root = "[workspace]\nmembers = [\"livekit-ffi\"]\n\n[workspace.dependencies]\nwebrtc-sys = { version = \"0.3.51\", path = \"webrtc-sys\" }\n";
        let pinned =
            write_version("Cargo.toml", None, root, Some("webrtc-sys"), &v("0.3.52")).unwrap();
        assert!(pinned.contains(r#"webrtc-sys = { version = "0.3.52""#));
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
        let written = write_version(
            "Cargo.lock",
            None,
            CARGO_LOCK,
            Some("livekit-api"),
            &v("0.5.0"),
        )
        .unwrap();
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

        let deps = internal_dependencies(
            "package.json",
            PACKAGE_JSON,
            &["@livekit/protocol", "vitest", "nope"],
        )
        .unwrap();
        assert_eq!(
            deps,
            vec!["@livekit/protocol"],
            "devDependencies make no edge"
        );
    }

    const PYPROJECT: &str = r#"[project]
name = "livekit-api"
version = "1.1.1"
dependencies = [
    "livekit-protocol>=1.1.0,<2.0.0", # keep in sync
    'Livekit_RTC[extra] == 1.2.0; python_version >= "3.9"',
    "aiohttp>=3.9",
]

[project.optional-dependencies]
dev = ["livekit-test-utils~=0.3"]
"#;

    #[test]
    fn pyproject_toml() {
        let written = check("livekit-api/pyproject.toml", PYPROJECT, "1.2.0");
        assert_one_line_changed(PYPROJECT, &written);
    }

    #[test]
    fn pyproject_dependencies_and_lower_bound_pins() {
        let deps = internal_dependencies(
            "livekit-api/pyproject.toml",
            PYPROJECT,
            &[
                "livekit-protocol",
                "livekit-rtc",
                "livekit-test-utils",
                "aiohttp",
                "livekit",
            ],
        )
        .unwrap();
        assert_eq!(
            deps,
            vec![
                "livekit-protocol",
                "livekit-rtc",
                "livekit-test-utils",
                "aiohttp"
            ]
        );

        let pinned = write_version(
            "livekit-api/pyproject.toml",
            None,
            PYPROJECT,
            Some("livekit-protocol"),
            &v("1.2.0"),
        )
        .unwrap();
        assert!(
            pinned.contains(r#""livekit-protocol>=1.2.0,<2.0.0", # keep in sync"#),
            "{pinned}"
        );
        assert!(
            pinned.contains(r#"version = "1.1.1""#),
            "own version untouched"
        );
        assert_one_line_changed(PYPROJECT, &pinned);

        let rc = write_version(
            "livekit-api/pyproject.toml",
            None,
            PYPROJECT,
            Some("livekit-rtc"),
            &v("1.3.0-rc.1"),
        )
        .unwrap();
        assert!(
            rc.contains(r#"'Livekit_RTC[extra] == 1.3.0rc1; python_version >= "3.9"'"#),
            "{rc}"
        );

        let tilde = write_version(
            "livekit-api/pyproject.toml",
            None,
            PYPROJECT,
            Some("livekit-test-utils"),
            &v("0.4.0"),
        )
        .unwrap();
        assert!(tilde.contains(r#""livekit-test-utils~=0.4.0""#));

        let none = write_version(
            "livekit-api/pyproject.toml",
            None,
            PYPROJECT,
            Some("nope"),
            &v("9.9.9"),
        )
        .unwrap();
        assert_eq!(none, PYPROJECT);
    }

    #[test]
    fn pyproject_prerelease_uses_pep440_spelling() {
        let rc = write_version("pyproject.toml", None, PYPROJECT, None, &v("1.2.0-rc.1")).unwrap();
        assert!(rc.contains("version = \"1.2.0rc1\""), "{rc}");
        assert_eq!(
            read_version("pyproject.toml", None, &rc).unwrap(),
            v("1.2.0-rc.1")
        );
        let beta = write_version("pyproject.toml", None, &rc, None, &v("1.2.0-beta.3")).unwrap();
        assert!(beta.contains("version = \"1.2.0b3\""));
        assert_eq!(
            read_version("pyproject.toml", None, &beta).unwrap(),
            v("1.2.0-beta.3")
        );
        let final_ = write_version("pyproject.toml", None, &beta, None, &v("1.2.0")).unwrap();
        assert!(final_.contains("version = \"1.2.0\""));
        assert_eq!(
            to_pep440(&v("1.2.0-next.1")),
            "1.2.0-next.1",
            "unknown labels keep semver"
        );
    }

    #[test]
    fn unknown_file_name_is_an_error() {
        assert!(read_version("Package.swift", None, "// swift").is_err());
    }

    const GRADLE: &str = "GROUP=io.livekit\nVERSION_NAME=2.29.2\nPOM_LICENCE_NAME=The Apache Software License, Version 2.0\n";
    const DART: &str = "class LiveKitClient {\n  static const version = '2.13.1';\n}\n";

    // Obligation contract-signature.VersionedFileHandler.* for regex files:
    // only the `version` group changes, pre-releases round-trip, a pin
    // request leaves the file alone, and a regex without the group fails.
    #[test]
    fn regex_file() {
        let re = Some("VERSION_NAME=(?<version>.+)");
        assert_eq!(
            read_version("gradle.properties", re, GRADLE).unwrap(),
            v("2.29.2")
        );
        let written =
            write_version("gradle.properties", re, GRADLE, None, &v("2.30.0-rc.1")).unwrap();
        assert_eq!(
            written,
            GRADLE.replace("2.29.2", "2.30.0-rc.1"),
            "MinimalEdit"
        );
        assert_eq!(
            read_version("gradle.properties", re, &written).unwrap(),
            v("2.30.0-rc.1"),
            "Roundtrip"
        );

        let re = Some("version = '(?<version>[^']+)'");
        let written = write_version("lib/src/livekit.dart", re, DART, None, &v("2.14.0")).unwrap();
        assert_eq!(written, DART.replace("2.13.1", "2.14.0"));
        assert_eq!(
            write_version("lib/src/livekit.dart", re, DART, Some("other"), &v("9.9.9")).unwrap(),
            DART
        );

        assert!(read_version("gradle.properties", Some("VERSION_NAME=(.+)"), GRADLE).is_err());
        assert!(read_version("gradle.properties", Some("NOPE=(?<version>.+)"), GRADLE).is_err());
    }
}
