//! Spec: ReleasePullRequest on GitHub. Pushes the release branch and
//! opens or updates the one pull request, carrying the release label and
//! a body with every package's changelog entry. With nothing planned the
//! open release PR is closed.
//! ponytail: GitHub access goes through the `gh` CLI, which CI has and
//! which carries the token; no HTTP client.

use std::{
    path::{Path, PathBuf},
    process,
};

use anyhow::{Context, Result, anyhow, ensure};
use serde::Deserialize;

use crate::{apply::render_entry, branches, git, plan::PlannedRelease, repo::Repo};

/// GitHub CLI client.
pub trait Client {
    /// Execute a raw command using the client.
    fn execute(&self, args: &[&str]) -> Result<String>;
}

/// GitHub CLI client using the system's installation.
pub struct SystemClient {
    slug: Option<String>,
    root: PathBuf,
}

impl SystemClient {
    /// A missing remote leaves `GH_REPO` unset, so commands that never
    /// call gh still work in a checkout without it.
    pub fn new(root: &Path, remote: &str) -> Self {
        let slug = crate::git::run(root, &["remote", "get-url", remote])
            .ok()
            .and_then(|u| repo_slug(&u));
        Self {
            slug,
            root: root.to_path_buf(),
        }
    }
}

impl Client for SystemClient {
    fn execute(&self, args: &[&str]) -> Result<String> {
        let mut cmd = process::Command::new("gh");
        cmd.current_dir(&self.root).args(args);
        // Runs gh pinned to the repository behind `remote` (`GH_REPO`), so a
        // clone with an `upstream` remote never resolves PR numbers against the
        // parent repository.
        if let Some(slug) = &self.slug {
            cmd.env("GH_REPO", slug);
        }
        let out = cmd.output().context("cannot run gh")?;
        ensure!(
            out.status.success(),
            "gh {}: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        );
        Ok(String::from_utf8(out.stdout)?.trim().to_string())
    }
}

/// Wrapper around another [`Client`] which filters out write operations.
///
/// If the arguments to execute represent a write operation, the command
/// will be printed to stdout rather than executed.
///
pub struct DryRunClient<C>(C);

impl<C: Client> DryRunClient<C> {
    pub fn new(inner: C) -> Self {
        Self(inner)
    }
}

impl<C: Client> Client for DryRunClient<C> {
    fn execute(&self, args: &[&str]) -> Result<String> {
        let is_write = matches!(
            args,
            ["pr", "create" | "edit" | "close" | "comment", ..]
                | ["release", "create" | "delete" | "edit" | "upload", ..]
                | ["label", "create", ..]
                | ["api", "-X", ..]
        );
        if !is_write {
            return self.0.execute(args);
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

/// `owner/repo` of a GitHub remote URL (ssh or https), if it is one.
pub fn repo_slug(url: &str) -> Option<String> {
    let rest = url.trim().strip_suffix(".git").unwrap_or(url.trim());
    let after = rest
        .split_once("github.com")
        .map(|(_, a)| a.trim_start_matches([':', '/']))?;
    let mut parts = after.split('/');
    Some(format!("{}/{}", parts.next()?, parts.next()?))
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct PullRequest {
    pub number: u64,
    #[serde(rename = "headRefName")]
    pub head: String,
    #[serde(rename = "baseRefName")]
    pub base: String,
}

/// Typed GitHub CLI subcommand.
pub trait Command {
    /// Typed output the command returns when successful.
    type Output;

    /// Convert the typed command into raw arguments for the GitHub CLI.
    fn to_raw_args(&self) -> Result<Vec<String>>;

    /// Parse the raw string output from the GitHub CLI into [`Self::Output`].
    fn parse_raw_output(output: &str) -> Result<Self::Output>;

    /// Execute the command using the given client.
    fn execute(&self, gh: &dyn Client) -> Result<Self::Output> {
        let raw_args = self
            .to_raw_args()
            .context("invalid arguments for GitHub command")?;
        let raw_args: Vec<&str> = raw_args.iter().map(String::as_str).collect();
        let raw_output = gh
            .execute(&raw_args)
            .context("failed to execute GitHub command")?;
        Self::parse_raw_output(&raw_output).context("failed to parse GitHub command output")
    }
}

/// Create a label, or update it when `force` is set and it exists.
pub struct LabelCreate<'a> {
    pub name: &'a str,
    pub color: &'a str,
    pub description: &'a str,
    pub force: bool,
}

impl<'a> Command for LabelCreate<'a> {
    type Output = ();

    fn to_raw_args(&self) -> Result<Vec<String>> {
        let mut args = Vec::from(["label", "create", self.name].map(String::from));
        if self.force {
            args.push("--force".into());
        }
        args.extend(["--color", self.color, "--description", self.description].map(String::from));
        Ok(args)
    }

    fn parse_raw_output(_output: &str) -> Result<Self::Output> {
        Ok(())
    }
}

/// Open a pull request. Outputs its number.
pub struct PrCreate<'a> {
    pub head: &'a str,
    pub base: &'a str,
    pub title: &'a str,
    pub body: &'a str,
    pub label: &'a str,
}

impl<'a> Command for PrCreate<'a> {
    type Output = u64;

    fn to_raw_args(&self) -> Result<Vec<String>> {
        Ok(Vec::from(
            [
                "pr", "create", "--head", self.head, "--base", self.base, "--title", self.title,
                "--body", self.body, "--label", self.label,
            ]
            .map(String::from),
        ))
    }

    /// Extract the PR number from the URL of the newly created PR.
    fn parse_raw_output(output: &str) -> Result<Self::Output> {
        output
            .trim()
            .rsplit('/')
            .next()
            .and_then(|n| n.parse().ok())
            .with_context(|| "PR number expected in output: '{output}'")
    }
}

/// Edit a pull request. Fields left `None` are not changed.
pub struct PrEdit<'a> {
    pub number: u64,
    pub title: Option<&'a str>,
    pub body: Option<&'a str>,
    pub add_label: Option<&'a str>,
    pub base: Option<&'a str>,
}

impl<'a> Command for PrEdit<'a> {
    type Output = ();

    fn to_raw_args(&self) -> Result<Vec<String>> {
        let mut args = vec!["pr".into(), "edit".into(), self.number.to_string()];
        for (flag, value) in [
            ("--title", self.title),
            ("--body", self.body),
            ("--add-label", self.add_label),
            ("--base", self.base),
        ] {
            if let Some(value) = value {
                args.extend([flag.into(), value.into()]);
            }
        }
        Ok(args)
    }

    fn parse_raw_output(_output: &str) -> Result<Self::Output> {
        Ok(())
    }
}

/// List open pull requests.
pub struct PrList<'a> {
    /// Only PRs whose head branch is this.
    pub head: Option<&'a str>,
    /// Only PRs carrying this label.
    pub label: Option<&'a str>,
    /// Required: gh silently stops at 30 when omitted.
    pub limit: u32,
}

impl<'a> Command for PrList<'a> {
    type Output = Vec<PullRequest>;

    fn to_raw_args(&self) -> Result<Vec<String>> {
        let mut args = Vec::from(["pr", "list", "--state", "open"].map(String::from));
        for (flag, value) in [("--head", self.head), ("--label", self.label)] {
            if let Some(value) = value {
                args.extend([flag.into(), value.into()]);
            }
        }
        args.extend([
            "--limit".into(),
            self.limit.to_string(),
            "--json".into(),
            "number,headRefName,baseRefName".into(),
        ]);
        Ok(args)
    }

    fn parse_raw_output(output: &str) -> Result<Self::Output> {
        Ok(serde_json::from_str(output)?)
    }
}

/// Close a pull request with a comment saying why.
pub struct PrClose<'a> {
    pub number: u64,
    pub comment: &'a str,
    pub delete_branch: bool,
}

impl<'a> Command for PrClose<'a> {
    type Output = ();

    fn to_raw_args(&self) -> Result<Vec<String>> {
        let mut args = vec!["pr".into(), "close".into(), self.number.to_string()];
        if self.delete_branch {
            args.push("--delete-branch".into());
        }
        args.extend(["--comment".into(), self.comment.into()]);
        Ok(args)
    }

    fn parse_raw_output(_output: &str) -> Result<Self::Output> {
        Ok(())
    }
}

/// The fields of a pull request [`PrView`] reads, flattened by its `--jq`.
#[derive(Deserialize)]
pub struct PrDetails {
    pub title: String,
    pub head: String,
    pub head_owner: String,
    pub head_repo: String,
    pub labels: Vec<String>,
    pub files: Vec<String>,
}

/// View one pull request.
pub struct PrView {
    pub number: u64,
}

impl Command for PrView {
    type Output = PrDetails;

    fn to_raw_args(&self) -> Result<Vec<String>> {
        Ok(vec![
            "pr".into(),
            "view".into(),
            self.number.to_string(),
            "--json".into(),
            "title,headRefName,headRepositoryOwner,headRepository,labels,files".into(),
            // Flatten API response
            "--jq".into(),
            "{title, head: .headRefName, head_owner: .headRepositoryOwner.login, \
             head_repo: .headRepository.name, labels: [.labels[].name], files: [.files[].path]}"
                .into(),
        ])
    }

    fn parse_raw_output(output: &str) -> Result<Self::Output> {
        Ok(serde_json::from_str(output)?)
    }
}

/// Delete a release.
pub struct ReleaseDelete<'a> {
    pub tag: &'a str,
}

impl<'a> Command for ReleaseDelete<'a> {
    type Output = ();

    fn to_raw_args(&self) -> Result<Vec<String>> {
        Ok(Vec::from(
            ["release", "delete", self.tag, "--yes"].map(String::from),
        ))
    }

    fn parse_raw_output(_output: &str) -> Result<Self::Output> {
        Ok(())
    }
}

/// Edit a release.
pub struct ReleaseEdit<'a> {
    pub tag: &'a str,
    pub draft: bool,
}

impl<'a> Command for ReleaseEdit<'a> {
    type Output = ();

    fn to_raw_args(&self) -> Result<Vec<String>> {
        Ok(vec![
            "release".into(),
            "edit".into(),
            self.tag.into(),
            format!("--draft={}", self.draft),
        ])
    }

    fn parse_raw_output(_output: &str) -> Result<Self::Output> {
        Ok(())
    }
}

/// Create a release for an existing tag.
pub struct ReleaseCreate<'a> {
    pub tag: &'a str,
    pub title: &'a str,
    pub notes: &'a str,
    /// Abort unless the tag is already on the remote; without it gh
    /// creates the tag from the default branch.
    pub verify_tag: bool,
    /// Files to attach.
    pub assets: &'a [String],
    pub draft: bool,
    pub prerelease: bool,
    /// `None` lets GitHub decide from date and version.
    pub latest: Option<bool>,
}

impl<'a> Command for ReleaseCreate<'a> {
    type Output = ();

    fn to_raw_args(&self) -> Result<Vec<String>> {
        let mut args = Vec::from(
            [
                "release", "create", self.tag, "--title", self.title, "--notes", self.notes,
            ]
            .map(String::from),
        );
        if self.verify_tag {
            args.push("--verify-tag".into());
        }
        args.extend(self.assets.iter().cloned());
        if self.draft {
            args.push("--draft".into());
        }
        if self.prerelease {
            args.push("--prerelease".into());
        }
        if let Some(latest) = self.latest {
            args.push(format!("--latest={latest}"));
        }
        Ok(args)
    }

    fn parse_raw_output(_output: &str) -> Result<Self::Output> {
        Ok(())
    }
}

/// The GitHub release behind a tag, as [`ReleaseView`] returns it.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Release {
    #[serde(rename = "isDraft")]
    pub draft: bool,
    pub asset_count: usize,
}

/// Look up the release behind a tag, drafts included, through `gh api
/// graphql`. Outputs `None` when the tag has no release.
pub struct ReleaseView<'a> {
    pub tag: &'a str,
}

impl<'a> Command for ReleaseView<'a> {
    type Output = Option<Release>;

    fn to_raw_args(&self) -> Result<Vec<String>> {
        Ok(vec![
            "api".into(),
            "graphql".into(),
            // -f, not -F: -F reads a value that starts with `@` as a file,
            // and npm tags such as `@scope/pkg@1.0.0` start with `@`.
            "-f".into(),
            format!("tag={}", self.tag),
            "-F".into(),
            "owner={owner}".into(),
            "-F".into(),
            "repo={repo}".into(),
            "-f".into(),
            "query=query($owner:String!,$repo:String!,$tag:String!){repository(owner:$owner,name:$repo){release(tagName:$tag){isDraft releaseAssets(first:100){nodes{name}}}}}".into(),
            "--jq".into(),
            ".data.repository.release | if . then {isDraft, asset_count: (.releaseAssets.nodes | length)} else empty end".into(),
        ])
    }

    fn parse_raw_output(output: &str) -> Result<Self::Output> {
        Ok(if output.is_empty() {
            None
        } else {
            Some(serde_json::from_str(output)?)
        })
    }
}

/// Attach files to a release.
pub struct ReleaseUpload<'a> {
    pub tag: &'a str,
    pub assets: &'a [String],
    /// Replace an existing asset of the same name instead of failing.
    pub clobber: bool,
}

impl<'a> Command for ReleaseUpload<'a> {
    type Output = ();

    fn to_raw_args(&self) -> Result<Vec<String>> {
        let mut args = Vec::from(["release", "upload", self.tag].map(String::from));
        if self.clobber {
            args.push("--clobber".into());
        }
        args.extend(self.assets.iter().cloned());
        Ok(args)
    }

    fn parse_raw_output(_output: &str) -> Result<Self::Output> {
        Ok(())
    }
}

/// Find the first comment on a pull request whose body contains a string.
/// Outputs its id.
pub struct PrCommentFind<'a> {
    pub number: u64,
    pub containing: &'a str,
}

impl<'a> Command for PrCommentFind<'a> {
    type Output = Option<u64>;

    fn to_raw_args(&self) -> Result<Vec<String>> {
        // A JSON string is a valid jq string literal, quotes and all.
        let needle = serde_json::to_string(self.containing)?;
        Ok(vec![
            "api".into(),
            format!("repos/{{owner}}/{{repo}}/issues/{}/comments", self.number),
            // Every page: the comment may be past the first 30.
            "--paginate".into(),
            "--jq".into(),
            format!(".[] | select(.body | contains({needle})) | .id"),
        ])
    }

    fn parse_raw_output(output: &str) -> Result<Self::Output> {
        Ok(output.lines().next().map(str::parse).transpose()?)
    }
}

/// Add a comment to a pull request.
pub struct PrCommentCreate<'a> {
    pub number: u64,
    pub body: &'a str,
}

impl<'a> Command for PrCommentCreate<'a> {
    type Output = ();

    fn to_raw_args(&self) -> Result<Vec<String>> {
        Ok(vec![
            "pr".into(),
            "comment".into(),
            self.number.to_string(),
            "--body".into(),
            self.body.into(),
        ])
    }

    fn parse_raw_output(_output: &str) -> Result<Self::Output> {
        Ok(())
    }
}

/// Replace the body of a pull request comment.
pub struct PrCommentEdit<'a> {
    pub id: u64,
    pub body: &'a str,
}

impl<'a> Command for PrCommentEdit<'a> {
    type Output = ();

    fn to_raw_args(&self) -> Result<Vec<String>> {
        Ok(vec![
            "api".into(),
            "-X".into(),
            "PATCH".into(),
            format!("repos/{{owner}}/{{repo}}/issues/comments/{}", self.id),
            "-f".into(),
            format!("body={}", self.body),
        ])
    }

    fn parse_raw_output(_output: &str) -> Result<Self::Output> {
        Ok(())
    }
}

/// Delete a pull request comment.
pub struct PrCommentDelete {
    pub id: u64,
}

impl Command for PrCommentDelete {
    type Output = ();

    fn to_raw_args(&self) -> Result<Vec<String>> {
        Ok(vec![
            "api".into(),
            "-X".into(),
            "DELETE".into(),
            format!("repos/{{owner}}/{{repo}}/issues/comments/{}", self.id),
        ])
    }

    fn parse_raw_output(_output: &str) -> Result<Self::Output> {
        Ok(())
    }
}

/// A pull request reduced to what links it and its author, as
/// [`PrFindByCommit`] returns it.
#[derive(Debug, Deserialize)]
pub struct PrSummary {
    pub number: u64,
    pub url: String,
    /// `None` when the author has no GitHub account.
    pub author: Option<String>,
}

/// Find the first pull request a commit belongs to.
pub struct PrFindByCommit<'a> {
    pub sha: &'a str,
}

impl<'a> Command for PrFindByCommit<'a> {
    type Output = Option<PrSummary>;

    fn to_raw_args(&self) -> Result<Vec<String>> {
        Ok(vec![
            "api".into(),
            format!("repos/{{owner}}/{{repo}}/commits/{}/pulls", self.sha),
            "--jq".into(),
            ".[0] | select(. != null) | {number, url: .html_url, author: .user.login}".into(),
        ])
    }

    fn parse_raw_output(output: &str) -> Result<Self::Output> {
        Ok(if output.is_empty() {
            None
        } else {
            Some(serde_json::from_str(output)?)
        })
    }
}

/// A commit, as [`CommitView`] returns it.
#[derive(Debug, Deserialize)]
pub struct Commit {
    pub url: String,
    /// `None` when the commit email is not on a GitHub account.
    pub author: Option<String>,
}

/// View one commit.
pub struct CommitView<'a> {
    pub sha: &'a str,
}

impl<'a> Command for CommitView<'a> {
    type Output = Commit;

    fn to_raw_args(&self) -> Result<Vec<String>> {
        Ok(vec![
            "api".into(),
            format!("repos/{{owner}}/{{repo}}/commits/{}", self.sha),
            "--jq".into(),
            "{url: .html_url, author: .author.login}".into(),
        ])
    }

    fn parse_raw_output(output: &str) -> Result<Self::Output> {
        Ok(serde_json::from_str(output)?)
    }
}

pub const MARKER: &str = "<!-- verwalter release -->";

/// Push every branch and make GitHub match the plan.
/// Spec: ChangelogUpdated, "PR link and author". For each changeset, the
/// commit that added it names the PR (via `commits/{sha}/pulls`) and its
/// author; a commit without a PR links the commit itself. Lookups that
/// fail leave the line without a suffix rather than block the release.
pub fn attribute(repo: &mut Repo, gh: &dyn Client) {
    let root = repo.root.clone();
    for c in &mut repo.changesets {
        let Some(sha) = git::added_in(
            &root,
            "HEAD",
            &format!("{}/{}", crate::repo::CHANGESET_DIR, c.file),
        ) else {
            continue;
        };
        let user = |login: Option<String>| {
            login
                .map(|l| format!(" ([@{l}](https://github.com/{l}))"))
                .unwrap_or_default()
        };
        c.attribution = match (PrFindByCommit { sha: &sha }).execute(gh) {
            Ok(Some(pr)) => Some(format!("- [#{}]({}){}", pr.number, pr.url, user(pr.author))),
            _ => CommitView { sha: &sha }.execute(gh).ok().map(|commit| {
                format!(
                    "- [`{}`]({}){}",
                    &sha[..7.min(sha.len())],
                    commit.url,
                    user(commit.author)
                )
            }),
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
    gh: &dyn Client,
) -> Result<Option<PullRequest>> {
    let label = &repo.config.release.label;

    LabelCreate {
        name: label,
        color: "0E8A16",
        description: "Release PR managed by verwalter",
        force: true,
    }
    .execute(gh)?;

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
        PrClose {
            number: stale.number,
            comment: "Nothing left to release.",
            delete_branch: true,
        }
        .execute(gh)?;
    }
    Ok(None)
}

/// Open a PR for `head` against `base`, or bring `existing` (the open PR
/// on `head`) up to date: title, body, label and base.
pub fn upsert_pr(
    gh: &dyn Client,
    existing: Option<&PullRequest>,
    head: &str,
    base: &str,
    title: &str,
    body: &str,
    label: &str,
) -> Result<PullRequest> {
    let number = match existing {
        Some(existing) => {
            PrEdit {
                number: existing.number,
                title: Some(title),
                body: Some(body),
                add_label: Some(label),
                base: None,
            }
            .execute(gh)?;
            if existing.base != base {
                PrEdit {
                    number: existing.number,
                    title: None,
                    body: None,
                    add_label: None,
                    base: Some(base),
                }
                .execute(gh)?;
            }
            existing.number
        }
        None => PrCreate {
            head,
            base,
            title,
            body,
            label,
        }
        .execute(gh)?,
    };
    Ok(PullRequest {
        number,
        head: head.to_string(),
        base: base.to_string(),
    })
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
pub fn open_pr(gh: &dyn Client, head: &str) -> Result<Option<PullRequest>> {
    let open = PrList {
        head: Some(head),
        label: None,
        limit: 1,
    }
    .execute(gh)?;
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
    fn release_view_passes_a_scoped_tag_as_a_raw_string() {
        let args = ReleaseView {
            tag: "@scope/pkg@1.0.0",
        }
        .to_raw_args()
        .unwrap();
        let i = args
            .iter()
            .position(|a| a == "tag=@scope/pkg@1.0.0")
            .unwrap();
        assert_eq!(args[i - 1], "-f");
        assert_eq!(ReleaseView::parse_raw_output("").unwrap(), None);
    }

    #[test]
    fn number_from_create_output() {
        assert_eq!(
            PrCreate::parse_raw_output("https://github.com/livekit/rust-sdks/pull/1312\n").unwrap(),
            1312
        );
        assert!(PrCreate::parse_raw_output("nope").is_err());
    }
}
