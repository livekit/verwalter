//! Spec: ApplyRelease, TagRelease, CreateGitHubRelease and the Resume*
//! rules, plus the WorkflowReleaseHandoff surface. Runs on the base branch
//! after a release PR merged. Nothing is remembered between runs: a
//! package whose current version has no tag is a release to finish, and a
//! tag without a GitHub release gets one. Both make a failed run resumable.

use std::{fs, path::Path};

use serde::Serialize;

use crate::{config::ReleaseMode, git, github::Gh, repo::Repo};

pub type Error = Box<dyn std::error::Error + Send + Sync>;

/// One line of the handoff to the publish workflow.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Released {
    pub package: String,
    pub previous_version: Option<String>,
    pub version: String,
    pub tag: String,
    pub prerelease: bool,
    /// committed | tagged | drafted | published, as in the spec.
    pub status: &'static str,
}

/// Finish every release that is not complete and report it. A package
/// whose tag exists and whose GitHub release is published (or not wanted)
/// is complete and not reported. A draft is reported on every run until
/// the workflow publishes it. `assets` are attached when a release is
/// created, so a workflow that builds first needs no draft at all.
pub fn run(repo: &Repo, remote: &str, assets: &[String], gh: Gh) -> Result<Vec<Released>, Error> {
    let root = repo.root.as_path();
    let head = git::run(root, &["rev-parse", "HEAD"])?;
    let mut out = Vec::new();

    for p in &repo.packages {
        let package = &repo.config.packages[&p.name];
        if package.release == ReleaseMode::None {
            continue;
        }
        let tag = repo.config.tag_for(&p.name, &p.version);

        // Spec: TagRelease / ResumeUntaggedReleases. An existing tag, local
        // or on the remote, counts as done wherever it points.
        let mut fresh = false;
        if !git::ref_exists(root, &tag) {
            if remote_has_tag(root, remote, &tag)? {
                git::run(root, &["fetch", "--quiet", remote, &format!("refs/tags/{tag}:refs/tags/{tag}")])?;
            } else {
                git::run(root, &["tag", &tag, &head])?;
                git::run(root, &["push", "--quiet", remote, &format!("refs/tags/{tag}")])?;
                fresh = true;
            }
        }
        let mut status = "tagged";

        // Spec: CreateGitHubRelease / ResumeTaggedReleasesWithoutGitHubRelease.
        if package.release == ReleaseMode::TagAndRelease {
            match gh(&["release", "view", &tag, "--json", "isDraft", "--jq", ".isDraft"]) {
                Ok(is_draft) => status = if is_draft.trim() == "true" { "drafted" } else { "published" },
                Err(_) => {
                    let notes = changelog_section(&fs::read_to_string(root.join(&package.changelog)).unwrap_or_default(), &p.version.to_string());
                    let title = format!("{} {}", p.name, p.version);
                    let mut args = vec!["release", "create", &tag, "--title", &title, "--notes", &notes, "--verify-tag"];
                    args.extend(assets.iter().map(String::as_str));
                    if package.draft {
                        args.push("--draft");
                    }
                    if p.version.is_prerelease() {
                        args.push("--prerelease");
                    }
                    gh(&args)?;
                    fresh = true;
                    status = if package.draft { "drafted" } else { "published" };
                }
            }
        }

        if !fresh && status != "drafted" {
            continue;
        }
        let changelog = fs::read_to_string(root.join(&package.changelog)).unwrap_or_default();
        out.push(Released {
            package: p.name.clone(),
            previous_version: previous_version(&changelog, &p.version.to_string()),
            version: p.version.to_string(),
            tag,
            prerelease: p.version.is_prerelease(),
            status,
        });
    }
    Ok(out)
}

/// Spec: MaintainerAbandonsRelease. Removes the GitHub release and the
/// tag, locally and on the remote, for a release whose commit turned out
/// broken. Missing pieces are skipped; the registry is not touched.
pub fn abandon(repo: &Repo, remote: &str, tag: &str, gh: Gh) -> Result<Vec<String>, Error> {
    let root = repo.root.as_path();
    let mut removed = Vec::new();
    if gh(&["release", "view", tag, "--json", "tagName"]).is_ok() {
        gh(&["release", "delete", tag, "--yes"])?;
        removed.push(format!("release {tag}"));
    }
    if remote_has_tag(root, remote, tag)? {
        git::run(root, &["push", "--quiet", "--delete", remote, &format!("refs/tags/{tag}")])?;
        removed.push(format!("{remote} tag {tag}"));
    }
    if git::ref_exists(root, tag) {
        git::run(root, &["tag", "--delete", tag])?;
        removed.push(format!("local tag {tag}"));
    }
    Ok(removed)
}

fn remote_has_tag(root: &Path, remote: &str, tag: &str) -> Result<bool, Error> {
    Ok(!git::run(root, &["ls-remote", "--tags", remote, &format!("refs/tags/{tag}")])?.is_empty())
}

/// The body under `## <version>` up to the next `## `.
pub fn changelog_section(changelog: &str, version: &str) -> String {
    let heading = format!("## {version}");
    let Some(start) = changelog.lines().position(|l| l.trim() == heading) else { return String::new() };
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
        .find_map(|l| l.strip_prefix("## ").map(|v| v.split_whitespace().next().unwrap_or("").to_string()))
}

/// Spec: WorkflowReleaseHandoff. Appends to `$GITHUB_OUTPUT` when set.
pub fn write_outputs(released: &[Released]) -> Result<String, Error> {
    let json = serde_json::to_string(released)?;
    if let Ok(path) = std::env::var("GITHUB_OUTPUT") {
        use std::io::Write;
        let mut f = fs::OpenOptions::new().append(true).create(true).open(path)?;
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
        assert_eq!(changelog_section(LOG, "0.4.0"), "### Minor Changes\n\n- a\n- b");
        assert_eq!(changelog_section(LOG, "0.3.1"), "### Patch Changes\n\n- old");
        assert_eq!(changelog_section(LOG, "9.9.9"), "");
        assert_eq!(previous_version(LOG, "0.4.0").as_deref(), Some("0.3.1"));
        assert_eq!(previous_version(LOG, "0.3.1"), None);
    }
}
