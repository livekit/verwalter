//! Spec: surface ReleaseStatus. Where every package stands and what
//! finishes its release, so a maintainer can take over from a failed
//! workflow run, and whether the release of each current version is
//! complete, so the last job of a run can assert it. Reads only.

use std::collections::BTreeSet;

use anyhow::Result;

use crate::{
    config::ReleaseMode,
    git,
    github::{self, Command as _},
    plan::plan,
    prerelease,
    repo::Repo,
};

/// The field names are the keys of `status --json`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct PackageStatus {
    pub package: String,
    pub version: String,
    pub tag: String,
    /// missing | local | remote | both
    pub tag_state: &'static str,
    /// none | draft | published | not wanted
    pub release: String,
    pub assets: usize,
    /// The `release.required_assets` that are not on the release.
    pub missing_assets: Vec<String>,
    /// Some(true|false) when the package has a registry configured.
    pub on_registry: Option<bool>,
    pub pending_changesets: Vec<String>,
    pub next_version: Option<String>,
    pub release_pr: Option<u64>,
    pub rc_pr: Option<u64>,
    /// What finishes the release, in words, with the command.
    pub next_step: String,
    /// Spec: Release.is_complete. None when the current version is
    /// completely released or is no release. Else what is missing and
    /// what finishes it, for `status --check`.
    pub incomplete: Option<String>,
}

pub fn status(repo: &Repo, remote: &str, gh: &dyn github::Client) -> Result<Vec<PackageStatus>> {
    let root = repo.root.as_path();
    let remote_tags: BTreeSet<String> = git::run(root, &["ls-remote", "--tags", remote])
        .map(|out| {
            out.lines()
                .filter_map(|l| l.split("refs/tags/").nth(1))
                .map(|t| t.trim_end_matches("^{}").to_string())
                .collect()
        })
        .unwrap_or_default();
    let open = github::PrList {
        head: None,
        label: Some(&repo.config.release.label),
        limit: 200,
    }
    .execute(gh)
    .unwrap_or_default();
    let planned = plan(
        &repo.packages,
        &repo.changesets,
        repo.config.versioning.dependent_bump_level,
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
        let mut missing_assets = Vec::new();
        let (release_state, assets) = if package.release.mode != ReleaseMode::TagAndRelease {
            ("not wanted".to_string(), 0)
        } else if !local && !on_remote {
            ("none".to_string(), 0)
        } else {
            match (github::ReleaseView { tag: &tag }).execute(gh) {
                Ok(Some(r)) => {
                    missing_assets = repo.config.missing_assets(
                        &p.name,
                        &p.version.to_string(),
                        &tag,
                        &r.assets,
                    );
                    (
                        if r.draft { "draft" } else { "published" }.to_string(),
                        r.asset_count,
                    )
                }
                Ok(None) => ("none".to_string(), 0),
                Err(_) => ("unknown".to_string(), 0),
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
            .find(|pr| pr.head == crate::branches::branch_name(&repo.config))
            .filter(|_| next_version.is_some())
            .map(|pr| pr.number);
        let rc_pr = open
            .iter()
            .find(|pr| {
                pr.head == prerelease::branch_name(&repo.config, &repo.config.git.base_branch)
            })
            .map(|pr| pr.number);

        let next_step = if package.release.mode == ReleaseMode::None {
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
            format!(
                "tag {tag} is not on {remote}: git push {remote} {tag}, then verwalter release --package {}",
                p.name
            )
        } else if release_state == "none" {
            format!(
                "tagged, no GitHub release: verwalter release --package {} [--asset <file>...]",
                p.name
            )
        } else if release_state == "draft" {
            format!(
                "draft release waits for the publish: verwalter publish, or with assets built later: verwalter release --package {} --asset <file>... --publish",
                p.name
            )
        } else if !missing_assets.is_empty() {
            format!(
                "the release lacks required assets ({}): verwalter release --package {} --asset <file>...",
                missing_assets.join(", "),
                p.name
            )
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

        let registry = if tag_state == "missing" {
            Ok(None)
        } else {
            crate::registry::exists(
                crate::registry::version_url(&repo.config, &p.name, &p.version),
                &crate::registry::curl,
            )
        };

        // Spec: Release.is_complete. A version without a tag and without a
        // changelog entry is no release (ReleaseNeedsChangelogEntry).
        // Pending changesets and open PRs are later releases.
        let mut lacks: Vec<String> = Vec::new();
        if package.release.mode != ReleaseMode::None
            && !(tag_state == "missing"
                && crate::release::lacks_changelog_entry(root, package, &p.version))
        {
            if !on_remote {
                lacks.push(format!("tag {tag} is not on {remote}"));
            }
            match release_state.as_str() {
                "none" => lacks.push("no GitHub release".into()),
                "draft" => lacks.push("the GitHub release is a draft".into()),
                "unknown" => lacks.push("the state of the GitHub release is unknown".into()),
                _ => {}
            }
            if !missing_assets.is_empty() {
                lacks.push(format!(
                    "required assets are missing: {}",
                    missing_assets.join(", ")
                ));
            }
        }
        // `next_step` does not look at the registry.
        let step = if lacks.is_empty() {
            "verwalter publish"
        } else {
            next_step.as_str()
        };
        if package.release.mode != ReleaseMode::None {
            match &registry {
                Ok(Some(false)) => lacks.push("the registry does not have the version".into()),
                Err(e) => lacks.push(format!("the registry did not answer ({e:#})")),
                _ => {}
            }
        }
        let incomplete = (!lacks.is_empty()).then(|| format!("{} -> {step}", lacks.join(", ")));
        out.push(PackageStatus {
            package: p.name.clone(),
            version: p.version.to_string(),
            tag,
            tag_state,
            release: release_state,
            assets,
            missing_assets,
            on_registry: registry.unwrap_or(None),
            pending_changesets: pending,
            next_version,
            release_pr,
            rc_pr,
            next_step,
            incomplete,
        });
    }
    Ok(out)
}
