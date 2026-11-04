//! Spec: ChangesetCheck, rule ChangesetCheckOnContributorPullRequest and
//! surface ContributorPullRequestCheck. The contributor sees one PR
//! comment, updated in place and removed once the check passes.

use std::{collections::BTreeSet, fs, path::Path};

use std::process::Command;

use anyhow::{Context, Result, anyhow, ensure};

use crate::{
    Version, changeset::Changeset, config::Config, github::Gh, repo::CHANGESET_DIR,
    version::BumpLevel,
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
    /// Spec: ChangesetCheck.also_released. Packages the merge releases
    /// without a changeset of their own, with the reason: a dependent,
    /// or a member of a fixed group. Filled by `also_released`.
    pub also: Vec<(String, String)>,
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
    let status = if head_branch.starts_with(&config.release.branch_prefix) {
        Status::ReleasePullRequest
    } else if labels.iter().any(|l| l == &config.check.exempt_label) {
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
        also: Vec::new(),
    }
}

/// Spec: ChangesetCheck.also_released. The plan over the PR's changesets
/// alone, minus the covered packages: what rides along, and why.
pub fn also_released(check: &mut Check, planned: &[crate::plan::PlannedRelease]) {
    check.also = planned
        .iter()
        .filter(|r| !check.covered.contains(&r.package))
        .map(|r| {
            let names = |v: &[String]| {
                v.iter()
                    .map(|n| format!("`{n}`"))
                    .collect::<Vec<_>>()
                    .join(", ")
            };
            let why = if !r.bumped_dependencies.is_empty() {
                format!("depends on {}", names(&r.bumped_dependencies))
            } else {
                let covered: Vec<String> = r
                    .fixed_with
                    .iter()
                    .filter(|m| check.covered.contains(*m))
                    .cloned()
                    .collect();
                format!("fixed with {}", names(&covered))
            };
            (r.package.clone(), why)
        })
        .collect();
}

/// `also_released` against the manifests in the checkout.
pub fn plan_also(
    root: &Path,
    config: &Config,
    check: &mut Check,
    changesets: &[Changeset],
) -> Result<()> {
    if check.bumps.is_empty() {
        return Ok(());
    }
    let (packages, _) = crate::repo::Repo::load_packages(root, config)?;
    let planned = crate::plan::plan(
        &packages,
        changesets,
        config.versioning.dependent_bump_level,
    );
    also_released(check, &planned);
    Ok(())
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
) -> Result<()> {
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
        let version = crate::repo::Repo::load_packages(root, config)?
            .0
            .into_iter()
            .find(|p| &p.name == name)
            .map(|p| p.version)
            .with_context(|| format!("{name}: not loaded"))?;
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
            .with_context(|| format!("{name}: api command"))?;
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
) -> Result<Option<std::path::PathBuf>> {
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
            ensure!(repo.config.packages.contains_key(p), "unknown package {p}");
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
        if !check.also.is_empty() {
            out.push_str("**Also released**\n\n");
            for (p, why) in &check.also {
                out.push_str(&format!("- `{p}`: {why}\n"));
            }
            out.push('\n');
        }
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
    let link = changeset_link(&missing, repo_slug, head_branch, title);
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
        config.check.exempt_label
    ));
    out
}

/// GitHub's new-file page on the PR branch, prefilled with a patch
/// changeset for `missing`.
fn changeset_link(missing: &[&str], repo_slug: &str, head_branch: &str, title: &str) -> String {
    let front: String = missing.iter().map(|p| format!("{p}: patch\n")).collect();
    let content = format!("---\n{front}---\n\n{title}\n");
    format!(
        "https://github.com/{repo_slug}/new/{head_branch}?filename={CHANGESET_DIR}/{}.md&value={}",
        slug(title),
        urlencode(&content)
    )
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
pub fn run(root: &Path, config: &Config, pr: u64, gh: Gh) -> Result<Check> {
    #[derive(serde::Deserialize)]
    struct Pr {
        title: String,
        #[serde(rename = "headRefName")]
        head: String,
        #[serde(rename = "headRepositoryOwner")]
        head_owner: Owner,
        #[serde(rename = "headRepository")]
        head_repo: Repo,
        labels: Vec<Label>,
        files: Vec<File>,
    }
    #[derive(serde::Deserialize)]
    struct Owner {
        login: String,
    }
    #[derive(serde::Deserialize)]
    struct Repo {
        name: String,
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
        "title,headRefName,headRepositoryOwner,headRepository,labels,files",
    ])?)?;
    let labels: Vec<String> = pr.labels.into_iter().map(|l| l.name).collect();
    let changed: Vec<String> = pr.files.into_iter().map(|f| f.path).collect();

    let existing = gh(&[
        "api",
        &format!("repos/{{owner}}/{{repo}}/issues/{n}/comments"),
        "--paginate",
        "--jq",
        &format!(".[] | select(.body | contains(\"{MARKER}\")) | .id"),
    ])
    .map(|out| out.lines().next().map(str::to_string));

    // A changeset the tool cannot use is the contributor's finding too:
    // it goes into the comment, not only into the workflow log.
    let changesets = match read_changesets(root, config, &changed) {
        Ok(c) => c,
        Err(e) => {
            let body = format!("{MARKER}\n### Changeset invalid\n\n{e:#}\n");
            report(&body);
            sync_comment(gh, &n, &existing, Some(&body));
            return Err(e);
        }
    };
    let mut check = evaluate(config, &pr.head, &labels, &changed, &changesets);
    plan_also(root, config, &mut check, &changesets)?;
    api_check(root, config, &mut check, &changesets)?;

    // The comment stays while it has something to say: a failure, or
    // the bumps the PR claims, as a sanity check for the reviewer.
    if check.status == Status::Failed || (check.status == Status::Passed && !check.bumps.is_empty())
    {
        // The PR branch lives in the head repository: on a fork PR that is
        // the fork, where the contributor can push and the base repo's
        // new-file page would 404.
        let slug = format!("{}/{}", pr.head_owner.login, pr.head_repo.name);
        let body = comment(&check, config, &slug, &pr.head, &pr.title);
        report(&body);
        let missing = check.missing();
        if !missing.is_empty() {
            let link = changeset_link(&missing, &slug, &pr.head, &pr.title);
            println!("Create the missing changeset: {link}");
        }
        sync_comment(gh, &n, &existing, Some(&body));
    } else {
        sync_comment(gh, &n, &existing, None);
    }
    Ok(check)
}

/// The comment body in the job summary, where GitHub renders it: a PR
/// from a fork gets a read-only token and no comment. The log keeps the
/// plain lines that `main` prints.
fn report(body: &str) {
    let body = body.trim_start_matches(MARKER).trim();
    if let Ok(path) = std::env::var("GITHUB_STEP_SUMMARY") {
        let written = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .and_then(|mut f| std::io::Write::write_all(&mut f, format!("{body}\n").as_bytes()));
        match written {
            Ok(()) => println!("The full result is in the job summary."),
            Err(e) => eprintln!("warning: cannot write the job summary: {e}"),
        }
    }
}

/// The PR's `.changeset/*.md` files, parsed and checked against the
/// config. The error names the file.
fn read_changesets(root: &Path, config: &Config, changed: &[String]) -> Result<Vec<Changeset>> {
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
        out.push(Changeset::parse_known(f, &content, &known).with_context(|| format!("`{f}`"))?);
    }
    Ok(out)
}

/// Writes the comment (`body`) or deletes it (None). Best effort: the
/// token of a PR from a fork cannot write, and the check passes or fails
/// on its result, not on the comment.
fn sync_comment(gh: Gh, pr: &str, existing: &Result<Option<String>>, body: Option<&str>) {
    let written = match (existing, body) {
        (Err(e), _) => Err(anyhow!("{e:#}")),
        (Ok(Some(id)), Some(body)) => gh(&[
            "api",
            "-X",
            "PATCH",
            &format!("repos/{{owner}}/{{repo}}/issues/comments/{id}"),
            "-f",
            &format!("body={body}"),
        ]),
        (Ok(None), Some(body)) => gh(&["pr", "comment", pr, "--body", body]),
        (Ok(Some(id)), None) => gh(&[
            "api",
            "-X",
            "DELETE",
            &format!("repos/{{owner}}/{{repo}}/issues/comments/{id}"),
        ]),
        (Ok(None), None) => return,
    };
    if let Err(e) = written {
        let e = format!("{e:#}");
        let first = e.lines().next().unwrap_or_default();
        println!("::warning::PR comment not updated, the result is in the job summary: {first}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONFIG: &str = r#"
[packages.livekit-protocol]
path = "livekit-protocol"
versioning.files = ["Cargo.toml"]
versioning.changelog = "CHANGELOG.md"

[packages.livekit-api]
path = "livekit-api"
versioning.files = ["Cargo.toml"]
versioning.changelog = "CHANGELOG.md"
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
            &s(&["src/lib.rs", ".changeset/x.md"]),
            &cs,
        );
        assert_eq!(c.status, Status::Passed);

        let c = evaluate(
            &config,
            "feat",
            &[],
            &s(&["src/lib.rs", "livekit-protocol/x.proto", ".changeset/x.md"]),
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
            &s(&["Cargo.toml"]),
            &[],
        );
        assert_eq!(c.status, Status::ReleasePullRequest);
    }

    // A PR from a fork has a read-only token: the comment write fails,
    // and the check still returns its own result. The changeset link
    // points at the fork, where the PR branch lives.
    #[test]
    fn run_returns_the_check_when_the_comment_cannot_be_written() {
        let config = Config::parse(CONFIG).unwrap();
        let mut writes = 0;
        let mut body = String::new();
        let mut gh = |args: &[&str]| -> Result<String> {
            match args {
                ["pr", "view", ..] => Ok(r#"{"title":"t","headRefName":"feat","headRepositoryOwner":{"login":"o"},"headRepository":{"name":"r"},"labels":[],"files":[{"path":"livekit-protocol/x.proto"}]}"#.into()),
                ["api", path, ..] if path.ends_with("/comments") => Ok(String::new()),
                _ => {
                    writes += 1;
                    body = args.last().unwrap_or(&"").to_string();
                    Err(anyhow!("HTTP 403: Resource not accessible by integration"))
                }
            }
        };
        let c = run(&std::env::temp_dir(), &config, 1, &mut gh).unwrap();
        assert_eq!(c.status, Status::Failed);
        assert_eq!(writes, 1);
        assert!(
            body.contains("https://github.com/o/r/new/feat?"),
            "link should open the new-file page on the PR head repo: {body}"
        );
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
            &s(&["b", ".changeset/x.md"]),
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
            &s(&["x", ".changeset/a.md", ".changeset/b.md"]),
            &cs,
        );
        assert_eq!(c.status, Status::Passed);
        let body = comment(&c, &config, "o/r", "feat", "t");
        assert_eq!(
            body,
            format!(
                "{MARKER}\n**This PR bumps**\n\n- `livekit-api`: major\n- `livekit-protocol`: minor\n\n"
            )
        );
    }

    // Spec: ChangesetCheck.also_released. A dependent and a fixed-group
    // member ride along, named with the reason and without a level.
    #[test]
    fn also_released_names_dependents_and_group_members() {
        use crate::plan::{PackageState, plan};
        let v = |s: &str| -> Version { s.parse().unwrap() };
        let mut packages = vec![
            PackageState::new("core", v("1.0.0"), vec![]),
            PackageState::new("ffi", v("1.0.0"), vec![]),
            PackageState::new("client", v("2.0.0"), vec!["core".into()]),
        ];
        packages[0].fixed_with = vec!["ffi".into()];
        packages[1].fixed_with = vec!["core".into()];
        let config = Config::parse("[packages.core]\npath = \"core\"\nversioning.files = [\"Cargo.toml\"]\nversioning.changelog = \"c\"\n[packages.ffi]\npath = \"ffi\"\nversioning.files = [\"Cargo.toml\"]\nversioning.changelog = \"c\"\n[packages.client]\npath = \"client\"\nversioning.files = [\"Cargo.toml\"]\nversioning.changelog = \"c\"\n").unwrap();
        let cs = [Changeset::parse(".changeset/a.md", "---\ncore: minor\n---\na\n").unwrap()];
        let mut c = evaluate(
            &config,
            "feat",
            &[],
            &s(&["core/x", ".changeset/a.md"]),
            &cs,
        );
        also_released(&mut c, &plan(&packages, &cs, BumpLevel::Patch));
        assert_eq!(
            c.also,
            [
                ("ffi".to_string(), "fixed with `core`".to_string()),
                ("client".to_string(), "depends on `core`".to_string())
            ]
        );
        let body = comment(&c, &config, "o/r", "feat", "t");
        assert!(body.contains(
            "**Also released**\n\n- `ffi`: fixed with `core`\n- `client`: depends on `core`\n"
        ));
    }

    #[test]
    fn single_package_repo_path_dot_matches_everything() {
        let config = Config::parse("[packages.x]\npath = \".\"\nversioning.files = [\"package.json\"]\nversioning.changelog = \"CHANGELOG.md\"\n").unwrap();
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
