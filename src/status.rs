//! Spec: surface ReleaseStatus. Where every package stands and what
//! finishes its release, so a maintainer can take over from a failed
//! workflow run. Reads only.

use std::collections::BTreeSet;

use serde::Serialize;

use crate::{
    config::ReleaseMode,
    git,
    github::{Gh, PullRequest},
    plan::plan,
    prerelease, release,
    repo::Repo,
    Error,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PackageStatus {
    pub package: String,
    pub version: String,
    pub tag: String,
    /// missing | local | remote | both
    pub tag_state: &'static str,
    /// none | draft | published | not wanted
    pub release: String,
    pub assets: usize,
    /// Some(true|false) when the package has a registry configured.
    pub on_registry: Option<bool>,
    pub pending_changesets: Vec<String>,
    pub next_version: Option<String>,
    pub release_pr: Option<u64>,
    pub rc_pr: Option<u64>,
    /// What finishes the release, in words, with the command.
    pub next_step: String,
}

pub fn status(repo: &Repo, remote: &str, gh: Gh) -> Result<Vec<PackageStatus>, Error> {
    status_with(repo, remote, gh, &crate::registry::curl)
}

pub fn status_with(
    repo: &Repo,
    remote: &str,
    gh: Gh,
    http: crate::registry::Http,
) -> Result<Vec<PackageStatus>, Error> {
    let root = repo.root.as_path();
    let remote_tags: BTreeSet<String> = git::run(root, &["ls-remote", "--tags", remote])
        .map(|out| {
            out.lines()
                .filter_map(|l| l.split("refs/tags/").nth(1))
                .map(|t| t.trim_end_matches("^{}").to_string())
                .collect()
        })
        .unwrap_or_default();
    let open: Vec<PullRequest> = gh(&[
        "pr",
        "list",
        "--state",
        "open",
        "--label",
        &repo.config.release_label,
        "--limit",
        "200",
        "--json",
        "number,headRefName,baseRefName",
    ])
    .ok()
    .and_then(|j| serde_json::from_str(&j).ok())
    .unwrap_or_default();
    let planned = plan(
        &repo.packages,
        &repo.changesets,
        repo.config.dependent_bump_level,
    );

    let mut out = Vec::new();
    for p in &repo.packages {
        let package = &repo.config.packages[&p.name];
        let tag = repo.config.tag_for(&p.name, &p.version);
        let local = git::ref_exists(root, &tag);
        let on_remote = remote_tags.contains(&tag);
        let tag_state = match (local, on_remote) {
            (true, true) => "both",
            (true, false) => "local",
            (false, true) => "remote",
            (false, false) => "missing",
        };
        let (release_state, assets) = if package.release != ReleaseMode::TagAndRelease {
            ("not wanted".to_string(), 0)
        } else if !local && !on_remote {
            ("none".to_string(), 0)
        } else {
            match release::view(gh, &tag) {
                Some(r) => (
                    if r.draft { "draft" } else { "published" }.to_string(),
                    r.assets.len(),
                ),
                None => ("none".to_string(), 0),
            }
        };
        let pending: Vec<String> = repo
            .changesets
            .iter()
            .filter(|c| c.unreleased_for(&p.name).is_some())
            .map(|c| c.file.clone())
            .collect();
        let next_version = planned
            .iter()
            .find(|r| r.package == p.name)
            .map(|r| r.to.to_string());
        let release_pr = open
            .iter()
            .find(|pr| pr.head == crate::branches::branch_name(repo))
            .filter(|_| next_version.is_some())
            .map(|pr| pr.number);
        let rc_pr = open
            .iter()
            .find(|pr| pr.head == prerelease::branch_name(repo, &p.name))
            .map(|pr| pr.number);

        let next_step = if package.release == ReleaseMode::None {
            match next_version.as_deref() {
                Some(v) => format!("release PR for {v}: verwalter pr"),
                None => "up to date".to_string(),
            }
        } else if tag_state == "missing" {
            format!(
                "{} is committed but not tagged: verwalter release --package {}",
                p.version, p.name
            )
        } else if tag_state == "local" {
            format!("tag {tag} is not on {remote}: git push {remote} {tag}, then verwalter release --package {}", p.name)
        } else if release_state == "none" {
            format!(
                "tagged, no GitHub release: verwalter release --package {} [--asset <file>...]",
                p.name
            )
        } else if release_state == "draft" {
            format!("draft release waits for assets: verwalter release --package {} --asset <file>... --publish", p.name)
        } else if let Some(n) = release_pr {
            format!(
                "merge release PR #{n} for {}",
                next_version.as_deref().unwrap_or("?")
            )
        } else if let Some(v) = &next_version {
            format!("release PR for {v} opens on the next push: verwalter pr")
        } else {
            "up to date".to_string()
        };

        let on_registry = if tag_state == "missing" {
            None
        } else {
            crate::registry::exists(
                crate::registry::version_url(&repo.config, &p.name, &p.version),
                http,
            )
            .unwrap_or(None)
        };
        out.push(PackageStatus {
            package: p.name.clone(),
            version: p.version.to_string(),
            tag,
            tag_state,
            release: release_state,
            assets,
            on_registry,
            pending_changesets: pending,
            next_version,
            release_pr,
            rc_pr,
            next_step,
        });
    }
    Ok(out)
}
