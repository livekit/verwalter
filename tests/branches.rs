//! End to end on a real git repository: plan, branches, stacking,
//! changeset consumption, released_packages from tags.

use std::{fs, path::{Path, PathBuf}, process::Command};

use verwalter::{branches, plan::plan, repo::Repo};

struct Repo_(PathBuf);

fn git(root: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false"])
        .args(args)
        .output()
        .unwrap();
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
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
    write(&root, "verwalter.toml", r#"
pin_files = ["Cargo.toml", "Cargo.lock"]

[packages.livekit-protocol]
path = "livekit-protocol"
versioned_files = ["livekit-protocol/Cargo.toml"]
changelog = "livekit-protocol/CHANGELOG.md"

[packages.livekit-api]
path = "livekit-api"
versioned_files = ["livekit-api/Cargo.toml"]
changelog = "livekit-api/CHANGELOG.md"

[packages.livekit]
path = "livekit"
versioned_files = ["livekit/Cargo.toml"]
changelog = "livekit/CHANGELOG.md"
"#);
    write(&root, "Cargo.toml", "[workspace]\nmembers = [\"livekit-protocol\", \"livekit-api\", \"livekit\"]\n\n[workspace.dependencies]\nlivekit-protocol = { version = \"0.3.1\", path = \"livekit-protocol\" }\nlivekit-api = { version = \"0.4.3\", path = \"livekit-api\" }\n");
    write(&root, "Cargo.lock", "version = 4\n\n[[package]]\nname = \"livekit\"\nversion = \"0.7.0\"\n\n[[package]]\nname = \"livekit-api\"\nversion = \"0.4.3\"\n\n[[package]]\nname = \"livekit-protocol\"\nversion = \"0.3.1\"\n");
    write(&root, "livekit-protocol/Cargo.toml", "[package]\nname = \"livekit-protocol\"\nversion = \"0.3.1\"\n");
    write(&root, "livekit-api/Cargo.toml", "[package]\nname = \"livekit-api\"\nversion = \"0.4.3\"\n\n[dependencies]\nlivekit-protocol = { workspace = true }\n");
    write(&root, "livekit/Cargo.toml", "[package]\nname = \"livekit\"\nversion = \"0.7.0\"\n\n[dependencies]\nlivekit-protocol = { workspace = true }\n");
    git(&root, &["add", "-A"]);
    git(&root, &["commit", "-q", "-m", "init"]);
    Repo_(root)
}

impl Drop for Repo_ {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn stacked_branches_from_one_changeset() {
    let r = fixture("stack");
    let root = &r.0;
    write(root, ".changeset/ndc.md", "---\nlivekit-protocol: minor\nlivekit: minor\n---\nNegotiated data channels\n");
    git(root, &["add", "-A"]);
    git(root, &["commit", "-q", "-m", "add changeset"]);

    let repo = Repo::load(root).unwrap();
    let plan = plan(&repo.packages, &repo.changesets, repo.config.dependent_bump_level);
    let branches = branches::create(&repo, &plan, "main").unwrap();

    let names: Vec<(&str, &str)> = branches.iter().map(|b| (b.name.as_str(), b.target.as_str())).collect();
    assert_eq!(
        names,
        [("release/livekit-protocol", "main"), ("release/livekit", "release/livekit-protocol"), ("release/livekit-api", "release/livekit-protocol")]
    );

    // Stacking is real: protocol's branch is an ancestor of both children, main of all.
    for child in ["release/livekit", "release/livekit-api"] {
        git(root, &["merge-base", "--is-ancestor", "release/livekit-protocol", child]);
    }
    // The user's checkout is untouched and still on main.
    assert_eq!(git(root, &["rev-parse", "--abbrev-ref", "HEAD"]), "main");
    assert_eq!(git(root, &["status", "--porcelain"]), "");

    // protocol's branch: own version, workspace pin, lock, changelog; the
    // changeset stays because livekit is not released yet.
    assert!(git(root, &["show", "release/livekit-protocol:livekit-protocol/Cargo.toml"]).contains("0.4.0"));
    assert!(git(root, &["show", "release/livekit-protocol:Cargo.toml"]).contains("livekit-protocol = { version = \"0.4.0\""));
    assert!(git(root, &["show", "release/livekit-protocol:livekit-protocol/CHANGELOG.md"]).contains("## 0.4.0"));
    git(root, &["show", "release/livekit-protocol:.changeset/ndc.md"]);

    // livekit's branch inherits protocol's bump and adds its own; the file
    // is consumed there because livekit is the last package it names.
    let cargo = git(root, &["show", "release/livekit:livekit/Cargo.toml"]);
    assert!(cargo.contains("version = \"0.8.0\""));
    assert!(git(root, &["show", "release/livekit:livekit-protocol/Cargo.toml"]).contains("0.4.0"));
    assert!(git(root, &["ls-tree", "--name-only", "release/livekit", ".changeset/"]).is_empty());

    // api only got a dependent patch.
    assert!(git(root, &["show", "release/livekit-api:livekit-api/Cargo.toml"]).contains("version = \"0.4.4\""));
    assert!(git(root, &["show", "release/livekit-api:livekit-api/CHANGELOG.md"]).contains("Updated dependencies: livekit-protocol@0.4.0"));

    // Re-running regenerates the same branches without error.
    let again = branches::create(&repo, &plan, "main").unwrap();
    assert_eq!(again.len(), 3);
}

#[test]
fn released_packages_come_from_tags() {
    let r = fixture("tags");
    let root = &r.0;
    write(root, ".changeset/ndc.md", "---\nlivekit-protocol: minor\nlivekit: minor\n---\nx\n");
    git(root, &["add", "-A"]);
    git(root, &["commit", "-q", "-m", "add changeset"]);

    // Simulate protocol's release PR merged and tagged, changeset kept.
    write(root, "livekit-protocol/Cargo.toml", "[package]\nname = \"livekit-protocol\"\nversion = \"0.4.0\"\n");
    git(root, &["commit", "-qam", "release: livekit-protocol 0.4.0"]);
    git(root, &["tag", "livekit-protocol-v0.4.0"]);

    let repo = Repo::load(root).unwrap();
    assert!(repo.changesets[0].released_packages.contains("livekit-protocol"));
    assert!(!repo.changesets[0].released_packages.contains("livekit"));

    let plan = plan(&repo.packages, &repo.changesets, repo.config.dependent_bump_level);
    let names: Vec<&str> = plan.iter().map(|r| r.package.as_str()).collect();
    assert_eq!(names, ["livekit"], "protocol is done; api got no dependent bump because protocol is not planned");
}

#[test]
fn pr_sync_pushes_and_creates_updates_closes() {
    let r = fixture("pr");
    let root = &r.0;
    write(root, ".changeset/ndc.md", "---\nlivekit-protocol: minor\nlivekit: minor\n---\nx\n");
    git(root, &["add", "-A"]);
    git(root, &["commit", "-q", "-m", "add changeset"]);
    let remote = std::env::temp_dir().join(format!("verwalter-it-{}-pr-remote.git", std::process::id()));
    let _ = fs::remove_dir_all(&remote);
    git(root, &["init", "-q", "--bare", remote.to_str().unwrap()]);
    git(root, &["remote", "add", "origin", remote.to_str().unwrap()]);

    let repo = Repo::load(root).unwrap();
    let plan = plan(&repo.packages, &repo.changesets, repo.config.dependent_bump_level);
    let branches = branches::create(&repo, &plan, "main").unwrap();

    let mut calls: Vec<String> = Vec::new();
    let mut gh = |args: &[&str]| -> Result<String, verwalter::github::Error> {
        calls.push(args.join(" "));
        Ok(match args {
            ["pr", "list", ..] => r#"[{"number":41,"headRefName":"release/livekit-api","baseRefName":"main"},{"number":99,"headRefName":"release/old-package","baseRefName":"main"}]"#.into(),
            ["pr", "create", ..] => format!("https://github.com/livekit/x/pull/{}", 100 + calls.len()),
            _ => String::new(),
        })
    };
    let prs = verwalter::github::sync(&repo, &plan, &branches, "origin", &mut gh).unwrap();

    // All three branches were pushed to the remote.
    let pushed = git(root, &["ls-remote", "--heads", "origin"]);
    let _ = fs::remove_dir_all(&remote);
    for b in ["release/livekit-protocol", "release/livekit", "release/livekit-api"] {
        assert!(pushed.contains(b), "{b} not pushed");
    }

    let heads: Vec<(u64, &str, &str)> = prs.iter().map(|p| (p.number, p.head.as_str(), p.base.as_str())).collect();
    assert_eq!(heads[0].1, "release/livekit-protocol");
    assert_eq!(heads[0].2, "main");
    assert_eq!(heads[1], (heads[1].0, "release/livekit", "release/livekit-protocol"));
    assert_eq!(heads[2], (41, "release/livekit-api", "release/livekit-protocol"), "existing PR kept its number, re-based onto the stack");

    assert!(calls[0].starts_with("label create release --force"));
    assert!(calls.iter().filter(|c| c.starts_with("pr create")).count() == 2);
    let edit = calls.iter().find(|c| c.starts_with("pr edit 41")).unwrap();
    assert!(edit.contains("--base release/livekit-protocol"));
    assert!(edit.contains("Stacked on #"), "body carries the stack note");
    assert!(calls.iter().any(|c| c.starts_with("pr close 99")));
}

#[test]
fn release_tags_and_creates_github_releases_resumably() {
    let r = fixture("release");
    let root = &r.0;
    let remote = std::env::temp_dir().join(format!("verwalter-it-{}-release-remote.git", std::process::id()));
    let _ = fs::remove_dir_all(&remote);
    git(root, &["init", "-q", "--bare", remote.to_str().unwrap()]);
    git(root, &["remote", "add", "origin", remote.to_str().unwrap()]);
    // Everything is already tagged except livekit, whose release PR just
    // merged: version bumped, changelog written. livekit-api is draft mode.
    git(root, &["tag", "livekit-protocol-v0.3.1"]);
    git(root, &["tag", "livekit-api-v0.4.3"]);
    git(root, &["push", "-q", "origin", "--tags"]);
    write(root, "livekit/Cargo.toml", "[package]\nname = \"livekit\"\nversion = \"0.8.0\"\n\n[dependencies]\nlivekit-protocol = { workspace = true }\n");
    write(root, "livekit/CHANGELOG.md", "# Changelog\n\n## 0.8.0\n\n### Minor Changes\n\n- Negotiated data channels\n\n## 0.7.0\n\n- init\n");
    let cfg = fs::read_to_string(root.join("verwalter.toml")).unwrap().replace("changelog = \"livekit/CHANGELOG.md\"", "changelog = \"livekit/CHANGELOG.md\"\ndraft = true");
    write(root, "verwalter.toml", &cfg);
    git(root, &["commit", "-qam", "release: livekit 0.8.0"]);

    let repo = Repo::load(root).unwrap();
    let mut calls: Vec<String> = Vec::new();
    let mut gh = |args: &[&str]| -> Result<String, verwalter::github::Error> {
        calls.push(args.join(" "));
        match args {
            // Older tags already have releases; the new one does not yet.
            ["release", "view", tag, ..] if *tag != "livekit-v0.8.0" => Ok("false".into()),
            ["release", "view", ..] => Err("release not found".into()),
            _ => Ok(String::new()),
        }
    };
    let released = verwalter::release::run(&repo, "origin", &mut gh).unwrap();

    assert!(git(root, &["ls-remote", "--tags", "origin"]).contains("livekit-v0.8.0"), "tag pushed");
    assert_eq!(git(root, &["rev-parse", "livekit-v0.8.0^{commit}"]), git(root, &["rev-parse", "HEAD"]));
    let create = calls.iter().find(|c| c.starts_with("release create livekit-v0.8.0")).unwrap();
    assert!(create.contains("--draft"), "draft mode from config");
    assert!(create.contains("- Negotiated data channels"), "notes from the changelog section");
    assert!(!create.contains("--prerelease"));

    let lk = released.iter().find(|r| r.package == "livekit").unwrap();
    assert_eq!((lk.version.as_str(), lk.tag.as_str(), lk.status, lk.previous_version.as_deref()), ("0.8.0", "livekit-v0.8.0", "drafted", Some("0.7.0")));
    assert_eq!(released.len(), 1, "complete releases are not reported");

    // Second run: nothing to tag, nothing to create.
    let before = calls.len();
    let mut gh2 = |args: &[&str]| -> Result<String, verwalter::github::Error> {
        calls.push(args.join(" "));
        Ok(if args[2] == "livekit-v0.8.0" { "true".into() } else { "false".into() })
    };
    let again = verwalter::release::run(&repo, "origin", &mut gh2).unwrap();
    assert!(!calls[before..].iter().any(|c| c.starts_with("release create")));
    assert_eq!(again.len(), 1, "the draft is reported until the workflow publishes it");
    assert_eq!(again[0].status, "drafted");
    let _ = fs::remove_dir_all(&remote);
}

#[test]
fn release_resumes_a_tag_without_github_release() {
    let r = fixture("release-resume");
    let root = &r.0;
    let remote = std::env::temp_dir().join(format!("verwalter-it-{}-resume-remote.git", std::process::id()));
    let _ = fs::remove_dir_all(&remote);
    git(root, &["init", "-q", "--bare", remote.to_str().unwrap()]);
    git(root, &["remote", "add", "origin", remote.to_str().unwrap()]);
    // The tag was pushed by a failed run, on an older commit.
    git(root, &["tag", "livekit-v0.7.0"]);
    git(root, &["tag", "livekit-api-v0.4.3"]);
    git(root, &["tag", "livekit-protocol-v0.3.1"]);
    write(root, "README.md", "x");
    git(root, &["add", "-A"]);
    git(root, &["commit", "-qm", "later"]);
    let repo = Repo::load(root).unwrap();
    let mut calls: Vec<String> = Vec::new();
    let mut gh = |args: &[&str]| -> Result<String, verwalter::github::Error> {
        calls.push(args.join(" "));
        match args {
            ["release", "view", "livekit-v0.7.0", ..] => Err("not found".into()),
            ["release", "view", ..] => Ok("false".into()),
            _ => Ok(String::new()),
        }
    };
    let released = verwalter::release::run(&repo, "origin", &mut gh).unwrap();
    assert_eq!(released.len(), 1);
    assert_eq!((released[0].package.as_str(), released[0].status), ("livekit", "published"));
    assert!(calls.iter().any(|c| c.starts_with("release create livekit-v0.7.0")));
    assert!(!calls.iter().any(|c| c.starts_with("release create livekit-api")));
    let _ = fs::remove_dir_all(&remote);
}
