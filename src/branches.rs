//! Spec: the branch side of OpenOrUpdateReleasePullRequest. Every run
//! regenerates the one release branch from the current base branch.
//! Pushing and the pull request itself are the GitHub step.

use std::{fs, path::Path};

use anyhow::{Context, Result, bail};

use crate::{apply, git, plan::PlannedRelease, repo::Repo};

/// `release/main`: the prefix plus the base branch, so it never collides
/// with the rc branches (`release/<package>-rc`) under the same prefix.
pub fn branch_name(repo: &Repo) -> String {
    format!(
        "{}{}",
        repo.config.release.branch_prefix, repo.config.git.base_branch
    )
}

/// Longest title, prefix included, before the package list is cut short.
pub const TITLE_MAX: usize = 72;

/// `release: a 1.2.0, b 0.3.1`: the commit message and PR title.
pub fn title(plan: &[PlannedRelease]) -> String {
    let parts: Vec<String> = plan
        .iter()
        .map(|r| format!("{} {}", r.package, r.to))
        .collect();
    let prefix = "release: ";
    format!("{prefix}{}", join_capped(&parts, TITLE_MAX - prefix.len()))
}

/// `a, b and 3 more`: joins `parts` while the result fits in `max` chars,
/// so a release of many dependent packages keeps a readable title. The
/// first part always shows.
pub fn join_capped(parts: &[String], max: usize) -> String {
    let all = parts.join(", ");
    if all.len() <= max {
        return all;
    }
    let mut out = String::new();
    for (i, part) in parts.iter().enumerate() {
        let after = parts.len() - i - 1;
        let suffix = if after > 0 {
            format!(" and {after} more").len()
        } else {
            0
        };
        if i > 0 && out.len() + 2 + part.len() + suffix > max {
            return format!("{out} and {} more", parts.len() - i);
        }
        if i > 0 {
            out.push_str(", ");
        }
        out.push_str(part);
    }
    out
}

/// Create or reset the release branch from `base`, usually
/// `origin/<base branch>`, with every planned release applied and the
/// changesets consumed. None when nothing is planned.
pub fn create(repo: &Repo, plan: &[PlannedRelease], base: &str) -> Result<Option<String>> {
    let root = repo.root.as_path();
    if !git::ref_exists(root, base) {
        bail!("base ref {base} does not exist");
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
    edit: impl FnOnce(&Path) -> Result<()>,
) -> Result<()> {
    // The counter keeps parallel runs in one process apart (tests).
    static COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let worktree = std::env::temp_dir().join(format!(
        "verwalter-{}-{n}-{}",
        std::process::id(),
        branch.replace('/', "_")
    ));
    let _ = fs::remove_dir_all(&worktree);
    let wt = worktree.to_str().context("temp dir is not valid UTF-8")?;
    git::run(root, &["worktree", "add", "--detach", "--force", wt, start])?;
    let result = (|| -> Result<()> {
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

#[cfg(test)]
mod tests {
    use super::join_capped;

    #[test]
    fn join_capped_cuts_the_list_within_max() {
        let parts: Vec<String> = ["aaaa", "bbbb", "cccc", "dddd"].map(String::from).into();
        assert_eq!(join_capped(&parts, 100), "aaaa, bbbb, cccc, dddd");
        assert_eq!(join_capped(&parts, 22), "aaaa, bbbb, cccc, dddd");
        assert_eq!(join_capped(&parts, 21), "aaaa, bbbb and 2 more");
        assert_eq!(join_capped(&parts, 1), "aaaa and 3 more");
    }
}
