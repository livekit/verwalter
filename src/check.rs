//! Spec: ChangesetCheck, rule ChangesetCheckOnContributorPullRequest and
//! surface ContributorPullRequestCheck. The contributor sees one PR
//! comment, updated in place and removed once the check passes.

use std::{collections::BTreeSet, fs, path::Path};

use crate::{changeset::Changeset, config::Config, github::Gh, repo::CHANGESET_DIR, Error};

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
    }
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
        None => crate::git::run(root, &["log", "-1", "--format=%s"])?.trim().to_string(),
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
    if name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_')) {
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
    let front: String = missing.iter().map(|p| format!("{p}: patch\n")).collect();
    let content = format!("---\n{front}---\n\n{title}\n");
    let slug = slug(title);
    let link = format!(
        "https://github.com/{repo_slug}/new/{head_branch}?filename={CHANGESET_DIR}/{slug}.md&value={}",
        urlencode(&content)
    );
    let mut out = format!("{MARKER}\n### Changeset missing\n\nThis PR changes versioned packages without a changeset for:\n\n");
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
/// or deletes the comment, returns the check. Changeset contents are read
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
    let check = evaluate(config, &pr.head, &labels, &changed, &changesets);

    if check.status == Status::Failed {
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
fn read_changesets(root: &Path, config: &Config, changed: &[String]) -> Result<Vec<Changeset>, Error> {
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
        let changeset = Changeset::parse(f, &content).map_err(|e| format!("`{f}`: {e}"))?;
        let unknown = changeset.unknown_packages(&known);
        if !unknown.is_empty() {
            return Err(format!("`{f}`: names unknown package {}", unknown.join(", ")).into());
        }
        out.push(changeset);
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
        assert!(body.contains("- `livekit-protocol`\n"));
        assert!(body.contains("Already covered: `livekit-api`"));
        assert!(body.contains("https://github.com/livekit/rust-sdks/new/feat/x?filename=.changeset/add_thing_v2.md&value=---%0Alivekit-protocol%3A%20patch%0A---"));
        assert!(body.contains("add the `internal` label"));
    }
}
