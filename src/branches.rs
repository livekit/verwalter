//! Spec: the branch side of OpenOrUpdateReleasePullRequests. Every run
//! regenerates every release branch from the current base branch, bottom
//! of the stack first, so a stacked branch always sits on a fresh parent.
//! Pushing and the pull requests themselves are the GitHub step.

use std::{fs, path::Path};

use crate::{apply, changeset::Changeset, git, plan::PlannedRelease, repo::Repo, Error};

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
    let mut branches = Vec::new();
    // Changesets the branches below wrote for their dependents, by the
    // package that wrote them (spec: ReleaseLeavesChangesetForDependents).
    let mut left_behind: Vec<(String, Changeset)> = Vec::new();

    for release in plan {
        let name = branch_name(repo, &release.package);
        let (start, target) = match release.stacked_on.as_deref() {
            Some(p) => (branch_name(repo, p), branch_name(repo, p)),
            None => (base.to_string(), repo.config.base_branch.clone()),
        };

        // This package and the ones under it in the stack: their PRs merge
        // before this one can.
        let mut released: Vec<&str> = vec![&release.package];
        let mut cur = release.stacked_on.as_deref();
        while let Some(p) = cur {
            released.push(p);
            cur = plan
                .iter()
                .find(|r| r.package == p)
                .and_then(|r| r.stacked_on.as_deref());
        }
        let inherited: Vec<&Changeset> = left_behind
            .iter()
            .filter(|(by, _)| released.contains(&by.as_str()))
            .map(|(_, c)| c)
            .collect();
        let mut left = None;

        commit_branch(
            root,
            &start,
            &name,
            &format!("release: {} {}", release.package, release.to),
            |worktree| {
                apply::apply(
                    worktree,
                    &repo.config,
                    &repo.packages,
                    &repo.changesets,
                    plan,
                    release,
                    true,
                )?;
                let dir = worktree.join(crate::repo::CHANGESET_DIR);
                // Spec: ReleaseRemovesPackageFromChangeset / ConsumeChangeset.
                // This package's line leaves every changeset that names it; the
                // file goes with its last line. The branches under this one
                // merge first, so their packages' lines go too (the changesets
                // were read from the base, not from the parent branch).
                for c in repo.changesets.iter().chain(inherited) {
                    let names: Vec<&str> =
                        c.bumps.iter().map(|b| b.package_name.as_str()).collect();
                    if c.bumps.is_empty() || names.iter().any(|n| released.contains(n)) {
                        let path = dir.join(&c.file);
                        match c.without(&released) {
                            Some(content) => fs::write(&path, content)?,
                            None => {
                                let _ = fs::remove_file(&path);
                            }
                        }
                    }
                }
                // Spec: ReleaseLeavesChangesetForDependents. The pins in every
                // dependent moved, so a dependent this PR does not release
                // needs a release of its own. Merging only the bottom of a
                // stack must not lose that bump with the consumed changeset.
                let waiting: Vec<&str> = apply::dependents(&repo.packages, &release.package)
                    .into_iter()
                    .filter(|d| !released.contains(d))
                    .collect();
                if !waiting.is_empty() {
                    let lines: Vec<String> = waiting
                        .iter()
                        .map(|d| format!("\"{d}\": {}", repo.config.dependent_bump_level))
                        .collect();
                    let content = format!(
                        "---\n{}\n---\nUpdated dependencies: {}@{}\n",
                        lines.join("\n"),
                        release.package,
                        release.to
                    );
                    let file = format!("{}-{}-dependents.md", slug(&release.package), release.to);
                    fs::create_dir_all(&dir)?;
                    fs::write(dir.join(&file), &content)?;
                    left = Some(Changeset::parse(&file, &content)?);
                }
                Ok(())
            },
        )?;

        if let Some(c) = left {
            left_behind.push((release.package.clone(), c));
        }
        branches.push(Branch {
            package: release.package.clone(),
            name,
            target,
        });
    }
    Ok(branches)
}

/// `@play/core` -> `play-core`: a package name as a file name.
fn slug(name: &str) -> String {
    name.trim_start_matches('@')
        .replace(|c: char| !c.is_ascii_alphanumeric() && c != '.', "-")
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
