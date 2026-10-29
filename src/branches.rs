//! Spec: the branch side of OpenOrUpdateReleasePullRequest. Every run
//! regenerates the one release branch from the current base branch.
//! Pushing and the pull request itself are the GitHub step.

use std::{fs, path::Path};

use crate::{apply, git, plan::PlannedRelease, repo::Repo, Error};

/// `release/main`: the prefix plus the base branch, so it never collides
/// with the rc branches (`release/<package>-rc`) under the same prefix.
pub fn branch_name(repo: &Repo) -> String {
    format!(
        "{}{}",
        repo.config.release_branch_prefix, repo.config.base_branch
    )
}

/// `release: a 1.2.0, b 0.3.1`: the commit message and PR title.
pub fn title(plan: &[PlannedRelease]) -> String {
    let parts: Vec<String> = plan
        .iter()
        .map(|r| format!("{} {}", r.package, r.to))
        .collect();
    format!("release: {}", parts.join(", "))
}

/// Create or reset the release branch from `base`, usually
/// `origin/<base branch>`, with every planned release applied and the
/// changesets consumed. None when nothing is planned.
pub fn create(repo: &Repo, plan: &[PlannedRelease], base: &str) -> Result<Option<String>, Error> {
    let root = repo.root.as_path();
    if !git::ref_exists(root, base) {
        return Err(format!("base ref {base} does not exist").into());
    }
    if plan.is_empty() {
        return Ok(None);
    }
    let name = branch_name(repo);

    commit_branch(root, base, &name, &title(plan), |worktree| {
        for release in plan {
            apply::apply(
                worktree,
                &repo.config,
                &repo.packages,
                &repo.changesets,
                plan,
                release,
                true,
            )?;
        }
        // Spec: ConsumeChangesets. Every pending changeset names a package
        // in this PR, so every file goes.
        let dir = worktree.join(crate::repo::CHANGESET_DIR);
        for c in &repo.changesets {
            fs::remove_file(dir.join(&c.file))?;
        }
        Ok(())
    })?;
    Ok(Some(name))
}

/// Run `edit` in a throwaway worktree at `start`, commit everything, and
/// point `branch` at the result. The user's checkout is never touched.
pub fn commit_branch(
    root: &Path,
    start: &str,
    branch: &str,
    message: &str,
    edit: impl FnOnce(&Path) -> Result<(), Error>,
) -> Result<(), Error> {
    // The counter keeps parallel runs in one process apart (tests).
    static COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let worktree = std::env::temp_dir().join(format!(
        "verwalter-{}-{n}-{}",
        std::process::id(),
        branch.replace('/', "_")
    ));
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
