//! Spec: rule MaintainerRequestsPreRelease and surface PreReleaseRequest.
//! One rc PR, apart from the release PR, against any branch. It carries
//! every package with a pending changeset and the fixed groups they are
//! in, each group at its own rc version, with the pins among them moved.
//! Nothing is chosen by hand: the changesets say what is in flight and
//! how far it moves. Changesets stay, so the final release still lists
//! them. Dependents without a changeset are not touched. After the
//! merge, `verwalter release` tags each as a pre-release.

use anyhow::{Result, bail, ensure};

use crate::{
    Version, apply, branches, git,
    github::{self, MARKER, PullRequest, open_pr, section, upsert_pr},
    plan::PlannedRelease,
    repo::Repo,
    version::{BumpLevel, next_prerelease_version},
};

/// The rc branch, named after the target like the release PR: `release/main-rc`.
pub fn branch_name(repo: &Repo, target: &str) -> String {
    format!("{}{target}-rc", repo.config.release.branch_prefix)
}

/// Versions of `package` that have a release tag.
pub fn released_versions(repo: &Repo, package: &str) -> Result<Vec<Version>> {
    let tags = git::run(&repo.root, &["tag", "--list"])?;
    Ok(tags
        .lines()
        .filter_map(|t| repo.config.version_from_tag(package, t))
        .collect())
}

/// The rc to open: every package with a pending changeset, with its fixed
/// group. Each group moves from its highest version by the highest level
/// its changesets name, past every rc a member has tagged. Dependencies
/// come before dependents, like a release plan.
pub fn plan(repo: &Repo) -> Result<Vec<PlannedRelease>> {
    let find = |name: &str| repo.packages.iter().find(|p| p.name == name);
    let pending = |name: &str| {
        repo.changesets
            .iter()
            .any(|c| c.unreleased_for(name).is_some())
    };
    let mut groups: Vec<Vec<String>> = Vec::new();
    for p in repo.packages.iter().filter(|p| pending(&p.name)) {
        if !groups.iter().any(|g| g.contains(&p.name)) {
            groups.push(
                std::iter::once(p.name.clone())
                    .chain(repo.config.fixed_with(&p.name))
                    .collect(),
            );
        }
    }
    if groups.is_empty() {
        bail!("no pending changesets: nothing to pre-release");
    }
    let in_rc = |name: &str| groups.iter().any(|g| g.iter().any(|m| m == name));

    let mut out = Vec::new();
    for group in &groups {
        let members: Vec<&crate::plan::PackageState> =
            group.iter().filter_map(|m| find(m)).collect();
        let level = repo
            .changesets
            .iter()
            .flat_map(|c| members.iter().filter_map(|m| c.unreleased_for(&m.name)))
            .max()
            .unwrap_or(BumpLevel::Patch);
        let base = members.iter().map(|m| &m.version).max().unwrap();
        let mut released = Vec::new();
        for m in &members {
            released.extend(released_versions(repo, &m.name)?);
        }
        let to = next_prerelease_version(
            base,
            &released,
            level,
            &repo.config.versioning.prerelease_label,
        );
        for m in &members {
            out.push(PlannedRelease {
                package: m.name.clone(),
                from: m.version.clone(),
                to: to.clone(),
                level,
                changesets: repo
                    .changesets
                    .iter()
                    .filter(|c| c.unreleased_for(&m.name).is_some())
                    .map(|c| c.file.clone())
                    .collect(),
                bumped_dependencies: m
                    .dependencies
                    .iter()
                    .filter(|d| in_rc(d))
                    .cloned()
                    .collect(),
                fixed_with: m.fixed_with.clone(),
            });
        }
    }
    let names: Vec<String> = out.iter().map(|r| r.package.clone()).collect();
    let deps = |n: &str| -> Vec<&str> {
        out.iter()
            .find(|r| r.package == n)
            .map(|r| r.bumped_dependencies.iter().map(String::as_str).collect())
            .unwrap_or_default()
    };
    let order: Vec<String> = crate::plan::dependency_order(names.iter().map(String::as_str), &deps)
        .into_iter()
        .map(String::from)
        .collect();
    out.sort_by_key(|r| order.iter().position(|n| *n == r.package));
    Ok(out)
}

/// Create the rc branch from `<remote>/<target>`, push it, open or update
/// the PR against `target`.
pub fn open(
    repo: &Repo,
    target: &str,
    remote: &str,
    dry_run: bool,
    gh: &dyn github::Client,
) -> Result<(Vec<PlannedRelease>, PullRequest)> {
    let releases = plan(repo)?;
    let branch = branch_name(repo, target);
    let start = format!("{remote}/{target}");
    ensure!(
        git::ref_exists(&repo.root, &start),
        "{start} does not exist; fetch it first"
    );
    // One version when the rc is one group, else "name version" each.
    let prefix = "pre-release: ";
    let title = if releases.iter().all(|r| r.to == releases[0].to) {
        let version = releases[0].to.to_string();
        let names: Vec<String> = releases.iter().map(|r| r.package.clone()).collect();
        let max = branches::TITLE_MAX.saturating_sub(prefix.len() + 1 + version.len());
        format!("{prefix}{} {version}", branches::join_capped(&names, max))
    } else {
        let parts: Vec<String> = releases
            .iter()
            .map(|r| format!("{} {}", r.package, r.to))
            .collect();
        format!(
            "{prefix}{}",
            branches::join_capped(&parts, branches::TITLE_MAX - prefix.len())
        )
    };
    branches::commit_branch(&repo.root, &start, &branch, &title, |worktree| {
        for release in &releases {
            apply::apply(
                worktree,
                &repo.config,
                &repo.packages,
                &repo.changesets,
                &releases,
                release,
            )?;
        }
        Ok(())
    })?;
    git::write(
        &repo.root,
        dry_run,
        &[
            "push",
            "--force",
            "--quiet",
            remote,
            &format!("{branch}:{branch}"),
        ],
    )?;

    let mut body = format!(
        "{MARKER}\nMerging this PR releases a pre-release of every package with a pending changeset from `{target}`. \
         Changesets stay pending; the next final release includes them.\n"
    );
    for release in &releases {
        body.push_str(&section(release, repo, &releases, ""));
    }
    let existing = open_pr(gh, &branch)?;
    let pr = upsert_pr(
        gh,
        existing.as_ref(),
        &branch,
        target,
        &title,
        &body,
        &repo.config.release.label,
    )?;
    Ok((releases, pr))
}
