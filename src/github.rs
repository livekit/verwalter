//! Spec: ReleasePullRequest on GitHub. Pushes the release branches and
//! opens or updates one pull request per branch, in stack order, carrying
//! the release label and a body with the changelog entry. Open release PRs
//! whose package is no longer planned are closed.
//! ponytail: GitHub access goes through the `gh` CLI, which CI has and
//! which carries the token; no HTTP client.

use std::{path::Path, process::Command};

use serde::Deserialize;

use crate::{apply::render_entry, branches::Branch, git, plan::PlannedRelease, repo::Repo, Error};

/// Runs `gh` with the given arguments and returns stdout. Injected so
/// tests can fake GitHub.
pub type Gh<'a> = &'a mut dyn FnMut(&[&str]) -> Result<String, Error>;

/// `owner/repo` of a GitHub remote URL (ssh or https), if it is one.
pub fn repo_slug(url: &str) -> Option<String> {
    let rest = url.trim().strip_suffix(".git").unwrap_or(url.trim());
    let after = rest.split_once("github.com").map(|(_, a)| a.trim_start_matches([':', '/']))?;
    let mut parts = after.split('/');
    Some(format!("{}/{}", parts.next()?, parts.next()?))
}

/// Runs gh pinned to the repository behind `remote` (`GH_REPO`), so a
/// clone with an `upstream` remote never resolves PR numbers against the
/// parent repository.
pub fn gh_cli<'a>(root: &'a Path, remote: &str) -> impl FnMut(&[&str]) -> Result<String, Error> + 'a {
    let slug = crate::git::run(root, &["remote", "get-url", remote]).ok().and_then(|u| repo_slug(&u));
    move |args: &[&str]| {
        let mut cmd = Command::new("gh");
        cmd.current_dir(root).args(args).env("GH_STACK_NO_UPDATE_NOTIFIER", "1");
        if let Some(slug) = &slug {
            cmd.env("GH_REPO", slug);
        }
        let out = cmd.output()?;
        if !out.status.success() {
            return Err(format!("gh {}: {}", args.join(" "), String::from_utf8_lossy(&out.stderr).trim()).into());
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
pub fn dry_run_gh(mut inner: impl FnMut(&[&str]) -> Result<String, Error>) -> impl FnMut(&[&str]) -> Result<String, Error> {
    move |args: &[&str]| {
        let is_write = matches!(
            args,
            ["pr", "create" | "edit" | "close" | "comment", ..]
                | ["release", "create" | "delete" | "edit", ..]
                | ["label", "create", ..]
                | ["stack", ..]
                | ["api", "-X", ..]
        );
        if !is_write {
            return inner(args);
        }
        let shown: Vec<&str> = args.iter().map(|a| if a.contains('\n') { "<body>" } else { *a }).collect();
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
        let Some(sha) = git::added_in(&root, "HEAD", &format!("{}/{}", crate::repo::CHANGESET_DIR, c.file)) else { continue };
        let user = |login: &str| if login.is_empty() { String::new() } else { format!(" ([@{login}](https://github.com/{login}))") };
        let pulls = gh(&["api", &format!("repos/{{owner}}/{{repo}}/commits/{sha}/pulls"), "--jq", r#".[0] | select(. != null) | "\(.number)\t\(.html_url)\t\(.user.login)""#]);
        let mut commit = || gh(&["api", &format!("repos/{{owner}}/{{repo}}/commits/{sha}"), "--jq", r#""\(.html_url)\t\(.author.login // "")""#]);
        c.attribution = match pulls.ok().filter(|s| !s.is_empty()).map(|s| s.splitn(3, '\t').map(String::from).collect::<Vec<_>>()) {
            Some(parts) if parts.len() == 3 => Some(format!("- [#{}]({}){}", parts[0], parts[1], user(&parts[2]))),
            _ => match commit().ok().filter(|s| !s.is_empty()).map(|s| s.splitn(2, '\t').map(String::from).collect::<Vec<_>>()) {
                Some(parts) if parts.len() == 2 => Some(format!("- [`{}`]({}){}", &sha[..7.min(sha.len())], parts[0], user(&parts[1]))),
                _ => None,
            },
        };
    }
}

pub fn sync(repo: &Repo, plan: &[PlannedRelease], branches: &[Branch], remote: &str, dry_run: bool, gh: Gh) -> Result<Vec<PullRequest>, Error> {
    let label = repo.config.release_label.as_str();
    gh(&["label", "create", label, "--force", "--color", "0E8A16", "--description", "Release PR managed by verwalter"])?;

    let open: Vec<PullRequest> = serde_json::from_str(&gh(&[
        "pr", "list", "--state", "open", "--label", label, "--limit", "200", "--json", "number,headRefName,baseRefName",
    ])?)?;

    let mut result = Vec::new();
    for (release, branch) in plan.iter().zip(branches) {
        git::write(&repo.root, dry_run, &["push", "--force", "--quiet", remote, &format!("{}:{}", branch.name, branch.name)]).map_err(|e| {
            if e.to_string().contains("directory file conflict") {
                let prefix = repo.config.release_branch_prefix.trim_end_matches('/');
                format!("{e}\na branch named `{prefix}` exists on {remote}, so `{}` cannot be created; rename that branch or set release_branch_prefix in verwalter.toml", branch.name).into()
            } else {
                e
            }
        })?;

        let parent = release.stacked_on.as_deref().and_then(|p| result.iter().find(|pr: &&PullRequest| pr.head == format!("{}{p}", repo.config.release_branch_prefix)));
        let title = format!("release: {} {}", release.package, release.to);
        let body = body(release, repo, plan, parent);

        let existing = open.iter().find(|pr| pr.head == branch.name);
        result.push(upsert_pr(gh, existing, &branch.name, &branch.target, &title, &body, label)?);
    }

    // Spec: a package with nothing left to release has no open release PR.
    for stale in open.iter().filter(|pr| !branches.iter().any(|b| b.name == pr.head)) {
        gh(&["pr", "close", &stale.number.to_string(), "--delete-branch", "--comment", "Nothing left to release for this package."])?;
    }

    // Declare the stack to GitHub (`gh stack link`, bottom to top) so the
    // PRs show as one stack and merging the top lands all of them. GitHub
    // stacks are linear: only a chain is linked, a tree is left as is. A
    // failure here is cosmetic and only printed.
    if let Some(chain) = linear_chain(plan) {
        let numbers: Vec<String> = chain
            .iter()
            .filter_map(|p| result.iter().find(|pr| pr.head == format!("{}{p}", repo.config.release_branch_prefix)))
            .map(|pr| pr.number.to_string())
            .collect();
        if numbers.len() == chain.len() && numbers.len() > 1 {
            let mut args = vec!["stack", "link"];
            args.extend(numbers.iter().map(String::as_str));
            if let Err(e) = gh(&args) {
                println!("warning: could not link the PRs into a GitHub stack (is the gh-stack extension installed?): {e}");
            }
        }
    }
    Ok(result)
}

/// The packages bottom to top when the plan is one chain: every release is
/// stacked on the previous one.
fn linear_chain(plan: &[PlannedRelease]) -> Option<Vec<&str>> {
    let mut prev: Option<&str> = None;
    for r in plan {
        if r.stacked_on.as_deref() != prev {
            return None;
        }
        prev = Some(&r.package);
    }
    Some(plan.iter().map(|r| r.package.as_str()).collect())
}

/// Open a PR for `head` against `base`, or bring `existing` (the open PR
/// on `head`) up to date: title, body, label and base.
pub fn upsert_pr(gh: Gh, existing: Option<&PullRequest>, head: &str, base: &str, title: &str, body: &str, label: &str) -> Result<PullRequest, Error> {
    let number = match existing {
        Some(existing) => {
            let n = existing.number.to_string();
            gh(&["pr", "edit", &n, "--title", title, "--body", body, "--add-label", label])?;
            // GitHub refuses a base change on a PR that is part of a
            // stack; `gh stack link` in `sync` corrects the chain instead.
            if existing.base != base {
                if let Err(e) = gh(&["pr", "edit", &n, "--base", base]) {
                    if !e.to_string().contains("part of a stack") {
                        return Err(e);
                    }
                }
            }
            existing.number
        }
        None => number_from_url(&gh(&["pr", "create", "--head", head, "--base", base, "--title", title, "--body", body, "--label", label])?)?,
    };
    Ok(PullRequest { number, head: head.to_string(), base: base.to_string() })
}

fn number_from_url(url: &str) -> Result<u64, Error> {
    url.trim().rsplit('/').next().and_then(|n| n.parse().ok()).ok_or_else(|| format!("unexpected `gh pr create` output: {url}").into())
}

fn body(release: &PlannedRelease, repo: &Repo, plan: &[PlannedRelease], parent: Option<&PullRequest>) -> String {
    let mut out = format!("{MARKER}\n");
    if let Some(parent) = parent {
        out.push_str(&format!("Stacked on #{} (`{}`). Merge that one first; merging this one into it releases both together.\n\n", parent.number, parent.head));
    }
    out.push_str(&format!("Merging this PR releases **{}** `{}` → `{}`.\n\n", release.package, release.from, release.to));
    out.push_str(render_entry(release, &repo.changesets, plan).trim_start_matches(|c| c != '\n').trim_start());
    if !release.changesets.is_empty() {
        out.push_str(&format!("\nChangesets: {}\n", release.changesets.iter().map(|c| format!("`{c}`")).collect::<Vec<_>>().join(", ")));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{config::Config, plan::PackageState, changeset::Changeset};
    use std::path::PathBuf;

    fn repo() -> Repo {
        let config = Config::parse("[packages.a]\npath = \"a\"\nversioned_files = [\"a/Cargo.toml\"]\nchangelog = \"a/CHANGELOG.md\"\n[packages.b]\npath = \"b\"\nversioned_files = [\"b/Cargo.toml\"]\nchangelog = \"b/CHANGELOG.md\"\n").unwrap();
        Repo {
            root: PathBuf::from("."),
            config,
            packages: vec![
                PackageState { name: "a".into(), version: "1.0.0".parse().unwrap(), dependencies: vec![] },
                PackageState { name: "b".into(), version: "2.0.0".parse().unwrap(), dependencies: vec!["a".into()] },
            ],
            changesets: vec![Changeset::parse("x.md", "---\na: minor\n---\nAdd thing\nmore detail\n").unwrap()],
        }
    }

    // Spec: ChangelogUpdated, PR link and author in livekit form. The
    // suffix sits on the first line; continuation lines keep their indent.
    #[test]
    fn attribution_follows_the_first_summary_line() {
        let mut r = repo();
        r.changesets[0].attribution = Some("- [#12](https://github.com/o/r/pull/12) ([@lukas](https://github.com/lukas))".into());
        let plan = crate::plan::plan(&r.packages, &r.changesets, crate::version::BumpLevel::Patch);
        let a = body(&plan[0], &r, &plan, None);
        assert!(a.contains("- Add thing - [#12](https://github.com/o/r/pull/12) ([@lukas](https://github.com/lukas))\n  more detail"), "{a}");
    }

    #[test]
    fn body_has_stack_note_changelog_and_changesets() {
        let r = repo();
        let plan = crate::plan::plan(&r.packages, &r.changesets, crate::version::BumpLevel::Patch);
        let parent = PullRequest { number: 7, head: "release/a".into(), base: "main".into() };
        let a = body(&plan[0], &r, &plan, None);
        assert!(a.starts_with(MARKER));
        assert!(a.contains("releases **a** `1.0.0` → `1.1.0`"));
        assert!(a.contains("### Minor Changes\n\n- Add thing"));
        assert!(a.contains("Changesets: `x.md`"));
        let b = body(&plan[1], &r, &plan, Some(&parent));
        assert!(b.contains("Stacked on #7 (`release/a`)"));
        assert!(b.contains("Updated dependencies: a@1.1.0"));
        assert!(!b.contains("Changesets:"));
    }

    #[test]
    fn repo_slug_from_remote_urls() {
        assert_eq!(repo_slug("git@github.com:lukasIO/rust-sdks.git").as_deref(), Some("lukasIO/rust-sdks"));
        assert_eq!(repo_slug("https://github.com/livekit/node-sdks").as_deref(), Some("livekit/node-sdks"));
        assert_eq!(repo_slug("https://gitlab.com/x/y.git"), None);
    }

    #[test]
    fn linear_chain_only_for_chains() {
        let r = repo();
        let plan = crate::plan::plan(&r.packages, &r.changesets, crate::version::BumpLevel::Patch);
        assert_eq!(linear_chain(&plan), Some(vec!["a", "b"]));
        let mut tree = plan.clone();
        tree.push(PlannedRelease { package: "c".into(), stacked_on: Some("a".into()), ..plan[1].clone() });
        assert_eq!(linear_chain(&tree), None);
    }

    #[test]
    fn number_from_create_output() {
        assert_eq!(number_from_url("https://github.com/livekit/rust-sdks/pull/1312\n").unwrap(), 1312);
        assert!(number_from_url("nope").is_err());
    }
}
