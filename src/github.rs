//! Spec: ReleasePullRequest on GitHub. Pushes the release branch and
//! opens or updates the one pull request, carrying the release label and
//! a body with every package's changelog entry. With nothing planned the
//! open release PR is closed.
//! ponytail: GitHub access goes through the `gh` CLI, which CI has and
//! which carries the token; no HTTP client.

use std::{path::Path, process::Command};

use anyhow::{Context, Result, anyhow, bail};
use serde::Deserialize;

use crate::{apply::render_entry, branches, git, plan::PlannedRelease, repo::Repo};

/// Runs `gh` with the given arguments and returns stdout. Injected so
/// tests can fake GitHub.
pub type Gh<'a> = &'a mut dyn FnMut(&[&str]) -> Result<String>;

/// `owner/repo` of a GitHub remote URL (ssh or https), if it is one.
pub fn repo_slug(url: &str) -> Option<String> {
    let rest = url.trim().strip_suffix(".git").unwrap_or(url.trim());
    let after = rest
        .split_once("github.com")
        .map(|(_, a)| a.trim_start_matches([':', '/']))?;
    let mut parts = after.split('/');
    Some(format!("{}/{}", parts.next()?, parts.next()?))
}

/// Runs gh pinned to the repository behind `remote` (`GH_REPO`), so a
/// clone with an `upstream` remote never resolves PR numbers against the
/// parent repository.
pub fn gh_cli<'a>(
    root: &'a Path,
    remote: &str,
) -> impl FnMut(&[&str]) -> Result<String> + 'a + use<'a> {
    let slug = crate::git::run(root, &["remote", "get-url", remote])
        .ok()
        .and_then(|u| repo_slug(&u));
    move |args: &[&str]| {
        let mut cmd = Command::new("gh");
        cmd.current_dir(root).args(args);
        if let Some(slug) = &slug {
            cmd.env("GH_REPO", slug);
        }
        let out = cmd.output().context("cannot run gh")?;
        if !out.status.success() {
            bail!(
                "gh {}: {}",
                args.join(" "),
                String::from_utf8_lossy(&out.stderr).trim()
            );
        }
        Ok(String::from_utf8(out.stdout)?.trim().to_string())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct PullRequest {
    pub number: u64,
    #[serde(rename = "headRefName")]
    pub head: String,
    #[serde(rename = "baseRefName")]
    pub base: String,
}

pub const MARKER: &str = "<!-- verwalter release -->";

/// Wraps a gh runner so that every write (`pr create|edit|close|comment`,
/// `release create|delete|edit`, `label create`, `api -X ...`) is printed
/// and faked, while reads go through.
pub fn dry_run_gh(
    mut inner: impl FnMut(&[&str]) -> Result<String>,
) -> impl FnMut(&[&str]) -> Result<String> {
    move |args: &[&str]| {
        let is_write = matches!(
            args,
            ["pr", "create" | "edit" | "close" | "comment", ..]
                | ["release", "create" | "delete" | "edit", ..]
                | ["label", "create", ..]
                | ["api", "-X", ..]
        );
        if !is_write {
            return inner(args);
        }
        let shown: Vec<&str> = args
            .iter()
            .map(|a| if a.contains('\n') { "<body>" } else { *a })
            .collect();
        println!("dry-run: gh {}", shown.join(" "));
        Ok(match args {
            ["pr", "create", ..] => "https://dry-run/pull/0".to_string(),
            _ => String::new(),
        })
    }
}

/// Push every branch and make GitHub match the plan.
/// Spec: ChangelogUpdated, "PR link and author". For each changeset, the
/// commit that added it names the PR (via `commits/{sha}/pulls`) and its
/// author; a commit without a PR links the commit itself. Lookups that
/// fail leave the line without a suffix rather than block the release.
pub fn attribute(repo: &mut Repo, gh: Gh) {
    let root = repo.root.clone();
    for c in &mut repo.changesets {
        let Some(sha) = git::added_in(
            &root,
            "HEAD",
            &format!("{}/{}", crate::repo::CHANGESET_DIR, c.file),
        ) else {
            continue;
        };
        // gh output is trimmed, so a missing login (commit email not on
        // GitHub) is "-" rather than an empty last field.
        let user = |login: &str| {
            if login == "-" {
                String::new()
            } else {
                format!(" ([@{login}](https://github.com/{login}))")
            }
        };
        let pulls = gh(&[
            "api",
            &format!("repos/{{owner}}/{{repo}}/commits/{sha}/pulls"),
            "--jq",
            r#".[0] | select(. != null) | "\(.number)\t\(.html_url)\t\(.user.login // "-")""#,
        ]);
        let mut commit = || {
            gh(&[
                "api",
                &format!("repos/{{owner}}/{{repo}}/commits/{sha}"),
                "--jq",
                r#""\(.html_url)\t\(.author.login // "-")""#,
            ])
        };
        c.attribution = match pulls
            .ok()
            .filter(|s| !s.is_empty())
            .map(|s| s.splitn(3, '\t').map(String::from).collect::<Vec<_>>())
        {
            Some(parts) if parts.len() == 3 => Some(format!(
                "- [#{}]({}){}",
                parts[0],
                parts[1],
                user(&parts[2])
            )),
            _ => match commit()
                .ok()
                .filter(|s| !s.is_empty())
                .map(|s| s.splitn(2, '\t').map(String::from).collect::<Vec<_>>())
            {
                Some(parts) if parts.len() == 2 => Some(format!(
                    "- [`{}`]({}){}",
                    &sha[..7.min(sha.len())],
                    parts[0],
                    user(&parts[1])
                )),
                _ => None,
            },
        };
    }
}

/// Push the release branch and make GitHub match the plan: one open
/// release PR while something is planned, none otherwise. Only the PR on
/// the release branch is touched: pre-release PRs carry the same label.
pub fn sync(
    repo: &Repo,
    plan: &[PlannedRelease],
    branch: Option<&str>,
    remote: &str,
    dry_run: bool,
    gh: Gh,
) -> Result<Option<PullRequest>> {
    let label = repo.config.release.label.as_str();
    gh(&[
        "label",
        "create",
        label,
        "--force",
        "--color",
        "0E8A16",
        "--description",
        "Release PR managed by verwalter",
    ])?;

    let existing = open_pr(gh, &branches::branch_name(repo))?;
    let existing = existing.as_ref();

    if let Some(branch) = branch {
        git::write(&repo.root, dry_run, &["push", "--force", "--quiet", remote, &format!("{branch}:{branch}")]).map_err(|e| {
            if e.to_string().contains("directory file conflict") {
                let prefix = repo.config.release.branch_prefix.trim_end_matches('/');
                anyhow!("{e:#}\na branch named `{prefix}` exists on {remote}, so `{branch}` cannot be created; rename that branch or set [release].branch_prefix in verwalter.toml")
            } else {
                e
            }
        })?;
        return Ok(Some(upsert_pr(
            gh,
            existing,
            branch,
            &repo.config.git.base_branch,
            &branches::title(plan),
            &body(plan, repo),
            label,
        )?));
    }

    // Spec: CloseReleasePullRequestWithNothingPending.
    if let Some(stale) = existing {
        gh(&[
            "pr",
            "close",
            &stale.number.to_string(),
            "--delete-branch",
            "--comment",
            "Nothing left to release.",
        ])?;
    }
    Ok(None)
}

/// Open a PR for `head` against `base`, or bring `existing` (the open PR
/// on `head`) up to date: title, body, label and base.
pub fn upsert_pr(
    gh: Gh,
    existing: Option<&PullRequest>,
    head: &str,
    base: &str,
    title: &str,
    body: &str,
    label: &str,
) -> Result<PullRequest> {
    let number = match existing {
        Some(existing) => {
            let n = existing.number.to_string();
            gh(&[
                "pr",
                "edit",
                &n,
                "--title",
                title,
                "--body",
                body,
                "--add-label",
                label,
            ])?;
            if existing.base != base {
                gh(&["pr", "edit", &n, "--base", base])?;
            }
            existing.number
        }
        None => number_from_url(&gh(&[
            "pr", "create", "--head", head, "--base", base, "--title", title, "--body", body,
            "--label", label,
        ])?)?,
    };
    Ok(PullRequest {
        number,
        head: head.to_string(),
        base: base.to_string(),
    })
}

fn number_from_url(url: &str) -> Result<u64> {
    url.trim()
        .rsplit('/')
        .next()
        .and_then(|n| n.parse().ok())
        .with_context(|| format!("unexpected `gh pr create` output: {url}"))
}

/// Append `key=value` lines to `$GITHUB_OUTPUT` when running in Actions.
pub fn append_output(lines: &str) -> Result<()> {
    if let Ok(path) = std::env::var("GITHUB_OUTPUT") {
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new()
            .append(true)
            .create(true)
            .open(path)?;
        f.write_all(lines.as_bytes())?;
    }
    Ok(())
}

/// The open PR whose head is `head`, if any.
pub fn open_pr(gh: Gh, head: &str) -> Result<Option<PullRequest>> {
    let open: Vec<PullRequest> = serde_json::from_str(&gh(&[
        "pr",
        "list",
        "--state",
        "open",
        "--head",
        head,
        "--json",
        "number,headRefName,baseRefName",
    ])?)?;
    Ok(open.into_iter().next())
}

/// One package's section of a PR body: heading, `note`, changelog entry.
pub fn section(
    release: &PlannedRelease,
    repo: &Repo,
    plan: &[PlannedRelease],
    note: &str,
) -> String {
    format!(
        "\n## {} `{}` → `{}`\n\n{note}{}",
        release.package,
        release.from,
        release.to,
        render_entry(release, &repo.changesets, plan)
            .trim_start_matches(|c| c != '\n')
            .trim_start()
    )
}

/// The PR body: one section per package with its changelog entry.
fn body(plan: &[PlannedRelease], repo: &Repo) -> String {
    let mut out = format!("{MARKER}\nMerging this PR releases:\n");
    for release in plan {
        // Spec: Package.first_release, for a version not yet tagged.
        let first = crate::release::first_release(
            repo,
            &release.package,
            &release.from,
            &crate::registry::curl,
        )
        .unwrap_or(false);
        let note = if first {
            "**First release**: no tag of this package exists yet. A registry with trusted publishing needs the first upload done by hand.\n\n"
        } else {
            ""
        };
        out.push_str(&section(release, repo, plan, note));
        if !release.changesets.is_empty() {
            out.push_str(&format!(
                "\nChangesets: {}\n",
                release
                    .changesets
                    .iter()
                    .map(|c| format!("`{c}`"))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{changeset::Changeset, config::Config, plan::PackageState};
    use std::path::PathBuf;

    fn repo() -> Repo {
        let config = Config::parse("[packages.a]\npath = \"a\"\nversioning.files = [\"Cargo.toml\"]\nversioning.changelog = \"CHANGELOG.md\"\n[packages.b]\npath = \"b\"\nversioning.files = [\"Cargo.toml\"]\nversioning.changelog = \"CHANGELOG.md\"\n").unwrap();
        Repo {
            root: PathBuf::from("."),
            config,
            packages: vec![
                PackageState::new("a", "1.0.0".parse().unwrap(), vec![]),
                PackageState::new("b", "2.0.0".parse().unwrap(), vec!["a".into()]),
            ],
            changesets: vec![
                Changeset::parse("x.md", "---\na: minor\n---\nAdd thing\nmore detail\n").unwrap(),
            ],
            warnings: Vec::new(),
        }
    }

    // Spec: ChangelogUpdated, PR link and author in livekit form. The
    // suffix sits on the first line; continuation lines keep their indent.
    #[test]
    fn attribution_follows_the_first_summary_line() {
        let mut r = repo();
        r.changesets[0].attribution = Some(
            "- [#12](https://github.com/o/r/pull/12) ([@lukas](https://github.com/lukas))".into(),
        );
        let plan = crate::plan::plan(&r.packages, &r.changesets, crate::version::BumpLevel::Patch);
        let a = body(&plan, &r);
        assert!(a.contains("- Add thing - [#12](https://github.com/o/r/pull/12) ([@lukas](https://github.com/lukas))\n  more detail"), "{a}");
    }

    #[test]
    fn body_has_a_section_per_package() {
        let r = repo();
        let plan = crate::plan::plan(&r.packages, &r.changesets, crate::version::BumpLevel::Patch);
        let b = body(&plan, &r);
        assert!(b.starts_with(MARKER));
        assert!(b.contains("## a `1.0.0` → `1.1.0`"));
        assert!(b.contains("**First release**"), "no a-v* tag exists here");
        assert!(b.contains("### Minor Changes\n\n- Add thing"));
        assert!(b.contains("Changesets: `x.md`"));
        assert!(b.contains("## b `2.0.0` → `2.0.1`"));
        assert!(b.contains("Updated dependencies: a@1.1.0"));
        assert_eq!(b.matches("Changesets:").count(), 1);
    }

    #[test]
    fn repo_slug_from_remote_urls() {
        assert_eq!(
            repo_slug("git@github.com:lukasIO/rust-sdks.git").as_deref(),
            Some("lukasIO/rust-sdks")
        );
        assert_eq!(
            repo_slug("https://github.com/livekit/node-sdks").as_deref(),
            Some("livekit/node-sdks")
        );
        assert_eq!(repo_slug("https://gitlab.com/x/y.git"), None);
    }

    #[test]
    fn number_from_create_output() {
        assert_eq!(
            number_from_url("https://github.com/livekit/rust-sdks/pull/1312\n").unwrap(),
            1312
        );
        assert!(number_from_url("nope").is_err());
    }
}
