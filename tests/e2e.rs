//! End to end on a real git repository: plan, branches, stacking,
//! changeset consumption by front-matter edits.

use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{Result, anyhow};
use verwalter::{branches, github::Client, plan::plan, repo::Repo};

/// Fake GitHub: answers each gh call with the wrapped closure.
struct MockClient<F>(F);

/// Blanket implementation for closures.
impl<F: Fn(&[&str]) -> Result<String>> Client for MockClient<F> {
    fn execute(&self, args: &[&str]) -> Result<String> {
        (self.0)(args)
    }
}

/// A release as [`FakeGitHub`] keeps it.
struct FakeRelease {
    draft: bool,
    /// Asset names, in upload order.
    assets: Vec<String>,
}

/// Stateful fake GitHub, for a scenario that spans several runs of the
/// tool. It keeps one release per tag and answers the gh calls `release`,
/// `publish` and `status` send. A call it has no answer for is an error,
/// and fails the test when the fake drops: the tool goes on after some
/// failed gh calls, so the error alone could pass unseen.
#[derive(Default)]
struct FakeGitHub {
    releases: std::cell::RefCell<std::collections::BTreeMap<String, FakeRelease>>,
    /// Every `gh release ...` call since the last `take_writes`.
    writes: std::cell::RefCell<Vec<String>>,
    unanswered: std::cell::RefCell<Vec<String>>,
}

impl FakeGitHub {
    /// None when the tag has no release.
    fn is_draft(&self, tag: &str) -> Option<bool> {
        self.releases.borrow().get(tag).map(|r| r.draft)
    }

    fn assets(&self, tag: &str) -> Vec<String> {
        self.releases
            .borrow()
            .get(tag)
            .map(|r| r.assets.clone())
            .unwrap_or_default()
    }

    /// The write calls since the last call of this, as gh arguments. The
    /// release notes are shown as `<notes>`.
    fn take_writes(&self) -> Vec<String> {
        self.writes.take()
    }
}

impl Client for FakeGitHub {
    fn execute(&self, args: &[&str]) -> Result<String> {
        let mut releases = self.releases.borrow_mut();
        if args.first() == Some(&"release") {
            let shown: Vec<&str> = args
                .iter()
                .map(|a| if a.contains('\n') { "<notes>" } else { *a })
                .collect();
            self.writes.borrow_mut().push(shown.join(" "));
        }
        // The name of an asset is the file name of its path.
        let name = |path: &str| path.rsplit('/').next().unwrap().to_string();
        match args {
            ["pr", "list", ..] => Ok("[]".into()),
            // ReleaseView: the output of its --jq, or nothing.
            ["api", "graphql", "-f", tag, ..] if tag.starts_with("tag=") => {
                Ok(match releases.get(&tag["tag=".len()..]) {
                    Some(r) => serde_json::json!({
                        "isDraft": r.draft,
                        "asset_count": r.assets.len(),
                        "assets": r.assets,
                    })
                    .to_string(),
                    None => String::new(),
                })
            }
            [
                "release",
                "create",
                tag,
                "--title",
                _,
                "--notes",
                _,
                rest @ ..,
            ] => {
                if releases.contains_key(*tag) {
                    anyhow::bail!("a release for {tag} exists already");
                }
                let mut release = FakeRelease {
                    draft: false,
                    assets: Vec::new(),
                };
                for arg in rest {
                    match *arg {
                        "--draft" => release.draft = true,
                        "--verify-tag" | "--prerelease" | "--latest=false" => {}
                        flag if flag.starts_with("--") => {
                            anyhow::bail!("FakeGitHub does not know the flag {flag}")
                        }
                        file => release.assets.push(name(file)),
                    }
                }
                releases.insert(tag.to_string(), release);
                Ok(String::new())
            }
            ["release", "upload", tag, files @ ..] => {
                let release = releases
                    .get_mut(*tag)
                    .ok_or_else(|| anyhow!("release not found: {tag}"))?;
                let clobber = files.contains(&"--clobber");
                for file in files.iter().filter(|f| **f != "--clobber") {
                    let name = name(file);
                    if !release.assets.contains(&name) {
                        release.assets.push(name);
                    } else if !clobber {
                        anyhow::bail!("asset {name} exists already on {tag}");
                    }
                }
                Ok(String::new())
            }
            ["release", "edit", tag, "--draft=false"] => {
                let release = releases
                    .get_mut(*tag)
                    .ok_or_else(|| anyhow!("release not found: {tag}"))?;
                release.draft = false;
                Ok(String::new())
            }
            ["release", "delete", tag, "--yes"] => {
                releases
                    .remove(*tag)
                    .ok_or_else(|| anyhow!("release not found: {tag}"))?;
                Ok(String::new())
            }
            _ => {
                let call = format!("gh {}", args.join(" "));
                self.unanswered.borrow_mut().push(call.clone());
                Err(anyhow!("FakeGitHub has no answer for: {call}"))
            }
        }
    }
}

impl Drop for FakeGitHub {
    fn drop(&mut self) {
        let unanswered = self.unanswered.borrow();
        if !unanswered.is_empty() && !std::thread::panicking() {
            panic!("FakeGitHub had no answer for: {unanswered:#?}");
        }
    }
}

struct Repo_(PathBuf);

fn git(root: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .arg("-C")
        .arg(root)
        .args([
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@t",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap().trim().to_string()
}

fn write(root: &Path, rel: &str, content: &str) {
    let p = root.join(rel);
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, content).unwrap();
}

fn fixture(name: &str) -> Repo_ {
    let root = std::env::temp_dir().join(format!("verwalter-it-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    git(&root, &["init", "-q", "-b", "main"]);
    write(
        &root,
        "verwalter.toml",
        r#"
[workspace]
pin_files = ["Cargo.toml", "Cargo.lock"]

[packages.livekit-protocol]
path = "livekit-protocol"
versioning.files = ["Cargo.toml"]
versioning.changelog = "CHANGELOG.md"

[packages.livekit-api]
path = "livekit-api"
versioning.files = ["Cargo.toml"]
versioning.changelog = "CHANGELOG.md"

[packages.livekit]
path = "livekit"
versioning.files = ["Cargo.toml"]
versioning.changelog = "CHANGELOG.md"
"#,
    );
    write(
        &root,
        "Cargo.toml",
        "[workspace]\nmembers = [\"livekit-protocol\", \"livekit-api\", \"livekit\"]\n\n[workspace.dependencies]\nlivekit-protocol = { version = \"0.3.1\", path = \"livekit-protocol\" }\nlivekit-api = { version = \"0.4.3\", path = \"livekit-api\" }\n",
    );
    write(
        &root,
        "Cargo.lock",
        "version = 4\n\n[[package]]\nname = \"livekit\"\nversion = \"0.7.0\"\n\n[[package]]\nname = \"livekit-api\"\nversion = \"0.4.3\"\n\n[[package]]\nname = \"livekit-protocol\"\nversion = \"0.3.1\"\n",
    );
    write(
        &root,
        "livekit-protocol/Cargo.toml",
        "[package]\nname = \"livekit-protocol\"\nversion = \"0.3.1\"\n",
    );
    write(
        &root,
        "livekit-api/Cargo.toml",
        "[package]\nname = \"livekit-api\"\nversion = \"0.4.3\"\n\n[dependencies]\nlivekit-protocol = { workspace = true }\n",
    );
    write(
        &root,
        "livekit/Cargo.toml",
        "[package]\nname = \"livekit\"\nversion = \"0.7.0\"\n\n[dependencies]\nlivekit-protocol = { workspace = true }\n",
    );
    // The release branch always writes the entry; a version without one
    // was edited by hand and is not released.
    write(
        &root,
        "livekit-protocol/CHANGELOG.md",
        "# Changelog\n\n## 0.3.1\n\n### Patch Changes\n\n- old\n",
    );
    write(
        &root,
        "livekit-api/CHANGELOG.md",
        "# Changelog\n\n## 0.4.3\n\n### Patch Changes\n\n- old\n",
    );
    write(
        &root,
        "livekit/CHANGELOG.md",
        "# Changelog\n\n## 0.7.0\n\n### Patch Changes\n\n- old\n",
    );
    git(&root, &["add", "-A"]);
    git(&root, &["commit", "-q", "-m", "init"]);
    Repo_(root)
}

/// A bare `origin` for `root`, removed when the guard drops.
fn remote(root: &Path, name: &str) -> Repo_ {
    let remote = std::env::temp_dir().join(format!(
        "verwalter-it-{}-{name}-remote.git",
        std::process::id()
    ));
    git(root, &["init", "-q", "--bare", remote.to_str().unwrap()]);
    git(root, &["remote", "add", "origin", remote.to_str().unwrap()]);
    Repo_(remote)
}

impl Drop for Repo_ {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn one_branch_from_one_changeset() {
    let r = fixture("branch");
    let root = &r.0;
    write(
        root,
        ".changeset/ndc.md",
        "---\nlivekit-protocol: minor\nlivekit: minor\n---\nNegotiated data channels\n",
    );
    git(root, &["add", "-A"]);
    git(root, &["commit", "-q", "-m", "add changeset"]);

    let repo = Repo::load(root).unwrap();
    let plan = plan(
        &repo.packages,
        &repo.changesets,
        repo.config.versioning.dependent_bump_level,
    );
    // Start from a remote-tracking ref, as CI does; the PR still targets `main`.
    git(root, &["update-ref", "refs/remotes/origin/main", "main"]);
    let branch = branches::create(&repo, &plan, "origin/main").unwrap();
    assert_eq!(branch.as_deref(), Some("release/main"));
    assert_eq!(
        git(root, &["log", "-1", "--format=%s", "release/main"]),
        "release: livekit-protocol 0.4.0, livekit 0.8.0, livekit-api 0.4.4"
    );

    // The user's checkout is untouched and still on main.
    assert_eq!(git(root, &["rev-parse", "--abbrev-ref", "HEAD"]), "main");
    assert_eq!(git(root, &["status", "--porcelain"]), "");

    // Every package in the one commit: own version, workspace pin, changelog.
    assert!(git(root, &["show", "release/main:livekit-protocol/Cargo.toml"]).contains("0.4.0"));
    assert!(
        git(root, &["show", "release/main:Cargo.toml"])
            .contains("livekit-protocol = { version = \"0.4.0\"")
    );
    assert!(
        git(
            root,
            &["show", "release/main:livekit-protocol/CHANGELOG.md"]
        )
        .contains("## 0.4.0")
    );
    assert!(
        git(root, &["show", "release/main:livekit/Cargo.toml"]).contains("version = \"0.8.0\"")
    );
    // api only got a dependent patch.
    assert!(
        git(root, &["show", "release/main:livekit-api/Cargo.toml"]).contains("version = \"0.4.4\"")
    );
    assert!(
        git(root, &["show", "release/main:livekit-api/CHANGELOG.md"])
            .contains("Updated dependencies: livekit-protocol@0.4.0")
    );
    // The changeset is consumed.
    assert!(
        git(
            root,
            &["ls-tree", "--name-only", "release/main", ".changeset/"]
        )
        .is_empty()
    );

    // Re-running regenerates the branch without error; nothing planned, no branch.
    assert!(branches::create(&repo, &plan, "main").unwrap().is_some());
    assert_eq!(branches::create(&repo, &[], "main").unwrap(), None);
}

// Spec: ConsumeChangesets. Merging the release PR deletes every changeset;
// a changeset naming a package that is not configured is refused up
// front. Tags play no part.
#[test]
fn merged_release_consumes_the_changesets() {
    let r = fixture("consume");
    let root = &r.0;
    write(
        root,
        ".changeset/ndc.md",
        "---\nlivekit-protocol: minor\nlivekit: minor\n---\nx\n",
    );
    write(
        root,
        ".changeset/other.md",
        "---\nlivekit-protocol: patch\nsomething-else: patch\n---\ny\n",
    );
    let err = format!(
        "{:#}",
        Repo::load(root).err().expect("unknown package refused")
    );
    assert!(
        err.contains("other.md: names unknown package something-else"),
        "{err}"
    );
    fs::remove_file(root.join(".changeset/other.md")).unwrap();
    git(root, &["add", "-A"]);
    git(root, &["commit", "-q", "-m", "add changesets"]);
    let repo = Repo::load(root).unwrap();
    let first = plan(
        &repo.packages,
        &repo.changesets,
        repo.config.versioning.dependent_bump_level,
    );
    branches::create(&repo, &first, "main").unwrap();
    git(root, &["merge", "-q", "--ff-only", "release/main"]);

    let repo = Repo::load(root).unwrap();
    assert!(git(root, &["ls-tree", "--name-only", "HEAD", ".changeset/"]).is_empty());
    assert!(
        plan(
            &repo.packages,
            &repo.changesets,
            repo.config.versioning.dependent_bump_level
        )
        .is_empty()
    );
}

#[test]
fn pr_sync_pushes_and_creates_updates_closes() {
    let r = fixture("pr");
    let root = &r.0;
    write(
        root,
        ".changeset/ndc.md",
        "---\nlivekit-protocol: minor\nlivekit: minor\n---\nx\n",
    );
    git(root, &["add", "-A"]);
    git(root, &["commit", "-q", "-m", "add changeset"]);
    let _remote = remote(root, "pr");

    let repo = Repo::load(root).unwrap();
    let plan = plan(
        &repo.packages,
        &repo.changesets,
        repo.config.versioning.dependent_bump_level,
    );
    let branch = branches::create(&repo, &plan, "main").unwrap();

    let calls = std::cell::RefCell::new(Vec::<String>::new());
    let has = |prefix: &str| calls.borrow().iter().any(|c| c.starts_with(prefix));
    // #40 is the open PR on the release branch, with a wrong base.
    let open = r#"[{"number":40,"headRefName":"release/main","baseRefName":"next"}]"#;
    let gh = MockClient(|args: &[&str]| -> Result<String> {
        calls.borrow_mut().push(args.join(" "));
        Ok(match args {
            ["pr", "list", ..] => open.into(),
            ["pr", "create", ..] => "https://github.com/livekit/x/pull/100".into(),
            _ => String::new(),
        })
    });
    let pr = verwalter::github::sync(&repo, &plan, branch.as_deref(), "origin", false, &gh)
        .unwrap()
        .unwrap();

    assert!(
        git(root, &["ls-remote", "--heads", "origin"]).contains("release/main"),
        "branch pushed"
    );
    assert_eq!(
        (pr.number, pr.head.as_str(), pr.base.as_str()),
        (40, "release/main", "main")
    );
    assert!(calls.borrow()[0].starts_with("label create release --force"));
    assert!(!has("pr create"), "existing PR updated in place");
    {
        let calls = calls.borrow();
        let edit = calls
            .iter()
            .find(|c| c.starts_with("pr edit 40 --title"))
            .unwrap();
        assert!(
            edit.contains(
                "--title release: livekit-protocol 0.4.0, livekit 0.8.0, livekit-api 0.4.4"
            ),
            "{edit}"
        );
        assert!(
            edit.contains("## livekit-api `0.4.3` → `0.4.4`"),
            "one section per package"
        );
    }
    assert!(has("pr edit 40 --base main"), "base corrected");
    assert!(
        has("pr list --state open --head release/main"),
        "only the release branch's PR is looked at"
    );

    // Nothing planned: the release PR is closed.
    calls.borrow_mut().clear();
    assert!(
        verwalter::github::sync(&repo, &[], None, "origin", false, &gh)
            .unwrap()
            .is_none()
    );
    assert!(has("pr close 40"));

    // A fresh repo gets a new PR.
    calls.borrow_mut().clear();
    let gh = MockClient(|args: &[&str]| -> Result<String> {
        calls.borrow_mut().push(args.join(" "));
        Ok(match args {
            ["pr", "list", ..] => "[]".into(),
            ["pr", "create", ..] => "https://github.com/livekit/x/pull/100".into(),
            _ => String::new(),
        })
    });
    let pr = verwalter::github::sync(&repo, &plan, branch.as_deref(), "origin", false, &gh)
        .unwrap()
        .unwrap();
    assert_eq!(pr.number, 100);
    assert!(has(
        "pr create --head release/main --base main --title release:"
    ));
}

#[test]
fn release_tags_and_creates_github_releases_resumably() {
    let r = fixture("release");
    let root = &r.0;
    let _remote = remote(root, "release");
    // Everything is already tagged except livekit, whose release PR just
    // merged: version bumped, changelog written. livekit-api is draft mode.
    git(root, &["tag", "livekit-protocol-v0.3.1"]);
    git(root, &["tag", "livekit-api-v0.4.3"]);
    git(root, &["push", "-q", "origin", "--tags"]);
    write(
        root,
        "livekit/Cargo.toml",
        "[package]\nname = \"livekit\"\nversion = \"0.8.0\"\n\n[dependencies]\nlivekit-protocol = { workspace = true }\n",
    );
    write(
        root,
        "livekit/CHANGELOG.md",
        "# Changelog\n\n## 0.8.0\n\n### Minor Changes\n\n- Negotiated data channels\n\n## 0.7.0\n\n- init\n",
    );
    let cfg = fs::read_to_string(root.join("verwalter.toml"))
        .unwrap()
        .replace(
            "path = \"livekit\"\nversioning.files = [\"Cargo.toml\"]\nversioning.changelog = \"CHANGELOG.md\"",
            "path = \"livekit\"\nversioning.files = [\"Cargo.toml\"]\nversioning.changelog = \"CHANGELOG.md\"\nrelease.draft = true",
        );
    write(root, "verwalter.toml", &cfg);
    git(root, &["commit", "-qam", "release: livekit 0.8.0"]);

    let repo = Repo::load(root).unwrap();
    let calls = std::cell::RefCell::new(Vec::<String>::new());
    let gh = MockClient(|args: &[&str]| -> Result<String> {
        calls.borrow_mut().push(args.join(" "));
        match args {
            // Older tags already have releases; the new one does not yet.
            ["api", "graphql", "-f", tag, ..] if *tag != "tag=livekit-v0.8.0" => {
                Ok(r#"{"isDraft":false,"asset_count":0}"#.into())
            }
            ["api", "graphql", ..] => Ok(String::new()),
            _ => Ok(String::new()),
        }
    });
    let released = verwalter::release::run(&repo, "origin", &Default::default(), &gh).unwrap();

    assert!(
        git(root, &["ls-remote", "--tags", "origin"]).contains("livekit-v0.8.0"),
        "tag pushed"
    );
    assert_eq!(
        git(root, &["rev-parse", "livekit-v0.8.0^{commit}"]),
        git(root, &["rev-parse", "HEAD"])
    );
    let create = calls
        .borrow()
        .iter()
        .find(|c| c.starts_with("release create livekit-v0.8.0"))
        .unwrap()
        .clone();
    assert!(create.contains("--draft"), "draft mode from config");
    assert!(
        create.contains("- Negotiated data channels"),
        "notes from the changelog section"
    );
    assert!(!create.contains("--prerelease"));
    assert!(!create.contains("--latest=false"));

    let lk = released.iter().find(|r| r.package == "livekit").unwrap();
    assert_eq!(
        (
            lk.version.as_str(),
            lk.tag.as_str(),
            lk.status,
            lk.previous_version.as_deref()
        ),
        ("0.8.0", "livekit-v0.8.0", "drafted", Some("0.7.0"))
    );
    assert_eq!(released.len(), 1, "complete releases are not reported");

    // Second run: nothing to tag, nothing to create.
    let before = calls.borrow().len();
    let gh2 = MockClient(|args: &[&str]| -> Result<String> {
        calls.borrow_mut().push(args.join(" "));
        Ok(if args[3] == "tag=livekit-v0.8.0" {
            r#"{"isDraft":true,"asset_count":0}"#.into()
        } else {
            r#"{"isDraft":false,"asset_count":0}"#.into()
        })
    });
    let again = verwalter::release::run(&repo, "origin", &Default::default(), &gh2).unwrap();
    assert!(
        !calls.borrow()[before..]
            .iter()
            .any(|c| c.starts_with("release create"))
    );
    assert_eq!(
        again.len(),
        1,
        "the draft is reported until the workflow publishes it"
    );
    assert_eq!(again[0].status, "drafted");
}

// A release below a version already tagged comes from a maintenance
// branch: it must not take GitHub's Latest badge from the newer line.
#[test]
fn maintenance_release_is_not_marked_latest() {
    let r = fixture("maintenance");
    let root = &r.0;
    let _remote = remote(root, "maintenance");
    git(root, &["tag", "livekit-protocol-v0.3.1"]);
    git(root, &["tag", "livekit-api-v0.4.3"]);
    git(root, &["tag", "livekit-v0.7.0"]);
    git(root, &["tag", "livekit-v1.0.0"]);
    git(root, &["push", "-q", "origin", "--tags"]);
    write(
        root,
        "livekit/Cargo.toml",
        "[package]\nname = \"livekit\"\nversion = \"0.7.1\"\n\n[dependencies]\nlivekit-protocol = { workspace = true }\n",
    );
    write(
        root,
        "livekit/CHANGELOG.md",
        "# Changelog\n\n## 0.7.1\n\n### Patch Changes\n\n- Fix\n\n## 0.7.0\n\n- init\n",
    );
    git(root, &["commit", "-qam", "release: livekit 0.7.1"]);

    let repo = Repo::load(root).unwrap();
    let calls = std::cell::RefCell::new(Vec::<String>::new());
    let gh = MockClient(|args: &[&str]| -> Result<String> {
        calls.borrow_mut().push(args.join(" "));
        match args {
            ["api", "graphql", "-f", "tag=livekit-v0.7.1", ..] => Ok(String::new()),
            ["api", "graphql", ..] => Ok(r#"{"isDraft":false,"asset_count":0}"#.into()),
            _ => Ok(String::new()),
        }
    });
    verwalter::release::run(&repo, "origin", &Default::default(), &gh).unwrap();
    let create = calls
        .borrow()
        .iter()
        .find(|c| c.starts_with("release create livekit-v0.7.1"))
        .unwrap()
        .clone();
    assert!(create.contains("--latest=false"), "{create}");
}

#[test]
fn release_resumes_a_tag_without_github_release() {
    let r = fixture("release-resume");
    let root = &r.0;
    let _remote = remote(root, "resume");
    // The tag was pushed by a failed run, on an older commit.
    git(root, &["tag", "livekit-v0.7.0"]);
    git(root, &["tag", "livekit-api-v0.4.3"]);
    git(root, &["tag", "livekit-protocol-v0.3.1"]);
    write(root, "README.md", "x");
    git(root, &["add", "-A"]);
    git(root, &["commit", "-qm", "later"]);
    let repo = Repo::load(root).unwrap();
    let calls = std::cell::RefCell::new(Vec::<String>::new());
    let gh = MockClient(|args: &[&str]| -> Result<String> {
        calls.borrow_mut().push(args.join(" "));
        match args {
            ["api", "graphql", "-f", "tag=livekit-v0.7.0", ..] => Ok(String::new()),
            ["api", "graphql", ..] => Ok(r#"{"isDraft":false,"asset_count":0}"#.into()),
            _ => Ok(String::new()),
        }
    });
    let released = verwalter::release::run(&repo, "origin", &Default::default(), &gh).unwrap();
    assert_eq!(released.len(), 1);
    assert_eq!(
        (released[0].package.as_str(), released[0].status),
        ("livekit", "published")
    );
    assert!(
        calls
            .borrow()
            .iter()
            .any(|c| c.starts_with("release create livekit-v0.7.0"))
    );
    assert!(
        !calls
            .borrow()
            .iter()
            .any(|c| c.starts_with("release create livekit-api"))
    );
}

#[test]
fn release_with_assets_then_abandon() {
    let r = fixture("assets");
    let root = &r.0;
    let _remote = remote(root, "assets");
    git(root, &["tag", "livekit-api-v0.4.3"]);
    git(root, &["tag", "livekit-protocol-v0.3.1"]);
    git(root, &["push", "-q", "origin", "--tags"]);

    let repo = Repo::load(root).unwrap();
    let calls = std::cell::RefCell::new(Vec::<String>::new());
    let gh = MockClient(|args: &[&str]| -> Result<String> {
        let exists = {
            let calls = calls.borrow();
            calls.iter().any(|c| c.starts_with("release create"))
                && !calls.iter().any(|c| c.starts_with("release delete"))
        };
        calls.borrow_mut().push(args.join(" "));
        match args {
            ["api", "graphql", "-f", "tag=livekit-v0.7.0", ..] if !exists => Ok(String::new()),
            ["api", "graphql", ..] => Ok(r#"{"isDraft":false,"asset_count":0}"#.into()),
            _ => Ok(String::new()),
        }
    });
    let assets = ["dist/a.tar.gz".to_string(), "dist/b.tar.gz".to_string()];
    let released = verwalter::release::run(
        &repo,
        "origin",
        &verwalter::release::Options {
            assets: assets.to_vec(),
            ..Default::default()
        },
        &gh,
    )
    .unwrap();
    assert_eq!(released.len(), 1);
    let create = calls
        .borrow()
        .iter()
        .find(|c| c.starts_with("release create livekit-v0.7.0"))
        .cloned()
        .unwrap();
    assert!(
        create.contains("--verify-tag dist/a.tar.gz dist/b.tar.gz"),
        "{create}"
    );
    assert!(!create.contains("--draft"));

    // A failed lookup (auth, network) stops abandon before it deletes the tag.
    let broken =
        MockClient(|_: &[&str]| -> Result<String> { Err(anyhow!("HTTP 401: Bad credentials")) });
    assert!(
        verwalter::release::abandon(&repo, "origin", "livekit-v0.7.0", false, &broken).is_err()
    );
    assert!(!git(root, &["tag", "--list", "livekit-v0.7.0"]).is_empty());

    let removed =
        verwalter::release::abandon(&repo, "origin", "livekit-v0.7.0", false, &gh).unwrap();
    assert_eq!(
        removed,
        [
            "release livekit-v0.7.0",
            "origin tag livekit-v0.7.0",
            "local tag livekit-v0.7.0"
        ]
    );
    assert!(
        calls
            .borrow()
            .iter()
            .any(|c| c == "release delete livekit-v0.7.0 --yes")
    );
    assert!(!git(root, &["ls-remote", "--tags", "origin"]).contains("livekit-v0.7.0"));
    assert!(git(root, &["tag", "--list", "livekit-v0.7.0"]).is_empty());

    // Abandoning again finds nothing and does not fail.
    let again = verwalter::release::abandon(&repo, "origin", "livekit-v0.7.0", false, &gh).unwrap();
    assert!(again.is_empty());
}

#[test]
fn prerelease_opens_an_rc_pr_without_consuming_changesets() {
    let r = fixture("rc");
    let root = &r.0;
    // The current version was tagged before the changeset landed.
    git(root, &["tag", "livekit-protocol-v0.3.1"]);
    write(
        root,
        ".changeset/ndc.md",
        "---\nlivekit-protocol: minor\nlivekit: minor\n---\nx\n",
    );
    git(root, &["add", "-A"]);
    git(root, &["commit", "-q", "-m", "add changeset"]);
    let _remote = remote(root, "rc");
    git(root, &["push", "-q", "origin", "main", "--tags"]);

    let repo = Repo::load(root).unwrap();
    let calls = std::cell::RefCell::new(Vec::<String>::new());
    let gh = MockClient(|args: &[&str]| -> Result<String> {
        calls.borrow_mut().push(args.join(" "));
        Ok(match args {
            ["pr", "list", ..] => "[]".into(),
            ["pr", "create", ..] => "https://github.com/x/y/pull/9".into(),
            _ => String::new(),
        })
    });
    let (releases, pr) = verwalter::prerelease::open(&repo, "main", "origin", false, &gh).unwrap();
    // Both pending packages are in the rc, dependencies first, each at
    // the level its changeset names.
    let got: Vec<(&str, String, String)> = releases
        .iter()
        .map(|r| (r.package.as_str(), r.from.to_string(), r.to.to_string()))
        .collect();
    assert_eq!(
        got,
        [
            ("livekit-protocol", "0.3.1".into(), "0.4.0-rc.1".into()),
            ("livekit", "0.7.0".into(), "0.8.0-rc.1".into()),
        ]
    );
    assert_eq!(
        (pr.number, pr.head.as_str(), pr.base.as_str()),
        (9, "release/main-rc", "main")
    );
    assert!(calls.borrow().iter().any(|c| c.starts_with("pr create --head release/main-rc --base main --title pre-release: livekit-protocol 0.4.0-rc.1, livekit 0.8.0-rc.1")), "{calls:?}");

    // Branch content: own version and lock, changelog; no pin in api, changeset kept.
    assert!(
        git(
            root,
            &["show", "release/main-rc:livekit-protocol/Cargo.toml"]
        )
        .contains("0.4.0-rc.1")
    );
    assert!(
        git(root, &["show", "release/main-rc:Cargo.lock"])
            .contains("name = \"livekit-protocol\"\nversion = \"0.4.0-rc.1\"")
    );
    assert!(
        git(
            root,
            &["show", "release/main-rc:livekit-protocol/CHANGELOG.md"]
        )
        .contains("## 0.4.0-rc.1")
    );
    assert!(
        git(root, &["show", "release/main-rc:livekit-api/Cargo.toml"]).contains("workspace = true")
    );
    git(root, &["show", "release/main-rc:.changeset/ndc.md"]);
    assert!(git(root, &["ls-remote", "--heads", "origin"]).contains("release/main-rc"));

    // After rc.1 is merged and tagged, the next request gives rc.2 and
    // the automatic plan promotes to the final version.
    git(
        root,
        &["tag", "livekit-protocol-v0.4.0-rc.1", "release/main-rc"],
    );
    git(root, &["tag", "livekit-v0.8.0-rc.1", "release/main-rc"]);
    git(root, &["merge", "-q", "--no-edit", "release/main-rc"]);
    let repo = Repo::load(root).unwrap();
    let next = verwalter::prerelease::plan(&repo).unwrap();
    assert_eq!(next[0].to.to_string(), "0.4.0-rc.2");
    let p = plan(
        &repo.packages,
        &repo.changesets,
        repo.config.versioning.dependent_bump_level,
    );
    assert_eq!(
        p.iter()
            .find(|r| r.package == "livekit-protocol")
            .unwrap()
            .to
            .to_string(),
        "0.4.0"
    );
}

// Spec: config.fixed in a pre-release. An rc of one member carries the
// whole group at the shared version.
#[test]
fn prerelease_of_a_fixed_group_member_covers_the_group() {
    let r = fixture("rc-fixed");
    let root = &r.0;
    let cfg = fs::read_to_string(root.join("verwalter.toml")).unwrap();
    write(
        root,
        "verwalter.toml",
        &format!("{cfg}\n[versioning]\nfixed = [[\"livekit-protocol\", \"livekit-api\"]]\n"),
    );
    write(
        root,
        ".changeset/ndc.md",
        "---\nlivekit-protocol: minor\n---\nx\n",
    );
    git(root, &["add", "-A"]);
    git(root, &["commit", "-q", "-m", "fixed group"]);
    let _remote = remote(root, "rc-fixed");
    git(root, &["push", "-q", "origin", "main"]);

    let repo = Repo::load(root).unwrap();
    assert_eq!(
        verwalter::plan::fixed_group_drift(&repo.packages).len(),
        1,
        "0.3.1 and 0.4.3 differ"
    );
    let gh = MockClient(|args: &[&str]| -> Result<String> {
        Ok(match args {
            ["pr", "list", ..] => "[]".into(),
            ["pr", "create", ..] => "https://github.com/x/y/pull/9".into(),
            _ => String::new(),
        })
    });
    let (releases, pr) = verwalter::prerelease::open(&repo, "main", "origin", false, &gh).unwrap();
    // api was asked for, protocol comes along; the version moves from the
    // group's highest (0.4.3) by the level of protocol's changeset.
    let got: Vec<(&str, &str, &str)> = releases
        .iter()
        .map(|r| {
            (
                r.package.as_str(),
                r.from.to_string().leak() as &str,
                r.to.to_string().leak() as &str,
            )
        })
        .collect();
    assert_eq!(
        got,
        [
            // Dependencies first, like a release plan: api depends on protocol.
            ("livekit-protocol", "0.3.1", "0.5.0-rc.1"),
            ("livekit-api", "0.4.3", "0.5.0-rc.1")
        ]
    );
    assert_eq!(pr.head, "release/main-rc");
    assert!(
        git(
            root,
            &["show", "release/main-rc:livekit-protocol/Cargo.toml"]
        )
        .contains("0.5.0-rc.1")
    );
    assert!(
        git(root, &["show", "release/main-rc:livekit-api/CHANGELOG.md"])
            .contains("- Updated dependencies: livekit-protocol@0.5.0-rc.1"),
        "api depends on protocol, so the dependency line wins over the group line, as in a release plan"
    );

    // The final release plan aligns the group the same way.
    let plan = plan(
        &repo.packages,
        &repo.changesets,
        repo.config.versioning.dependent_bump_level,
    );
    let got: Vec<(&str, &str)> = plan
        .iter()
        .map(|r| (r.package.as_str(), r.to.to_string().leak() as &str))
        .collect();
    assert_eq!(
        got,
        [
            ("livekit-protocol", "0.5.0"),
            ("livekit", "0.7.1"),
            ("livekit-api", "0.5.0")
        ],
        "livekit depends on protocol and gets the dependent patch"
    );
}

#[test]
fn dry_run_writes_nothing() {
    let r = fixture("dry");
    let root = &r.0;
    write(
        root,
        ".changeset/ndc.md",
        "---\nlivekit-protocol: minor\n---\nx\n",
    );
    git(root, &["add", "-A"]);
    git(root, &["commit", "-q", "-m", "add changeset"]);
    let _remote = remote(root, "dry");
    git(root, &["push", "-q", "origin", "main"]);

    let repo = Repo::load(root).unwrap();
    let plan = plan(
        &repo.packages,
        &repo.changesets,
        repo.config.versioning.dependent_bump_level,
    );
    let branch = branches::create(&repo, &plan, "main").unwrap();
    let writes = std::cell::Cell::new(0);
    let inner = MockClient(|args: &[&str]| -> Result<String> {
        if !matches!(args, ["pr", "list", ..] | ["api", "graphql", ..]) {
            writes.set(writes.get() + 1);
        }
        Ok(match args {
            ["pr", "list", ..] => "[]".into(),
            ["api", "graphql", ..] => String::new(),
            _ => String::new(),
        })
    });
    let gh = verwalter::github::DryRunClient::new(inner);
    let pr = verwalter::github::sync(&repo, &plan, branch.as_deref(), "origin", true, &gh).unwrap();
    assert!(pr.is_some());
    let released = verwalter::release::run(
        &repo,
        "origin",
        &verwalter::release::Options {
            dry_run: true,
            ..Default::default()
        },
        &gh,
    )
    .unwrap();
    assert_eq!(released.len(), 3, "reported as if tagged and created");
    assert_eq!(writes.get(), 0, "no gh write reached the inner runner");
    assert!(
        git(root, &["ls-remote", "--heads", "origin"])
            .lines()
            .all(|l| l.ends_with("refs/heads/main")),
        "nothing pushed"
    );
    assert!(git(root, &["tag", "--list"]).is_empty(), "no tag created");
}

#[test]
fn release_finishes_a_partial_release_for_one_package() {
    let r = fixture("partial");
    let root = &r.0;
    let _remote = remote(root, "partial");
    // livekit: tagged, draft release without assets (the upload failed).
    // livekit-api: not tagged at all, but out of scope via --package.
    git(root, &["tag", "livekit-v0.7.0"]);
    git(root, &["tag", "livekit-protocol-v0.3.1"]);
    git(root, &["push", "-q", "origin", "--tags"]);

    let repo = Repo::load(root).unwrap();
    let calls = std::cell::RefCell::new(Vec::<String>::new());
    let gh = MockClient(|args: &[&str]| -> Result<String> {
        let published = calls
            .borrow()
            .iter()
            .any(|c| c.starts_with("release edit livekit-v0.7.0 --draft=false"));
        calls.borrow_mut().push(args.join(" "));
        match args {
            ["api", "graphql", "-f", "tag=livekit-v0.7.0", ..] => {
                Ok(format!(r#"{{"isDraft":{},"asset_count":0}}"#, !published))
            }
            ["api", "graphql", ..] => Ok(r#"{"isDraft":false,"asset_count":1}"#.into()),
            _ => Ok(String::new()),
        }
    });
    let options = verwalter::release::Options {
        assets: vec!["dist/a.tar.gz".into()],
        package: Some("livekit".into()),
        publish: true,
        ..Default::default()
    };
    let released = verwalter::release::run(&repo, "origin", &options, &gh).unwrap();
    assert_eq!(released.len(), 1);
    assert_eq!(
        (released[0].package.as_str(), released[0].status),
        ("livekit", "published")
    );
    let calls = calls.borrow();
    assert!(
        calls
            .iter()
            .any(|c| c == "release upload livekit-v0.7.0 --clobber dist/a.tar.gz"),
        "{calls:?}"
    );
    assert!(
        calls
            .iter()
            .any(|c| c == "release edit livekit-v0.7.0 --draft=false")
    );
    assert!(
        !calls.iter().any(|c| c.contains("livekit-api")),
        "other packages untouched"
    );
    assert!(
        git(root, &["tag", "--list", "livekit-api-v0.4.3"]).is_empty(),
        "api not tagged"
    );
}

// Spec: AssetsComeFromTheTaggedSource. A recovery run from a commit after
// the release must not attach binaries of newer source to the tagged
// version. A release without assets is still created from any commit.
#[test]
fn release_refuses_assets_when_the_source_changed_since_the_tag() {
    let r = fixture("assets-changed");
    let root = &r.0;
    let _remote = remote(root, "assets-changed");
    git(root, &["tag", "livekit-v0.7.0"]);
    git(root, &["tag", "livekit-api-v0.4.3"]);
    git(root, &["tag", "livekit-protocol-v0.3.1"]);
    git(root, &["push", "-q", "origin", "--tags"]);
    // After the release: livekit changes.
    write(root, "livekit/src/lib.rs", "pub fn newer() {}\n");
    git(root, &["add", "-A"]);
    git(root, &["commit", "-q", "-m", "after the release"]);

    let repo = Repo::load(root).unwrap();
    // The fake GitHub has a published release for the tag, or none.
    let has_release = std::cell::Cell::new(false);
    let calls = std::cell::RefCell::new(Vec::<String>::new());
    let gh = MockClient(|args: &[&str]| -> Result<String> {
        calls.borrow_mut().push(args.join(" "));
        match args {
            ["api", "graphql", ..] if has_release.get() => {
                Ok(r#"{"isDraft":false,"asset_count":0}"#.into())
            }
            _ => Ok(String::new()),
        }
    });
    let wrote = |what: &str| calls.borrow().iter().any(|c| c.starts_with(what));
    let mut options = verwalter::release::Options {
        assets: vec!["dist/a.tar.gz".into()],
        package: Some("livekit".into()),
        ..Default::default()
    };

    // No GitHub release yet: it is not created with the assets.
    let err = verwalter::release::run(&repo, "origin", &options, &gh)
        .unwrap_err()
        .to_string();
    assert!(err.contains("ref=livekit-v0.7.0"), "{err}");
    assert!(err.contains("resume with: verwalter release"), "{err}");
    assert!(!wrote("release create"), "{calls:?}");

    // The release exists: the assets are not uploaded to it.
    has_release.set(true);
    let err = verwalter::release::run(&repo, "origin", &options, &gh)
        .unwrap_err()
        .to_string();
    assert!(err.contains("ref=livekit-v0.7.0"), "{err}");
    assert!(!wrote("release upload"), "{calls:?}");

    // The flag turns the check off.
    options.allow_changed_source = true;
    verwalter::release::run(&repo, "origin", &options, &gh).unwrap();
    assert!(wrote("release upload livekit-v0.7.0"), "{calls:?}");

    // Without assets, the release is created for the old tag.
    has_release.set(false);
    let options = verwalter::release::Options {
        package: Some("livekit".into()),
        ..Default::default()
    };
    let released = verwalter::release::run(&repo, "origin", &options, &gh).unwrap();
    assert_eq!(
        (released[0].package.as_str(), released[0].status),
        ("livekit", "published")
    );
    assert!(wrote("release create livekit-v0.7.0"), "{calls:?}");
}

// Spec: RequiredAssetsBeforePublic. A draft is not published while a
// name in `release.required_assets` is missing on it.
#[test]
fn release_publishes_a_draft_only_with_its_required_assets() {
    let r = fixture("required-assets");
    let root = &r.0;
    let _remote = remote(root, "required-assets");
    git(root, &["tag", "livekit-v0.7.0"]);
    git(root, &["push", "-q", "origin", "--tags"]);
    // The last table of the fixture config is livekit.
    let base = fs::read_to_string(root.join("verwalter.toml")).unwrap();
    write(
        root,
        "verwalter.toml",
        &format!("{base}release.required_assets = [\"notes.txt\", \"ffi-{{version}}.zip\"]\n"),
    );
    let repo = Repo::load(root).unwrap();

    // The draft has notes.txt only.
    let calls = std::cell::RefCell::new(Vec::<String>::new());
    let gh = MockClient(|args: &[&str]| -> Result<String> {
        calls.borrow_mut().push(args.join(" "));
        match args {
            ["api", "graphql", ..] => {
                Ok(r#"{"isDraft":true,"asset_count":1,"assets":["notes.txt"]}"#.into())
            }
            _ => Ok(String::new()),
        }
    });
    let mut options = verwalter::release::Options {
        package: Some("livekit".into()),
        publish: true,
        ..Default::default()
    };
    let err = verwalter::release::run(&repo, "origin", &options, &gh)
        .unwrap_err()
        .to_string();
    assert!(
        err.contains("lacks required assets: ffi-0.7.0.zip. It stays a draft"),
        "{err}"
    );
    assert!(
        err.contains("resume with: verwalter release --package livekit --publish"),
        "{err}"
    );
    assert!(!calls.borrow().iter().any(|c| c.starts_with("release edit")));

    // The same run with the missing file uploads it and publishes.
    options.assets = vec!["dist/ffi-0.7.0.zip".into()];
    let released = verwalter::release::run(&repo, "origin", &options, &gh).unwrap();
    assert_eq!(released[0].status, "published");
    assert!(
        calls
            .borrow()
            .contains(&"release edit livekit-v0.7.0 --draft=false".to_string())
    );
}

// Spec: RequiredAssetsBeforePublic. A release that is created public has
// only the assets of the run that creates it.
#[test]
fn release_is_not_created_public_without_its_required_assets() {
    let r = fixture("required-assets-create");
    let root = &r.0;
    let _remote = remote(root, "required-assets-create");
    let base = fs::read_to_string(root.join("verwalter.toml")).unwrap();
    write(
        root,
        "verwalter.toml",
        &format!("{base}release.required_assets = [\"ffi.zip\"]\n"),
    );
    let repo = Repo::load(root).unwrap();
    let calls = std::cell::RefCell::new(Vec::<String>::new());
    // No tag has a release yet.
    let gh = MockClient(|args: &[&str]| -> Result<String> {
        calls.borrow_mut().push(args.join(" "));
        Ok(String::new())
    });
    let mut options = verwalter::release::Options {
        package: Some("livekit".into()),
        ..Default::default()
    };
    let err = verwalter::release::run(&repo, "origin", &options, &gh)
        .unwrap_err()
        .to_string();
    assert!(
        err.contains("required assets are not among the --asset files: ffi.zip"),
        "{err}"
    );
    assert!(
        !calls
            .borrow()
            .iter()
            .any(|c| c.starts_with("release create"))
    );

    options.assets = vec!["dist/ffi.zip".into()];
    let released = verwalter::release::run(&repo, "origin", &options, &gh).unwrap();
    assert_eq!(released[0].status, "published");
}

#[test]
fn release_error_names_the_resume_command() {
    let r = fixture("resume-hint");
    let root = &r.0;
    // No remote: the tag push fails.
    let repo = Repo::load(root).unwrap();
    let gh = MockClient(|_: &[&str]| -> Result<String> { Ok(String::new()) });
    let err = verwalter::release::run(
        &repo,
        "origin",
        &verwalter::release::Options {
            assets: vec!["dist/x".into()],
            ..Default::default()
        },
        &gh,
    )
    .unwrap_err()
    .to_string();
    assert!(
        err.contains("resume with: verwalter release --package livekit-protocol --asset dist/x"),
        "{err}"
    );
}

#[test]
fn status_reports_state_and_next_step() {
    let r = fixture("status");
    let root = &r.0;
    let _remote = remote(root, "status");
    // protocol: complete. api: tagged locally only, draft without assets.
    // livekit: version committed, no tag; a changeset is pending.
    git(root, &["tag", "livekit-protocol-v0.3.1"]);
    git(root, &["push", "-q", "origin", "livekit-protocol-v0.3.1"]);
    git(root, &["tag", "livekit-api-v0.4.3"]);
    write(root, ".changeset/x.md", "---\nlivekit: patch\n---\nx\n");
    git(root, &["add", "-A"]);
    git(root, &["commit", "-q", "-m", "cs"]);

    let repo = Repo::load(root).unwrap();
    let gh = MockClient(|args: &[&str]| -> Result<String> {
        match args {
            ["pr", "list", ..] => {
                Ok(r#"[{"number":12,"headRefName":"release/main","baseRefName":"main"}]"#.into())
            }
            ["api", "graphql", "-f", "tag=livekit-protocol-v0.3.1", ..] => {
                Ok(r#"{"isDraft":false,"asset_count":2}"#.into())
            }
            ["api", "graphql", "-f", "tag=livekit-api-v0.4.3", ..] => {
                Ok(r#"{"isDraft":true,"asset_count":0}"#.into())
            }
            _ => Err(anyhow!("not found")),
        }
    });
    let rows = verwalter::status::status(&repo, "origin", &gh).unwrap();
    let by = |n: &str| rows.iter().find(|r| r.package == n).unwrap();

    let protocol = by("livekit-protocol");
    assert_eq!(
        (
            protocol.tag_state,
            protocol.release.as_str(),
            protocol.assets
        ),
        ("both", "published", 2)
    );
    assert_eq!(protocol.next_step, "up to date");

    let api = by("livekit-api");
    assert_eq!((api.tag_state, api.release.as_str()), ("local", "draft"));
    assert!(
        api.next_step
            .starts_with("tag livekit-api-v0.4.3 is not on origin"),
        "{}",
        api.next_step
    );

    let lk = by("livekit");
    assert_eq!(lk.tag_state, "missing");
    assert_eq!(lk.pending_changesets, ["x.md"]);
    assert_eq!(lk.release_pr, Some(12));
    assert_eq!(lk.next_version.as_deref(), Some("0.7.1"));
    assert!(
        lk.next_step.contains("verwalter release --package livekit"),
        "untagged version comes first: {}",
        lk.next_step
    );
}

// Spec: Release.is_complete. `status --check` fails for a row with
// `incomplete` set.
#[test]
fn status_check_finds_the_incomplete_releases() {
    let r = fixture("status-check");
    let root = &r.0;
    let _remote = remote(root, "status-check");
    git(root, &["tag", "livekit-protocol-v0.3.1"]);
    git(root, &["tag", "livekit-api-v0.4.3"]);
    git(root, &["tag", "livekit-v0.7.0"]);
    git(root, &["push", "-q", "origin", "--tags"]);
    // The last table of the fixture config is livekit.
    let base = fs::read_to_string(root.join("verwalter.toml")).unwrap();
    write(
        root,
        "verwalter.toml",
        &format!("{base}release.required_assets = [\"a.zip\", \"b.zip\"]\n"),
    );

    // protocol: public. api: a draft. livekit: public, without b.zip.
    let gh = MockClient(|args: &[&str]| -> Result<String> {
        match args {
            ["pr", "list", ..] => Ok("[]".into()),
            ["api", "graphql", "-f", "tag=livekit-api-v0.4.3", ..] => {
                Ok(r#"{"isDraft":true,"asset_count":0,"assets":[]}"#.into())
            }
            ["api", "graphql", "-f", "tag=livekit-v0.7.0", ..] => {
                Ok(r#"{"isDraft":false,"asset_count":1,"assets":["a.zip"]}"#.into())
            }
            ["api", "graphql", ..] => Ok(r#"{"isDraft":false,"asset_count":0,"assets":[]}"#.into()),
            _ => Err(anyhow!("not found")),
        }
    });
    let repo = Repo::load(root).unwrap();
    let rows = verwalter::status::status(&repo, "origin", &gh).unwrap();
    let by = |n: &str| rows.iter().find(|r| r.package == n).unwrap();

    assert_eq!(by("livekit-protocol").incomplete, None);
    assert_eq!(
        by("livekit-api").incomplete.as_deref(),
        Some(
            format!(
                "the GitHub release is a draft -> {}",
                by("livekit-api").next_step
            )
            .as_str()
        )
    );
    let livekit = by("livekit").incomplete.clone().unwrap();
    assert!(
        livekit.starts_with("required assets are missing: b.zip -> "),
        "{livekit}"
    );
    // `status --json` prints these rows.
    let json = serde_json::to_value(&rows).unwrap();
    let row = json
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["package"] == "livekit")
        .unwrap();
    assert_eq!(row["version"], "0.7.0");
    assert_eq!(row["tag_state"], "both");
    assert_eq!(row["release"], "published");
    assert_eq!(row["missing_assets"], serde_json::json!(["b.zip"]));
    assert_eq!(row["incomplete"], livekit.as_str());

    // A version bumped by hand has no tag and no changelog entry: it is
    // no release, so nothing is missing.
    write(
        root,
        "livekit-protocol/Cargo.toml",
        "[package]\nname = \"livekit-protocol\"\nversion = \"0.3.2\"\n",
    );
    let repo = Repo::load(root).unwrap();
    let rows = verwalter::status::status(&repo, "origin", &gh).unwrap();
    let protocol = rows
        .iter()
        .find(|r| r.package == "livekit-protocol")
        .unwrap();
    assert_eq!(protocol.tag_state, "missing");
    assert_eq!(protocol.incomplete, None);
}

#[test]
fn release_runs_in_dependency_order_and_stops_at_the_first_failure() {
    let r = fixture("order");
    let root = &r.0;
    let _remote = remote(root, "order");

    // All three untagged: protocol first, then its dependents by name.
    let repo = Repo::load(root).unwrap();
    let calls = std::cell::RefCell::new(Vec::<String>::new());
    let gh = MockClient(|args: &[&str]| -> Result<String> {
        calls.borrow_mut().push(args.join(" "));
        match args {
            ["api", "graphql", ..] => Ok(String::new()),
            _ => Ok(String::new()),
        }
    });
    let released = verwalter::release::run(&repo, "origin", &Default::default(), &gh).unwrap();
    let order: Vec<&str> = released.iter().map(|r| r.package.as_str()).collect();
    assert_eq!(order, ["livekit-protocol", "livekit", "livekit-api"]);
    assert!(released.iter().all(|r| r.first_release), "no earlier tags");

    // Fail at the dependency: nothing happens to the dependents.
    let r2 = fixture("order-fail");
    let root2 = &r2.0;
    let repo2 = Repo::load(root2).unwrap();
    let gh2 = MockClient(|args: &[&str]| -> Result<String> {
        match args {
            ["api", "graphql", ..] => Ok(String::new()),
            ["release", "create", tag, ..] if tag.starts_with("livekit-protocol") => {
                Err(anyhow!("boom"))
            }
            _ => Ok(String::new()),
        }
    });
    // Give it a remote so the tag push works and the gh failure is the first error.
    let _remote2 = remote(root2, "order-fail");
    let err = verwalter::release::run(&repo2, "origin", &Default::default(), &gh2)
        .unwrap_err()
        .to_string();
    assert!(err.starts_with("livekit-protocol: "), "{err}");
    assert!(
        git(root2, &["tag", "--list", "livekit-protocol-*"]).contains("livekit-protocol-v0.3.1"),
        "the dependency was tagged before its release failed"
    );
    assert!(
        git(root2, &["tag", "--list", "livekit-api-*"]).is_empty(),
        "dependents untouched"
    );
    assert!(git(root2, &["tag", "--list", "livekit-v*"]).is_empty());
}

// Spec: ReleaseNeedsChangelogEntry. A hand-edited version has no entry
// and no tag; the run leaves it alone unless the package is named.
#[test]
fn hand_bumped_version_is_not_released() {
    let r = fixture("hand-bump");
    let root = &r.0;
    let _remote = remote(root, "hand-bump");
    git(root, &["tag", "livekit-protocol-v0.3.1"]);
    git(root, &["tag", "livekit-api-v0.4.3"]);
    git(root, &["tag", "livekit-v0.7.0"]);
    write(
        root,
        "livekit/Cargo.toml",
        "[package]\nname = \"livekit\"\nversion = \"0.7.1\"\n\n[dependencies]\nlivekit-protocol = { workspace = true }\n",
    );
    git(root, &["commit", "-q", "-am", "bump by hand"]);
    let repo = Repo::load(root).unwrap();
    let gh = MockClient(|args: &[&str]| -> Result<String> {
        match args {
            ["api", "graphql", "-f", "tag=livekit-v0.7.1", ..] => Ok(String::new()),
            ["api", "graphql", ..] => Ok(r#"{"isDraft":false,"asset_count":0}"#.into()),
            _ => Ok(String::new()),
        }
    });
    let released = verwalter::release::run(&repo, "origin", &Default::default(), &gh).unwrap();
    assert!(released.is_empty(), "{released:?}");
    assert!(git(root, &["tag", "--list", "livekit-v0.7.1"]).is_empty());

    let released = verwalter::release::run(
        &repo,
        "origin",
        &verwalter::release::Options {
            package: Some("livekit".into()),
            ..Default::default()
        },
        &gh,
    )
    .unwrap();
    assert_eq!(released.len(), 1);
    assert!(!released[0].first_release);
    assert!(!git(root, &["tag", "--list", "livekit-v0.7.1"]).is_empty());
}

// Spec: VersionedFilesAgree. The first file wins, the rest is a warning.
#[test]
fn versioned_files_that_disagree_are_reported() {
    let r = fixture("drift");
    let root = &r.0;
    let cfg = fs::read_to_string(root.join("verwalter.toml")).unwrap();
    write(
        root,
        "verwalter.toml",
        &cfg.replace(
            "path = \"livekit\"\nversioning.files = [\"Cargo.toml\"]",
            "path = \"livekit\"\nversioning.files = [\"Cargo.toml\", { path = \"README.md\", regex = 'livekit = \"(?<version>[^\"]+)\"' }]",
        ),
    );
    write(root, "livekit/README.md", "livekit = \"0.6.9\"\n");
    let repo = Repo::load(root).unwrap();
    assert_eq!(
        repo.packages
            .iter()
            .find(|p| p.name == "livekit")
            .unwrap()
            .version
            .to_string(),
        "0.7.0"
    );
    assert_eq!(repo.warnings.len(), 1);
    assert!(
        repo.warnings[0].contains("livekit/README.md says 0.6.9"),
        "{}",
        repo.warnings[0]
    );
}

// Spec: ContributorWritesChangeset.
#[test]
fn changeset_command_covers_the_changed_packages() {
    let r = fixture("changeset-cmd");
    let root = &r.0;
    git(root, &["checkout", "-q", "-b", "feat"]);
    write(root, "livekit-api/src.rs", "x");
    write(root, "livekit-protocol/x.proto", "y");
    git(root, &["add", "-A"]);
    git(root, &["commit", "-q", "-m", "Add thing: v2"]);
    write(root, "livekit/untracked.rs", "z");
    let repo = Repo::load(root).unwrap();
    let path = verwalter::check::write_changeset(
        &repo,
        "main",
        verwalter::version::BumpLevel::Minor,
        None,
        &[],
    )
    .unwrap()
    .unwrap();
    assert!(
        path.ends_with(".changeset/add_thing_v2.md"),
        "{}",
        path.display()
    );
    let content = fs::read_to_string(&path).unwrap();
    assert_eq!(
        content,
        "---\nlivekit: minor\nlivekit-api: minor\nlivekit-protocol: minor\n---\n\nAdd thing: v2\n"
    );
    // Now covered: nothing to write. A second summary gets its own file.
    let repo = Repo::load(root).unwrap();
    assert!(
        verwalter::check::write_changeset(
            &repo,
            "main",
            verwalter::version::BumpLevel::Patch,
            None,
            &[]
        )
        .unwrap()
        .is_none()
    );
    // Named packages win over the detection and over coverage.
    let named = verwalter::check::write_changeset(
        &repo,
        "main",
        verwalter::version::BumpLevel::Major,
        Some("Only api"),
        &["livekit-api".to_string()],
    )
    .unwrap()
    .unwrap();
    assert_eq!(
        fs::read_to_string(&named).unwrap(),
        "---\nlivekit-api: major\n---\n\nOnly api\n"
    );
    fs::remove_file(&named).unwrap();
    assert!(
        verwalter::check::write_changeset(
            &repo,
            "main",
            verwalter::version::BumpLevel::Patch,
            None,
            &["nope".to_string()]
        )
        .is_err()
    );
    write(root, "livekit/more.rs", "w");
    fs::remove_file(&path).unwrap();
    let repo = Repo::load(root).unwrap();
    let again = verwalter::check::write_changeset(
        &repo,
        "main",
        verwalter::version::BumpLevel::Patch,
        Some("Fix: it"),
        &[],
    )
    .unwrap()
    .unwrap();
    assert!(again.ends_with("fix_it.md"));
    assert!(
        fs::read_to_string(&again)
            .unwrap()
            .starts_with("---\nlivekit: patch\nlivekit-api: patch\nlivekit-protocol: patch\n---")
    );
}

// Spec: WorkflowPublishesInDependencyOrder. protocol first; api and
// livekit depend on it and run after, in parallel; a failure blocks only
// the dependents; a version the registry has is skipped.
#[test]
fn publish_runs_dependencies_first_and_blocks_dependents_of_a_failure() {
    // No GitHub release behind the tags: nothing to flip.
    let no_gh = MockClient(|_: &[&str]| -> Result<String> { Ok(String::new()) });
    let r = fixture("publish");
    let root = &r.0;
    git(root, &["tag", "livekit-protocol-v0.3.1"]);
    git(root, &["tag", "livekit-api-v0.4.3"]);
    git(root, &["tag", "livekit-v0.7.0"]);
    let base = fs::read_to_string(root.join("verwalter.toml")).unwrap();
    // The command runs in the package's path, so the order file is shared
    // through VERWALTER_ROOT. `[publish].publish` is the default for all.
    let with_command = |command: &str| {
        write(
            root,
            "verwalter.toml",
            &format!("{base}\n[publish]\ncommand = '{command}'\n"),
        );
        Repo::load(root).unwrap()
    };
    let no_registry = |_: &str| -> Result<u16> { Err(anyhow!("no registry configured")) };
    let opts = verwalter::publish::Options {
        jobs: Some(4),
        dry_run: false,
        allow_changed_source: false,
        only: None,
        wait: std::time::Duration::ZERO,
    };

    // A pnpm workspace above the package, not at the root, makes the npm
    // default `pnpm publish`: a dry run names the command it would run.
    write(
        root,
        "verwalter.toml",
        &format!("{base}\n[publish]\nregistry = \"npm\"\n"),
    );
    write(root, "livekit-api/pnpm-workspace.yaml", "packages: []\n");
    let dry = verwalter::publish::Options {
        jobs: Some(1),
        dry_run: true,
        allow_changed_source: false,
        only: Some(["livekit-api".to_string()].into_iter().collect()),
        wait: std::time::Duration::ZERO,
    };
    let missing = |_: &str| -> Result<u16> { Ok(404) };
    let out = verwalter::publish::run(&Repo::load(root).unwrap(), &dry, &missing, &no_gh).unwrap();
    assert!(out[0].reason.contains("pnpm publish"), "{out:?}");
    let dry = verwalter::publish::Options {
        only: Some(["livekit".to_string()].into_iter().collect()),
        ..dry
    };
    let out = verwalter::publish::run(&Repo::load(root).unwrap(), &dry, &missing, &no_gh).unwrap();
    assert!(out[0].reason.contains("npm publish"), "{out:?}");
    fs::remove_file(root.join("livekit-api/pnpm-workspace.yaml")).unwrap();
    write(root, "verwalter.toml", &base);

    // `registry = "none"` on the package, no command: nothing to publish,
    // left out without an error. The default none still errors below.
    write(
        root,
        "verwalter.toml",
        &base.replace(
            "path = \"livekit\"\nversioning.files = [\"Cargo.toml\"]\nversioning.changelog = \"CHANGELOG.md\"",
            "path = \"livekit\"\nversioning.files = [\"Cargo.toml\"]\nversioning.changelog = \"CHANGELOG.md\"\npublish.registry = \"none\"",
        ),
    );
    let github_only = verwalter::publish::Options {
        jobs: Some(1),
        dry_run: true,
        allow_changed_source: false,
        only: Some(["livekit".to_string()].into_iter().collect()),
        wait: std::time::Duration::ZERO,
    };
    assert!(
        verwalter::publish::run(
            &Repo::load(root).unwrap(),
            &github_only,
            &no_registry,
            &no_gh
        )
        .unwrap()
        .is_empty()
    );
    write(root, "verwalter.toml", &base);

    // No command at all: an error before anything runs.
    let err = verwalter::publish::run(&Repo::load(root).unwrap(), &opts, &no_registry, &no_gh)
        .expect_err("no command");
    assert!(err.to_string().contains("no publish command"), "{err}");

    // Everything succeeds: the order file shows protocol before the others.
    let repo = with_command(
        "echo $VERWALTER_PACKAGE $VERWALTER_VERSION $VERWALTER_TAG $VERWALTER_REGISTRY first=$VERWALTER_FIRST_RELEASE npm=$VERWALTER_NPM_TAG {npm_tag} $(pwd | xargs basename) >> $VERWALTER_ROOT/order",
    );
    let outcomes = verwalter::publish::run(&repo, &opts, &no_registry, &no_gh).unwrap();
    assert!(
        outcomes.iter().all(|o| o.status == "published"),
        "{outcomes:?}"
    );
    let order = fs::read_to_string(root.join("order")).unwrap();
    let lines: Vec<&str> = order.lines().collect();
    assert_eq!(lines.len(), 3);
    assert_eq!(
        lines[0],
        "livekit-protocol 0.3.1 livekit-protocol-v0.3.1 none first=true npm=latest latest livekit-protocol",
        "{order}"
    );
    fs::remove_file(root.join("order")).unwrap();

    // Spec: WorkflowRunsPublish.releases. Only the packages this run
    // released are run; a tagged dependency outside the list is not.
    let only = verwalter::publish::Options {
        jobs: Some(4),
        dry_run: false,
        allow_changed_source: false,
        only: Some(["livekit-api".to_string()].into_iter().collect()),
        wait: std::time::Duration::ZERO,
    };
    let outcomes = verwalter::publish::run(&repo, &only, &no_registry, &no_gh).unwrap();
    assert_eq!(
        outcomes
            .iter()
            .map(|o| o.package.as_str())
            .collect::<Vec<_>>(),
        ["livekit-api"]
    );
    assert_eq!(
        fs::read_to_string(root.join("order"))
            .unwrap()
            .lines()
            .count(),
        1
    );
    fs::remove_file(root.join("order")).unwrap();

    // protocol fails: both dependents are blocked, nothing else ran.
    let repo =
        with_command("echo {package} >> $VERWALTER_ROOT/order; [ {package} != livekit-protocol ]");
    let outcomes = verwalter::publish::run(&repo, &opts, &no_registry, &no_gh).unwrap();
    let by: std::collections::BTreeMap<&str, (&str, &str)> = outcomes
        .iter()
        .map(|o| (o.package.as_str(), (o.status, o.reason.as_str())))
        .collect();
    assert_eq!(by["livekit-protocol"].0, "failed");
    assert_eq!(
        by["livekit-api"],
        ("blocked", "livekit-protocol did not publish")
    );
    assert_eq!(
        by["livekit"],
        ("blocked", "livekit-protocol did not publish")
    );
    assert_eq!(
        fs::read_to_string(root.join("order")).unwrap().trim(),
        "livekit-protocol"
    );
    assert!(verwalter::publish::failed(&outcomes));
    fs::remove_file(root.join("order")).unwrap();

    // api fails: livekit does not depend on it and still publishes.
    let repo = with_command("[ {package} != livekit-api ]");
    let outcomes = verwalter::publish::run(
        &repo,
        &verwalter::publish::Options {
            jobs: Some(1),
            dry_run: false,
            allow_changed_source: false,
            only: None,
            wait: std::time::Duration::ZERO,
        },
        &no_registry,
        &no_gh,
    )
    .unwrap();
    let by: std::collections::BTreeMap<&str, &str> = outcomes
        .iter()
        .map(|o| (o.package.as_str(), o.status))
        .collect();
    assert_eq!(by["livekit-protocol"], "published");
    assert_eq!(by["livekit-api"], "failed");
    assert_eq!(by["livekit"], "published");

    // protocol on crates with the package's own command; the registry has
    // it already, so it is skipped and the dependents still run. A 503 is
    // an error, not "missing".
    write(
        root,
        "verwalter.toml",
        &format!("{}\n[publish]\ncommand = 'echo {{package}} >> $VERWALTER_ROOT/order'\n", base.replace(
            "path = \"livekit-protocol\"\nversioning.files = [\"Cargo.toml\"]\nversioning.changelog = \"CHANGELOG.md\"",
            "path = \"livekit-protocol\"\nversioning.files = [\"Cargo.toml\"]\nversioning.changelog = \"CHANGELOG.md\"\npublish.registry = \"crates\"\npublish.command = \"exit 7\"",
        )),
    );
    let repo = Repo::load(root).unwrap();
    assert_eq!(
        repo.config
            .publish_command("livekit-protocol", false, None)
            .as_deref(),
        Some("exit 7")
    );
    let has_protocol = |url: &str| -> Result<u16> {
        Ok(if url.contains("livekit-protocol") {
            200
        } else {
            404
        })
    };
    let outcomes = verwalter::publish::run(&repo, &opts, &has_protocol, &no_gh).unwrap();
    let by: std::collections::BTreeMap<&str, &str> = outcomes
        .iter()
        .map(|o| (o.package.as_str(), o.status))
        .collect();
    assert_eq!(by["livekit-protocol"], "exists");
    assert_eq!(by["livekit-api"], "published");
    assert!(
        !fs::read_to_string(root.join("order"))
            .unwrap()
            .contains("protocol")
    );
    let down = |_: &str| -> Result<u16> { Ok(503) };
    assert!(verwalter::publish::run(&repo, &opts, &down, &no_gh).is_err());
}

// Spec: config.publish.workspace. One command for the crates, run once
// from the root; a package with its own command stays apart and waits
// for the members it depends on.
#[test]
fn workspace_publish_command_runs_once_and_reports_per_member() {
    // No GitHub release behind the tags: nothing to flip.
    let no_gh = MockClient(|_: &[&str]| -> Result<String> { Ok(String::new()) });
    let r = fixture("publish-ws");
    let root = &r.0;
    git(root, &["tag", "livekit-protocol-v0.3.1"]);
    git(root, &["tag", "livekit-api-v0.4.3"]);
    git(root, &["tag", "livekit-v0.7.0"]);
    let base = fs::read_to_string(root.join("verwalter.toml")).unwrap();
    // livekit gets its own command; protocol and api go through the
    // workspace command.
    write(
        root,
        "verwalter.toml",
        &format!(
            "{}\n[publish]\nworkspace = 'echo ws $VERWALTER_PACKAGE $(pwd | xargs basename) >> $VERWALTER_ROOT/order'\n",
            base.replace(
                "path = \"livekit\"\nversioning.files = [\"Cargo.toml\"]\nversioning.changelog = \"CHANGELOG.md\"",
                "path = \"livekit\"\nversioning.files = [\"Cargo.toml\"]\nversioning.changelog = \"CHANGELOG.md\"\npublish.command = \"echo own {package} >> $VERWALTER_ROOT/order\""
            )
        ),
    );
    let repo = Repo::load(root).unwrap();
    assert!(repo.config.in_workspace_publish("livekit-protocol"));
    assert!(!repo.config.in_workspace_publish("livekit"));
    let no_registry = |_: &str| -> Result<u16> { Err(anyhow!("no registry")) };
    let opts = verwalter::publish::Options {
        jobs: Some(4),
        dry_run: false,
        allow_changed_source: false,
        only: None,
        wait: std::time::Duration::ZERO,
    };
    let outcomes = verwalter::publish::run(&repo, &opts, &no_registry, &no_gh).unwrap();
    let by: std::collections::BTreeMap<&str, &str> = outcomes
        .iter()
        .map(|o| (o.package.as_str(), o.status))
        .collect();
    assert_eq!(by.len(), 3, "{outcomes:?}");
    assert!(by.values().all(|s| *s == "published"), "{outcomes:?}");
    let order = fs::read_to_string(root.join("order")).unwrap();
    let dir = root.file_name().unwrap().to_str().unwrap();
    assert_eq!(
        order,
        format!("ws [workspace] {dir}\nown livekit\n"),
        "workspace once, from the root, before livekit"
    );

    // With a registry, the members are checked after the command: one
    // missing is a failure of that member only.
    write(
        root,
        "verwalter.toml",
        &format!(
            "{}\n[publish]\nregistry = \"crates\"\nworkspace = 'true'\n",
            base.replace(
                "path = \"livekit\"\nversioning.files = [\"Cargo.toml\"]\nversioning.changelog = \"CHANGELOG.md\"",
                "path = \"livekit\"\nversioning.files = [\"Cargo.toml\"]\nversioning.changelog = \"CHANGELOG.md\"\npublish.command = \"true\""
            )
        ),
    );
    let repo = Repo::load(root).unwrap();
    let only_api = |url: &str| -> Result<u16> {
        Ok(if url.contains("livekit-api") {
            200
        } else {
            404
        })
    };
    let outcomes = verwalter::publish::run(&repo, &opts, &only_api, &no_gh).unwrap();
    let by: std::collections::BTreeMap<&str, &str> = outcomes
        .iter()
        .map(|o| (o.package.as_str(), o.status))
        .collect();
    assert_eq!(by["livekit-api"], "published");
    assert_eq!(by["livekit-protocol"], "failed");
    assert_eq!(by["livekit"], "blocked", "depends on protocol");
}

// Spec: PublishWaitsForTheRegistry. npm shows a new version only after
// its scan. Until then a read answers 404, as for a version that was
// never published.
#[test]
fn publish_waits_for_the_registry_to_show_the_version() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let no_gh = MockClient(|_: &[&str]| -> Result<String> { Ok(String::new()) });
    let r = fixture("publish-wait");
    let root = &r.0;
    git(root, &["tag", "livekit-protocol-v0.3.1"]);
    let base = fs::read_to_string(root.join("verwalter.toml")).unwrap();
    write(
        root,
        "verwalter.toml",
        &format!("{base}\n[publish]\nregistry = \"npm\"\ncommand = 'true'\n"),
    );
    let repo = Repo::load(root).unwrap();
    let opts = |wait| verwalter::publish::Options {
        jobs: Some(1),
        dry_run: false,
        allow_changed_source: false,
        only: Some(["livekit-protocol".to_string()].into_iter().collect()),
        wait,
    };
    // The version URL answers 404 for the first `hidden` questions.
    let asks = AtomicUsize::new(0);
    let registry = |hidden: usize| {
        asks.store(0, Ordering::SeqCst);
        let asks = &asks;
        move |url: &str| -> Result<u16> {
            if !url.ends_with("/0.3.1") {
                return Ok(200);
            }
            Ok(if asks.fetch_add(1, Ordering::SeqCst) < hidden {
                404
            } else {
                200
            })
        }
    };

    // Hidden for the check before the command, shown right after it.
    let out = verwalter::publish::run(
        &repo,
        &opts(std::time::Duration::from_secs(60)),
        &registry(1),
        &no_gh,
    )
    .unwrap();
    assert_eq!(out[0].status, "published", "{out:?}");
    assert_eq!(asks.load(Ordering::SeqCst), 2);

    // Never shown: the package fails, and the reason says that the
    // command passed.
    let out = verwalter::publish::run(
        &repo,
        &opts(std::time::Duration::from_millis(1)),
        &registry(usize::MAX),
        &no_gh,
    )
    .unwrap();
    assert_eq!(out[0].status, "failed", "{out:?}");
    assert!(
        out[0].reason.starts_with(
            "the publish command passed, but the registry does not show livekit-protocol 0.3.1"
        ),
        "{out:?}"
    );

    // No wait: the registry is not asked after the command.
    let out = verwalter::publish::run(
        &repo,
        &opts(std::time::Duration::ZERO),
        &registry(usize::MAX),
        &no_gh,
    )
    .unwrap();
    assert_eq!(out[0].status, "published", "{out:?}");
    assert_eq!(asks.load(Ordering::SeqCst), 1);
}

// Spec: PublishBuildsTheTaggedSource. A run by hand from a commit after
// the release must not publish newer source under the tagged version.
#[test]
fn publish_fails_a_package_whose_source_changed_since_its_tag() {
    let no_gh = MockClient(|_: &[&str]| -> Result<String> { Ok(String::new()) });
    let r = fixture("publish-changed");
    let root = &r.0;
    git(root, &["tag", "livekit-protocol-v0.3.1"]);
    git(root, &["tag", "livekit-api-v0.4.3"]);
    git(root, &["tag", "livekit-v0.7.0"]);
    // After the release: api changes, and a file outside every package.
    write(root, "livekit-api/src/lib.rs", "pub fn newer() {}\n");
    write(root, "README.md", "docs\n");
    git(root, &["add", "-A"]);
    git(root, &["commit", "-q", "-m", "after the release"]);
    let base = fs::read_to_string(root.join("verwalter.toml")).unwrap();
    write(
        root,
        "verwalter.toml",
        &format!("{base}\n[publish]\nregistry = \"crates\"\ncommand = 'true'\n"),
    );
    let repo = Repo::load(root).unwrap();
    let missing = |_: &str| -> Result<u16> { Ok(404) };
    let mut opts = verwalter::publish::Options {
        jobs: Some(1),
        dry_run: false,
        only: None,
        allow_changed_source: false,
        wait: std::time::Duration::ZERO,
    };
    let status = |outcomes: &[verwalter::publish::Outcome]| {
        outcomes
            .iter()
            .map(|o| (o.package.clone(), (o.status, o.reason.clone())))
            .collect::<std::collections::BTreeMap<_, _>>()
    };

    // api fails before its command runs, and names the tag to run from.
    // The other packages publish: the README is no package's file.
    let by = status(&verwalter::publish::run(&repo, &opts, &missing, &no_gh).unwrap());
    assert_eq!(by["livekit-api"].0, "failed");
    assert!(
        by["livekit-api"].1.contains("ref=livekit-api-v0.4.3"),
        "{by:?}"
    );
    assert_eq!(by["livekit-protocol"].0, "published");
    assert_eq!(by["livekit"].0, "published");

    // A version the registry has is skipped before the check.
    let has_api = |url: &str| -> Result<u16> {
        Ok(if url.contains("livekit-api") {
            200
        } else {
            404
        })
    };
    let by = status(&verwalter::publish::run(&repo, &opts, &has_api, &no_gh).unwrap());
    assert_eq!(by["livekit-api"].0, "exists");

    // The flag turns the check off.
    opts.allow_changed_source = true;
    let by = status(&verwalter::publish::run(&repo, &opts, &missing, &no_gh).unwrap());
    assert_eq!(by["livekit-api"].0, "published");
    opts.allow_changed_source = false;

    // A workspace command covers protocol and api: one changed member
    // stops it for both. livekit depends on protocol and is blocked.
    write(
        root,
        "verwalter.toml",
        &format!(
            "{}\n[publish]\nregistry = \"crates\"\nworkspace = 'true'\n",
            base.replace(
                "path = \"livekit\"\nversioning.files = [\"Cargo.toml\"]\nversioning.changelog = \"CHANGELOG.md\"",
                "path = \"livekit\"\nversioning.files = [\"Cargo.toml\"]\nversioning.changelog = \"CHANGELOG.md\"\npublish.command = \"true\""
            )
        ),
    );
    let repo = Repo::load(root).unwrap();
    let by = status(&verwalter::publish::run(&repo, &opts, &missing, &no_gh).unwrap());
    assert_eq!(by["livekit-api"].0, "failed");
    assert!(by["livekit-api"].1.contains("ref=livekit-api-v0.4.3"));
    assert_eq!(by["livekit-protocol"].0, "failed");
    assert!(by["livekit-protocol"].1.contains("livekit-api"), "{by:?}");
    assert_eq!(by["livekit"].0, "blocked");
}

#[test]
fn named_package_is_reported_even_when_complete() {
    let r = fixture("named");
    let root = &r.0;
    let _remote = remote(root, "named");
    git(root, &["tag", "livekit-api-v0.4.3"]);
    git(root, &["push", "-q", "origin", "--tags"]);
    let repo = Repo::load(root).unwrap();
    let gh = MockClient(|args: &[&str]| -> Result<String> {
        match args {
            ["api", "graphql", ..] => Ok(r#"{"isDraft":false,"asset_count":1}"#.into()),
            other => panic!("unexpected write {other:?}"),
        }
    });
    let options = verwalter::release::Options {
        package: Some("livekit-api".into()),
        ..Default::default()
    };
    let released = verwalter::release::run(&repo, "origin", &options, &gh).unwrap();
    assert_eq!(released.len(), 1);
    assert_eq!(
        (released[0].package.as_str(), released[0].status),
        ("livekit-api", "published")
    );
}

// Spec: ChangelogUpdated, "PR link and author". The commit that added a
// changeset names the PR; without one the commit itself is linked.
#[test]
fn changesets_are_attributed_to_their_pr_or_commit() {
    let r = fixture("attribution");
    let root = &r.0;
    write(
        root,
        ".changeset/pr.md",
        "---\nlivekit-protocol: minor\n---\nFrom a PR\n",
    );
    git(root, &["add", "-A"]);
    git(root, &["commit", "-q", "-m", "from a pr"]);
    let pr_sha = git(root, &["rev-parse", "HEAD"]);
    write(
        root,
        ".changeset/push.md",
        "---\nlivekit-api: patch\n---\nPushed directly\n",
    );
    git(root, &["add", "-A"]);
    git(root, &["commit", "-q", "-m", "pushed"]);
    let push_sha = git(root, &["rev-parse", "HEAD"]);

    let mut repo = Repo::load(root).unwrap();
    let gh = MockClient(|args: &[&str]| -> Result<String> {
        let path = args.get(1).copied().unwrap_or("");
        Ok(
            if path == format!("repos/{{owner}}/{{repo}}/commits/{pr_sha}/pulls") {
                r#"{"number":12,"url":"https://github.com/o/r/pull/12","author":"lukas"}"#.into()
            } else if path == format!("repos/{{owner}}/{{repo}}/commits/{push_sha}") {
                format!(r#"{{"url":"https://github.com/o/r/commit/{push_sha}","author":null}}"#)
            } else {
                String::new()
            },
        )
    });
    verwalter::github::attribute(&mut repo, &gh);
    let by_file = |f: &str| {
        repo.changesets
            .iter()
            .find(|c| c.file == f)
            .unwrap()
            .attribution
            .clone()
            .unwrap()
    };
    assert_eq!(
        by_file("pr.md"),
        "- [#12](https://github.com/o/r/pull/12) ([@lukas](https://github.com/lukas))"
    );
    // No GitHub user behind the commit email: link only, no author.
    assert_eq!(
        by_file("push.md"),
        format!(
            "- [`{}`](https://github.com/o/r/commit/{push_sha})",
            &push_sha[..7]
        )
    );
}

// Spec: ApiCheckOnCoveredPackages. The command's answer is a change kind;
// the claimed level is a digit. livekit-api is 0.4.3, so a breaking change
// needs minor and a patch changeset is too low. livekit at 0.7.0 claims
// minor, which covers `major`. A failing command is its own finding.
#[test]
fn api_check_compares_the_claimed_level_with_the_commands_answer() {
    let repo = fixture("api-check");
    let root = &repo.0;
    let mut config = fs::read_to_string(root.join("verwalter.toml")).unwrap();
    config.push_str("\n[check]\napi = \"echo \\\"$VERWALTER_PACKAGE $VERWALTER_LEVEL $VERWALTER_BASELINE_TAG\\\" >> $VERWALTER_ROOT/calls; echo major\"\n");
    // VERWALTER_ROOT must be absolute: the command runs in the package.
    let root = &root.canonicalize().unwrap();
    config = config.replace(
        "path = \"livekit-protocol\"\nversioning.files = [\"Cargo.toml\"]\nversioning.changelog = \"CHANGELOG.md\"",
        "path = \"livekit-protocol\"\nversioning.files = [\"Cargo.toml\"]\nversioning.changelog = \"CHANGELOG.md\"\ncheck.api = \"exit 3\"",
    );
    let config = verwalter::config::Config::parse(&config).unwrap();
    write(
        root,
        ".changeset/x.md",
        "---\nlivekit-api: patch\nlivekit: minor\nlivekit-protocol: major\n---\nx\n",
    );
    let changesets = [verwalter::changeset::Changeset::parse(
        ".changeset/x.md",
        &fs::read_to_string(root.join(".changeset/x.md")).unwrap(),
    )
    .unwrap()];
    let changed: Vec<String> = [
        "livekit-api/src/lib.rs",
        "livekit/src/lib.rs",
        "livekit-protocol/x.proto",
        ".changeset/x.md",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    let mut check = verwalter::check::evaluate(&config, "feat", &[], &changed, &changesets);
    assert_eq!(check.status, verwalter::check::Status::Passed);
    verwalter::check::api_check(root, &config, &mut check, &changesets).unwrap();
    assert_eq!(check.status, verwalter::check::Status::Failed);
    let findings: Vec<(&str, Option<verwalter::version::BumpLevel>)> = check
        .api
        .iter()
        .map(|f| (f.package.as_str(), f.required))
        .collect();
    assert_eq!(
        findings,
        [
            ("livekit-api", Some(verwalter::version::BumpLevel::Minor)),
            ("livekit-protocol", None)
        ]
    );
    assert!(check.api[1].output.contains("the command failed"));
    let calls = fs::read_to_string(root.join("calls")).unwrap();
    assert_eq!(
        calls,
        "livekit minor livekit-v0.7.0\nlivekit-api patch livekit-api-v0.4.3\n"
    );

    // An exempt PR runs no command.
    fs::remove_file(root.join("calls")).unwrap();
    let mut check = verwalter::check::evaluate(
        &config,
        "feat",
        &["internal".to_string()],
        &changed,
        &changesets,
    );
    verwalter::check::api_check(root, &config, &mut check, &changesets).unwrap();
    assert!(!root.join("calls").exists());
}

// Spec: MaintainerRequestsPreRelease. An rc of a dependent carries every
// in-repo dependency with a pending changeset, at its own rc version,
// and pins the dependent to it. A dependency without a pending changeset,
// and dependents outside the rc, are left alone.
#[test]
fn prerelease_of_a_dependent_carries_pending_dependencies() {
    let r = fixture("rc-deps");
    let root = &r.0;
    git(root, &["tag", "livekit-protocol-v0.3.1"]);
    git(root, &["tag", "livekit-api-v0.4.3"]);
    // protocol and api change; livekit depends on protocol only.
    write(
        root,
        ".changeset/x.md",
        "---\nlivekit-protocol: minor\nlivekit-api: patch\n---\nx\n",
    );
    git(root, &["add", "-A"]);
    git(root, &["commit", "-q", "-m", "changesets"]);
    let _remote = remote(root, "rc-deps");
    git(root, &["push", "-q", "origin", "main", "--tags"]);
    let repo = Repo::load(root).unwrap();

    // api depends on protocol, which has a pending changeset: both go.
    let plan = verwalter::prerelease::plan(&repo).unwrap();
    let got: Vec<(&str, String, Vec<String>)> = plan
        .iter()
        .map(|p| {
            (
                p.package.as_str(),
                p.to.to_string(),
                p.bumped_dependencies.clone(),
            )
        })
        .collect();
    assert_eq!(
        got,
        [
            ("livekit-protocol", "0.4.0-rc.1".to_string(), vec![]),
            (
                "livekit-api",
                "0.4.4-rc.1".to_string(),
                vec!["livekit-protocol".to_string()]
            ),
        ]
    );

    let gh = MockClient(|args: &[&str]| -> Result<String> {
        Ok(match args {
            ["pr", "list", ..] => "[]".into(),
            ["pr", "create", ..] => "https://github.com/x/y/pull/3".into(),
            _ => String::new(),
        })
    });
    let (releases, pr) = verwalter::prerelease::open(&repo, "main", "origin", false, &gh).unwrap();
    assert_eq!(releases.len(), 2);
    assert_eq!(pr.head, "release/main-rc");
    let show = |f: &str| git(root, &["show", &format!("release/main-rc:{f}")]);
    assert!(show("livekit-protocol/Cargo.toml").contains("0.4.0-rc.1"));
    assert!(show("livekit-api/Cargo.toml").contains("0.4.4-rc.1"));
    // The workspace pin moves to the rc; livekit, outside the rc, is untouched.
    assert!(
        show("Cargo.toml").contains("livekit-protocol = { version = \"0.4.0-rc.1\""),
        "{}",
        show("Cargo.toml")
    );
    assert!(show("livekit/Cargo.toml").contains("version = \"0.7.0\""));
    assert!(show(".changeset/x.md").contains("livekit-protocol: minor"));
}

// Spec: Go modules. A go.work names the packages by directory, the go.mod
// carries the version in its module-line comment (else the latest tag),
// `require` lines are the dependencies and move with a release, tags are
// `<dir>/vX.Y.Z`, and the Go module proxy request is the publish.
#[test]
fn go_workspace_modules_release_and_publish_through_the_proxy() {
    // No GitHub release behind the tags: nothing to flip.
    let no_gh = MockClient(|_: &[&str]| -> Result<String> { Ok(String::new()) });
    let r = Repo_(std::env::temp_dir().join(format!("verwalter-it-{}-go", std::process::id())));
    let root = &r.0;
    let _ = fs::remove_dir_all(root);
    fs::create_dir_all(root).unwrap();
    git(root, &["init", "-q", "-b", "main"]);
    write(
        root,
        "verwalter.toml",
        "[workspace]\nmembers = [\"sdk/*\"]\n\n[versioning]\nfiles = [\"go.mod\"]\n\n[publish]\nregistry = \"goproxy\"\n",
    );
    write(
        root,
        "go.work",
        "go 1.24\n\nuse (\n\t./sdk/core\n\t./sdk/auth\n)\n",
    );
    write(
        root,
        "sdk/core/go.mod",
        "module github.com/acme/sdk/core\n\ngo 1.24\n",
    );
    write(
        root,
        "sdk/auth/go.mod",
        "module github.com/acme/sdk/auth // v0.3.0\n\ngo 1.24\n\nrequire github.com/acme/sdk/core v1.2.0\n",
    );
    git(root, &["add", "-A"]);
    git(root, &["commit", "-q", "-m", "init"]);
    git(root, &["tag", "sdk/core/v1.2.0"]);
    git(root, &["tag", "sdk/auth/v0.3.0"]);
    write(
        root,
        ".changeset/scopes.md",
        "---\n\"sdk/core\": minor\n---\nToken scopes.\n",
    );
    git(root, &["add", "-A"]);
    git(root, &["commit", "-q", "-m", "changeset"]);

    let repo = Repo::load(root).unwrap();
    let core = repo.packages.iter().find(|p| p.name == "sdk/core").unwrap();
    assert_eq!(
        core.version.to_string(),
        "1.2.0",
        "no comment: the latest tag"
    );
    let auth = repo.packages.iter().find(|p| p.name == "sdk/auth").unwrap();
    assert_eq!(auth.dependencies, ["sdk/core"]);
    let planned = plan(
        &repo.packages,
        &repo.changesets,
        repo.config.versioning.dependent_bump_level,
    );
    let to: Vec<(String, String)> = planned
        .iter()
        .map(|p| (p.package.clone(), p.to.to_string()))
        .collect();
    assert_eq!(
        to,
        [
            ("sdk/core".to_string(), "1.3.0".to_string()),
            ("sdk/auth".to_string(), "0.3.1".to_string())
        ]
    );

    branches::create(&repo, &planned, "main").unwrap();
    let core_mod = git(root, &["show", "release/main:sdk/core/go.mod"]);
    assert!(
        core_mod.starts_with("module github.com/acme/sdk/core // v1.3.0\n"),
        "{core_mod}"
    );
    let auth_mod = git(root, &["show", "release/main:sdk/auth/go.mod"]);
    assert!(
        auth_mod.starts_with("module github.com/acme/sdk/auth // v0.3.1\n"),
        "{auth_mod}"
    );
    assert!(
        auth_mod.ends_with("require github.com/acme/sdk/core v1.3.0"),
        "{auth_mod}"
    );

    // Merged and tagged: the proxy has core, not yet auth.
    git(root, &["merge", "-q", "--ff-only", "release/main"]);
    let repo = Repo::load(root).unwrap();
    for name in ["sdk/core", "sdk/auth"] {
        let p = repo.packages.iter().find(|p| p.name == name).unwrap();
        let tag = repo.config.tag_for(name, &p.version);
        assert!(tag.starts_with(&format!("{name}/v")), "{tag}");
        git(root, &["tag", &tag]);
    }
    let asked = std::sync::Mutex::new(Vec::new());
    let proxy = |url: &str| -> Result<u16> {
        asked.lock().unwrap().push(url.to_string());
        Ok(if url.contains("/core/") { 200 } else { 404 })
    };
    let opts = verwalter::publish::Options {
        jobs: Some(1),
        dry_run: false,
        allow_changed_source: false,
        only: None,
        wait: std::time::Duration::ZERO,
    };
    let out = verwalter::publish::run(&repo, &opts, &proxy, &no_gh).unwrap();
    let status: Vec<(&str, &str)> = out.iter().map(|o| (o.package.as_str(), o.status)).collect();
    assert_eq!(status, [("sdk/core", "published"), ("sdk/auth", "failed")]);
    assert!(out[1].reason.contains("re-run publish"));
    assert_eq!(
        asked.lock().unwrap()[0],
        "https://proxy.golang.org/github.com/acme/sdk/core/@v/v1.3.0.info"
    );

    // A dry run names the request and makes none.
    let dry = verwalter::publish::Options {
        dry_run: true,
        ..opts
    };
    let none = |_: &str| -> Result<u16> { Err(anyhow!("no request in a dry run")) };
    let out = verwalter::publish::run(&repo, &dry, &none, &no_gh).unwrap();
    assert!(
        out[0].reason.starts_with("GET https://proxy.golang.org/"),
        "{out:?}"
    );
}

// Spec: WorkflowAttachesAssets. A matrix build downloads its artifacts
// into one directory, and `--asset <dir>` attaches every file in it.
#[test]
fn asset_directory_attaches_every_file_in_it() {
    let r = fixture("asset-dir");
    let root = &r.0;
    let _remote = remote(root, "asset-dir");
    git(root, &["tag", "livekit-api-v0.4.3"]);
    git(root, &["tag", "livekit-protocol-v0.3.1"]);
    git(root, &["push", "-q", "origin", "--tags"]);
    for f in [
        "dist/sdk-linux.tar.gz",
        "dist/sdk-windows.zip",
        "dist/.DS_Store",
        "dist/nested/x.zip",
    ] {
        write(root, f, "x");
    }
    fs::create_dir_all(root.join("empty")).unwrap();

    let repo = Repo::load(root).unwrap();
    let calls = std::cell::RefCell::new(Vec::<String>::new());
    let gh = MockClient(|args: &[&str]| -> Result<String> {
        calls.borrow_mut().push(args.join(" "));
        match args {
            ["api", "graphql", ..] => Ok(String::new()),
            _ => Ok(String::new()),
        }
    });
    let options = |assets: &[&str]| verwalter::release::Options {
        assets: assets.iter().map(|a| a.to_string()).collect(),
        dry_run: true,
        ..Default::default()
    };
    let err = verwalter::release::run(&repo, "origin", &options(&["empty"]), &gh).unwrap_err();
    assert!(
        err.to_string()
            .contains("--asset empty: the directory has no files"),
        "{err}"
    );

    verwalter::release::run(&repo, "origin", &options(&["dist/", "notes.txt"]), &gh).unwrap();
    let create = calls
        .borrow()
        .iter()
        .find(|c| c.starts_with("release create livekit-v0.7.0"))
        .cloned()
        .unwrap();
    assert!(
        create.contains("--verify-tag dist/sdk-linux.tar.gz dist/sdk-windows.zip notes.txt"),
        "{create}"
    );

    // A glob the shell expanded after `--asset` is refused with the fix.
    let out = Command::new(env!("CARGO_BIN_EXE_verwalter"))
        .current_dir(root)
        .args([
            "release",
            "--asset",
            "dist/sdk-linux.tar.gz",
            "dist/sdk-windows.zip",
            "--dry-run",
        ])
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("pass `--asset dist`"),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

// Spec: config.versioning.fixed, the [workspace.package] group. Crates
// with `version.workspace = true` take the root Cargo.toml as their first
// file and release together; the pins on them move in their dependents.
#[test]
fn crates_on_the_workspace_version_release_together() {
    let root = std::env::temp_dir().join(format!(
        "verwalter-it-{}-workspace-version",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    let r = Repo_(root);
    let root = &r.0;
    git(root, &["init", "-q", "-b", "main"]);
    write(
        root,
        "verwalter.toml",
        "[workspace]\nmembers = [\"crates/*\"]\n\n[versioning]\nfiles = [\"Cargo.toml\"]\n",
    );
    write(
        root,
        "Cargo.toml",
        "[workspace]\nmembers = [\"crates/*\"]\n\n[workspace.package]\nversion = \"1.0.0\"\n",
    );
    write(
        root,
        "Cargo.lock",
        "version = 4\n\n[[package]]\nname = \"x\"\nversion = \"1.0.0\"\n\n[[package]]\nname = \"y\"\nversion = \"1.0.0\"\n\n[[package]]\nname = \"z\"\nversion = \"0.3.0\"\n",
    );
    write(
        root,
        "crates/x/Cargo.toml",
        "[package]\nname = \"x\"\nversion.workspace = true\n",
    );
    write(
        root,
        "crates/y/Cargo.toml",
        "[package]\nname = \"y\"\nversion.workspace = true\n\n[dependencies]\nx = { path = \"../x\", version = \"1.0.0\" }\n",
    );
    write(
        root,
        "crates/z/Cargo.toml",
        "[package]\nname = \"z\"\nversion = \"0.3.0\"\n\n[dependencies]\nx = { path = \"../x\", version = \"1.0.0\" }\n",
    );
    write(root, ".changeset/a.md", "---\nx: major\n---\nBreaking\n");
    git(root, &["add", "-A"]);
    git(root, &["commit", "-q", "-m", "init"]);

    let repo = Repo::load(root).unwrap();
    assert_eq!(repo.config.fixed_with("x"), ["y"]);
    let plan = plan(
        &repo.packages,
        &repo.changesets,
        repo.config.versioning.dependent_bump_level,
    );
    let planned: Vec<String> = plan
        .iter()
        .map(|r| format!("{} {}", r.package, r.to))
        .collect();
    assert_eq!(planned, ["x 2.0.0", "y 2.0.0", "z 0.3.1"]);

    branches::create(&repo, &plan, "main").unwrap();
    let show = |file: &str| git(root, &["show", &format!("release/main:{file}")]);
    assert!(show("Cargo.toml").contains("version = \"2.0.0\""));
    assert!(show("crates/y/Cargo.toml").contains("x = { path = \"../x\", version = \"2.0.0\" }"));
    let z = show("crates/z/Cargo.toml");
    assert!(z.contains("version = \"0.3.1\""));
    assert!(z.contains("x = { path = \"../x\", version = \"2.0.0\" }"));
    let lock = show("Cargo.lock");
    assert!(lock.contains("name = \"x\"\nversion = \"2.0.0\""));
    assert!(lock.contains("name = \"y\"\nversion = \"2.0.0\""));
    assert!(lock.contains("name = \"z\"\nversion = \"0.3.1\""));
}

// A release branch must not fetch LFS objects: CI has no LFS credentials
// and the tool only edits text files. The pointer stays a pointer.
#[test]
fn release_branch_leaves_lfs_pointers_alone() {
    if Command::new("git-lfs").arg("version").output().is_err() {
        eprintln!("git-lfs not installed, skipping");
        return;
    }
    let r = fixture("lfs");
    let root = &r.0;
    git(root, &["lfs", "install", "--local"]);
    git(root, &["lfs", "track", "*.bin"]);
    // A pointer whose object is nowhere: a smudge would fail.
    let pointer = "version https://git-lfs.github.com/spec/v1\noid sha256:ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad\nsize 3\n";
    write(root, "blob.bin", pointer);
    write(
        root,
        ".changeset/one.md",
        "---\nlivekit-protocol: patch\n---\nOne\n",
    );
    git(root, &["add", "-A"]);
    git(root, &["commit", "-q", "-m", "lfs pointer and changeset"]);

    let repo = Repo::load(root).unwrap();
    let plan = plan(
        &repo.packages,
        &repo.changesets,
        repo.config.versioning.dependent_bump_level,
    );
    assert!(branches::create(&repo, &plan, "main").unwrap().is_some());
    assert_eq!(
        git(root, &["show", "release/main:blob.bin"]),
        pointer.trim()
    );
}

// Spec: Package.dependencies. A dependency the manifests do not name is
// configured; a release of it bumps the dependent, whose manifest holds no
// pin to move. The unpublished crate is a package with release.mode none.
#[test]
fn configured_dependency_fans_out_without_a_pin() {
    let r = fixture("configured-dep");
    let root = &r.0;
    write(
        root,
        "verwalter.toml",
        r#"
[packages.krisp]
path = "rust/krisp"
versioning.files = ["Cargo.toml"]
release.mode = "none"
publish.registry = "none"

[packages.noise-node]
path = "nodejs"
dependencies = ["krisp"]
versioning.files = ["package.json"]

[packages.noise-py]
path = "python"
dependencies = ["krisp"]
versioning.files = ["pyproject.toml"]
"#,
    );
    write(
        root,
        "rust/krisp/Cargo.toml",
        "[package]\nname = \"krisp\"\nversion = \"0.1.0\"\n",
    );
    let package_json = "{\n  \"name\": \"noise-node\",\n  \"version\": \"0.3.0\",\n  \"peerDependencies\": {\n    \"@livekit/rtc-node\": \"^1.0.0\"\n  }\n}\n";
    write(root, "nodejs/package.json", package_json);
    write(
        root,
        "python/pyproject.toml",
        "[project]\nname = \"noise-py\"\nversion = \"0.3.2\"\ndependencies = [\"livekit>=0.21.3\"]\n",
    );
    for p in ["rust/krisp", "nodejs", "python"] {
        write(root, &format!("{p}/CHANGELOG.md"), "# Changelog\n");
    }
    write(
        root,
        ".changeset/krisp.md",
        "---\nkrisp: minor\n---\nFaster filter\n",
    );
    git(root, &["add", "-A"]);
    git(root, &["commit", "-q", "-m", "krisp change"]);

    let repo = Repo::load(root).unwrap();
    let by = |n: &str| repo.packages.iter().find(|p| p.name == n).unwrap();
    assert_eq!(by("noise-node").dependencies, ["krisp"]);
    assert_eq!(by("noise-py").dependencies, ["krisp"]);
    let plan = plan(
        &repo.packages,
        &repo.changesets,
        repo.config.versioning.dependent_bump_level,
    );
    branches::create(&repo, &plan, "main").unwrap();
    assert_eq!(
        git(root, &["log", "-1", "--format=%s", "release/main"]),
        "release: krisp 0.2.0, noise-node 0.3.1, noise-py 0.3.3"
    );
    assert!(git(root, &["show", "release/main:rust/krisp/Cargo.toml"]).contains("0.2.0"));
    // Only the version moved: there is no pin on krisp to write.
    assert_eq!(
        git(root, &["show", "release/main:nodejs/package.json"]),
        package_json.replace("0.3.0", "0.3.1").trim()
    );
    assert!(
        git(root, &["show", "release/main:python/pyproject.toml"])
            .contains("version = \"0.3.3\"\ndependencies = [\"livekit>=0.21.3\"]")
    );
    assert!(
        git(root, &["show", "release/main:nodejs/CHANGELOG.md"])
            .contains("Updated dependencies: krisp@0.2.0")
    );
}

// Spec: PublishFlipsDrafts. A package the tool publishes is released as
// a draft; `publish` flips it once the registry has the version, and a
// flip that fails is a failed outcome, so a re-run tries it again.
#[test]
fn publish_flips_the_draft_release_once_the_registry_has_it() {
    let r = fixture("publish-draft");
    let root = &r.0;
    git(root, &["tag", "livekit-protocol-v0.3.1"]);
    git(root, &["tag", "livekit-api-v0.4.3"]);
    git(root, &["tag", "livekit-v0.7.0"]);
    let base = fs::read_to_string(root.join("verwalter.toml")).unwrap();
    write(
        root,
        "verwalter.toml",
        &format!("{base}\n[publish]\ncommand = 'true'\n"),
    );
    let repo = Repo::load(root).unwrap();
    // A publish command makes every package a draft by default.
    assert!(repo.config.draft("livekit-protocol"));
    let no_registry = |_: &str| -> Result<u16> { Err(anyhow!("no registry configured")) };
    let opts = verwalter::publish::Options {
        jobs: Some(1),
        dry_run: false,
        allow_changed_source: false,
        only: None,
        wait: std::time::Duration::ZERO,
    };

    // protocol and api are drafts, livekit is published already.
    let calls = std::cell::RefCell::new(Vec::<String>::new());
    let gh = MockClient(|args: &[&str]| -> Result<String> {
        calls.borrow_mut().push(args.join(" "));
        match args {
            ["api", "graphql", "-f", "tag=livekit-v0.7.0", ..] => {
                Ok(r#"{"isDraft":false,"asset_count":0}"#.into())
            }
            ["api", "graphql", ..] => Ok(r#"{"isDraft":true,"asset_count":0}"#.into()),
            ["release", "edit", "livekit-api-v0.4.3", "--draft=false"] => Err(anyhow!("HTTP 502")),
            _ => Ok(String::new()),
        }
    });
    let outcomes = verwalter::publish::run(&repo, &opts, &no_registry, &gh).unwrap();
    let by = |n: &str| outcomes.iter().find(|o| o.package == n).unwrap();
    assert_eq!(by("livekit-protocol").status, "published");
    assert_eq!(by("livekit").status, "published");
    assert_eq!(by("livekit-api").status, "failed");
    assert!(
        by("livekit-api")
            .reason
            .contains("draft release livekit-api-v0.4.3")
    );
    assert!(verwalter::publish::failed(&outcomes));
    assert!(
        calls
            .borrow()
            .contains(&"release edit livekit-protocol-v0.3.1 --draft=false".to_string())
    );
    assert!(
        calls
            .borrow()
            .contains(&"release edit livekit-api-v0.4.3 --draft=false".to_string())
    );
    assert!(
        !calls
            .borrow()
            .contains(&"release edit livekit-v0.7.0 --draft=false".to_string())
    );

    // The explicit value still wins: release.draft = false, nothing to flip.
    write(
        root,
        "verwalter.toml",
        &format!("{base}\n[publish]\ncommand = 'true'\n[release]\ndraft = false\n"),
    );
    let repo = Repo::load(root).unwrap();
    assert!(!repo.config.draft("livekit-protocol"));
}

// Spec: RequiredAssetsBeforePublic. `publish` leaves a draft without a
// required asset as it is and fails for that package. Its publish command
// does not run: the command cannot be undone, and the release would stay
// a draft. A package without the key is published and flipped as before.
#[test]
fn publish_leaves_a_draft_without_a_required_asset() {
    let r = fixture("publish-required-assets");
    let root = &r.0;
    git(root, &["tag", "livekit-protocol-v0.3.1"]);
    git(root, &["tag", "livekit-api-v0.4.3"]);
    git(root, &["tag", "livekit-v0.7.0"]);
    // The last table of the fixture config is livekit.
    let base = fs::read_to_string(root.join("verwalter.toml")).unwrap();
    write(
        root,
        "verwalter.toml",
        &format!(
            "{base}release.required_assets = [\"sdk-{{tag}}.zip\"]\n\n[publish]\ncommand = 'echo \"$VERWALTER_PACKAGE\" >> \"$VERWALTER_ROOT/published.log\"'\n"
        ),
    );
    let repo = Repo::load(root).unwrap();
    let no_registry = |_: &str| -> Result<u16> { Err(anyhow!("no registry configured")) };
    let opts = verwalter::publish::Options {
        jobs: Some(1),
        dry_run: false,
        only: None,
        allow_changed_source: false,
        wait: std::time::Duration::ZERO,
    };

    // Every release is a draft without assets.
    let calls = std::cell::RefCell::new(Vec::<String>::new());
    let gh = MockClient(|args: &[&str]| -> Result<String> {
        calls.borrow_mut().push(args.join(" "));
        match args {
            ["api", "graphql", ..] => Ok(r#"{"isDraft":true,"asset_count":0,"assets":[]}"#.into()),
            _ => Ok(String::new()),
        }
    });
    let outcomes = verwalter::publish::run(&repo, &opts, &no_registry, &gh).unwrap();
    let by = |n: &str| outcomes.iter().find(|o| o.package == n).unwrap();

    assert_eq!(by("livekit").status, "failed");
    assert!(
        by("livekit")
            .reason
            .contains("lacks required assets: sdk-livekit-v0.7.0.zip"),
        "{}",
        by("livekit").reason
    );
    assert!(verwalter::publish::failed(&outcomes));
    assert!(
        !calls
            .borrow()
            .contains(&"release edit livekit-v0.7.0 --draft=false".to_string())
    );
    let ran = fs::read_to_string(root.join("published.log")).unwrap();
    assert!(!ran.lines().any(|l| l == "livekit"), "{ran}");
    assert!(ran.lines().any(|l| l == "livekit-protocol"), "{ran}");

    assert_eq!(by("livekit-protocol").status, "published");
    assert!(
        calls
            .borrow()
            .contains(&"release edit livekit-protocol-v0.3.1 --draft=false".to_string())
    );
}

#[test]
fn runs_from_a_subdirectory() {
    let r = fixture("subdirectory");
    let out = Command::new(env!("CARGO_BIN_EXE_verwalter"))
        .current_dir(r.0.join("livekit-api"))
        .arg("plan")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

// Scenario: a release with required assets that takes four runs to finish.
// Spec: RequiredAssetsBeforePublic, AssetsComeFromTheTaggedSource,
// PublishBuildsTheTaggedSource, Release.is_complete. One fake GitHub lives
// through all runs; every step loads the repository again, as a run of
// the tool does.
#[test]
fn scenario_a_release_that_lacks_an_asset_is_repaired_from_its_tag() {
    const TAG: &str = "v0.7.0";
    let r = fixture("scenario-assets");
    let root = &r.0;
    let _remote = remote(root, "scenario-assets");
    // One package, so its tags are `v<version>`. Its release needs two
    // assets. Its publish command only writes the tag to a log, so the
    // test can see each time it ran.
    write(
        root,
        "verwalter.toml",
        r#"
[packages.livekit]
path = "livekit"
versioning.files = ["Cargo.toml"]
versioning.changelog = "CHANGELOG.md"
release.required_assets = ["artifact.txt", "extra.txt"]
publish.command = 'echo {tag} >> "$VERWALTER_ROOT/published.log"'
"#,
    );
    git(root, &["commit", "-q", "-am", "one package"]);
    let published_log = || fs::read_to_string(root.join("published.log")).unwrap_or_default();

    let gh = FakeGitHub::default();
    // The package has no registry, so nothing may ask one.
    let no_registry = |_: &str| -> Result<u16> { Err(anyhow!("no registry configured")) };
    let publish_options = verwalter::publish::Options::default();
    let status_of_livekit = || {
        let repo = Repo::load(root).unwrap();
        let rows = verwalter::status::status(&repo, "origin", &gh).unwrap();
        rows.into_iter().find(|r| r.package == "livekit").unwrap()
    };

    // Step A. The release workflow runs after the release PR merged. Its
    // build made only one of the two files.
    write(root, "dist/artifact.txt", "built\n");
    let repo = Repo::load(root).unwrap();

    // `release --asset dist` creates the tag and a draft with that file.
    let options = verwalter::release::Options {
        assets: vec!["dist".into()],
        ..Default::default()
    };
    let released = verwalter::release::run(&repo, "origin", &options, &gh).unwrap();
    assert_eq!(
        (released[0].tag.as_str(), released[0].status),
        (TAG, "drafted")
    );
    assert!(!git(root, &["ls-remote", "--tags", "origin", TAG]).is_empty());
    assert_eq!(gh.is_draft(TAG), Some(true));
    assert_eq!(gh.assets(TAG), ["artifact.txt"]);
    assert_eq!(
        gh.take_writes(),
        [
            "release create v0.7.0 --title livekit 0.7.0 --notes <notes> --verify-tag dist/artifact.txt --draft"
        ]
    );

    // `publish` runs the command, then leaves the draft as it is and
    // fails the package: extra.txt is missing.
    let outcomes = verwalter::publish::run(&repo, &publish_options, &no_registry, &gh).unwrap();
    assert_eq!(published_log(), "v0.7.0\n");
    assert_eq!(outcomes[0].status, "failed");
    assert!(
        outcomes[0]
            .reason
            .contains("draft release v0.7.0 lacks required assets: extra.txt"),
        "{}",
        outcomes[0].reason
    );
    assert_eq!(gh.is_draft(TAG), Some(true));
    assert!(gh.take_writes().is_empty());

    // `status` says the release is incomplete: a draft, without extra.txt.
    let status = status_of_livekit();
    assert_eq!(status.release, "draft");
    assert_eq!(status.missing_assets, ["extra.txt"]);
    let incomplete = status.incomplete.unwrap();
    assert!(
        incomplete.starts_with(
            "the GitHub release is a draft, required assets are missing: extra.txt -> "
        ),
        "{incomplete}"
    );

    // Step B1. The branch moves: a commit changes a file of the package,
    // the version stays. A maintainer builds both files from that commit
    // and runs `release --package livekit --asset dist` by hand.
    write(root, "livekit/src/lib.rs", "pub fn newer() {}\n");
    git(root, &["add", "livekit/src/lib.rs"]);
    git(root, &["commit", "-q", "-m", "after the release"]);
    write(root, "dist/extra.txt", "built\n");
    let repo = Repo::load(root).unwrap();

    // The files are not built from the tag: `release` refuses, names the
    // ref to build from, and uploads nothing.
    let with_both_files = verwalter::release::Options {
        assets: vec!["dist".into()],
        package: Some("livekit".into()),
        ..Default::default()
    };
    let err = verwalter::release::run(&repo, "origin", &with_both_files, &gh)
        .unwrap_err()
        .to_string();
    assert!(err.contains("ref=v0.7.0"), "{err}");
    assert!(gh.take_writes().is_empty());
    assert_eq!(gh.assets(TAG), ["artifact.txt"]);

    // Step B2. The same commit, a run by hand without assets.
    // `release --package livekit` reports the draft and writes nothing.
    let without_assets = verwalter::release::Options {
        package: Some("livekit".into()),
        ..Default::default()
    };
    let released = verwalter::release::run(&repo, "origin", &without_assets, &gh).unwrap();
    assert_eq!(
        (released[0].tag.as_str(), released[0].status),
        (TAG, "drafted")
    );

    // `publish` fails the package before its command runs: the log still
    // has only the line of step A.
    let outcomes = verwalter::publish::run(&repo, &publish_options, &no_registry, &gh).unwrap();
    assert_eq!(outcomes[0].status, "failed");
    assert_eq!(
        outcomes[0].reason,
        "HEAD changed the source since v0.7.0: publish from ref=v0.7.0"
    );
    assert_eq!(published_log(), "v0.7.0\n");
    assert!(gh.take_writes().is_empty());
    assert_eq!(gh.is_draft(TAG), Some(true));

    // Step C. The repair: the maintainer checks out the tag, builds both
    // files there, and runs `release --package livekit --asset dist`.
    git(root, &["checkout", "-q", TAG]);
    let repo = Repo::load(root).unwrap();

    // Both files go to the draft.
    let released = verwalter::release::run(&repo, "origin", &with_both_files, &gh).unwrap();
    assert_eq!(released[0].status, "drafted");
    assert_eq!(gh.assets(TAG), ["artifact.txt", "extra.txt"]);
    assert_eq!(
        gh.take_writes(),
        ["release upload v0.7.0 --clobber dist/artifact.txt dist/extra.txt"]
    );

    // `publish` runs the command again and makes the release public.
    let outcomes = verwalter::publish::run(&repo, &publish_options, &no_registry, &gh).unwrap();
    assert_eq!(
        (outcomes[0].status, outcomes[0].reason.as_str()),
        ("published", "")
    );
    assert_eq!(published_log(), "v0.7.0\nv0.7.0\n");
    assert_eq!(gh.is_draft(TAG), Some(false));
    assert_eq!(gh.take_writes(), ["release edit v0.7.0 --draft=false"]);

    // `status` says the release is complete.
    let status = status_of_livekit();
    assert_eq!(status.release, "published");
    assert_eq!(status.incomplete, None);
}

// Scenario: a version edited by hand is no release.
// Spec: ReleaseNeedsChangelogEntry, Release.is_complete.
#[test]
fn scenario_a_version_edited_by_hand_is_not_an_incomplete_release() {
    let r = fixture("scenario-hand-edit");
    let root = &r.0;
    let _remote = remote(root, "scenario-hand-edit");
    let gh = FakeGitHub::default();
    let incomplete = || {
        let repo = Repo::load(root).unwrap();
        let rows = verwalter::status::status(&repo, "origin", &gh).unwrap();
        rows.into_iter()
            .map(|r| (r.package, r.incomplete))
            .collect::<std::collections::BTreeMap<_, _>>()
    };

    // The release run releases the three packages: `status` has nothing
    // incomplete.
    let repo = Repo::load(root).unwrap();
    let released = verwalter::release::run(&repo, "origin", &Default::default(), &gh).unwrap();
    assert_eq!(released.len(), 3);
    assert_eq!(gh.is_draft("livekit-v0.7.0"), Some(false));
    assert!(incomplete().values().all(Option::is_none));
    gh.take_writes();

    // A maintainer edits the version of livekit by hand and commits it.
    // No changelog entry, no tag.
    write(
        root,
        "livekit/Cargo.toml",
        "[package]\nname = \"livekit\"\nversion = \"0.7.1\"\n\n[dependencies]\nlivekit-protocol = { workspace = true }\n",
    );
    git(root, &["commit", "-q", "-am", "bump by hand"]);

    // `status` shows the missing tag, but 0.7.1 is not an incomplete
    // release. The other packages stay complete.
    let repo = Repo::load(root).unwrap();
    let rows = verwalter::status::status(&repo, "origin", &gh).unwrap();
    let livekit = rows.iter().find(|r| r.package == "livekit").unwrap();
    assert_eq!(
        (livekit.version.as_str(), livekit.tag_state),
        ("0.7.1", "missing")
    );
    assert_eq!(livekit.incomplete, None);
    assert!(incomplete().values().all(Option::is_none));

    // The next release run leaves it alone.
    let released = verwalter::release::run(&repo, "origin", &Default::default(), &gh).unwrap();
    assert!(released.is_empty(), "{released:?}");
    assert_eq!(gh.is_draft("livekit-v0.7.1"), None);
    assert!(gh.take_writes().is_empty());
}
