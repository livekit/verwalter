//! Spec: WorkspaceConsistency. The workspace manifests say which packages
//! exist and which are private; verwalter.toml says which are versioned.
//! A publishable member that is not configured would silently escape
//! versioning, so `check` reports the difference. A private member may
//! still be configured: livekit-ffi has `publish = false` and ships as
//! binaries on its GitHub release. Cargo, pnpm, uv and Go workspaces
//! are understood.

use std::{
    fs,
    path::{Path, PathBuf},
};

use crate::{config::Config, handler::normalise_py};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Member {
    pub name: String,
    /// Directory relative to the repository root, `.` for the root package.
    pub path: String,
    /// `publish = false`, `"private": true`, `Private :: Do Not Upload`.
    pub private: bool,
    /// Cargo `version.workspace = true`: the version is the root
    /// manifest's `[workspace.package].version`.
    pub inherits_version: bool,
}

/// Every package the workspace manifests declare.
pub fn members(root: &Path) -> Vec<Member> {
    let mut out = Vec::new();
    out.extend(cargo_members(root));
    out.extend(pnpm_members(root));
    out.extend(uv_members(root));
    out.extend(go_members(root));
    out.sort_by(|a, b| a.path.cmp(&b.path));
    out.dedup();
    out
}

fn cargo_members(root: &Path) -> Vec<Member> {
    let Ok(doc) = fs::read_to_string(root.join("Cargo.toml"))
        .and_then(|s| s.parse::<toml::Table>().map_err(std::io::Error::other))
    else {
        return Vec::new();
    };
    let mut dirs: Vec<PathBuf> = Vec::new();
    if let Some(ws) = doc.get("workspace") {
        let patterns = |key: &str| {
            ws.get(key)
                .and_then(|m| m.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|p| p.as_str().map(String::from))
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default()
        };
        let excluded: Vec<PathBuf> = patterns("exclude")
            .iter()
            .flat_map(|p| expand(root, p))
            .collect();
        dirs.extend(
            patterns("members")
                .iter()
                .flat_map(|p| expand(root, p))
                .filter(|d| !excluded.contains(d)),
        );
    }
    if doc.get("package").is_some() {
        dirs.push(root.to_path_buf());
    }
    dirs.into_iter()
        .filter_map(|dir| {
            let manifest = fs::read_to_string(dir.join("Cargo.toml"))
                .ok()?
                .parse::<toml::Table>()
                .ok()?;
            let package = manifest.get("package")?;
            let unpublished = |v: Option<&toml::Value>| match v {
                Some(toml::Value::Boolean(b)) => !b,
                Some(toml::Value::Array(registries)) => registries.is_empty(),
                _ => false,
            };
            let private = match package.get("publish") {
                // `publish.workspace = true` inherits [workspace.package].
                Some(toml::Value::Table(t))
                    if t.get("workspace") == Some(&toml::Value::Boolean(true)) =>
                {
                    unpublished(
                        doc.get("workspace")
                            .and_then(|w| w.get("package"))
                            .and_then(|p| p.get("publish")),
                    )
                }
                other => unpublished(other),
            };
            Some(Member {
                name: package.get("name")?.as_str()?.to_string(),
                path: relative(root, &dir),
                private,
                inherits_version: package
                    .get("version")
                    .and_then(|v| v.get("workspace"))
                    .and_then(|w| w.as_bool())
                    == Some(true),
            })
        })
        .collect()
}

fn pnpm_members(root: &Path) -> Vec<Member> {
    let Ok(yaml) = fs::read_to_string(root.join("pnpm-workspace.yaml")) else {
        return Vec::new();
    };
    // ponytail: the `packages:` list is read line by line; a YAML parser
    // is not worth a dependency for a list of globs.
    let mut patterns = Vec::new();
    let mut in_packages = false;
    for line in yaml.lines() {
        if line.starts_with("packages:") {
            in_packages = true;
            continue;
        }
        if in_packages {
            let t = line.trim();
            if let Some(item) = t.strip_prefix("- ") {
                patterns.push(item.trim_matches(|c| c == '\'' || c == '"').to_string());
            } else if !t.is_empty() && !line.starts_with(' ') {
                break;
            }
        }
    }
    patterns
        .iter()
        .flat_map(|p| expand(root, p))
        .filter_map(|dir| {
            let json: serde_json::Value =
                serde_json::from_str(&fs::read_to_string(dir.join("package.json")).ok()?).ok()?;
            Some(Member {
                name: json.get("name")?.as_str()?.to_string(),
                path: relative(root, &dir),
                private: json
                    .get("private")
                    .and_then(|p| p.as_bool())
                    .unwrap_or(false),
                inherits_version: false,
            })
        })
        .collect()
}

fn uv_members(root: &Path) -> Vec<Member> {
    let Ok(doc) = fs::read_to_string(root.join("pyproject.toml"))
        .and_then(|s| s.parse::<toml::Table>().map_err(std::io::Error::other))
    else {
        return Vec::new();
    };
    let mut dirs: Vec<PathBuf> = doc
        .get("tool")
        .and_then(|t| t.get("uv"))
        .and_then(|u| u.get("workspace"))
        .and_then(|w| w.get("members"))
        .and_then(|m| m.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|p| p.as_str())
                .flat_map(|p| expand(root, p))
                .collect()
        })
        .unwrap_or_default();
    if doc.get("project").is_some() {
        dirs.push(root.to_path_buf());
    }
    dirs.into_iter()
        .filter_map(|dir| {
            let manifest = fs::read_to_string(dir.join("pyproject.toml"))
                .ok()?
                .parse::<toml::Table>()
                .ok()?;
            let project = manifest.get("project")?;
            // `Private :: Do Not Upload` is refused by PyPI; `[tool.uv]
            // package = false` marks a uv workspace root that is no package.
            let private = project
                .get("classifiers")
                .and_then(|c| c.as_array())
                .is_some_and(|c| {
                    c.iter()
                        .any(|v| v.as_str() == Some("Private :: Do Not Upload"))
                })
                || manifest
                    .get("tool")
                    .and_then(|t| t.get("uv"))
                    .and_then(|u| u.get("package"))
                    .and_then(|p| p.as_bool())
                    == Some(false);
            Some(Member {
                name: project.get("name")?.as_str()?.to_string(),
                path: relative(root, &dir),
                private,
                inherits_version: false,
            })
        })
        .collect()
}

/// The modules a root `go.work` uses. A module is named by its directory,
/// which is also its tag prefix; the module at the root by the last
/// element of its module path. Go has no private flag.
fn go_members(root: &Path) -> Vec<Member> {
    let Ok(work) = fs::read_to_string(root.join("go.work")) else {
        return Vec::new();
    };
    let mut dirs = Vec::new();
    let mut in_block = false;
    for line in work.lines() {
        let line = line.split("//").next().unwrap_or_default().trim();
        if in_block {
            if line == ")" {
                in_block = false;
            } else if !line.is_empty() {
                dirs.push(line.trim_matches('"').to_string());
            }
        } else if let Some(rest) = line.strip_prefix("use") {
            match rest.trim() {
                "(" => in_block = true,
                dir if !dir.is_empty() && rest.starts_with([' ', '\t']) => {
                    dirs.push(dir.trim_matches('"').to_string())
                }
                _ => {}
            }
        }
    }
    dirs.into_iter()
        .filter_map(|dir| {
            let path = match dir.trim_start_matches("./").trim_end_matches('/') {
                "" | "." => ".".to_string(),
                p => p.to_string(),
            };
            let (module, _) = crate::handler::go_module(
                &fs::read_to_string(root.join(&path).join("go.mod")).ok()?,
            )?;
            let name = if path == "." {
                let mut parts = module.rsplit('/');
                let last = parts.next().unwrap_or_default();
                let is_major =
                    last.len() > 1 && last.starts_with('v') && last[1..].parse::<u64>().is_ok();
                if is_major {
                    parts.next().unwrap_or(last)
                } else {
                    last
                }
                .to_string()
            } else {
                path.clone()
            };
            Some(Member {
                name,
                path,
                private: false,
                inherits_version: false,
            })
        })
        .collect()
}

/// Directories matching a workspace glob such as `packages/*` or `crates/a`.
/// Only `*` as a whole path component is supported.
/// Spec: Package.versioned_files, the Cargo.lock case. The crates whose Cargo.lock entry moves
/// with the root Cargo.toml's version: those with `version.workspace =
/// true`, private ones included. None when the root manifest's own
/// `[package].version` is the version a bump writes.
pub fn cargo_version_inheritors(root: &Path) -> Vec<Member> {
    let own_version = fs::read_to_string(root.join("Cargo.toml"))
        .ok()
        .and_then(|s| s.parse::<toml::Table>().ok())
        .is_some_and(|doc| {
            doc.get("package")
                .and_then(|p| p.get("version"))
                .is_some_and(|v| v.is_str())
        });
    if own_version {
        return Vec::new();
    }
    cargo_members(root)
        .into_iter()
        .filter(|m| m.inherits_version)
        .collect()
}

pub(crate) fn expand(root: &Path, pattern: &str) -> Vec<PathBuf> {
    let mut dirs = vec![root.to_path_buf()];
    for component in pattern.trim_matches('/').split('/') {
        let mut next = Vec::new();
        for dir in &dirs {
            if component == "*" || component == "**" {
                if let Ok(entries) = fs::read_dir(dir) {
                    let mut found: Vec<PathBuf> = entries
                        .flatten()
                        .map(|e| e.path())
                        .filter(|p| p.is_dir())
                        .collect();
                    found.sort();
                    next.extend(found);
                }
            } else {
                let p = dir.join(component);
                if p.is_dir() {
                    next.push(p);
                }
            }
        }
        dirs = next;
    }
    dirs
}

pub(crate) fn relative(root: &Path, dir: &Path) -> String {
    let rel = dir
        .strip_prefix(root)
        .unwrap_or(dir)
        .to_string_lossy()
        .replace('\\', "/");
    if rel.is_empty() { ".".to_string() } else { rel }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Finding {
    /// A publishable workspace member that verwalter.toml does not list.
    Unmanaged { name: String, path: String },
    /// A configured package whose directory has no manifest.
    Missing { name: String, path: String },
}

impl std::fmt::Display for Finding {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unmanaged { name, path } => write!(
                f,
                "{name} ({path}) is publishable but not in verwalter.toml: add [packages.{name}], match it with `members`, or list it in `exclude`"
            ),
            Self::Missing { name, path } => write!(
                f,
                "{name} is in verwalter.toml but {path} has none of its versioned files: remove it or fix `path`"
            ),
        }
    }
}

/// Spec: WorkspaceConsistency.
pub fn check(root: &Path, config: &Config) -> Vec<Finding> {
    let mut findings = Vec::new();
    let members = members(root);
    // Names compare normalised (PEP 503 style): a pyproject `play_py_ext`
    // is the configured `play-py-ext`.
    let configured = |name: &str| {
        config
            .packages
            .keys()
            .any(|k| normalise_py(k) == normalise_py(name))
    };
    for m in &members {
        if !m.private
            && !configured(&m.name)
            && !excluded(&config.workspace.exclude, &m.name, &m.path)
        {
            findings.push(Finding::Unmanaged {
                name: m.name.clone(),
                path: m.path.clone(),
            });
        }
    }
    for (name, p) in &config.packages {
        let present = root.join(&p.path).is_dir()
            && p.versioning
                .files
                .iter()
                .any(|f| root.join(f.path()).is_file());
        if !present {
            findings.push(Finding::Missing {
                name: name.clone(),
                path: p.path.clone(),
            });
        }
    }
    findings
}

/// `exclude` entries match a member's name or its path; a trailing `*`
/// matches a prefix (`examples/*`).
pub(crate) fn excluded(exclude: &[String], name: &str, path: &str) -> bool {
    exclude
        .iter()
        .any(|pat| matches(pat, name) || matches(pat, path))
}

fn matches(pattern: &str, value: &str) -> bool {
    match pattern.strip_suffix('*') {
        Some(prefix) => value.starts_with(prefix),
        None => pattern == value,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Tmp(PathBuf);
    impl Drop for Tmp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn tmp(name: &str) -> Tmp {
        let root = std::env::temp_dir().join(format!("verwalter-ws-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        Tmp(root)
    }

    fn write(root: &Path, rel: &str, content: &str) {
        let p = root.join(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, content).unwrap();
    }

    #[test]
    fn cargo_workspace_members_and_publish_flag() {
        let t = tmp("cargo");
        write(
            &t.0,
            "Cargo.toml",
            "[workspace]\nmembers = [\"crates/*\", \"tools/bench\"]\nexclude = [\"crates/skipped\"]\n\n[workspace.package]\npublish = false\n",
        );
        write(
            &t.0,
            "crates/inherits/Cargo.toml",
            "[package]\nname = \"inherits\"\nversion = \"0.1.0\"\npublish.workspace = true\n",
        );
        write(
            &t.0,
            "crates/a/Cargo.toml",
            "[package]\nname = \"a\"\nversion = \"0.1.0\"\n",
        );
        write(
            &t.0,
            "crates/b/Cargo.toml",
            "[package]\nname = \"b\"\nversion = \"0.1.0\"\npublish = false\n",
        );
        write(
            &t.0,
            "crates/skipped/Cargo.toml",
            "[package]\nname = \"skipped\"\nversion = \"0.1.0\"\n",
        );
        write(
            &t.0,
            "tools/bench/Cargo.toml",
            "[package]\nname = \"bench\"\nversion = \"0.1.0\"\npublish = []\n",
        );
        let m = members(&t.0);
        let v: Vec<(&str, &str, bool)> = m
            .iter()
            .map(|m| (m.name.as_str(), m.path.as_str(), m.private))
            .collect();
        assert_eq!(
            v,
            [
                ("a", "crates/a", false),
                ("b", "crates/b", true),
                ("inherits", "crates/inherits", true),
                ("bench", "tools/bench", true)
            ]
        );
    }

    #[test]
    fn pnpm_and_uv_members() {
        let t = tmp("pnpm-uv");
        write(
            &t.0,
            "pnpm-workspace.yaml",
            "packages:\n  - 'packages/*'\n  - 'examples/*'\n\ncatalog:\n  x: 1\n",
        );
        write(
            &t.0,
            "packages/sdk/package.json",
            r#"{"name": "sdk", "version": "1.0.0"}"#,
        );
        write(
            &t.0,
            "examples/demo/package.json",
            r#"{"name": "demo", "private": true}"#,
        );
        write(
            &t.0,
            "pyproject.toml",
            "[project]\nname = \"root\"\nversion = \"0\"\n[tool.uv]\npackage = false\n[tool.uv.workspace]\nmembers = [\"py/*\"]\n",
        );
        write(
            &t.0,
            "py/lib/pyproject.toml",
            "[project]\nname = \"lib\"\nversion = \"1.0.0\"\n",
        );
        write(
            &t.0,
            "py/internal/pyproject.toml",
            "[project]\nname = \"internal\"\nversion = \"1.0.0\"\nclassifiers = [\"Private :: Do Not Upload\"]\n",
        );
        let m = members(&t.0);
        let v: Vec<(&str, bool)> = m.iter().map(|m| (m.name.as_str(), m.private)).collect();
        assert_eq!(
            v,
            [
                ("root", true),
                ("demo", true),
                ("sdk", false),
                ("internal", true),
                ("lib", false)
            ]
        );
    }

    #[test]
    fn findings_and_exclude() {
        let t = tmp("findings");
        write(
            &t.0,
            "Cargo.toml",
            "[workspace]\nmembers = [\"crates/*\", \"examples/*\"]\n",
        );
        write(
            &t.0,
            "crates/a/Cargo.toml",
            "[package]\nname = \"a\"\nversion = \"0.1.0\"\n",
        );
        write(
            &t.0,
            "crates/b/Cargo.toml",
            "[package]\nname = \"b\"\nversion = \"0.1.0\"\n",
        );
        write(
            &t.0,
            "crates/c/Cargo.toml",
            "[package]\nname = \"c\"\nversion = \"0.1.0\"\npublish = false\n",
        );
        write(
            &t.0,
            "examples/e/Cargo.toml",
            "[package]\nname = \"e\"\nversion = \"0.1.0\"\n",
        );
        let config = Config::parse(
            "[workspace]\nexclude = [\"examples/*\", \"b\"]\n[packages.a]\npath = \"crates/a\"\nversioning.files = [\"Cargo.toml\"]\nversioning.changelog = \"CHANGELOG.md\"\n[packages.c]\npath = \"crates/c\"\nversioning.files = [\"Cargo.toml\"]\nversioning.changelog = \"x\"\n[packages.gone]\npath = \"crates/gone\"\nversioning.files = [\"Cargo.toml\"]\nversioning.changelog = \"x\"\n",
        )
        .unwrap();
        let f = check(&t.0, &config);
        assert_eq!(
            f,
            [Finding::Missing {
                name: "gone".into(),
                path: "crates/gone".into()
            }],
            "b and examples/* are excluded; a and the private c are fine"
        );
        let open = Config::parse("[packages.a]\npath = \"crates/a\"\nversioning.files = [\"Cargo.toml\"]\nversioning.changelog = \"x\"\n").unwrap();
        let unmanaged: Vec<String> = check(&t.0, &open)
            .iter()
            .filter_map(|f| {
                if let Finding::Unmanaged { name, .. } = f {
                    Some(name.clone())
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(unmanaged, ["b", "e"]);
    }

    // Spec: config.workspace.members and exclude.
    #[test]
    fn members_find_publishable_packages() {
        let t = tmp("members");
        write(&t.0, "pnpm-workspace.yaml", "packages:\n  - 'packages/*'\n");
        for (dir, json) in [
            ("sdk", r#"{"name": "sdk", "version": "1.0.0"}"#),
            ("rtc", r#"{"name": "rtc", "version": "1.0.0"}"#),
            ("internal", r#"{"name": "internal", "private": true}"#),
            ("skip", r#"{"name": "skip", "version": "1.0.0"}"#),
        ] {
            write(&t.0, &format!("packages/{dir}/package.json"), json);
        }
        write(&t.0, "packages/docs/README.md", "");
        write(
            &t.0,
            "tools/x/package.json",
            r#"{"name": "x", "version": "1.0.0"}"#,
        );
        let load = |exclude: &str| {
            write(
                &t.0,
                "verwalter.toml",
                &format!(
                    "[workspace]\nmembers = [\"packages/*\"]\nexclude = {exclude}\n[versioning]\nfiles = [\"package.json\"]\n[packages.rtc]\nrelease.draft = true\n[packages.x]\npath = \"tools/x\"\n"
                ),
            );
            Config::load(&t.0, &t.0.join("verwalter.toml"))
        };
        let e = load("[\"skip\"]").unwrap_err();
        assert!(e.to_string().contains("packages/docs"), "{e}");
        let c = load("[\"skip\", \"packages/docs\"]").unwrap();
        assert_eq!(c.packages.keys().collect::<Vec<_>>(), ["rtc", "sdk", "x"]);
        assert_eq!(c.packages["rtc"].path, "packages/rtc");
        assert!(c.packages["rtc"].release.draft);
        assert_eq!(
            c.packages["sdk"].versioning.files[0].path(),
            "packages/sdk/package.json"
        );
        assert!(check(&t.0, &c).is_empty());
        // Without a checkout, an entry without a path is an error.
        assert!(
            Config::parse(
                "[versioning]\nfiles = [\"package.json\"]\n[packages.rtc]\nrelease.draft = true\n"
            )
            .is_err()
        );
    }

    #[test]
    fn go_work_members_are_named_by_directory() {
        let t = tmp("go-work");
        write(
            &t.0,
            "go.work",
            "go 1.24\n\nuse .\n\nuse (\n\t./sdk/auth // the auth module\n\t\"./sdk/core\"\n\t./missing\n)\n",
        );
        write(&t.0, "go.mod", "module github.com/acme/sdk/v2 // v2.1.0\n");
        write(&t.0, "sdk/auth/go.mod", "module github.com/acme/sdk/auth\n");
        write(&t.0, "sdk/core/go.mod", "module github.com/acme/sdk/core\n");
        let v: Vec<(String, String)> = members(&t.0)
            .into_iter()
            .map(|m| (m.name, m.path))
            .collect();
        assert_eq!(
            v,
            [
                ("sdk".into(), ".".into()),
                ("sdk/auth".into(), "sdk/auth".into()),
                ("sdk/core".into(), "sdk/core".into())
            ]
        );
        // `members` takes them like any workspace, with the Go tag default.
        write(
            &t.0,
            "verwalter.toml",
            "[workspace]\nmembers = [\"sdk/*\"]\n[versioning]\nfiles = [\"go.mod\"]\n",
        );
        let c = Config::load(&t.0, &t.0.join("verwalter.toml")).unwrap();
        assert_eq!(
            c.packages.keys().collect::<Vec<_>>(),
            ["sdk/auth", "sdk/core"]
        );
        let v: crate::Version = "1.2.0".parse().unwrap();
        assert_eq!(c.tag_for("sdk/auth", &v), "sdk/auth/v1.2.0");
        assert_eq!(c.dependency_key("sdk/core"), "github.com/acme/sdk/core");
    }

    #[test]
    fn go_modules_in_major_version_directories_are_refused() {
        let t = tmp("go-major-dir");
        write(&t.0, "go.mod", "module github.com/acme/app // v1.0.0\n");
        write(
            &t.0,
            "lib/v2/go.mod",
            "module github.com/acme/lib/v2 // v2.0.0\n",
        );
        let load = |extra: &str| {
            write(
                &t.0,
                "verwalter.toml",
                &format!(
                    "[versioning]\nfiles = [\"go.mod\"]\n[packages.app]\npath = \".\"\n[packages.lib]\npath = \"lib/v2\"\n{extra}"
                ),
            );
            Config::load(&t.0, &t.0.join("verwalter.toml"))
        };
        assert!(
            load("")
                .unwrap_err()
                .to_string()
                .contains("major-version directory")
        );
        let c = load("release.tag = \"lib/v{version}\"\n").unwrap();
        let v: crate::Version = "1.2.0".parse().unwrap();
        assert_eq!(c.tag_for("app", &v), "v1.2.0", "the root module of several");
    }
}
