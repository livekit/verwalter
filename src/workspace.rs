//! Spec: WorkspaceConsistency. The workspace manifests say which packages
//! exist and which are private; verwalter.toml says which are versioned.
//! A publishable member that is not configured would silently escape
//! versioning, so `check` reports the difference. A private member may
//! still be configured: livekit-ffi has `publish = false` and ships as
//! binaries on its GitHub release. Cargo, pnpm and uv workspaces are
//! understood.

use std::{
    fs,
    path::{Path, PathBuf},
};

use crate::config::Config;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Member {
    pub name: String,
    /// Directory relative to the repository root, `.` for the root package.
    pub path: String,
    /// `publish = false`, `"private": true`, `Private :: Do Not Upload`.
    pub private: bool,
}

/// Every package the workspace manifests declare.
pub fn members(root: &Path) -> Vec<Member> {
    let mut out = Vec::new();
    out.extend(cargo_members(root));
    out.extend(pnpm_members(root));
    out.extend(uv_members(root));
    out.sort_by(|a, b| a.path.cmp(&b.path));
    out.dedup();
    out
}

fn cargo_members(root: &Path) -> Vec<Member> {
    let Ok(doc) = fs::read_to_string(root.join("Cargo.toml")).and_then(|s| s.parse::<toml::Value>().map_err(std::io::Error::other)) else { return Vec::new() };
    let mut dirs: Vec<PathBuf> = Vec::new();
    if let Some(ws) = doc.get("workspace") {
        let patterns = |key: &str| ws.get(key).and_then(|m| m.as_array()).map(|a| a.iter().filter_map(|p| p.as_str().map(String::from)).collect::<Vec<_>>()).unwrap_or_default();
        let excluded: Vec<PathBuf> = patterns("exclude").iter().flat_map(|p| expand(root, p)).collect();
        dirs.extend(patterns("members").iter().flat_map(|p| expand(root, p)).filter(|d| !excluded.contains(d)));
    }
    if doc.get("package").is_some() {
        dirs.push(root.to_path_buf());
    }
    dirs.into_iter()
        .filter_map(|dir| {
            let manifest = fs::read_to_string(dir.join("Cargo.toml")).ok()?.parse::<toml::Value>().ok()?;
            let package = manifest.get("package")?;
            let private = match package.get("publish") {
                Some(toml::Value::Boolean(b)) => !b,
                Some(toml::Value::Array(registries)) => registries.is_empty(),
                _ => false,
            };
            Some(Member { name: package.get("name")?.as_str()?.to_string(), path: relative(root, &dir), private })
        })
        .collect()
}

fn pnpm_members(root: &Path) -> Vec<Member> {
    let Ok(yaml) = fs::read_to_string(root.join("pnpm-workspace.yaml")) else { return Vec::new() };
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
            let json: serde_json::Value = serde_json::from_str(&fs::read_to_string(dir.join("package.json")).ok()?).ok()?;
            Some(Member {
                name: json.get("name")?.as_str()?.to_string(),
                path: relative(root, &dir),
                private: json.get("private").and_then(|p| p.as_bool()).unwrap_or(false),
            })
        })
        .collect()
}

fn uv_members(root: &Path) -> Vec<Member> {
    let Ok(doc) = fs::read_to_string(root.join("pyproject.toml")).and_then(|s| s.parse::<toml::Value>().map_err(std::io::Error::other)) else { return Vec::new() };
    let mut dirs: Vec<PathBuf> = doc
        .get("tool")
        .and_then(|t| t.get("uv"))
        .and_then(|u| u.get("workspace"))
        .and_then(|w| w.get("members"))
        .and_then(|m| m.as_array())
        .map(|a| a.iter().filter_map(|p| p.as_str()).flat_map(|p| expand(root, p)).collect())
        .unwrap_or_default();
    if doc.get("project").is_some() {
        dirs.push(root.to_path_buf());
    }
    dirs.into_iter()
        .filter_map(|dir| {
            let manifest = fs::read_to_string(dir.join("pyproject.toml")).ok()?.parse::<toml::Value>().ok()?;
            let project = manifest.get("project")?;
            // `Private :: Do Not Upload` is refused by PyPI; `[tool.uv]
            // package = false` marks a uv workspace root that is no package.
            let private = project
                .get("classifiers")
                .and_then(|c| c.as_array())
                .is_some_and(|c| c.iter().any(|v| v.as_str() == Some("Private :: Do Not Upload")))
                || manifest.get("tool").and_then(|t| t.get("uv")).and_then(|u| u.get("package")).and_then(|p| p.as_bool()) == Some(false);
            Some(Member { name: project.get("name")?.as_str()?.to_string(), path: relative(root, &dir), private })
        })
        .collect()
}

/// Directories matching a workspace glob such as `packages/*` or `crates/a`.
/// Only `*` as a whole path component is supported.
fn expand(root: &Path, pattern: &str) -> Vec<PathBuf> {
    let mut dirs = vec![root.to_path_buf()];
    for component in pattern.trim_matches('/').split('/') {
        let mut next = Vec::new();
        for dir in &dirs {
            if component == "*" || component == "**" {
                if let Ok(entries) = fs::read_dir(dir) {
                    let mut found: Vec<PathBuf> = entries.flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect();
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

fn relative(root: &Path, dir: &Path) -> String {
    let rel = dir.strip_prefix(root).unwrap_or(dir).to_string_lossy().replace('\\', "/");
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
            Self::Unmanaged { name, path } => write!(f, "{name} ({path}) is publishable but not in verwalter.toml: add [packages.{name}] or list it under `ignore`"),
            Self::Missing { name, path } => write!(f, "{name} is in verwalter.toml but {path} has no package manifest: remove it or fix `path`"),
        }
    }
}

/// Spec: WorkspaceConsistency. `ignore` entries match a member's name or
/// its path; a trailing `*` matches a prefix (`examples/*`).
pub fn check(root: &Path, config: &Config) -> Vec<Finding> {
    let ignored = |m: &Member| config.ignore.iter().any(|pat| matches(pat, &m.name) || matches(pat, &m.path));
    let mut findings = Vec::new();
    let members = members(root);
    for m in &members {
        if !m.private && !config.packages.contains_key(&m.name) && !ignored(m) {
            findings.push(Finding::Unmanaged { name: m.name.clone(), path: m.path.clone() });
        }
    }
    for (name, p) in &config.packages {
        let dir = root.join(&p.path);
        let has_manifest = ["Cargo.toml", "package.json", "pyproject.toml", "pubspec.yaml", "go.mod"].iter().any(|m| dir.join(m).is_file());
        if !has_manifest {
            findings.push(Finding::Missing { name: name.clone(), path: p.path.clone() });
        }
    }
    findings
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
        write(&t.0, "Cargo.toml", "[workspace]\nmembers = [\"crates/*\", \"tools/bench\"]\nexclude = [\"crates/skipped\"]\n");
        write(&t.0, "crates/a/Cargo.toml", "[package]\nname = \"a\"\nversion = \"0.1.0\"\n");
        write(&t.0, "crates/b/Cargo.toml", "[package]\nname = \"b\"\nversion = \"0.1.0\"\npublish = false\n");
        write(&t.0, "crates/skipped/Cargo.toml", "[package]\nname = \"skipped\"\nversion = \"0.1.0\"\n");
        write(&t.0, "tools/bench/Cargo.toml", "[package]\nname = \"bench\"\nversion = \"0.1.0\"\npublish = []\n");
        let m = members(&t.0);
        let v: Vec<(&str, &str, bool)> = m.iter().map(|m| (m.name.as_str(), m.path.as_str(), m.private)).collect();
        assert_eq!(v, [("a", "crates/a", false), ("b", "crates/b", true), ("bench", "tools/bench", true)]);
    }

    #[test]
    fn pnpm_and_uv_members() {
        let t = tmp("pnpm-uv");
        write(&t.0, "pnpm-workspace.yaml", "packages:\n  - 'packages/*'\n  - 'examples/*'\n\ncatalog:\n  x: 1\n");
        write(&t.0, "packages/sdk/package.json", r#"{"name": "sdk", "version": "1.0.0"}"#);
        write(&t.0, "examples/demo/package.json", r#"{"name": "demo", "private": true}"#);
        write(&t.0, "pyproject.toml", "[project]\nname = \"root\"\nversion = \"0\"\n[tool.uv]\npackage = false\n[tool.uv.workspace]\nmembers = [\"py/*\"]\n");
        write(&t.0, "py/lib/pyproject.toml", "[project]\nname = \"lib\"\nversion = \"1.0.0\"\n");
        write(&t.0, "py/internal/pyproject.toml", "[project]\nname = \"internal\"\nversion = \"1.0.0\"\nclassifiers = [\"Private :: Do Not Upload\"]\n");
        let m = members(&t.0);
        let v: Vec<(&str, bool)> = m.iter().map(|m| (m.name.as_str(), m.private)).collect();
        assert_eq!(v, [("root", true), ("demo", true), ("sdk", false), ("internal", true), ("lib", false)]);
    }

    #[test]
    fn findings_and_ignore() {
        let t = tmp("findings");
        write(&t.0, "Cargo.toml", "[workspace]\nmembers = [\"crates/*\", \"examples/*\"]\n");
        write(&t.0, "crates/a/Cargo.toml", "[package]\nname = \"a\"\nversion = \"0.1.0\"\n");
        write(&t.0, "crates/b/Cargo.toml", "[package]\nname = \"b\"\nversion = \"0.1.0\"\n");
        write(&t.0, "crates/c/Cargo.toml", "[package]\nname = \"c\"\nversion = \"0.1.0\"\npublish = false\n");
        write(&t.0, "examples/e/Cargo.toml", "[package]\nname = \"e\"\nversion = \"0.1.0\"\n");
        let config = Config::parse(
            "ignore = [\"examples/*\", \"b\"]\n[packages.a]\npath = \"crates/a\"\nversioned_files = [\"crates/a/Cargo.toml\"]\nchangelog = \"crates/a/CHANGELOG.md\"\n[packages.c]\npath = \"crates/c\"\nversioned_files = [\"crates/c/Cargo.toml\"]\nchangelog = \"x\"\n[packages.gone]\npath = \"crates/gone\"\nversioned_files = [\"crates/gone/Cargo.toml\"]\nchangelog = \"x\"\n",
        )
        .unwrap();
        let f = check(&t.0, &config);
        assert_eq!(f, [Finding::Missing { name: "gone".into(), path: "crates/gone".into() }], "b and examples/* are ignored; a and the private c are fine");
        let open = Config::parse("[packages.a]\npath = \"crates/a\"\nversioned_files = [\"crates/a/Cargo.toml\"]\nchangelog = \"x\"\n").unwrap();
        let unmanaged: Vec<String> = check(&t.0, &open).iter().filter_map(|f| if let Finding::Unmanaged { name, .. } = f { Some(name.clone()) } else { None }).collect();
        assert_eq!(unmanaged, ["b", "e"]);
    }
}
