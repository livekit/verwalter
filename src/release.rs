//! Spec: ApplyRelease, TagRelease, CreateGitHubRelease and the Resume*
//! rules, plus the WorkflowReleaseHandoff surface. Runs on the base branch
//! after a release PR merged. Nothing is remembered between runs: a
//! package whose current version has no tag is a release to finish, and a
//! tag without a GitHub release gets one. Both make a failed run resumable.

use std::{fs, path::Path};

use serde::Serialize;

use crate::{config::ReleaseMode, git, github::Gh, repo::Repo, Error};

/// One line of the handoff to the publish workflow.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Released {
    pub package: String,
    pub previous_version: Option<String>,
    pub version: String,
    pub tag: String,
    pub prerelease: bool,
    /// No earlier tag of the package: a registry with trusted publishing
    /// needs this upload done by hand.
    pub first_release: bool,
    /// committed | tagged | drafted | published, as in the spec.
    pub status: &'static str,
}

/// What `run` may do beyond tagging and creating releases.
#[derive(Debug, Clone, Default)]
pub struct Options {
    /// Attached when a release is created; uploaded with `--clobber` to an
    /// existing release when it is a draft or `package` names it.
    pub assets: Vec<String>,
    /// Only this package; the others are left as they are.
    pub package: Option<String>,
    /// Spec: WorkflowPublishesRelease. Flip a draft to published once its
    /// assets are there.
    pub publish: bool,
    pub dry_run: bool,
}

/// The GitHub release behind a tag, as `gh release view` reports it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
pub struct ReleaseState {
    #[serde(rename = "isDraft")]
    pub draft: bool,
    #[serde(default)]
    pub assets: Vec<Asset>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
pub struct Asset {
    pub name: String,
}

pub fn view(gh: Gh, tag: &str) -> Option<ReleaseState> {
    gh(&["release", "view", tag, "--json", "isDraft,assets"])
        .ok()
        .and_then(|j| serde_json::from_str(&j).ok())
}

/// Finish every release that is not complete and report it. A package
/// whose tag exists and whose GitHub release is published (or not wanted)
/// is complete and not reported, unless assets were uploaded to it. A
/// draft is reported on every run until it is published.
///
/// Packages are handled in dependency order and the run stops at the
/// first failure, so a dependent is never released before its dependency.
pub fn run(repo: &Repo, remote: &str, options: &Options, gh: Gh) -> Result<Vec<Released>, Error> {
    let deps_of = |name: &str| -> Vec<&str> {
        repo.packages
            .iter()
            .find(|p| p.name == name)
            .map(|p| p.dependencies.iter().map(String::as_str).collect())
            .unwrap_or_default()
    };
    let order =
        crate::plan::dependency_order(repo.packages.iter().map(|p| p.name.as_str()), &deps_of);
    let mut out: Vec<Released> = Vec::new();
    for name in order {
        if options.package.as_deref().is_some_and(|only| only != name) {
            continue;
        }
        let version = &repo
            .packages
            .iter()
            .find(|p| p.name == name)
            .expect("ordered from packages")
            .version;
        if let Some(released) = release_one(repo, remote, options, name, version, gh)
            .map_err(|e| resume_hint(e, name, options))?
        {
            out.push(released);
        }
    }
    Ok(out)
}

fn resume_hint(e: Error, package: &str, options: &Options) -> Error {
    let assets: String = options
        .assets
        .iter()
        .map(|a| format!(" --asset {a}"))
        .collect();
    let publish = if options.publish { " --publish" } else { "" };
    format!("{package}: {e}\nresume with: verwalter release --package {package}{assets}{publish}  (add --dry-run to see the steps first)").into()
}

fn release_one(
    repo: &Repo,
    remote: &str,
    options: &Options,
    name: &str,
    version: &crate::Version,
    gh: Gh,
) -> Result<Option<Released>, Error> {
    let root = repo.root.as_path();
    let package = &repo.config.packages[name];
    if package.release == ReleaseMode::None {
        return Ok(None);
    }
    let dry_run = options.dry_run;
    let tag = repo.config.tag_for(name, version);

    // Spec: TagRelease / ResumeUntaggedReleases. An existing tag, local
    // or on the remote, counts as done wherever it points.
    let mut fresh = false;
    let changelog = fs::read_to_string(root.join(&package.changelog)).unwrap_or_default();
    if !git::ref_exists(root, &tag) {
        // Spec: ReleaseNeedsChangelogEntry. The release branch always writes
        // one; a version edited by hand has none and is not a release.
        let heading = format!("## {version}");
        if options.package.is_none() && !changelog.lines().any(|l| l.trim() == heading) {
            eprintln!(
                "warning: {name} {version} has no tag and no changelog entry; not released (edited by hand?). `release --package {name}` releases it anyway"
            );
            return Ok(None);
        }
        if remote_has_tag(root, remote, &tag)? {
            git::run(
                root,
                &[
                    "fetch",
                    "--quiet",
                    remote,
                    &format!("refs/tags/{tag}:refs/tags/{tag}"),
                ],
            )?;
        } else {
            let head = git::run(root, &["rev-parse", "HEAD"])?;
            git::write(root, dry_run, &["tag", &tag, &head])?;
            git::write(
                root,
                dry_run,
                &["push", "--quiet", remote, &format!("refs/tags/{tag}")],
            )?;
            fresh = true;
        }
    }
    let mut status = "tagged";

    // Spec: CreateGitHubRelease / ResumeTaggedReleasesWithoutGitHubRelease,
    // then WorkflowPublishesRelease for drafts.
    if package.release == ReleaseMode::TagAndRelease {
        match view(gh, &tag) {
            Some(existing) => {
                let mut draft = existing.draft;
                // Assets go to an existing release only when it is still a
                // draft or the package was named: in a monorepo, `--asset`
                // must not land on every complete release.
                if !options.assets.is_empty() && (draft || options.package.is_some()) {
                    let mut args = vec!["release", "upload", tag.as_str(), "--clobber"];
                    args.extend(options.assets.iter().map(String::as_str));
                    gh(&args)?;
                    fresh = true;
                }
                if draft && options.publish {
                    gh(&["release", "edit", &tag, "--draft=false"])?;
                    draft = false;
                    fresh = true;
                }
                status = if draft { "drafted" } else { "published" };
            }
            None => {
                let notes = changelog_section(&changelog, &version.to_string());
                let title = format!("{name} {version}");
                let mut args = vec![
                    "release",
                    "create",
                    &tag,
                    "--title",
                    &title,
                    "--notes",
                    &notes,
                    "--verify-tag",
                ];
                args.extend(options.assets.iter().map(String::as_str));
                let draft = package.draft && !options.publish;
                if draft {
                    args.push("--draft");
                }
                if version.is_prerelease() {
                    args.push("--prerelease");
                }
                match gh(&args) {
                    Ok(_) => {
                        fresh = true;
                        status = if draft { "drafted" } else { "published" };
                    }
                    // Two release runs at once (two PRs merged together):
                    // the other one won the create, which is as good as done.
                    Err(e) => match view(gh, &tag) {
                        Some(existing) => {
                            status = if existing.draft {
                                "drafted"
                            } else {
                                "published"
                            }
                        }
                        None => return Err(e),
                    },
                }
            }
        }
    }

    // A package named with --package is always reported, so a manual run
    // can hand one release to the publish step again.
    if !fresh && status != "drafted" && options.package.is_none() {
        return Ok(None);
    }
    let first_release = first_release(repo, name, version)?;
    Ok(Some(Released {
        package: name.to_string(),
        previous_version: previous_version(&changelog, &version.to_string()),
        version: version.to_string(),
        tag,
        prerelease: version.is_prerelease(),
        first_release,
        status,
    }))
}

/// Spec: Package.first_release. The registry has no version of the
/// package when one is configured, else no tag other than `version`.
pub fn first_release(repo: &Repo, name: &str, version: &crate::Version) -> Result<bool, Error> {
    match crate::registry::exists(
        crate::registry::package_url(&repo.config, name),
        &crate::registry::curl,
    )? {
        Some(has_any) => Ok(!has_any),
        None => first_release_by_tags(repo, name, version),
    }
}

/// No tag of the package other than `version` itself.
pub fn first_release_by_tags(
    repo: &Repo,
    name: &str,
    version: &crate::Version,
) -> Result<bool, Error> {
    Ok(crate::prerelease::released_versions(repo, name)?
        .iter()
        .all(|v| v == version))
}

/// Spec: MaintainerAbandonsRelease. Removes the GitHub release and the
/// tag, locally and on the remote, for a release whose commit turned out
/// broken. Missing pieces are skipped; the registry is not touched.
pub fn abandon(
    repo: &Repo,
    remote: &str,
    tag: &str,
    dry_run: bool,
    gh: Gh,
) -> Result<Vec<String>, Error> {
    let root = repo.root.as_path();
    let mut removed = Vec::new();
    if gh(&["release", "view", tag, "--json", "tagName"]).is_ok() {
        gh(&["release", "delete", tag, "--yes"])?;
        removed.push(format!("release {tag}"));
    }
    if remote_has_tag(root, remote, tag)? {
        git::write(
            root,
            dry_run,
            &[
                "push",
                "--quiet",
                "--delete",
                remote,
                &format!("refs/tags/{tag}"),
            ],
        )?;
        removed.push(format!("{remote} tag {tag}"));
    }
    if git::ref_exists(root, tag) {
        git::write(root, dry_run, &["tag", "--delete", tag])?;
        removed.push(format!("local tag {tag}"));
    }
    Ok(removed)
}

fn remote_has_tag(root: &Path, remote: &str, tag: &str) -> Result<bool, Error> {
    Ok(!git::run(
        root,
        &["ls-remote", "--tags", remote, &format!("refs/tags/{tag}")],
    )?
    .is_empty())
}

/// The body under `## <version>` up to the next `## `.
pub fn changelog_section(changelog: &str, version: &str) -> String {
    let heading = format!("## {version}");
    let Some(start) = changelog.lines().position(|l| l.trim() == heading) else {
        return String::new();
    };
    changelog
        .lines()
        .skip(start + 1)
        .take_while(|l| !l.starts_with("## "))
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string()
}

/// ponytail: the previous version is the next `## ` heading below this
/// one in the changelog. Good enough for the handoff; git tags would be
/// the exact source.
fn previous_version(changelog: &str, version: &str) -> Option<String> {
    let heading = format!("## {version}");
    changelog
        .lines()
        .skip_while(|l| l.trim() != heading)
        .skip(1)
        .find_map(|l| {
            l.strip_prefix("## ")
                .map(|v| v.split_whitespace().next().unwrap_or("").to_string())
        })
}

/// Spec: WorkflowReleaseHandoff. Appends to `$GITHUB_OUTPUT` when set.
pub fn write_outputs(released: &[Released]) -> Result<String, Error> {
    let json = serde_json::to_string(released)?;
    if let Ok(path) = std::env::var("GITHUB_OUTPUT") {
        use std::io::Write;
        let mut f = fs::OpenOptions::new()
            .append(true)
            .create(true)
            .open(path)?;
        writeln!(f, "released={}", !released.is_empty())?;
        writeln!(f, "releases={json}")?;
    }
    Ok(json)
}

#[cfg(test)]
mod tests {
    use super::*;

    const LOG: &str = "# Changelog\n\n## 0.4.0\n\n### Minor Changes\n\n- a\n- b\n\n## 0.3.1\n\n### Patch Changes\n\n- old\n";

    #[test]
    fn changelog_section_and_previous() {
        assert_eq!(
            changelog_section(LOG, "0.4.0"),
            "### Minor Changes\n\n- a\n- b"
        );
        assert_eq!(
            changelog_section(LOG, "0.3.1"),
            "### Patch Changes\n\n- old"
        );
        assert_eq!(changelog_section(LOG, "9.9.9"), "");
        assert_eq!(previous_version(LOG, "0.4.0").as_deref(), Some("0.3.1"));
        assert_eq!(previous_version(LOG, "0.3.1"), None);
    }
}
