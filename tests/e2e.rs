//! End to end on a real git repository: plan, branches, stacking,
//! changeset consumption by front-matter edits.

use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use verwalter::{branches, plan::plan, repo::Repo};

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
"#,
    );
    write(&root, "Cargo.toml", "[workspace]\nmembers = [\"livekit-protocol\", \"livekit-api\", \"livekit\"]\n\n[workspace.dependencies]\nlivekit-protocol = { version = \"0.3.1\", path = \"livekit-protocol\" }\nlivekit-api = { version = \"0.4.3\", path = \"livekit-api\" }\n");
    write(&root, "Cargo.lock", "version = 4\n\n[[package]]\nname = \"livekit\"\nversion = \"0.7.0\"\n\n[[package]]\nname = \"livekit-api\"\nversion = \"0.4.3\"\n\n[[package]]\nname = \"livekit-protocol\"\nversion = \"0.3.1\"\n");
    write(
        &root,
        "livekit-protocol/Cargo.toml",
        "[package]\nname = \"livekit-protocol\"\nversion = \"0.3.1\"\n",
    );
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
        repo.config.dependent_bump_level,
    );
    // Start from a remote-tracking ref, as CI does; PRs still target `main`.
    git(root, &["update-ref", "refs/remotes/origin/main", "main"]);
    let branches = branches::create(&repo, &plan, "origin/main").unwrap();

    let names: Vec<(&str, &str)> = branches
        .iter()
        .map(|b| (b.name.as_str(), b.target.as_str()))
        .collect();
    assert_eq!(
        names,
        [
            ("release/livekit-protocol", "main"),
            ("release/livekit", "release/livekit-protocol"),
            ("release/livekit-api", "release/livekit-protocol")
        ]
    );

    // Stacking is real: protocol's branch is an ancestor of both children, main of all.
    for child in ["release/livekit", "release/livekit-api"] {
        git(
            root,
            &[
                "merge-base",
                "--is-ancestor",
                "release/livekit-protocol",
                child,
            ],
        );
    }
    // The user's checkout is untouched and still on main.
    assert_eq!(git(root, &["rev-parse", "--abbrev-ref", "HEAD"]), "main");
    assert_eq!(git(root, &["status", "--porcelain"]), "");

    // protocol's branch: own version, workspace pin, lock, changelog; the
    // changeset stays because livekit is not released yet.
    assert!(git(
        root,
        &[
            "show",
            "release/livekit-protocol:livekit-protocol/Cargo.toml"
        ]
    )
    .contains("0.4.0"));
    assert!(git(root, &["show", "release/livekit-protocol:Cargo.toml"])
        .contains("livekit-protocol = { version = \"0.4.0\""));
    assert!(git(
        root,
        &[
            "show",
            "release/livekit-protocol:livekit-protocol/CHANGELOG.md"
        ]
    )
    .contains("## 0.4.0"));
    git(
        root,
        &["show", "release/livekit-protocol:.changeset/ndc.md"],
    );

    // livekit's branch inherits protocol's bump and adds its own; the file
    // is consumed there because livekit is the last package it names.
    let cargo = git(root, &["show", "release/livekit:livekit/Cargo.toml"]);
    assert!(cargo.contains("version = \"0.8.0\""));
    assert!(git(
        root,
        &["show", "release/livekit:livekit-protocol/Cargo.toml"]
    )
    .contains("0.4.0"));
    // What is left is protocol's note for api, which merges separately.
    assert_eq!(
        git(
            root,
            &["ls-tree", "--name-only", "release/livekit", ".changeset/"]
        ),
        ".changeset/livekit-protocol-0.4.0-dependents.md"
    );

    // api only got a dependent patch.
    assert!(git(
        root,
        &["show", "release/livekit-api:livekit-api/Cargo.toml"]
    )
    .contains("version = \"0.4.4\""));
    assert!(git(
        root,
        &["show", "release/livekit-api:livekit-api/CHANGELOG.md"]
    )
    .contains("Updated dependencies: livekit-protocol@0.4.0"));

    // Re-running regenerates the same branches without error.
    let again = branches::create(&repo, &plan, "main").unwrap();
    assert_eq!(again.len(), 3);
}

// Spec: ReleaseRemovesPackageFromChangeset. After protocol's release PR
// merges, the shared changeset only names livekit, so only livekit is
// planned; tags play no part.
#[test]
fn merged_release_removes_its_line_from_shared_changesets() {
    let r = fixture("shared");
    let root = &r.0;
    write(
        root,
        ".changeset/ndc.md",
        "---\nlivekit-protocol: minor\nlivekit: minor\n---\nx\n",
    );
    git(root, &["add", "-A"]);
    git(root, &["commit", "-q", "-m", "add changeset"]);
    let repo = Repo::load(root).unwrap();
    let first = plan(
        &repo.packages,
        &repo.changesets,
        repo.config.dependent_bump_level,
    );
    branches::create(&repo, &first, "main").unwrap();
    git(
        root,
        &["merge", "-q", "--ff-only", "release/livekit-protocol"],
    );

    let repo = Repo::load(root).unwrap();
    assert_eq!(
        git(root, &["show", "HEAD:.changeset/ndc.md"]),
        "---\nlivekit: minor\n---\nx"
    );
    assert_eq!(repo.changesets[0].unreleased_for("livekit-protocol"), None);
    let plan = plan(
        &repo.packages,
        &repo.changesets,
        repo.config.dependent_bump_level,
    );
    let names: Vec<&str> = plan.iter().map(|r| r.package.as_str()).collect();
    assert_eq!(
        names,
        ["livekit", "livekit-api"],
        "protocol is done; api is planned from the changeset protocol left for it"
    );
}

// Spec: ReleaseLeavesChangesetForDependents. Protocol's branch moves the
// pins in api and livekit. Merging it alone must not lose their bumps, so
// it leaves a changeset naming them; the stacked branches consume their
// own line from it like from any other changeset.
#[test]
fn bottom_of_stack_leaves_a_changeset_for_its_dependents() {
    let r = fixture("dependents");
    let root = &r.0;
    write(
        root,
        ".changeset/proto.md",
        "---\nlivekit-protocol: minor\n---\nx\n",
    );
    git(root, &["add", "-A"]);
    git(root, &["commit", "-q", "-m", "add changeset"]);
    let repo = Repo::load(root).unwrap();
    let first = plan(
        &repo.packages,
        &repo.changesets,
        repo.config.dependent_bump_level,
    );
    branches::create(&repo, &first, "main").unwrap();

    let left = "livekit-protocol-0.4.0-dependents.md";
    assert_eq!(
        git(root, &["show", &format!("release/livekit-protocol:.changeset/{left}")]),
        "---\n\"livekit\": patch\n\"livekit-api\": patch\n---\nUpdated dependencies: livekit-protocol@0.4.0"
    );
    // livekit's stacked branch takes its own line out and leaves api's.
    assert_eq!(
        git(
            root,
            &["show", &format!("release/livekit:.changeset/{left}")]
        ),
        "---\n\"livekit-api\": patch\n---\nUpdated dependencies: livekit-protocol@0.4.0"
    );
    // No dependents of its own: livekit leaves nothing behind.
    assert_eq!(
        git(
            root,
            &["ls-tree", "--name-only", "release/livekit", ".changeset/"]
        ),
        format!(".changeset/{left}")
    );

    // Only protocol merges: both dependents are still planned, now as base PRs.
    git(
        root,
        &["merge", "-q", "--ff-only", "release/livekit-protocol"],
    );
    let repo = Repo::load(root).unwrap();
    let plan = plan(
        &repo.packages,
        &repo.changesets,
        repo.config.dependent_bump_level,
    );
    let stack: Vec<(&str, Option<&str>, &str)> = plan
        .iter()
        .map(|r| {
            (
                r.package.as_str(),
                r.stacked_on.as_deref(),
                r.to.to_string().leak() as &str,
            )
        })
        .collect();
    assert_eq!(
        stack,
        [("livekit", None, "0.7.1"), ("livekit-api", None, "0.4.4")]
    );
    branches::create(&repo, &plan, "main").unwrap();
    assert!(git(
        root,
        &["show", "release/livekit-api:livekit-api/CHANGELOG.md"]
    )
    .contains("- Updated dependencies: livekit-protocol@0.4.0"));
    assert!(
        git(
            root,
            &[
                "ls-tree",
                "--name-only",
                "release/livekit-api",
                ".changeset/"
            ]
        )
        .contains(left),
        "livekit's line is still pending"
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
    let remote =
        std::env::temp_dir().join(format!("verwalter-it-{}-pr-remote.git", std::process::id()));
    let _ = fs::remove_dir_all(&remote);
    git(root, &["init", "-q", "--bare", remote.to_str().unwrap()]);
    git(root, &["remote", "add", "origin", remote.to_str().unwrap()]);

    let repo = Repo::load(root).unwrap();
    let plan = plan(
        &repo.packages,
        &repo.changesets,
        repo.config.dependent_bump_level,
    );
    let branches = branches::create(&repo, &plan, "main").unwrap();

    let mut calls: Vec<String> = Vec::new();
    let mut gh = |args: &[&str]| -> Result<String, verwalter::Error> {
        calls.push(args.join(" "));
        Ok(match args {
            ["pr", "list", ..] => r#"[{"number":40,"headRefName":"release/livekit-protocol","baseRefName":"main"},{"number":41,"headRefName":"release/livekit-api","baseRefName":"main"},{"number":99,"headRefName":"release/old-package","baseRefName":"main"}]"#.into(),
            ["pr", "create", ..] => format!("https://github.com/livekit/x/pull/{}", 100 + calls.len()),
            _ => String::new(),
        })
    };
    let prs = verwalter::github::sync(&repo, &plan, &branches, "origin", false, &mut gh).unwrap();

    // All three branches were pushed to the remote.
    let pushed = git(root, &["ls-remote", "--heads", "origin"]);
    let _ = fs::remove_dir_all(&remote);
    for b in [
        "release/livekit-protocol",
        "release/livekit",
        "release/livekit-api",
    ] {
        assert!(pushed.contains(b), "{b} not pushed");
    }

    let heads: Vec<(u64, &str, &str)> = prs
        .iter()
        .map(|p| (p.number, p.head.as_str(), p.base.as_str()))
        .collect();
    assert_eq!(heads[0], (40, "release/livekit-protocol", "main"));
    assert!(
        !calls.iter().any(|c| c.starts_with("pr edit 40 --base")),
        "base unchanged: not touched (a stacked PR would refuse it)"
    );
    assert_eq!(
        heads[1],
        (heads[1].0, "release/livekit", "release/livekit-protocol")
    );
    assert_eq!(
        heads[2],
        (41, "release/livekit-api", "release/livekit-protocol"),
        "existing PR kept its number, re-based onto the stack"
    );

    assert!(calls[0].starts_with("label create release --force"));
    assert!(calls.iter().filter(|c| c.starts_with("pr create")).count() == 1);
    assert!(
        calls
            .iter()
            .any(|c| c == "pr edit 41 --base release/livekit-protocol"),
        "base changed in its own call"
    );
    let edit = calls
        .iter()
        .find(|c| c.starts_with("pr edit 41 --title"))
        .unwrap();
    assert!(edit.contains("Stacked on #"), "body carries the stack note");
    assert!(calls.iter().any(|c| c.starts_with("pr close 99")));
    assert!(
        !calls.iter().any(|c| c.starts_with("stack link")),
        "a tree is not linked as a GitHub stack"
    );
}

#[test]
fn pr_sync_links_a_chain_as_a_github_stack() {
    let r = fixture("stack-link");
    let root = &r.0;
    // api depends on protocol, livekit on api: one chain.
    write(root, "livekit/Cargo.toml", "[package]\nname = \"livekit\"\nversion = \"0.7.0\"\n\n[dependencies]\nlivekit-api = { workspace = true }\n");
    write(
        root,
        ".changeset/ndc.md",
        "---\nlivekit-protocol: minor\n---\nx\n",
    );
    git(root, &["add", "-A"]);
    git(root, &["commit", "-q", "-m", "chain"]);
    let remote = std::env::temp_dir().join(format!(
        "verwalter-it-{}-stack-remote.git",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&remote);
    git(root, &["init", "-q", "--bare", remote.to_str().unwrap()]);
    git(root, &["remote", "add", "origin", remote.to_str().unwrap()]);

    let repo = Repo::load(root).unwrap();
    let plan = plan(
        &repo.packages,
        &repo.changesets,
        repo.config.dependent_bump_level,
    );
    let branches = branches::create(&repo, &plan, "main").unwrap();
    let mut calls: Vec<String> = Vec::new();
    let mut gh = |args: &[&str]| -> Result<String, verwalter::Error> {
        calls.push(args.join(" "));
        Ok(match args {
            ["pr", "list", ..] => "[]".into(),
            ["pr", "create", ..] => format!("https://github.com/x/y/pull/{}", 200 + calls.len()),
            _ => String::new(),
        })
    };
    verwalter::github::sync(&repo, &plan, &branches, "origin", false, &mut gh).unwrap();
    let link = calls
        .iter()
        .find(|c| c.starts_with("stack link"))
        .expect("chain linked");
    let numbers: Vec<&str> = link.split_whitespace().skip(2).collect();
    assert_eq!(numbers.len(), 3, "{link}");
    let _ = fs::remove_dir_all(&remote);
}

#[test]
fn release_tags_and_creates_github_releases_resumably() {
    let r = fixture("release");
    let root = &r.0;
    let remote = std::env::temp_dir().join(format!(
        "verwalter-it-{}-release-remote.git",
        std::process::id()
    ));
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
    let cfg = fs::read_to_string(root.join("verwalter.toml"))
        .unwrap()
        .replace(
            "changelog = \"livekit/CHANGELOG.md\"",
            "changelog = \"livekit/CHANGELOG.md\"\ndraft = true",
        );
    write(root, "verwalter.toml", &cfg);
    git(root, &["commit", "-qam", "release: livekit 0.8.0"]);

    let repo = Repo::load(root).unwrap();
    let mut calls: Vec<String> = Vec::new();
    let mut gh = |args: &[&str]| -> Result<String, verwalter::Error> {
        calls.push(args.join(" "));
        match args {
            // Older tags already have releases; the new one does not yet.
            ["release", "view", tag, ..] if *tag != "livekit-v0.8.0" => {
                Ok(r#"{"isDraft":false,"assets":[]}"#.into())
            }
            ["release", "view", ..] => Err("release not found".into()),
            _ => Ok(String::new()),
        }
    };
    let released = verwalter::release::run(&repo, "origin", &Default::default(), &mut gh).unwrap();

    assert!(
        git(root, &["ls-remote", "--tags", "origin"]).contains("livekit-v0.8.0"),
        "tag pushed"
    );
    assert_eq!(
        git(root, &["rev-parse", "livekit-v0.8.0^{commit}"]),
        git(root, &["rev-parse", "HEAD"])
    );
    let create = calls
        .iter()
        .find(|c| c.starts_with("release create livekit-v0.8.0"))
        .unwrap();
    assert!(create.contains("--draft"), "draft mode from config");
    assert!(
        create.contains("- Negotiated data channels"),
        "notes from the changelog section"
    );
    assert!(!create.contains("--prerelease"));

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
    let before = calls.len();
    let mut gh2 = |args: &[&str]| -> Result<String, verwalter::Error> {
        calls.push(args.join(" "));
        Ok(if args[2] == "livekit-v0.8.0" {
            r#"{"isDraft":true,"assets":[]}"#.into()
        } else {
            r#"{"isDraft":false,"assets":[]}"#.into()
        })
    };
    let again = verwalter::release::run(&repo, "origin", &Default::default(), &mut gh2).unwrap();
    assert!(!calls[before..]
        .iter()
        .any(|c| c.starts_with("release create")));
    assert_eq!(
        again.len(),
        1,
        "the draft is reported until the workflow publishes it"
    );
    assert_eq!(again[0].status, "drafted");
    let _ = fs::remove_dir_all(&remote);
}

#[test]
fn release_resumes_a_tag_without_github_release() {
    let r = fixture("release-resume");
    let root = &r.0;
    let remote = std::env::temp_dir().join(format!(
        "verwalter-it-{}-resume-remote.git",
        std::process::id()
    ));
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
    let mut gh = |args: &[&str]| -> Result<String, verwalter::Error> {
        calls.push(args.join(" "));
        match args {
            ["release", "view", "livekit-v0.7.0", ..] => Err("not found".into()),
            ["release", "view", ..] => Ok(r#"{"isDraft":false,"assets":[]}"#.into()),
            _ => Ok(String::new()),
        }
    };
    let released = verwalter::release::run(&repo, "origin", &Default::default(), &mut gh).unwrap();
    assert_eq!(released.len(), 1);
    assert_eq!(
        (released[0].package.as_str(), released[0].status),
        ("livekit", "published")
    );
    assert!(calls
        .iter()
        .any(|c| c.starts_with("release create livekit-v0.7.0")));
    assert!(!calls
        .iter()
        .any(|c| c.starts_with("release create livekit-api")));
    let _ = fs::remove_dir_all(&remote);
}

#[test]
fn release_with_assets_then_abandon() {
    let r = fixture("assets");
    let root = &r.0;
    let remote = std::env::temp_dir().join(format!(
        "verwalter-it-{}-assets-remote.git",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&remote);
    git(root, &["init", "-q", "--bare", remote.to_str().unwrap()]);
    git(root, &["remote", "add", "origin", remote.to_str().unwrap()]);
    git(root, &["tag", "livekit-api-v0.4.3"]);
    git(root, &["tag", "livekit-protocol-v0.3.1"]);
    git(root, &["push", "-q", "origin", "--tags"]);

    let repo = Repo::load(root).unwrap();
    let calls = std::cell::RefCell::new(Vec::<String>::new());
    let mut gh = |args: &[&str]| -> Result<String, verwalter::Error> {
        let exists = {
            let calls = calls.borrow();
            calls.iter().any(|c| c.starts_with("release create"))
                && !calls.iter().any(|c| c.starts_with("release delete"))
        };
        calls.borrow_mut().push(args.join(" "));
        match args {
            ["release", "view", "livekit-v0.7.0", ..] if !exists => Err("not found".into()),
            ["release", "view", ..] => Ok(r#"{"isDraft":false,"assets":[]}"#.into()),
            _ => Ok(String::new()),
        }
    };
    let assets = ["dist/a.tar.gz".to_string(), "dist/b.tar.gz".to_string()];
    let released = verwalter::release::run(
        &repo,
        "origin",
        &verwalter::release::Options {
            assets: assets.to_vec(),
            ..Default::default()
        },
        &mut gh,
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

    let removed =
        verwalter::release::abandon(&repo, "origin", "livekit-v0.7.0", false, &mut gh).unwrap();
    assert_eq!(
        removed,
        [
            "release livekit-v0.7.0",
            "origin tag livekit-v0.7.0",
            "local tag livekit-v0.7.0"
        ]
    );
    assert!(calls
        .borrow()
        .iter()
        .any(|c| c == "release delete livekit-v0.7.0 --yes"));
    assert!(!git(root, &["ls-remote", "--tags", "origin"]).contains("livekit-v0.7.0"));
    assert!(git(root, &["tag", "--list", "livekit-v0.7.0"]).is_empty());

    // Abandoning again finds nothing and does not fail.
    let again =
        verwalter::release::abandon(&repo, "origin", "livekit-v0.7.0", false, &mut gh).unwrap();
    assert!(again.is_empty());
    let _ = fs::remove_dir_all(&remote);
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
    let remote =
        std::env::temp_dir().join(format!("verwalter-it-{}-rc-remote.git", std::process::id()));
    let _ = fs::remove_dir_all(&remote);
    git(root, &["init", "-q", "--bare", remote.to_str().unwrap()]);
    git(root, &["remote", "add", "origin", remote.to_str().unwrap()]);
    git(root, &["push", "-q", "origin", "main", "--tags"]);

    let repo = Repo::load(root).unwrap();
    let mut calls: Vec<String> = Vec::new();
    let mut gh = |args: &[&str]| -> Result<String, verwalter::Error> {
        calls.push(args.join(" "));
        Ok(match args {
            ["pr", "list", ..] => "[]".into(),
            ["pr", "create", ..] => "https://github.com/x/y/pull/9".into(),
            _ => String::new(),
        })
    };
    let (release, pr) = verwalter::prerelease::open(
        &repo,
        "livekit-protocol",
        "main",
        None,
        "origin",
        false,
        &mut gh,
    )
    .unwrap();
    assert_eq!(
        (
            release.from.to_string().as_str(),
            release.to.to_string().as_str()
        ),
        ("0.3.1", "0.4.0-rc.1"),
        "level from the changeset"
    );
    assert_eq!(
        (pr.number, pr.head.as_str(), pr.base.as_str()),
        (9, "release/livekit-protocol-rc", "main")
    );
    assert!(calls.iter().any(|c| c.starts_with("pr create --head release/livekit-protocol-rc --base main --title pre-release: livekit-protocol 0.4.0-rc.1")));

    // Branch content: own version and lock, changelog; no pin in api, changeset kept.
    assert!(git(
        root,
        &[
            "show",
            "release/livekit-protocol-rc:livekit-protocol/Cargo.toml"
        ]
    )
    .contains("0.4.0-rc.1"));
    assert!(
        git(root, &["show", "release/livekit-protocol-rc:Cargo.lock"])
            .contains("name = \"livekit-protocol\"\nversion = \"0.4.0-rc.1\"")
    );
    assert!(git(
        root,
        &[
            "show",
            "release/livekit-protocol-rc:livekit-protocol/CHANGELOG.md"
        ]
    )
    .contains("## 0.4.0-rc.1"));
    assert!(git(
        root,
        &["show", "release/livekit-protocol-rc:livekit-api/Cargo.toml"]
    )
    .contains("workspace = true"));
    git(
        root,
        &["show", "release/livekit-protocol-rc:.changeset/ndc.md"],
    );
    assert!(git(root, &["ls-remote", "--heads", "origin"]).contains("release/livekit-protocol-rc"));

    // After rc.1 is merged and tagged, the next request gives rc.2 and
    // the automatic plan promotes to the final version.
    git(
        root,
        &[
            "tag",
            "livekit-protocol-v0.4.0-rc.1",
            "release/livekit-protocol-rc",
        ],
    );
    git(
        root,
        &["merge", "-q", "--no-edit", "release/livekit-protocol-rc"],
    );
    let repo = Repo::load(root).unwrap();
    let next = verwalter::prerelease::plan(&repo, "livekit-protocol", None).unwrap();
    assert_eq!(next.to.to_string(), "0.4.0-rc.2");
    let p = plan(
        &repo.packages,
        &repo.changesets,
        repo.config.dependent_bump_level,
    );
    assert_eq!(
        p.iter()
            .find(|r| r.package == "livekit-protocol")
            .unwrap()
            .to
            .to_string(),
        "0.4.0"
    );
    let _ = fs::remove_dir_all(&remote);
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
    let remote = std::env::temp_dir().join(format!(
        "verwalter-it-{}-dry-remote.git",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&remote);
    git(root, &["init", "-q", "--bare", remote.to_str().unwrap()]);
    git(root, &["remote", "add", "origin", remote.to_str().unwrap()]);
    git(root, &["push", "-q", "origin", "main"]);

    let repo = Repo::load(root).unwrap();
    let plan = plan(
        &repo.packages,
        &repo.changesets,
        repo.config.dependent_bump_level,
    );
    let branches = branches::create(&repo, &plan, "main").unwrap();
    let writes = std::cell::Cell::new(0);
    let inner = |args: &[&str]| -> Result<String, verwalter::Error> {
        if !matches!(args, ["pr", "list", ..] | ["release", "view", ..]) {
            writes.set(writes.get() + 1);
        }
        Ok(match args {
            ["pr", "list", ..] => "[]".into(),
            ["release", "view", ..] => Err("not found")?,
            _ => String::new(),
        })
    };
    let mut gh = verwalter::github::dry_run_gh(inner);
    let prs = verwalter::github::sync(&repo, &plan, &branches, "origin", true, &mut gh).unwrap();
    assert_eq!(prs.len(), 3);
    let released = verwalter::release::run(
        &repo,
        "origin",
        &verwalter::release::Options {
            dry_run: true,
            ..Default::default()
        },
        &mut gh,
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
    let _ = fs::remove_dir_all(&remote);
}

#[test]
fn release_finishes_a_partial_release_for_one_package() {
    let r = fixture("partial");
    let root = &r.0;
    let remote = std::env::temp_dir().join(format!(
        "verwalter-it-{}-partial-remote.git",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&remote);
    git(root, &["init", "-q", "--bare", remote.to_str().unwrap()]);
    git(root, &["remote", "add", "origin", remote.to_str().unwrap()]);
    // livekit: tagged, draft release without assets (the upload failed).
    // livekit-api: not tagged at all, but out of scope via --package.
    git(root, &["tag", "livekit-v0.7.0"]);
    git(root, &["tag", "livekit-protocol-v0.3.1"]);
    git(root, &["push", "-q", "origin", "--tags"]);

    let repo = Repo::load(root).unwrap();
    let calls = std::cell::RefCell::new(Vec::<String>::new());
    let mut gh = |args: &[&str]| -> Result<String, verwalter::Error> {
        let published = calls
            .borrow()
            .iter()
            .any(|c| c.starts_with("release edit livekit-v0.7.0 --draft=false"));
        calls.borrow_mut().push(args.join(" "));
        match args {
            ["release", "view", "livekit-v0.7.0", ..] => {
                Ok(format!(r#"{{"isDraft":{},"assets":[]}}"#, !published))
            }
            ["release", "view", ..] => Ok(r#"{"isDraft":false,"assets":[{"name":"x"}]}"#.into()),
            _ => Ok(String::new()),
        }
    };
    let options = verwalter::release::Options {
        assets: vec!["dist/a.tar.gz".into()],
        package: Some("livekit".into()),
        publish: true,
        ..Default::default()
    };
    let released = verwalter::release::run(&repo, "origin", &options, &mut gh).unwrap();
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
    assert!(calls
        .iter()
        .any(|c| c == "release edit livekit-v0.7.0 --draft=false"));
    assert!(
        !calls.iter().any(|c| c.contains("livekit-api")),
        "other packages untouched"
    );
    assert!(
        git(root, &["tag", "--list", "livekit-api-v0.4.3"]).is_empty(),
        "api not tagged"
    );
    let _ = fs::remove_dir_all(&remote);
}

#[test]
fn release_error_names_the_resume_command() {
    let r = fixture("resume-hint");
    let root = &r.0;
    // No remote: the tag push fails.
    let repo = Repo::load(root).unwrap();
    let mut gh = |_: &[&str]| -> Result<String, verwalter::Error> { Ok(String::new()) };
    let err = verwalter::release::run(
        &repo,
        "origin",
        &verwalter::release::Options {
            assets: vec!["dist/x".into()],
            ..Default::default()
        },
        &mut gh,
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
    let remote = std::env::temp_dir().join(format!(
        "verwalter-it-{}-status-remote.git",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&remote);
    git(root, &["init", "-q", "--bare", remote.to_str().unwrap()]);
    git(root, &["remote", "add", "origin", remote.to_str().unwrap()]);
    // protocol: complete. api: tagged locally only, draft without assets.
    // livekit: version committed, no tag; a changeset is pending.
    git(root, &["tag", "livekit-protocol-v0.3.1"]);
    git(root, &["push", "-q", "origin", "livekit-protocol-v0.3.1"]);
    git(root, &["tag", "livekit-api-v0.4.3"]);
    write(root, ".changeset/x.md", "---\nlivekit: patch\n---\nx\n");
    git(root, &["add", "-A"]);
    git(root, &["commit", "-q", "-m", "cs"]);

    let repo = Repo::load(root).unwrap();
    let mut gh = |args: &[&str]| -> Result<String, verwalter::Error> {
        match args {
            ["pr", "list", ..] => Ok(
                r#"[{"number":12,"headRefName":"release/livekit","baseRefName":"main"}]"#.into(),
            ),
            ["release", "view", "livekit-protocol-v0.3.1", ..] => {
                Ok(r#"{"isDraft":false,"assets":[{"name":"a"},{"name":"b"}]}"#.into())
            }
            ["release", "view", "livekit-api-v0.4.3", ..] => {
                Ok(r#"{"isDraft":true,"assets":[]}"#.into())
            }
            _ => Err("not found".into()),
        }
    };
    let rows = verwalter::status::status(&repo, "origin", &mut gh).unwrap();
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
    let _ = fs::remove_dir_all(&remote);
}

#[test]
fn release_runs_in_dependency_order_and_stops_at_the_first_failure() {
    let r = fixture("order");
    let root = &r.0;
    let remote = std::env::temp_dir().join(format!(
        "verwalter-it-{}-order-remote.git",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&remote);
    git(root, &["init", "-q", "--bare", remote.to_str().unwrap()]);
    git(root, &["remote", "add", "origin", remote.to_str().unwrap()]);

    // All three untagged: protocol first, then its dependents by name.
    let repo = Repo::load(root).unwrap();
    let calls = std::cell::RefCell::new(Vec::<String>::new());
    let mut gh = |args: &[&str]| -> Result<String, verwalter::Error> {
        calls.borrow_mut().push(args.join(" "));
        match args {
            ["release", "view", ..] => Err("not found".into()),
            _ => Ok(String::new()),
        }
    };
    let released = verwalter::release::run(&repo, "origin", &Default::default(), &mut gh).unwrap();
    let order: Vec<&str> = released.iter().map(|r| r.package.as_str()).collect();
    assert_eq!(order, ["livekit-protocol", "livekit", "livekit-api"]);
    assert!(released[0].depends_on.is_empty());
    assert_eq!(released[1].depends_on, ["livekit-protocol"]);
    assert_eq!(released[2].depends_on, ["livekit-protocol"]);

    // Fail at the dependency: nothing happens to the dependents.
    let r2 = fixture("order-fail");
    let root2 = &r2.0;
    let repo2 = Repo::load(root2).unwrap();
    let mut gh2 = |args: &[&str]| -> Result<String, verwalter::Error> {
        match args {
            ["release", "view", ..] => Err("not found".into()),
            ["release", "create", tag, ..] if tag.starts_with("livekit-protocol") => {
                Err("boom".into())
            }
            _ => Ok(String::new()),
        }
    };
    // Give it a remote so the tag push works and the gh failure is the first error.
    let remote2 = std::env::temp_dir().join(format!(
        "verwalter-it-{}-order-fail-remote.git",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&remote2);
    git(root2, &["init", "-q", "--bare", remote2.to_str().unwrap()]);
    git(
        root2,
        &["remote", "add", "origin", remote2.to_str().unwrap()],
    );
    let err = verwalter::release::run(&repo2, "origin", &Default::default(), &mut gh2)
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
    let _ = fs::remove_dir_all(&remote);
    let _ = fs::remove_dir_all(&remote2);
}

#[test]
fn named_package_is_reported_even_when_complete() {
    let r = fixture("named");
    let root = &r.0;
    let remote = std::env::temp_dir().join(format!(
        "verwalter-it-{}-named-remote.git",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&remote);
    git(root, &["init", "-q", "--bare", remote.to_str().unwrap()]);
    git(root, &["remote", "add", "origin", remote.to_str().unwrap()]);
    git(root, &["tag", "livekit-api-v0.4.3"]);
    git(root, &["push", "-q", "origin", "--tags"]);
    let repo = Repo::load(root).unwrap();
    let mut gh = |args: &[&str]| -> Result<String, verwalter::Error> {
        match args {
            ["release", "view", ..] => Ok(r#"{"isDraft":false,"assets":[{"name":"a"}]}"#.into()),
            other => panic!("unexpected write {other:?}"),
        }
    };
    let options = verwalter::release::Options {
        package: Some("livekit-api".into()),
        ..Default::default()
    };
    let released = verwalter::release::run(&repo, "origin", &options, &mut gh).unwrap();
    assert_eq!(released.len(), 1);
    assert_eq!(
        (released[0].package.as_str(), released[0].status),
        ("livekit-api", "published")
    );
    let _ = fs::remove_dir_all(&remote);
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
    let mut gh = |args: &[&str]| -> Result<String, verwalter::Error> {
        let path = args.get(1).copied().unwrap_or("");
        Ok(
            if path == format!("repos/{{owner}}/{{repo}}/commits/{pr_sha}/pulls") {
                "12\thttps://github.com/o/r/pull/12\tlukas".into()
            } else if path == format!("repos/{{owner}}/{{repo}}/commits/{push_sha}") {
                format!("https://github.com/o/r/commit/{push_sha}\t-")
            } else {
                String::new()
            },
        )
    };
    verwalter::github::attribute(&mut repo, &mut gh);
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
