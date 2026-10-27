//! Spec: the branch side of OpenOrUpdateReleasePullRequests. Every run
//! regenerates every release branch from the current base branch, bottom
//! of the stack first, so a stacked branch always sits on a fresh parent.
//! Pushing and the pull requests themselves are the GitHub step.

use std::{fs, path::Path};

use crate::{apply, git, plan::PlannedRelease, repo::Repo};

pub type Error = Box<dyn std::error::Error + Send + Sync>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Branch {
    pub package: String,
    pub name: String,
    /// The branch this one is stacked on, or the base branch.
    pub target: String,
}

pub fn branch_name(repo: &Repo, package: &str) -> String {
    format!("{}{package}", repo.config.release_branch_prefix)
}

/// Create or reset one local branch per planned release. `base` is the ref
/// the bottom of the stack starts from, usually `origin/<base branch>`;
/// the PR of the bottom branch targets the configured base branch name.
pub fn create(repo: &Repo, plan: &[PlannedRelease], base: &str) -> Result<Vec<Branch>, Error> {
    let root = repo.root.as_path();
    if !git::ref_exists(root, base) {
        return Err(format!("base ref {base} does not exist").into());
    }
    let dependencies = repo.dependencies();
    let mut branches = Vec::new();

    for release in plan {
        let name = branch_name(repo, &release.package);
        let (start, target) = match release.stacked_on.as_deref() {
            Some(p) => (branch_name(repo, p), branch_name(repo, p)),
            None => (base.to_string(), repo.config.base_branch.clone()),
        };

        commit_branch(root, &start, &name, &format!("release: {} {}", release.package, release.to), |worktree| {
            apply::apply(worktree, &repo.config, &dependencies, &repo.changesets, plan, release)?;
            // Packages released by the branches under this one count as
            // released: their PRs merge before this one can.
            let mut released: Vec<&str> = vec![&release.package];
            let mut cur = release.stacked_on.as_deref();
            while let Some(p) = cur {
                released.push(p);
                cur = plan.iter().find(|r| r.package == p).and_then(|r| r.stacked_on.as_deref());
            }
            for c in &repo.changesets {
                if consumed_by(c, release, &released) {
                    let _ = fs::remove_file(worktree.join(crate::repo::CHANGESET_DIR).join(&c.file));
                }
            }
            Ok(())
        })?;

        branches.push(Branch { package: release.package.clone(), name, target });
    }
    Ok(branches)
}

/// Run `edit` in a throwaway worktree at `start`, commit everything, and
/// point `branch` at the result. The user's checkout is never touched.
pub fn commit_branch(root: &Path, start: &str, branch: &str, message: &str, edit: impl FnOnce(&Path) -> Result<(), Error>) -> Result<(), Error> {
    // The counter keeps parallel runs in one process apart (tests).
    static COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let worktree = std::env::temp_dir().join(format!("verwalter-{}-{n}-{}", std::process::id(), branch.replace('/', "_")));
    let _ = fs::remove_dir_all(&worktree);
    let wt = worktree.to_str().ok_or("temp dir is not valid UTF-8")?;
    git::run(root, &["worktree", "add", "--detach", "--force", wt, start])?;
    let result = (|| -> Result<(), Error> {
        edit(&worktree)?;
        git::run(&worktree, &["add", "-A"])?;
        let mut args = git::identity_args(&worktree);
        args.extend(["commit", "--quiet", "--allow-empty", "-m", message]);
        git::run(&worktree, &args)?;
        git::run(&worktree, &["branch", "--force", branch, "HEAD"])?;
        Ok(())
    })();
    let _ = git::run(root, &["worktree", "remove", "--force", wt]);
    result
}

/// Spec: ConsumeChangeset. The file goes with the release of the last
/// package it names, counting releases already tagged and releases lower
/// in the same stack.
fn consumed_by(changeset: &crate::changeset::Changeset, release: &PlannedRelease, released_in_stack: &[&str]) -> bool {
    changeset.unreleased_for(&release.package).is_some()
        && changeset
            .bumps
            .iter()
            .all(|b| released_in_stack.contains(&b.package_name.as_str()) || changeset.released_packages.contains(&b.package_name))
}
