//! Spec: rule MaintainerRequestsPreRelease and surface PreReleaseRequest.
//! One package, one rc PR, apart from the release PR, against any branch.
//! The branch bumps the package's own files and changelog only: changesets
//! stay, so the final release still lists them, and dependents are not
//! pinned to an rc. A member of a fixed group takes the whole group along
//! at the shared rc version. After the merge, `verwalter release` tags it
//! as a pre-release.

use crate::{
    apply, branches, git,
    github::{upsert_pr, Gh, PullRequest, MARKER},
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

/// The rc to open: the named package first, then the rest of its fixed
/// group, all at one version. `level` overrides the level the pending
/// changesets of the group imply; with neither, patch.
pub fn plan(
    repo: &Repo,
    package: &str,
    level: Option<BumpLevel>,
) -> Result<Vec<PlannedRelease>, Error> {
    let p = repo
        .packages
        .iter()
        .find(|p| p.name == package)
        .ok_or_else(|| format!("unknown package {package}"))?;
    let members: Vec<&crate::plan::PackageState> = std::iter::once(p)
        .chain(
            p.fixed_with
                .iter()
                .filter_map(|o| repo.packages.iter().find(|q| &q.name == o)),
        )
        .collect();
    let changesets_of = |name: &str| -> Vec<String> {
        repo.changesets
            .iter()
            .filter(|c| c.unreleased_for(name).is_some())
            .map(|c| c.file.clone())
            .collect()
    };
    let level = level
        .or_else(|| {
            repo.changesets
                .iter()
                .flat_map(|c| members.iter().filter_map(|m| c.unreleased_for(&m.name)))
                .max()
        })
        .unwrap_or(BumpLevel::Patch);
    let level = members
        .iter()
        .filter_map(|m| m.max_bump)
        .fold(level, BumpLevel::min);
    // The group moves from its highest version and past every rc any
    // member has tagged.
    let base = members.iter().map(|m| &m.version).max().unwrap();
    let mut released = Vec::new();
    for m in &members {
        released.extend(released_versions(repo, &m.name)?);
    }
    let to = next_prerelease_version(base, &released, level, &repo.config.prerelease_label);
    Ok(members
        .iter()
        .map(|m| PlannedRelease {
            package: m.name.clone(),
            from: m.version.clone(),
            to: to.clone(),
            level,
            changesets: changesets_of(&m.name),
            bumped_dependencies: Vec::new(),
            fixed_with: m.fixed_with.clone(),
        })
        .collect())
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
    let to = releases[0].to.clone();
    let names: Vec<&str> = releases.iter().map(|r| r.package.as_str()).collect();
    let branch = branch_name(repo, package);
    let start = format!("{remote}/{target}");
    if !git::ref_exists(&repo.root, &start) {
        return Err(format!("{start} does not exist; fetch it first").into());
    }
    let title = format!("pre-release: {} {to}", names.join(", "));
    branches::commit_branch(&repo.root, &start, &branch, &title, |worktree| {
        for release in &releases {
            apply::apply(
                worktree,
                &repo.config,
                &repo.packages,
                &repo.changesets,
                &releases,
                release,
                false,
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
        "{MARKER}\nMerging this PR releases pre-release `{to}` from `{target}`. \
         Changesets stay pending; the next final release includes them.\n"
    );
    for release in &releases {
        body.push_str(&format!(
            "\n## {} `{}` → `{to}`\n\n",
            release.package, release.from
        ));
        body.push_str(
            apply::render_entry(release, &repo.changesets, &releases)
                .trim_start_matches(|c| c != '\n')
                .trim_start(),
        );
    }
    let open: Vec<PullRequest> = serde_json::from_str(&gh(&[
        "pr",
        "list",
        "--state",
        "open",
        "--head",
        &branch,
        "--json",
        "number,headRefName,baseRefName",
    ])?)?;
    let pr = upsert_pr(
        gh,
        open.first(),
        &branch,
        target,
        &title,
        &body,
        &repo.config.release_label,
    )?;
    Ok((releases, pr))
}
