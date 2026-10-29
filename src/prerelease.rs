//! Spec: rule MaintainerRequestsPreRelease and surface PreReleaseRequest.
//! One package, one rc PR, apart from the release PR, against any branch.
//! The rc carries the package, its fixed group, and every in-repo
//! dependency with a pending changeset, transitively, each group at its
//! own rc version, with the pins between them moved: an rc of a dependent
//! must not ship against a dependency whose change is only on main.
//! Changesets stay, so the final release still lists them. Dependents
//! outside the rc are not touched. After the merge, `verwalter release`
//! tags each as a pre-release.

use crate::{
    apply, branches, git,
    github::{open_pr, section, upsert_pr, Gh, PullRequest, MARKER},
    plan::PlannedRelease,
    repo::Repo,
    version::{next_prerelease_version, BumpLevel},
    Error, Version,
};

/// One rc branch per fixed group, named after its first member by name,
/// whichever member was requested: two members never open two rc PRs.
pub fn branch_name(repo: &Repo, package: &str) -> String {
    let name = repo
        .config
        .fixed_with(package)
        .into_iter()
        .chain(std::iter::once(package.to_string()))
        .min()
        .unwrap();
    format!("{}{name}-rc", repo.config.release_branch_prefix)
}

/// Versions of `package` that have a release tag.
pub fn released_versions(repo: &Repo, package: &str) -> Result<Vec<Version>, Error> {
    let tags = git::run(&repo.root, &["tag", "--list"])?;
    Ok(tags
        .lines()
        .filter_map(|t| repo.config.version_from_tag(package, t))
        .collect())
}

/// The rc to open: the named package and its fixed group first, then
/// every in-repo dependency with a pending changeset, transitively, with
/// its own group. Each group has one rc version. `level` overrides the
/// level the requested group's pending changesets imply; a pulled-in
/// group takes the level its own changesets name; with neither, patch.
/// Dependencies come before dependents, like a release plan.
pub fn plan(
    repo: &Repo,
    package: &str,
    level: Option<BumpLevel>,
) -> Result<Vec<PlannedRelease>, Error> {
    let find = |name: &str| repo.packages.iter().find(|p| p.name == name);
    find(package).ok_or_else(|| format!("unknown package {package}"))?;
    let group_of = |name: &str| -> Vec<String> {
        std::iter::once(name.to_string())
            .chain(repo.config.fixed_with(name))
            .collect()
    };
    let pending = |name: &str| {
        repo.changesets
            .iter()
            .any(|c| c.unreleased_for(name).is_some())
    };

    // Groups in the rc, requested first, then dependencies to a fixpoint.
    let mut groups: Vec<Vec<String>> = vec![group_of(package)];
    let mut i = 0;
    while i < groups.len() {
        let deps: Vec<String> = groups[i]
            .iter()
            .filter_map(|m| find(m))
            .flat_map(|m| m.dependencies.iter().cloned())
            .collect();
        for d in deps {
            if pending(&d) && !groups.iter().any(|g| g.contains(&d)) {
                groups.push(group_of(&d));
            }
        }
        i += 1;
    }
    let in_rc = |name: &str| groups.iter().any(|g| g.iter().any(|m| m == name));

    let mut out = Vec::new();
    for (i, group) in groups.iter().enumerate() {
        let members: Vec<&crate::plan::PackageState> =
            group.iter().filter_map(|m| find(m)).collect();
        let from_changesets = repo
            .changesets
            .iter()
            .flat_map(|c| members.iter().filter_map(|m| c.unreleased_for(&m.name)))
            .max();
        let level = if i == 0 {
            level.or(from_changesets)
        } else {
            from_changesets
        }
        .unwrap_or(BumpLevel::Patch);
        // The group moves from its highest version and past every rc any
        // member has tagged.
        let base = members.iter().map(|m| &m.version).max().unwrap();
        let mut released = Vec::new();
        for m in &members {
            released.extend(released_versions(repo, &m.name)?);
        }
        let to = next_prerelease_version(base, &released, level, &repo.config.prerelease_label);
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
    // Dependencies first: the order `apply` and `release` expect.
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
    package: &str,
    target: &str,
    level: Option<BumpLevel>,
    remote: &str,
    dry_run: bool,
    gh: Gh,
) -> Result<(Vec<PlannedRelease>, PullRequest), Error> {
    let releases = plan(repo, package, level)?;
    let branch = branch_name(repo, package);
    let start = format!("{remote}/{target}");
    if !git::ref_exists(&repo.root, &start) {
        return Err(format!("{start} does not exist; fetch it first").into());
    }
    // One version when the rc is one group, else "name version" each.
    let title = if releases.iter().all(|r| r.to == releases[0].to) {
        format!(
            "pre-release: {} {}",
            releases
                .iter()
                .map(|r| r.package.as_str())
                .collect::<Vec<_>>()
                .join(", "),
            releases[0].to
        )
    } else {
        format!(
            "pre-release: {}",
            releases
                .iter()
                .map(|r| format!("{} {}", r.package, r.to))
                .collect::<Vec<_>>()
                .join(", ")
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
        "{MARKER}\nMerging this PR releases a pre-release from `{target}`. \
         Changesets stay pending; the next final release includes them.\n"
    );
    if releases.len() > 1 {
        body.push_str(
            "Dependencies with a pending changeset are pre-released with the package, and it is pinned to their rc.\n",
        );
    }
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
        &repo.config.release_label,
    )?;
    Ok((releases, pr))
}
