//! Spec: ChangesetCheck, rule ChangesetCheckOnContributorPullRequest and
//! surface ContributorPullRequestCheck. The contributor sees one PR
//! comment, updated in place and removed once the check passes.

use std::{collections::BTreeSet, fs, path::Path};

use std::process::Command;

use crate::{
    changeset::Changeset, config::Config, github::Gh, handler, repo::CHANGESET_DIR,
    version::BumpLevel, Error, Version,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Passed,
    Failed,
    Exempt,
    /// A release PR consumes changesets rather than adding one.
    ReleasePullRequest,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Check {
    pub status: Status,
    pub affected: BTreeSet<String>,
    pub covered: BTreeSet<String>,
    /// Spec: ChangesetCheck.api_findings. Empty until `api_check` ran.
    pub api: Vec<ApiFinding>,
    /// Spec: ChangesetCheck.bumps. What the PR's changesets claim, one
    /// line per package at the highest level, for the comment.
    pub bumps: Vec<crate::changeset::Bump>,
}

/// Spec: value ApiFinding. A covered package whose changeset level is
/// below what the API check requires, or whose check did not run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiFinding {
    pub package: String,
    pub claimed: BumpLevel,
    /// None when the command failed; `output` then carries the reason.
    pub required: Option<BumpLevel>,
    pub output: String,
}

impl Check {
    /// Spec: ChangesetCheck.missing_packages.
    pub fn missing(&self) -> Vec<&str> {
        self.affected
            .difference(&self.covered)
            .map(String::as_str)
            .collect()
    }
}

/// The pure part. `changed` is every file the PR touches; `changesets`
/// the parsed `.changeset/*.md` files among them.
pub fn evaluate(
    config: &Config,
    head_branch: &str,
    labels: &[String],
    changed: &[String],
    changesets: &[Changeset],
) -> Check {
    let affected: BTreeSet<String> = config
        .packages
        .iter()
        .filter(|(_, p)| changed.iter().any(|f| in_package(f, &p.path)))
        .map(|(name, _)| name.clone())
        .collect();
    let covered: BTreeSet<String> = changesets
        .iter()
        .flat_map(|c| c.bumps.iter().map(|b| b.package_name.clone()))
        .collect();
    let bumps = covered
        .iter()
        .map(|name| crate::changeset::Bump {
            package_name: name.clone(),
            level: changesets
                .iter()
                .flat_map(|c| c.bumps.iter())
                .filter(|b| &b.package_name == name)
                .map(|b| b.level)
                .max()
                .unwrap_or(BumpLevel::Patch),
        })
        .collect();
    let status = if head_branch.starts_with(&config.release_branch_prefix) {
        Status::ReleasePullRequest
    } else if labels.iter().any(|l| l == &config.exempt_label) {
        Status::Exempt
    } else if affected.iter().all(|a| covered.contains(a)) {
        Status::Passed
    } else {
        Status::Failed
    };
    Check {
        status,
        affected,
        covered,
        api: Vec::new(),
        bumps,
    }
}

/// Spec: required_level. The digit a change kind moves: `major` is the
/// first non-zero digit, so a breaking change on 0.x needs minor and on
/// 0.0.x patch. The tool's answer is a kind of change, the changeset
/// level a digit.
pub fn required_level(current: &Version, kind: BumpLevel) -> BumpLevel {
    let s = current.stable_component().to_string();
    let zeros = s.split('.').take(2).take_while(|d| *d == "0").count();
    let mut level = kind;
    for _ in 0..zeros {
        level = match level {
            BumpLevel::Major => BumpLevel::Minor,
            _ => BumpLevel::Patch,
        };
    }
    level
}

/// Spec: ApiCheckOnCoveredPackages. Runs the API command for every
/// affected package that a changeset covers, and compares the claimed
/// level with what the command proves. Mutates `check`: findings, and
/// `failed` when there are any.
pub fn api_check(
    root: &Path,
    config: &Config,
    check: &mut Check,
    changesets: &[Changeset],
) -> Result<(), Error> {
    if !matches!(check.status, Status::Passed | Status::Failed) {
        return Ok(());
    }
    for name in check.affected.intersection(&check.covered) {
        let Some(command) = config.api_command(name) else {
            continue;
        };
        let claimed = changesets
            .iter()
            .flat_map(|c| c.bumps.iter())
            .filter(|b| &b.package_name == name)
            .map(|b| b.level)
            .max()
            .unwrap_or(BumpLevel::Patch);
        let package = &config.packages[name];
        let file = &package.versioned_files[0];
        let content = fs::read_to_string(root.join(file.path()))
            .map_err(|e| format!("{}: {e}", file.path()))?;
        let version = handler::read_version(file.path(), file.regex(), &content)?;
        let out = Command::new("sh")
            .arg("-c")
            .arg(command)
            .current_dir(root.join(&package.path))
            .env("VERWALTER_PACKAGE", name)
            .env("VERWALTER_ROOT", root)
            .env("VERWALTER_VERSION", version.to_string())
            .env("VERWALTER_BASELINE_TAG", config.tag_for(name, &version))
            .env("VERWALTER_LEVEL", claimed.to_string())
            .output()
            .map_err(|e| format!("{name}: api command: {e}"))?;
        let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
        let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
        let kind = stdout.split_whitespace().last().unwrap_or("");
        let finding = if !out.status.success() {
            Some((
                None,
                format!("the command failed ({})\n{stdout}{stderr}", out.status),
            ))
        } else {
            match kind.parse::<BumpLevel>() {
                _ if kind.is_empty() => None,
                Ok(kind) => {
                    let required = required_level(&version, kind);
                    (claimed < required).then_some((Some(required), stderr))
                }
                Err(()) => Some((
                    None,
                    format!("the command printed `{kind}`, not major, minor or patch\n{stderr}"),
                )),
            }
        };
        if let Some((required, output)) = finding {
            check.api.push(ApiFinding {
                package: name.clone(),
                claimed,
                required,
                output: output.trim().to_string(),
            });
        }
    }
    if !check.api.is_empty() {
        check.status = Status::Failed;
    }
    Ok(())
}

fn in_package(file: &str, package_path: &str) -> bool {
    if file.starts_with(&format!("{CHANGESET_DIR}/")) {
        return false;
    }
    let path = package_path.trim_end_matches('/');
    path == "." || path.is_empty() || file == path || file.starts_with(&format!("{path}/"))
}

pub const MARKER: &str = "<!-- verwalter changeset-check -->";

/// Spec: ContributorWritesChangeset. The local counterpart of the check:
/// the packages that the branch and working tree change against `base`
/// and that no pending changeset covers get one file with `level` each.
/// `summary` defaults to the last commit's subject. `packages`, when given,
/// replaces the detection: exactly those names, whether or not the branch
/// touches them or a changeset covers them. Returns the file, or None when
/// nothing is missing.
pub fn write_changeset(
    repo: &crate::repo::Repo,
    base: &str,
    level: crate::version::BumpLevel,
    summary: Option<&str>,
    packages: &[String],
) -> Result<Option<std::path::PathBuf>, Error> {
    let root = &repo.root;
    let missing: Vec<String> = if packages.is_empty() {
        let merge_base = crate::git::run(root, &["merge-base", base, "HEAD"])?;
        let mut changed: Vec<String> =
            crate::git::run(root, &["diff", "--name-only", merge_base.trim()])?
                .lines()
                .map(String::from)
                .collect();
        changed.extend(
            crate::git::run(root, &["ls-files", "--others", "--exclude-standard"])?
                .lines()
                .map(String::from),
        );
        let check = evaluate(&repo.config, "", &[], &changed, &repo.changesets);
        check.missing().into_iter().map(String::from).collect()
    } else {
        for p in packages {
            if !repo.config.packages.contains_key(p) {
                return Err(format!("unknown package {p}").into());
            }
        }
        packages.to_vec()
    };
    if missing.is_empty() {
        return Ok(None);
    }
    let summary = match summary {
        Some(s) => s.to_string(),
        None => crate::git::run(root, &["log", "-1", "--format=%s"])?
            .trim()
            .to_string(),
    };
    let front: String = missing
        .iter()
        .map(|p| format!("{}: {level}\n", quote(p)))
        .collect();
    let dir = root.join(CHANGESET_DIR);
    fs::create_dir_all(&dir)?;
    let base_name = slug(&summary);
    let mut path = dir.join(format!("{base_name}.md"));
    let mut n = 2;
    while path.exists() {
        path = dir.join(format!("{base_name}_{n}.md"));
        n += 1;
    }
    fs::write(&path, format!("---\n{front}---\n\n{summary}\n"))?;
    Ok(Some(path))
}

/// Names with characters outside `[A-Za-z0-9_-]` (`@scope/name`) are quoted.
fn quote(name: &str) -> String {
    if name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
    {
        name.to_string()
    } else {
        format!("\"{name}\"")
    }
}

fn slug(title: &str) -> String {
    let slug: String = title
        .to_lowercase()
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|w| !w.is_empty())
        .collect::<Vec<_>>()
        .join("_")
        .chars()
        .take(60)
        .collect();
    if slug.is_empty() {
        "changeset".to_string()
    } else {
        slug
    }
}

/// The PR comment for a failed check, with a link that opens GitHub's
/// new-file page with a patch changeset for the missing packages.
pub fn comment(
    check: &Check,
    config: &Config,
    repo_slug: &str,
    head_branch: &str,
    title: &str,
) -> String {
    let missing = check.missing();
    let mut out = format!("{MARKER}\n");
    if !check.bumps.is_empty() {
        out.push_str("**This PR bumps**\n\n");
        for b in &check.bumps {
            out.push_str(&format!("- `{}`: {}\n", b.package_name, b.level));
        }
        out.push('\n');
    }
    if !check.api.is_empty() {
        out.push_str("### Bump level too low\n\n");
        for f in &check.api {
            match f.required {
                Some(required) => out.push_str(&format!(
                    "- `{}`: the changeset declares `{}`, the API check requires `{required}`. Run `verwalter changeset --level {required} --package {}` on the branch, or raise the level in the changeset.\n",
                    f.package, f.claimed, f.package
                )),
                None => out.push_str(&format!("- `{}`: the API check did not run.\n", f.package)),
            }
            if !f.output.is_empty() {
                out.push_str(&format!("\n  <details><summary>API check output</summary>\n\n  ```\n{}\n  ```\n  </details>\n", indent(&f.output)));
            }
        }
        out.push('\n');
    }
    if missing.is_empty() {
        return out;
    }
    let front: String = missing.iter().map(|p| format!("{p}: patch\n")).collect();
    let content = format!("---\n{front}---\n\n{title}\n");
    let slug = slug(title);
    let link = format!(
        "https://github.com/{repo_slug}/new/{head_branch}?filename={CHANGESET_DIR}/{slug}.md&value={}",
        urlencode(&content)
    );
    out.push_str(
        "### Changeset missing\n\nThis PR changes versioned packages without a changeset for:\n\n",
    );
    for p in &missing {
        out.push_str(&format!("- `{p}`\n"));
    }
    if !check.covered.is_empty() {
        out.push_str("\nAlready covered: ");
        out.push_str(
            &check
                .covered
                .iter()
                .map(|p| format!("`{p}`"))
                .collect::<Vec<_>>()
                .join(", "),
        );
        out.push('\n');
    }
    out.push_str(&format!(
        "\n[**Create a changeset for the missing packages**]({link}) (prefilled with `patch`; edit the bump levels as needed), \
         or run `verwalter changeset --level <patch|minor|major>` on the branch.\n\n\
         If this change needs no release, add the `{}` label.\n",
        config.exempt_label
    ));
    out
}

fn indent(s: &str) -> String {
    s.lines()
        .map(|l| format!("  {l}"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn urlencode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Spec: the whole rule, against GitHub. Reads the PR, evaluates, upserts
/// the comment when it has something to say, deletes it otherwise,
/// returns the check. Changeset contents are read
/// from the checkout, which CI has at the PR head.
pub fn run(root: &Path, config: &Config, pr: u64, gh: Gh) -> Result<Check, Error> {
    #[derive(serde::Deserialize)]
    struct Pr {
        title: String,
        #[serde(rename = "headRefName")]
        head: String,
        labels: Vec<Label>,
        files: Vec<File>,
    }
    #[derive(serde::Deserialize)]
    struct Label {
        name: String,
    }
    #[derive(serde::Deserialize)]
    struct File {
        path: String,
    }
    let n = pr.to_string();
    let pr: Pr = serde_json::from_str(&gh(&[
        "pr",
        "view",
        &n,
        "--json",
        "title,headRefName,labels,files",
    ])?)?;
    let labels: Vec<String> = pr.labels.into_iter().map(|l| l.name).collect();
    let changed: Vec<String> = pr.files.into_iter().map(|f| f.path).collect();

    let existing = gh(&[
        "api",
        &format!("repos/{{owner}}/{{repo}}/issues/{n}/comments"),
        "--paginate",
        "--jq",
        &format!(".[] | select(.body | contains(\"{MARKER}\")) | .id"),
    ])?;
    let existing = existing.lines().next().map(str::to_string);

    // A changeset the tool cannot use is the contributor's finding too:
    // it goes into the comment, not only into the workflow log.
    let changesets = match read_changesets(root, config, &changed) {
        Ok(c) => c,
        Err(e) => {
            let body = format!("{MARKER}\n### Changeset invalid\n\n{e}\n");
            upsert_comment(gh, &n, existing.as_deref(), &body)?;
            return Err(e);
        }
    };
    let mut check = evaluate(config, &pr.head, &labels, &changed, &changesets);
    api_check(root, config, &mut check, &changesets)?;

    // The comment stays while it has something to say: a failure, or
    // the bumps the PR claims, as a sanity check for the reviewer.
    if check.status == Status::Failed || (check.status == Status::Passed && !check.bumps.is_empty())
    {
        let slug = gh(&[
            "repo",
            "view",
            "--json",
            "nameWithOwner",
            "--jq",
            ".nameWithOwner",
        ])?;
        let body = comment(&check, config, &slug, &pr.head, &pr.title);
        upsert_comment(gh, &n, existing.as_deref(), &body)?;
    } else if let Some(id) = existing {
        gh(&[
            "api",
            "-X",
            "DELETE",
            &format!("repos/{{owner}}/{{repo}}/issues/comments/{id}"),
        ])?;
    }
    Ok(check)
}

/// The PR's `.changeset/*.md` files, parsed and checked against the
/// config. The error names the file.
fn read_changesets(
    root: &Path,
    config: &Config,
    changed: &[String],
) -> Result<Vec<Changeset>, Error> {
    let known: Vec<&str> = config.packages.keys().map(String::as_str).collect();
    let mut out = Vec::new();
    for f in changed.iter().filter(|f| {
        f.starts_with(&format!("{CHANGESET_DIR}/"))
            && f.ends_with(".md")
            && !f.ends_with("README.md")
    }) {
        let Ok(content) = fs::read_to_string(root.join(f)) else {
            continue;
        };
        out.push(Changeset::parse_known(f, &content, &known).map_err(|e| format!("`{f}`: {e}"))?);
    }
    Ok(out)
}

fn upsert_comment(gh: Gh, pr: &str, existing: Option<&str>, body: &str) -> Result<(), Error> {
    match existing {
        Some(id) => gh(&[
            "api",
            "-X",
            "PATCH",
            &format!("repos/{{owner}}/{{repo}}/issues/comments/{id}"),
            "-f",
            &format!("body={body}"),
        ])?,
        None => gh(&["pr", "comment", pr, "--body", body])?,
    };
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONFIG: &str = r#"
[packages.livekit-protocol]
path = "livekit-protocol"
versioned_files = ["livekit-protocol/Cargo.toml"]
changelog = "livekit-protocol/CHANGELOG.md"

[packages.livekit-api]
path = "livekit-api"
versioned_files = ["livekit-api/Cargo.toml"]
changelog = "livekit-api/CHANGELOG.md"
"#;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    // obligations: rule-success/failure ChangesetCheckOnContributorPullRequest
    #[test]
    fn affected_covered_and_status() {
        let config = Config::parse(CONFIG).unwrap();
        let cs =
            [Changeset::parse(".changeset/x.md", "---\nlivekit-api: patch\n---\nx\n").unwrap()];

        let c = evaluate(
            &config,
            "feat",
            &[],
            &s(&["livekit-api/src/lib.rs", ".changeset/x.md"]),
            &cs,
        );
        assert_eq!(c.status, Status::Passed);

        let c = evaluate(
            &config,
            "feat",
            &[],
            &s(&[
                "livekit-api/src/lib.rs",
                "livekit-protocol/x.proto",
                ".changeset/x.md",
            ]),
            &cs,
        );
        assert_eq!(c.status, Status::Failed);
        assert_eq!(c.missing(), ["livekit-protocol"]);

        let c = evaluate(
            &config,
            "feat",
            &s(&["internal"]),
            &s(&["livekit-protocol/x.proto"]),
            &[],
        );
        assert_eq!(c.status, Status::Exempt);

        let c = evaluate(
            &config,
            "feat",
            &[],
            &s(&["README.md", ".github/workflows/ci.yml"]),
            &[],
        );
        assert_eq!(
            (c.status, c.affected.len()),
            (Status::Passed, 0),
            "no versioned package touched"
        );

        let c = evaluate(
            &config,
            "release/livekit-api",
            &[],
            &s(&["livekit-api/Cargo.toml"]),
            &[],
        );
        assert_eq!(c.status, Status::ReleasePullRequest);
    }

    // Spec: required_level. A change kind becomes the digit it moves.
    #[test]
    fn required_level_shifts_for_leading_zeros() {
        let v = |s: &str| -> Version { s.parse().unwrap() };
        assert_eq!(
            required_level(&v("1.2.3"), BumpLevel::Major),
            BumpLevel::Major
        );
        assert_eq!(
            required_level(&v("1.2.3"), BumpLevel::Minor),
            BumpLevel::Minor
        );
        assert_eq!(
            required_level(&v("0.4.0"), BumpLevel::Major),
            BumpLevel::Minor
        );
        assert_eq!(
            required_level(&v("0.4.0"), BumpLevel::Minor),
            BumpLevel::Patch
        );
        assert_eq!(
            required_level(&v("0.0.3"), BumpLevel::Major),
            BumpLevel::Patch
        );
        assert_eq!(
            required_level(&v("0.4.0-rc.1"), BumpLevel::Major),
            BumpLevel::Minor
        );
    }

    #[test]
    fn comment_names_the_level_the_api_check_requires() {
        let config = Config::parse(CONFIG).unwrap();
        let mut c = evaluate(
            &config,
            "feat/x",
            &[],
            &s(&["livekit-api/b", ".changeset/x.md"]),
            &[Changeset::parse(".changeset/x.md", "---\nlivekit-api: patch\n---\nx\n").unwrap()],
        );
        c.api.push(ApiFinding {
            package: "livekit-api".into(),
            claimed: BumpLevel::Patch,
            required: Some(BumpLevel::Minor),
            output: "fn x now takes 1 parameters".into(),
        });
        let body = comment(&c, &config, "livekit/rust-sdks", "feat/x", "t");
        assert!(body.contains("### Bump level too low"));
        assert!(body.contains("`livekit-api`: the changeset declares `patch`, the API check requires `minor`. Run `verwalter changeset --level minor --package livekit-api`"));
        assert!(body.contains("  fn x now takes 1 parameters"));
        assert!(!body.contains("Changeset missing"));
    }

    // Spec: ChangesetCheck.bumps. A passing PR still gets the comment,
    // with one line per package at the highest claimed level.
    #[test]
    fn passing_comment_lists_the_claimed_bumps() {
        let config = Config::parse(CONFIG).unwrap();
        let cs = [
            Changeset::parse(
                ".changeset/a.md",
                "---\nlivekit-api: patch\nlivekit-protocol: minor\n---\na\n",
            )
            .unwrap(),
            Changeset::parse(".changeset/b.md", "---\nlivekit-api: major\n---\nb\n").unwrap(),
        ];
        let c = evaluate(
            &config,
            "feat",
            &[],
            &s(&["livekit-api/x", ".changeset/a.md", ".changeset/b.md"]),
            &cs,
        );
        assert_eq!(c.status, Status::Passed);
        let body = comment(&c, &config, "o/r", "feat", "t");
        assert_eq!(body, format!("{MARKER}\n**This PR bumps**\n\n- `livekit-api`: major\n- `livekit-protocol`: minor\n\n"));
    }

    #[test]
    fn single_package_repo_path_dot_matches_everything() {
        let config = Config::parse("[packages.x]\npath = \".\"\nversioned_files = [\"package.json\"]\nchangelog = \"CHANGELOG.md\"\n").unwrap();
        let c = evaluate(&config, "feat", &[], &s(&["src/index.ts"]), &[]);
        assert_eq!(c.missing(), ["x"]);
        let c = evaluate(&config, "feat", &[], &s(&[".changeset/y.md"]), &[]);
        assert!(
            c.affected.is_empty(),
            "a changeset alone touches no package"
        );
    }

    #[test]
    fn comment_lists_missing_and_prefills_link() {
        let config = Config::parse(CONFIG).unwrap();
        let c = evaluate(
            &config,
            "feat/x",
            &[],
            &s(&["livekit-protocol/a", "livekit-api/b", ".changeset/x.md"]),
            &[Changeset::parse(".changeset/x.md", "---\nlivekit-api: minor\n---\nx\n").unwrap()],
        );
        let body = comment(&c, &config, "livekit/rust-sdks", "feat/x", "Add thing: v2");
        assert!(body.starts_with(MARKER));
        assert!(body.contains("**This PR bumps**\n\n- `livekit-api`: minor\n"));
        assert!(body.contains("- `livekit-protocol`\n"));
        assert!(body.contains("Already covered: `livekit-api`"));
        assert!(body.contains("https://github.com/livekit/rust-sdks/new/feat/x?filename=.changeset/add_thing_v2.md&value=---%0Alivekit-protocol%3A%20patch%0A---"));
        assert!(body.contains("add the `internal` label"));
    }
}
