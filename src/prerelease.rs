//! Spec: rule MaintainerRequestsPreRelease and surface PreReleaseRequest.
//! One package, one rc PR, never stacked, against any branch. The branch
//! bumps the package's own files and changelog only: changesets stay, so
//! the final release still lists them, and dependents are not pinned to
//! an rc. After the merge, `verwalter release` tags it as a pre-release.

use crate::{
    apply, branches, git,
    github::{Gh, PullRequest, MARKER},
    plan::PlannedRelease,
    repo::Repo,
    version::{next_prerelease_version, BumpLevel},
    Version,
};

pub type Error = Box<dyn std::error::Error + Send + Sync>;

pub fn branch_name(repo: &Repo, package: &str) -> String {
    format!("{}{package}-rc", repo.config.release_branch_prefix)
}

/// Versions of `package` that have a release tag.
pub fn released_versions(repo: &Repo, package: &str) -> Result<Vec<Version>, Error> {
    let tags = git::run(&repo.root, &["tag", "--list"])?;
    Ok(tags.lines().filter_map(|t| repo.config.version_from_tag(package, t)).collect())
}

/// The rc to open. `level` overrides the level the pending changesets
/// imply; with neither, patch.
pub fn plan(repo: &Repo, package: &str, level: Option<BumpLevel>) -> Result<PlannedRelease, Error> {
    let p = repo.packages.iter().find(|p| p.name == package).ok_or_else(|| format!("unknown package {package}"))?;
    let changesets: Vec<String> = repo.changesets.iter().filter(|c| c.unreleased_for(package).is_some()).map(|c| c.file.clone()).collect();
    let level = level
        .or_else(|| repo.changesets.iter().filter_map(|c| c.unreleased_for(package)).max())
        .unwrap_or(BumpLevel::Patch);
    let to = next_prerelease_version(&p.version, &released_versions(repo, package)?, level, &repo.config.prerelease_label);
    Ok(PlannedRelease { package: package.to_string(), from: p.version.clone(), to, level, changesets, bumped_dependencies: Vec::new(), stacked_on: None })
}

/// Create the rc branch from `<remote>/<target>`, push it, open or update
/// the PR against `target`.
pub fn open(repo: &Repo, package: &str, target: &str, level: Option<BumpLevel>, remote: &str, dry_run: bool, gh: Gh) -> Result<(PlannedRelease, PullRequest), Error> {
    let release = plan(repo, package, level)?;
    let branch = branch_name(repo, package);
    let start = format!("{remote}/{target}");
    if !git::ref_exists(&repo.root, &start) {
        return Err(format!("{start} does not exist; fetch it first").into());
    }
    let dependencies = repo.dependencies();
    branches::commit_branch(&repo.root, &start, &branch, &format!("pre-release: {} {}", package, release.to), |worktree| {
        apply::apply_with(worktree, &repo.config, &dependencies, &repo.changesets, std::slice::from_ref(&release), &release, false)?;
        Ok(())
    })?;
    git::write(&repo.root, dry_run, &["push", "--force", "--quiet", remote, &format!("{branch}:{branch}")])?;

    let label = repo.config.release_label.as_str();
    let title = format!("pre-release: {package} {}", release.to);
    let body = format!(
        "{MARKER}\nMerging this PR releases **{package}** `{}` as pre-release `{}` from `{target}`. \
         Changesets stay pending; the next final release includes them.\n\n{}",
        release.from,
        release.to,
        apply::render_entry(&release, &repo.changesets, std::slice::from_ref(&release)).trim_start_matches(|c| c != '\n').trim_start()
    );
    let open: Vec<PullRequest> = serde_json::from_str(&gh(&["pr", "list", "--state", "open", "--head", &branch, "--json", "number,headRefName,baseRefName"])?)?;
    let pr = match open.first() {
        Some(existing) => {
            gh(&["pr", "edit", &existing.number.to_string(), "--base", target, "--title", &title, "--body", &body, "--add-label", label])?;
            PullRequest { number: existing.number, head: branch, base: target.to_string() }
        }
        None => {
            let out = gh(&["pr", "create", "--head", &branch, "--base", target, "--title", &title, "--body", &body, "--label", label])?;
            let number = out.trim().rsplit('/').next().and_then(|n| n.parse().ok()).ok_or("unexpected `gh pr create` output")?;
            PullRequest { number, head: branch, base: target.to_string() }
        }
    };
    Ok((release, pr))
}
