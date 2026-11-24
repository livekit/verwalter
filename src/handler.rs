//! Spec: contract VersionedFileHandler, implemented on top of
//! knope-versioning. The file format is picked by file name; see
//! `knope_versioning::versioned_file::Config` for the supported names.
//! Functions take content, not paths, so they stay pure.

use anyhow::{Context, Result, bail, ensure};
use knope_versioning::{
    Action, GoVersioning, Package, VersionedFile, VersionedFileConfig, package::Name,
    release_notes::ReleaseNotes, versioned_file::cargo,
};
use relative_path::RelativePathBuf;
use toml_edit::DocumentMut;

use crate::Version;

const NO_TAGS: &[&str] = &[];

fn is_go_mod(path: &str) -> bool {
    path.rsplit('/').next() == Some("go.mod")
}

/// go.mod: the module path and the version in the `// vX.Y.Z` comment on
/// the module line, knope's convention. None without a module line.
pub fn go_module(content: &str) -> Option<(String, Option<Version>)> {
    let line = content.lines().find_map(|l| l.strip_prefix("module "))?;
    let (path, comment) = line.split_once("//").unwrap_or((line, ""));
    let version = comment
        .trim()
        .strip_prefix('v')
        .and_then(|v| v.parse().ok());
    Some((path.trim().trim_matches('"').to_string(), version))
}

/// Spec: GoMajorVersion. Go finds v2 and later only under a module path
/// that ends in `/vN`, and v0 or v1 only without it. The tool does not
/// change the path: the imports move with it.
pub fn go_major_error(module: &str, version: &Version) -> Option<String> {
    let major: u64 = version
        .stable_component()
        .to_string()
        .split('.')
        .next()?
        .parse()
        .ok()?;
    let suffix = module
        .rsplit_once('/')
        .and_then(|(_, s)| s.strip_prefix('v'))
        .and_then(|n| n.parse::<u64>().ok())
        .filter(|n| *n >= 2);
    // gopkg.in carries the major version as `.vN` and has its own rules.
    if module.starts_with("gopkg.in/") || suffix == (major >= 2).then_some(major) {
        return None;
    }
    Some(if major >= 2 {
        format!(
            "{module}: {version} needs the module path {module}/v{major}. Change it and every import in the PR that adds the major changeset"
        )
    } else {
        format!("{module}: {version} is below v2, but the module path ends in a major version")
    })
}

/// One `require` entry of a go.mod line: the module path and its version.
/// `in_block` is whether the line is inside `require ( ... )`.
fn go_require(line: &str, in_block: bool) -> Option<(&str, &str)> {
    let line = line.trim();
    let rest = if in_block {
        line
    } else {
        line.strip_prefix("require")
            .filter(|r| r.starts_with([' ', '\t']))?
    };
    let mut parts = rest.split_whitespace();
    let path = parts.next()?.trim_matches('"');
    let version = parts.next()?;
    (path != "(" && path != "//").then_some((path, version))
}

/// Every line of a go.mod with whether it is a `require` entry.
fn go_mod_lines(content: &str) -> Vec<(&str, Option<(&str, &str)>)> {
    let mut in_block = false;
    content
        .split_inclusive('\n')
        .map(|line| {
            let t = line.trim();
            if in_block && t == ")" {
                in_block = false;
                return (line, None);
            }
            if !in_block && t.strip_prefix("require").is_some_and(|r| r.trim() == "(") {
                in_block = true;
                return (line, None);
            }
            (line, go_require(line, in_block))
        })
        .collect()
}

/// Spec: write_dependency_version for go.mod. The `require` entry of
/// `module` moves to `version`; comments such as `// indirect` stay.
/// go.sum is not touched: it gets the new sum once the tag exists.
fn go_mod_pin(content: &str, module: &str, version: &Version) -> String {
    go_mod_lines(content)
        .into_iter()
        .map(|(line, entry)| match entry {
            Some((path, old)) if path == module => {
                let after = line.find(path).map_or(0, |i| i + path.len());
                match line[after..].find(old) {
                    Some(i) => format!(
                        "{}v{version}{}",
                        &line[..after + i],
                        &line[after + i + old.len()..]
                    ),
                    None => line.to_string(),
                }
            }
            _ => line.to_string(),
        })
        .collect()
}

fn is_pyproject(path: &str) -> bool {
    path.rsplit('/').next() == Some("pyproject.toml")
}

/// `dynamic = ["version"]`: the build backend computes the version, and
/// the file holds none. Some(true) when the backend is maturin, which
/// reads it from the Cargo.toml next to it.
pub fn pyproject_dynamic_version(content: &str) -> Option<bool> {
    let doc: toml::Table = content.parse().ok()?;
    let dynamic = doc.get("project")?.get("dynamic")?.as_array()?;
    if !dynamic.iter().any(|v| v.as_str() == Some("version")) {
        return None;
    }
    Some(
        doc.get("build-system")
            .and_then(|b| b.get("build-backend"))
            .and_then(|b| b.as_str())
            == Some("maturin"),
    )
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
        if let Some((stable, n)) = s.split_once(label)
            && !n.is_empty()
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
    s.to_string()
}

/// Rewrite the `version = "..."` line of a pyproject.toml to semver form so
/// knope can parse it.
fn normalise_pyproject(content: &str) -> String {
    content
        .lines()
        .map(|line| {
            let trimmed = line.trim_start();
            if let Some(rest) = trimmed.strip_prefix("version")
                && let Some(value) = rest.trim_start().strip_prefix('=')
            {
                let value = value.trim();
                if let Some(inner) = value.strip_prefix('"').and_then(|v| v.strip_suffix('"')) {
                    let indent = &line[..line.len() - trimmed.len()];
                    return format!("{indent}version = \"{}\"", from_pep440(inner));
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
    dependency: Option<&str>,
    content: &str,
) -> Result<(VersionedFileConfig, VersionedFile)> {
    let config = VersionedFileConfig::new(
        RelativePathBuf::from(path),
        dependency.map(String::from),
        None,
    )?;
    let content = if is_pyproject(path) {
        normalise_pyproject(content)
    } else if is_go_mod(path) && go_module(content).is_some_and(|(_, v)| v.is_none()) {
        // knope reads the version from the comment, else from tags it is
        // not given here. The placeholder is overwritten on write.
        let line = content
            .lines()
            .find(|l| l.starts_with("module "))
            .unwrap_or_default();
        content.replacen(line, &format!("{} // v0.0.0", line.trim_end()), 1)
    } else {
        content.to_string()
    };
    let file = VersionedFile::new(&config, content, NO_TAGS)?;
    Ok((config, file))
}

/// The package version in a manifest. Lock files have none. `regex` picks
/// the version out of a file knope has no format for (config::VersionedFile).
pub fn read_version(path: &str, regex: Option<&str>, content: &str) -> Result<Version> {
    if let Some(re) = regex {
        let re = regex::Regex::new(re)?;
        let m = re
            .captures(content)
            .and_then(|c| c.name("version"))
            .with_context(|| format!("{path}: regex `{re}` has no `version` match"))?;
        return Ok(m.as_str().parse()?);
    }
    if is_go_mod(path) {
        let (module, version) =
            go_module(content).with_context(|| format!("{path}: no module line"))?;
        return version.with_context(|| {
            format!("{path}: no `// vX.Y.Z` comment on the module line of {module}")
        });
    }
    if is_pyproject(path)
        && let Some(maturin) = pyproject_dynamic_version(content)
    {
        let instead = if maturin {
            "list the Cargo.toml next to it, where maturin reads it"
        } else {
            "list the file the build backend reads it from, with a regex"
        };
        bail!("{path}: the version is dynamic: {instead}");
    }
    Ok(load(path, None, content)?.1.version()?)
}

/// A regex file: the `version` group of every match becomes `version`,
/// nothing else moves. The regex wins over the file name, so a Cargo.lock
/// entry or a README line can be addressed directly.
fn regex_write(path: &str, re: &str, content: &str, version: &Version) -> Result<String> {
    let re = regex::Regex::new(re)?;
    let mut out = String::with_capacity(content.len());
    let mut last = 0;
    let mut matched = false;
    for caps in re.captures_iter(content) {
        let Some(group) = caps.name("version") else {
            bail!("{path}: regex `{re}` has no `version` group");
        };
        matched = true;
        out.push_str(&content[last..group.start()]);
        out.push_str(&version.to_string());
        last = group.end();
    }
    ensure!(matched, "{path}: regex `{re}` matches nothing");
    out.push_str(&content[last..]);
    Ok(out)
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
) -> Result<String> {
    if let Some(re) = regex {
        // A regex file carries one version, its own. Nothing to pin.
        if dependency.is_some() {
            return Ok(content.to_string());
        }
        return regex_write(path, re, content, version);
    }
    if path.rsplit('/').next() == Some("Cargo.toml") && dependency.is_none() {
        // `version.workspace = true`: the root Cargo.toml holds the
        // version, and the crate's manifest has none to write.
        let doc: DocumentMut = content.parse()?;
        if doc
            .get("package")
            .and_then(|p| p.get("version"))
            .and_then(|v| v.get("workspace"))
            .and_then(|w| w.as_bool())
            == Some(true)
        {
            return Ok(content.to_string());
        }
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
    if is_go_mod(path) {
        if let Some(module) = dependency {
            return Ok(go_mod_pin(content, module, version));
        }
        let (module, _) = go_module(content).with_context(|| format!("{path}: no module line"))?;
        if let Some(e) = go_major_error(&module, version) {
            bail!(e);
        }
    }
    if let (true, Some(dep)) = (path.rsplit('/').next() == Some("uv.lock"), dependency) {
        return uv_lock_pin(content, dep, version);
    }
    if let (true, Some(dep)) = (is_pyproject(path), dependency) {
        // knope's pyproject handler has no dependency mode; it would set the
        // package's own version instead.
        return Ok(pyproject_pin(content, dep, version));
    }
    let (config, file) = load(path, dependency, content)?;
    let mut package = Package::new(
        Name::from(dependency.unwrap_or("package")),
        NO_TAGS,
        vec![config],
        std::slice::from_ref(&file),
        ReleaseNotes::default(),
        None,
    )?;
    let files = package.set_version(version.clone(), GoVersioning::IgnoreMajorRules, vec![file])?;
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
pub(crate) fn normalise_py(name: &str) -> String {
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
    let Ok(doc) = content.parse::<toml::Table>() else {
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

/// Spec: write_dependency_version for pyproject.toml. Moves every
/// requirement on `dep` to `version` (move_specifier). The whole
/// specifier list goes to move_specifier, in any order, as uv_lock_pin
/// does, so both files stay in sync. Line-based, so formatting, comments
/// and markers are preserved.
fn pyproject_pin(content: &str, dep: &str, version: &Version) -> String {
    let wanted = normalise_py(dep);
    let clause = r"(?:===|[<>!=~]=|<|>)\s*[0-9][0-9A-Za-z.+!*-]*";
    let re = regex::Regex::new(&format!(
        r#"(["'])\s*([A-Za-z0-9][A-Za-z0-9._-]*)(\[[^\]]*\])?\s*\(?\s*({clause}(?:\s*,\s*{clause})*)"#
    ))
    .expect("static regex");
    re.replace_all(content, |caps: &regex::Captures| {
        let whole = caps.get(0).expect("match");
        if normalise_py(&caps[2]) == wanted {
            let spec = caps.get(4).expect("specifier group");
            format!(
                "{}{}",
                &content[whole.start()..spec.start()],
                move_specifier(spec.as_str(), version)
            )
        } else {
            whole.as_str().to_string()
        }
    })
    .into_owned()
}

/// A PEP 440 specifier list on an in-repo dependency, moved to `version`:
/// the first lower bound (`>=`, `==`, `~=`) becomes `version`, and `~=`
/// and a `==` wildcard keep their number of release parts. An upper bound (`<`, `<=`) that
/// would exclude `version` moves to the next version at the level of the
/// old bound: `>=2.0.1,<3.0.0` for 3.0.0 becomes `>=3.0.0,<4.0.0`, and
/// `<2.5` for 2.5.0 becomes `<2.6.0`. Everything else stays.
fn move_specifier(spec: &str, version: &Version) -> String {
    let new = to_pep440(version);
    let target = release(&new);
    let re =
        regex::Regex::new(r"(>=|==|~=|<=|<)(\s*)([0-9][0-9A-Za-z.+!*-]*)").expect("static regex");
    let mut floor = false;
    re.replace_all(spec, |c: &regex::Captures| {
        let (op, space, old) = (&c[1], &c[2], &c[3]);
        match op {
            // PEP 440: `<3.0.0` also excludes 3.0.0rc1, so compare releases.
            "<" if target >= release(old) => format!("<{space}{}", ceiling(old, target)),
            "<=" if target > release(old) => format!("<{space}{}", ceiling(old, target)),
            "<" | "<=" => c[0].to_string(),
            _ if floor => c[0].to_string(),
            _ => {
                floor = true;
                let new = match old.strip_suffix(".*") {
                    // `==2.*` for 3.0.0 is `==3.*`.
                    Some(prefix) => {
                        format!(
                            "{}.*",
                            compatible(prefix, &version.stable_component().to_string())
                        )
                    }
                    None if op == "~=" => compatible(old, &new),
                    None => new.clone(),
                };
                format!("{op}{space}{new}")
            }
        }
    })
    .into_owned()
}

/// The release numbers of a version: `3`, `3.0` and `3.0.0rc1` are 3.0.0.
fn release(v: &str) -> [u64; 3] {
    let mut out = [0; 3];
    for (slot, part) in out.iter_mut().zip(v.split('.')) {
        let digits: String = part.chars().take_while(char::is_ascii_digit).collect();
        *slot = digits.parse().unwrap_or(0);
    }
    out
}

/// The next version after `target` at the level of the upper bound
/// `bound`, given by its last non-zero part: `3.0.0` is a major bound,
/// `2.5` a minor one, `0.0.3` a patch one.
fn ceiling(bound: &str, [major, minor, patch]: [u64; 3]) -> String {
    match release(bound).iter().rposition(|&n| n > 0) {
        Some(2) => format!("{major}.{minor}.{}", patch + 1),
        Some(1) => format!("{major}.{}.0", minor + 1),
        _ => format!("{}.0.0", major + 1),
    }
}

/// `new` cut to the number of release parts in `old`, pre-release kept:
/// `~=2.0` for 3.0.0 is `~=3.0`, so it still allows 3.1.
fn compatible(old: &str, new: &str) -> String {
    let end = new
        .find(|c: char| !c.is_ascii_digit() && c != '.')
        .unwrap_or(new.len());
    let (numbers, pre) = new.split_at(end);
    let parts: Vec<&str> = numbers.split('.').take(old.split('.').count()).collect();
    format!("{}{pre}", parts.join("."))
}

/// Spec: write_dependency_version for uv.lock, which knope does not know.
/// The `[[package]]` entry of `dep` gets `version`, unless uv left it out
/// for a dynamic version, and every `requires-dist` and `requires-dev`
/// specifier on `dep` moves as the pyproject pin does. Without that,
/// `uv sync --locked` rejects the lock.
fn uv_lock_pin(content: &str, dep: &str, version: &Version) -> Result<String> {
    let wanted = normalise_py(dep);
    let new = to_pep440(version);
    let mut doc: DocumentMut = content.parse()?;
    let Some(packages) = doc
        .get_mut("package")
        .and_then(|p| p.as_array_of_tables_mut())
    else {
        return Ok(content.to_string());
    };
    for entry in packages.iter_mut() {
        let name = entry.get("name").and_then(|n| n.as_str()).unwrap_or("");
        if normalise_py(name) == wanted && entry.contains_key("version") {
            entry["version"] = toml_edit::value(new.as_str());
        }
        let Some(metadata) = entry
            .get_mut("metadata")
            .and_then(|m| m.as_table_like_mut())
        else {
            continue;
        };
        let lists: Vec<&mut toml_edit::Array> = metadata
            .iter_mut()
            .flat_map(|(key, item)| match key.get() {
                "requires-dist" => item.as_array_mut().into_iter().collect(),
                "requires-dev" => item
                    .as_table_like_mut()
                    .into_iter()
                    .flat_map(|groups| groups.iter_mut())
                    .filter_map(|(_, group)| group.as_array_mut())
                    .collect(),
                _ => Vec::new(),
            })
            .collect();
        for req in lists.into_iter().flat_map(|list| list.iter_mut()) {
            let Some(table) = req.as_inline_table_mut() else {
                continue;
            };
            let name = table.get("name").and_then(|n| n.as_str()).unwrap_or("");
            if normalise_py(name) != wanted {
                continue;
            }
            let Some(old) = table
                .get("specifier")
                .and_then(|s| s.as_str())
                .map(String::from)
            else {
                continue;
            };
            table["specifier"] = move_specifier(&old, version).into();
        }
    }
    Ok(doc.to_string())
}

/// The directory of every project in a uv workspace, from its uv.lock:
/// the `editable` or `virtual` source of each `[[package]]`, relative to
/// the lock. Their pyproject.toml files hold the pins uv.lock records.
pub fn uv_lock_projects(content: &str) -> Vec<String> {
    let Ok(doc) = content.parse::<toml::Table>() else {
        return Vec::new();
    };
    let Some(packages) = doc.get("package").and_then(|p| p.as_array()) else {
        return Vec::new();
    };
    packages
        .iter()
        .filter_map(|p| p.get("source"))
        .filter_map(|s| s.get("editable").or_else(|| s.get("virtual")))
        .filter_map(|path| path.as_str())
        .map(String::from)
        .collect()
}

/// Spec: internal_dependencies. Which of `candidates` (the other package
/// names in the repo, module paths for go.mod) this manifest depends on.
/// Only runtime dependencies count: a devDependency (package.json) does
/// not make a package a dependent for bumps and ordering, as in changesets.
pub fn internal_dependencies(
    path: &str,
    content: &str,
    candidates: &[&str],
) -> Result<Vec<String>> {
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
        // go.mod names dependencies by module path: the candidates are
        // module paths here (Config::dependency_key).
        "go.mod" => {
            let required: Vec<&str> = go_mod_lines(content)
                .into_iter()
                .filter_map(|(_, entry)| entry.map(|(path, _)| path))
                .collect();
            candidates
                .iter()
                .filter(|c| required.contains(c))
                .map(|c| c.to_string())
                .collect()
        }
        // ponytail: pubspec.yaml reports no dependencies yet; add a branch
        // when a monorepo in that language needs dependency ordering.
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
    fn package_json_workspace_range_is_left_alone() {
        let json = PACKAGE_JSON.replace("\"1.30.0\"", "\"workspace:*\"");
        let written = write_version(
            "package.json",
            None,
            &json,
            Some("@livekit/protocol"),
            &v("1.31.0"),
        )
        .unwrap();
        assert_eq!(written, json);
    }

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

    const UV_LOCK: &str = r#"version = 1
revision = 2

[[package]]
name = "livekit-api"
version = "1.1.1"
source = { editable = "livekit-api" }
dependencies = [
    { name = "livekit-protocol" },
]

[package.metadata]
requires-dist = [
    { name = "aiohttp", specifier = ">=3.9" },
    { name = "livekit-protocol", specifier = ">=1.1.0,<2.0.0" },
]

[[package]]
name = "livekit-protocol"
version = "1.1.0"
source = { editable = "livekit-protocol" }
"#;

    // Spec: write_dependency_version for uv.lock.
    #[test]
    fn uv_lock_own_entry_and_requires_dist() {
        let own = write_version(
            "uv.lock",
            None,
            UV_LOCK,
            Some("livekit-protocol"),
            &v("1.2.0-rc.1"),
        )
        .unwrap();
        assert!(
            own.contains("name = \"livekit-protocol\"\nversion = \"1.2.0rc1\""),
            "{own}"
        );
        assert!(own.contains("specifier = \">=1.2.0rc1,<2.0.0\""), "{own}");
        assert!(own.contains("version = \"1.1.1\""), "api untouched");
        assert!(
            own.contains("{ name = \"aiohttp\", specifier = \">=3.9\" }"),
            "others untouched"
        );
        let none = write_version("uv.lock", None, UV_LOCK, Some("nope"), &v("9.9.9")).unwrap();
        assert_eq!(none, UV_LOCK);
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
        assert!(tilde.contains(r#""livekit-test-utils~=0.4""#), "{tilde}");

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

    // Spec: write_dependency_version for pyproject.toml and uv.lock. Each
    // row is a specifier, the release, and the specifier after the move.
    #[test]
    fn move_specifier_table() {
        let rows = [
            // The first lower bound becomes the release.
            (">=2.0.1", "2.0.2", ">=2.0.2"),
            ("==2.0.1", "2.1.0", "==2.1.0"),
            (">= 2.0.1", "2.1.0", ">= 2.1.0"),
            (">=2.0.1,>=2.0.0", "2.1.0", ">=2.1.0,>=2.0.0"),
            // An upper bound that allows the release stays.
            (">=2.0.1,<3.0.0", "2.1.0", ">=2.1.0,<3.0.0"),
            (">=2.0.1,<=2.9", "2.5.0", ">=2.5.0,<=2.9"),
            (">=2.0.1,<=3.0.0", "3.0.0", ">=3.0.0,<=3.0.0"),
            // One that excludes it moves to the next version at its level.
            (">=2.0.1,<3.0.0", "3.0.0", ">=3.0.0,<4.0.0"),
            (">=2.0.1,<3", "3.0.0", ">=3.0.0,<4.0.0"),
            (">=2.0.1,<3.0", "3.0.0", ">=3.0.0,<4.0.0"),
            (">=2.0,<2.5", "2.5.0", ">=2.5.0,<2.6.0"),
            (">=2.0,<2.5", "3.0.0", ">=3.0.0,<3.1.0"),
            (">=2.0.1,<2.0.5", "2.0.5", ">=2.0.5,<2.0.6"),
            (">=2.0.1,<=2.9", "3.0.0", ">=3.0.0,<3.1.0"),
            (">=2.0.1,<=2.9.0", "2.9.1", ">=2.9.1,<2.10.0"),
            (">=0.4.0,<0.5.0", "0.5.0", ">=0.5.0,<0.6.0"),
            (">=0.4.0,<1.0.0", "1.0.0", ">=1.0.0,<2.0.0"),
            (">=0.0.2,<0.0.3", "0.0.3", ">=0.0.3,<0.0.4"),
            ("<3.0.0", "3.0.0", "<4.0.0"),
            ("<= 2.9", "3.0.0", "< 3.1.0"),
            // PEP 440: `<3.0.0` excludes 3.0.0rc1 too.
            (">=2.0.1,<3.0.0", "3.0.0-rc.1", ">=3.0.0rc1,<4.0.0"),
            (">=2.0.1,<3.0.0", "3.0.0-beta.2", ">=3.0.0b2,<4.0.0"),
            (">=2.0.1,<=3.0.0", "3.0.0-rc.1", ">=3.0.0rc1,<=3.0.0"),
            // Any order and spacing.
            ("<3, >=2.0.1", "3.0.0", "<4.0.0, >=3.0.0"),
            ("<3 , >= 2.0.1", "3.0.0", "<4.0.0 , >= 3.0.0"),
            // `~=` keeps its number of release parts.
            ("~=2.0", "3.0.0", "~=3.0"),
            ("~=2.0", "2.1.0", "~=2.1"),
            ("~=2.0", "2.0.2", "~=2.0"),
            ("~=2.0", "3.0.0-rc.1", "~=3.0rc1"),
            ("~=2.0.1", "2.1.0", "~=2.1.0"),
            ("~=0.3", "0.4.0", "~=0.4"),
            ("~=2.0,<2.5", "2.5.0", "~=2.5,<2.6.0"),
            // `==` with a wildcard keeps it.
            ("==2.*", "3.0.0", "==3.*"),
            ("==2.0.*", "2.1.0", "==2.1.*"),
            ("==2.*", "3.0.0-rc.1", "==3.*"),
            // Other operators stay, as does a `!=` on an older release.
            (">=1.0,!=1.5.0", "2.1.0", ">=2.1.0,!=1.5.0"),
            ("!=2.*,>=1.0", "3.0.0", "!=2.*,>=3.0.0"),
            (">2.0.0", "3.0.0", ">2.0.0"),
            ("!=2.0.3", "3.0.0", "!=2.0.3"),
        ];
        let mut failures = Vec::new();
        for (spec, release, expected) in rows {
            let moved = move_specifier(spec, &v(release));
            if moved != expected {
                failures.push(format!("{spec} for {release}: {moved}, want {expected}"));
            }
        }
        assert!(failures.is_empty(), "\n{}", failures.join("\n"));
    }

    // Every combination of lower and upper bound, for every kind of
    // release, against pep440_rs, an independent PEP 440 implementation.
    // A moved specifier admits the release, a moved `>=` or `==` floor
    // admits no older version, an upper bound that allowed the release
    // stays, and a second move changes nothing.
    #[test]
    fn moved_specifiers_admit_the_release() {
        use pep440_rs::{Version as Pep, VersionSpecifiers};
        use std::str::FromStr;
        let admits = |spec: &str, version: &str| {
            VersionSpecifiers::from_str(spec)
                .unwrap_or_else(|e| panic!("{spec}: {e}"))
                .contains(&Pep::from_str(version).unwrap())
        };
        let groups = [
            (
                vec![
                    ">=2.0.1", "==2.0.1", "~=2.0", "~=2.0.1", "==2.*", "==2.0.*", ">2.0.0",
                ],
                vec![
                    "<3.0.0", "<3", "<2.5", "<2.0.5", "<=2.9", "<=3.0.0", "!=2.0.0",
                ],
                vec![
                    "2.0.2",
                    "2.0.5",
                    "2.1.0",
                    "2.5.0",
                    "2.9.0",
                    "2.9.1",
                    "2.10.0",
                    "3.0.0",
                    "3.0.0-rc.1",
                    "3.0.0-beta.2",
                    "3.1.0",
                    "3.1.0-alpha.1",
                    "4.0.0",
                    "10.0.0",
                ],
            ),
            (
                vec![">=0.4.0", "==0.4.0", "~=0.4", "~=0.4.0", "==0.*", ">=0.0.2"],
                vec!["<0.5.0", "<1.0.0", "<1", "<0.0.3", "<=0.4.9"],
                vec![
                    "0.0.3",
                    "0.4.1",
                    "0.5.0",
                    "0.5.0-rc.1",
                    "0.6.0",
                    "1.0.0",
                    "1.0.0-rc.1",
                ],
            ),
        ];
        let mut failures = Vec::new();
        for (floors, caps, releases) in &groups {
            let mut specs: Vec<String> = Vec::new();
            for floor in floors.iter() {
                specs.push(floor.to_string());
                for cap in caps.iter() {
                    specs.push(cap.to_string());
                    specs.push(format!("{floor},{cap}"));
                    specs.push(format!("{cap}, {floor}"));
                }
            }
            specs.sort();
            specs.dedup();
            let probes: Vec<String> = releases.iter().map(|r| to_pep440(&v(r))).collect();
            for spec in &specs {
                for release in releases.iter() {
                    let new = to_pep440(&v(release));
                    let moved = move_specifier(spec, &v(release));
                    let case = format!("{spec} for {release} gives {moved}");
                    if !admits(&moved, &new) {
                        failures.push(format!("{case}: excludes {new}"));
                    }
                    if move_specifier(&moved, &v(release)) != moved {
                        failures.push(format!("{case}: a second move changes it"));
                    }
                    let floor = spec.split(',').map(str::trim).find(|c| {
                        (c.starts_with(">=") || c.starts_with("==")) && !c.ends_with('*')
                    });
                    if floor.is_some() {
                        let new = Pep::from_str(&new).unwrap();
                        for older in probes.iter().filter(|p| Pep::from_str(p).unwrap() < new) {
                            if admits(&moved, older) {
                                failures.push(format!("{case}: admits older {older}"));
                            }
                        }
                    }
                    let caps = |s: &str| -> Vec<String> {
                        s.split(',')
                            .map(|c| c.trim().to_string())
                            .filter(|c| c.starts_with('<'))
                            .collect()
                    };
                    let old_caps = caps(spec);
                    if !old_caps.is_empty()
                        && admits(&old_caps.join(","), &new)
                        && caps(&moved) != old_caps
                    {
                        failures.push(format!("{case}: moved a cap that allowed {new}"));
                    }
                }
            }
        }
        assert!(failures.is_empty(), "\n{}", failures.join("\n"));
    }

    // pyproject.toml and uv.lock must move the same specifiers, or
    // `uv sync --locked` rejects the lock.
    #[test]
    fn pyproject_and_uv_lock_move_the_same_specifiers() {
        let specs = [
            ">=2.0.1",
            ">=2.0.1,<3.0.0",
            "<3.0.0",
            "<3,>=2.0.1",
            ">=2.0.1,<=2.9",
            "~=2.0",
            "==2.*",
            "==2.0.1",
            ">=2.0,<2.5",
            "!=2.0.0,>=2.0.1",
            ">2.0.0",
        ];
        let releases = ["2.0.2", "2.5.0", "3.0.0", "3.0.0-rc.1"];
        // A runtime requirement is in requires-dist, a dev group in
        // requires-dev.
        let sections = [
            (
                "[project]\ndependencies",
                "[package.metadata]\nrequires-dist",
            ),
            (
                "[dependency-groups]\ndev",
                "[package.metadata.requires-dev]\ndev",
            ),
        ];
        let mut failures = Vec::new();
        for spec in specs {
            for release in releases {
                for (py_section, lock_section) in sections {
                    let pyproject = format!("{py_section} = [\"livekit-protocol{spec}\"]\n");
                    let lock = format!(
                        "version = 1\n\n[[package]]\nname = \"livekit-api\"\nsource = {{ editable = \"livekit-api\" }}\n\n{lock_section} = [{{ name = \"livekit-protocol\", specifier = \"{spec}\" }}]\n"
                    );
                    let moved = move_specifier(spec, &v(release));
                    let py = pyproject_pin(&pyproject, "livekit-protocol", &v(release));
                    let uv = uv_lock_pin(&lock, "livekit-protocol", &v(release)).unwrap();
                    if !py.contains(&format!("\"livekit-protocol{moved}\"")) {
                        failures.push(format!("pyproject {spec} for {release}:\n{py}"));
                    }
                    if !uv.contains(&format!("specifier = \"{moved}\"")) {
                        failures.push(format!("uv.lock {spec} for {release}:\n{uv}"));
                    }
                }
            }
        }
        assert!(failures.is_empty(), "\n{}", failures.join("\n"));
    }

    // Each row is one requirement line in pyproject.toml, and the line
    // after a 3.0.0 release of livekit-protocol.
    #[test]
    fn pyproject_pin_requirement_forms() {
        let rows = [
            (
                r#""livekit-protocol>=2.0.1,<3.0.0","#,
                r#""livekit-protocol>=3.0.0,<4.0.0","#,
            ),
            (
                r#""livekit-protocol >= 2.0.1, < 3","#,
                r#""livekit-protocol >= 3.0.0, < 4.0.0","#,
            ),
            (
                r#"'livekit-protocol>=2.0.1',"#,
                r#"'livekit-protocol>=3.0.0',"#,
            ),
            (
                r#""LiveKit_Protocol>=2.0.1","#,
                r#""LiveKit_Protocol>=3.0.0","#,
            ),
            (
                r#""livekit.protocol>=2.0.1","#,
                r#""livekit.protocol>=3.0.0","#,
            ),
            (
                r#""livekit-protocol[codecs]>=2.0.1","#,
                r#""livekit-protocol[codecs]>=3.0.0","#,
            ),
            (
                r#"'livekit-protocol>=2.0.1; python_version >= "3.9"',"#,
                r#"'livekit-protocol>=3.0.0; python_version >= "3.9"',"#,
            ),
            (
                r#""livekit-protocol (>=2.0.1,<3.0.0)","#,
                r#""livekit-protocol (>=3.0.0,<4.0.0)","#,
            ),
            (
                r#""livekit-protocol>=2.0.1", # keep in sync"#,
                r#""livekit-protocol>=3.0.0", # keep in sync"#,
            ),
            (
                r#""livekit-protocol>=2.0.1", "livekit-api>=1.0","#,
                r#""livekit-protocol>=3.0.0", "livekit-api>=1.0","#,
            ),
            // Not a requirement on livekit-protocol: no change.
            (r#""livekit-protocol","#, r#""livekit-protocol","#),
            (
                r#""livekit-protocol-ext>=2.0.1","#,
                r#""livekit-protocol-ext>=2.0.1","#,
            ),
            (r#""livekit>=2.0.1","#, r#""livekit>=2.0.1","#),
        ];
        let mut failures = Vec::new();
        for (line, expected) in rows {
            let file = |line: &str| {
                format!(
                    "[project]\nname = \"livekit-api\"\nrequires-python = \">=3.9\"\ndependencies = [\n    {line}\n]\n"
                )
            };
            let pinned = pyproject_pin(&file(line), "livekit-protocol", &v("3.0.0"));
            if pinned != file(expected) {
                failures.push(format!("{line}\n  gives {pinned}\n  want  {expected}"));
            }
        }
        assert!(failures.is_empty(), "\n{}", failures.join("\n"));
    }

    // Each row is one requires-dist entry in uv.lock, and the entry after
    // a 3.0.0 release of livekit-protocol.
    #[test]
    fn uv_lock_pin_requires_dist_forms() {
        let rows = [
            (
                r#"{ name = "livekit-protocol", specifier = ">=2.0.1,<3.0.0" }"#,
                r#"{ name = "livekit-protocol", specifier = ">=3.0.0,<4.0.0" }"#,
            ),
            (
                r#"{ name = "livekit-protocol", editable = "livekit-protocol", specifier = "<3,>=2.0.1" }"#,
                r#"{ name = "livekit-protocol", editable = "livekit-protocol", specifier = "<4.0.0,>=3.0.0" }"#,
            ),
            (
                r#"{ name = "livekit-protocol", marker = "python_full_version >= '3.9'", specifier = ">=2.0.1" }"#,
                r#"{ name = "livekit-protocol", marker = "python_full_version >= '3.9'", specifier = ">=3.0.0" }"#,
            ),
            (
                r#"{ name = "livekit-protocol", extras = ["codecs"], specifier = "~=2.0" }"#,
                r#"{ name = "livekit-protocol", extras = ["codecs"], specifier = "~=3.0" }"#,
            ),
            (
                r#"{ name = "LiveKit_Protocol", specifier = ">=2.0.1" }"#,
                r#"{ name = "LiveKit_Protocol", specifier = ">=3.0.0" }"#,
            ),
            // Not a requirement with a specifier on livekit-protocol.
            (
                r#"{ name = "livekit-protocol", editable = "livekit-protocol" }"#,
                r#"{ name = "livekit-protocol", editable = "livekit-protocol" }"#,
            ),
            (
                r#"{ name = "livekit-protocol-ext", specifier = ">=2.0.1" }"#,
                r#"{ name = "livekit-protocol-ext", specifier = ">=2.0.1" }"#,
            ),
        ];
        let mut failures = Vec::new();
        for (entry, expected) in rows {
            let lock = |entry: &str| {
                format!(
                    "version = 1\n\n[[package]]\nname = \"livekit-api\"\nsource = {{ editable = \"livekit-api\" }}\n\n[package.metadata]\nrequires-dist = [{entry}]\n"
                )
            };
            let pinned = uv_lock_pin(&lock(entry), "livekit-protocol", &v("3.0.0")).unwrap();
            if pinned != lock(expected) {
                failures.push(format!("{entry}\n  gives {pinned}\n  want  {expected}"));
            }
        }
        assert!(failures.is_empty(), "\n{}", failures.join("\n"));
    }

    // Every dev group moves, and other packages in it stay.
    #[test]
    fn uv_lock_pin_requires_dev_groups() {
        let lock = r#"version = 1

[[package]]
name = "python-sdks"
source = { virtual = "." }

[package.metadata.requires-dev]
dev = [
    { name = "livekit-protocol", specifier = ">=2.0.1,<3.0.0" },
    { name = "pytest", specifier = ">=8" },
]
test = [{ name = "livekit-protocol", marker = "python_full_version >= '3.11'", specifier = "~=2.0" }]
"#;
        let pinned = uv_lock_pin(lock, "livekit-protocol", &v("3.0.0")).unwrap();
        let expected = lock
            .replace(">=2.0.1,<3.0.0", ">=3.0.0,<4.0.0")
            .replace("~=2.0", "~=3.0");
        assert_eq!(pinned, expected);
    }

    // Every project of the workspace, by its source in uv.lock. A package
    // from a registry has none of its own.
    #[test]
    fn uv_lock_projects_are_the_editable_and_virtual_sources() {
        let lock = r#"version = 1

[[package]]
name = "aiohttp"
version = "3.9.0"
source = { registry = "https://pypi.org/simple" }

[[package]]
name = "livekit-api"
source = { editable = "livekit-api" }

[[package]]
name = "python-sdks"
source = { virtual = "." }
"#;
        assert_eq!(uv_lock_projects(lock), vec!["livekit-api", "."]);
        assert!(uv_lock_projects("not toml [").is_empty());
    }

    // The package entry gets the version only where uv wrote one: uv
    // leaves dynamic versions out of the lock.
    #[test]
    fn uv_lock_pin_own_version_only_where_present() {
        let fixed = "version = 1\n\n[[package]]\nname = \"livekit-protocol\"\nversion = \"2.0.1\"\nsource = { editable = \"livekit-protocol\" }\n";
        let dynamic = "version = 1\n\n[[package]]\nname = \"livekit-protocol\"\nsource = { editable = \"livekit-protocol\" }\n";
        let pin = |lock: &str| uv_lock_pin(lock, "livekit-protocol", &v("3.0.0-rc.1")).unwrap();
        assert_eq!(pin(fixed), fixed.replace("2.0.1", "3.0.0rc1"));
        assert_eq!(pin(dynamic), dynamic);
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
    // A regex wins over the file name: two Cargo.lock entries, one each.
    #[test]
    fn regex_addresses_a_lock_entry() {
        let lock = "[[package]]\nname = \"a\"\nversion = \"0.2.2\"\n\n[[package]]\nname = \"a-uniffi\"\nversion = \"0.2.2\"\n";
        let re = "name = \"a\"\nversion = \"(?<version>[^\"]+)\"";
        assert_eq!(
            read_version("Cargo.lock", Some(re), lock).unwrap(),
            v("0.2.2")
        );
        let out = write_version("Cargo.lock", Some(re), lock, None, &v("0.3.0")).unwrap();
        assert_eq!(
            out,
            "[[package]]\nname = \"a\"\nversion = \"0.3.0\"\n\n[[package]]\nname = \"a-uniffi\"\nversion = \"0.2.2\"\n"
        );
        assert!(
            write_version(
                "Cargo.lock",
                Some("name = \"zzz\""),
                lock,
                None,
                &v("1.0.0")
            )
            .is_err()
        );
    }

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

    const GO_MOD: &str = "module github.com/acme/sdk/auth // v1.4.2\n\ngo 1.24\n\nrequire github.com/acme/sdk/core v1.2.0\n\nrequire (\n\tgithub.com/acme/sdk/proto v0.3.1 // indirect\n\tgolang.org/x/net v0.30.0\n)\n";

    #[test]
    fn go_mod_version_is_the_module_line_comment() {
        let written = check("go.mod", GO_MOD, "1.5.0");
        assert!(written.starts_with("module github.com/acme/sdk/auth // v1.5.0\n"));
        assert!(
            written.ends_with("golang.org/x/net v0.30.0\n)\n"),
            "MinimalEdit"
        );
    }

    #[test]
    fn go_mod_without_comment_has_no_version_until_written() {
        let bare = "module github.com/acme/greeter\n\ngo 1.24\n";
        assert!(read_version("go.mod", None, bare).is_err());
        let written = write_version("go.mod", None, bare, None, &v("0.1.0")).unwrap();
        assert_eq!(
            written,
            "module github.com/acme/greeter // v0.1.0\n\ngo 1.24\n"
        );
    }

    #[test]
    fn go_major_versions_need_the_module_path_suffix() {
        let bare = "module github.com/acme/greeter // v1.9.0\n";
        assert!(
            write_version("go.mod", None, bare, None, &v("2.0.0"))
                .unwrap_err()
                .to_string()
                .contains("github.com/acme/greeter/v2")
        );
        let v2 = "module github.com/acme/greeter/v2 // v1.9.0\n";
        assert_eq!(
            write_version("go.mod", None, v2, None, &v("2.0.0")).unwrap(),
            "module github.com/acme/greeter/v2 // v2.0.0\n"
        );
        assert!(write_version("go.mod", None, v2, None, &v("3.0.0")).is_err());
        assert!(go_major_error("github.com/acme/greeter/v2", &v("1.0.0")).is_some());
        assert!(go_major_error("gopkg.in/yaml.v3", &v("3.0.1")).is_none());
    }

    #[test]
    fn go_mod_requires_are_dependencies_and_pins() {
        let candidates = [
            "github.com/acme/sdk/core",
            "github.com/acme/sdk/proto",
            "github.com/acme/sdk/other",
        ];
        assert_eq!(
            internal_dependencies("auth/go.mod", GO_MOD, &candidates).unwrap(),
            ["github.com/acme/sdk/core", "github.com/acme/sdk/proto"]
        );
        let core = write_version(
            "auth/go.mod",
            None,
            GO_MOD,
            Some(candidates[0]),
            &v("1.3.0"),
        )
        .unwrap();
        assert!(core.contains("\nrequire github.com/acme/sdk/core v1.3.0\n"));
        let proto = write_version(
            "auth/go.mod",
            None,
            &core,
            Some(candidates[1]),
            &v("0.4.0-rc.1"),
        )
        .unwrap();
        assert!(proto.contains("\tgithub.com/acme/sdk/proto v0.4.0-rc.1 // indirect\n"));
        assert_eq!(
            proto
                .replace("v1.3.0", "v1.2.0")
                .replace("v0.4.0-rc.1", "v0.3.1"),
            GO_MOD,
            "nothing else moves"
        );
        assert_eq!(
            write_version("go.mod", None, GO_MOD, Some("github.com/x/y"), &v("9.9.9")).unwrap(),
            GO_MOD
        );
    }

    #[test]
    fn dynamic_pyproject_version_names_where_it_lives() {
        let maturin = "[build-system]\nbuild-backend = \"maturin\"\n\n[project]\nname = \"ext\"\ndynamic = [\"version\"]\n";
        let e = read_version("py/pyproject.toml", None, maturin).unwrap_err();
        assert!(
            e.to_string().contains("list the Cargo.toml next to it"),
            "{e}"
        );
        let hatch = maturin.replace("maturin", "hatchling.build");
        let e = read_version("py/pyproject.toml", None, &hatch).unwrap_err();
        assert!(e.to_string().contains("with a regex"), "{e}");
    }
}
