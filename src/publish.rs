//! Spec: rule WorkflowPublishesInDependencyOrder and surface
//! WorkflowPublishHandoff. verwalter runs the publish command once per
//! tagged package, dependencies first, up to `jobs` at once. A failure
//! blocks every transitive dependent and nothing else. A version the
//! registry already has is skipped before the command runs. Nothing is
//! remembered between runs: a re-run repeats the command and the checks.

use std::{
    collections::{BTreeMap, BTreeSet},
    process::Command,
    sync::mpsc,
};

use serde::Serialize;

use crate::{
    config::ReleaseMode,
    git,
    registry::{self, Http},
    repo::Repo,
    Error,
};

#[derive(Debug, Clone, Default)]
pub struct Options {
    /// Overrides `[publish].jobs`.
    pub jobs: Option<usize>,
    pub dry_run: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Outcome {
    pub package: String,
    pub version: String,
    pub tag: String,
    /// published | exists | failed | blocked | dry-run
    pub status: &'static str,
    /// The failed dependency for `blocked`, the exit status for `failed`.
    pub reason: String,
}

struct Candidate {
    version: crate::Version,
    tag: String,
    deps: Vec<String>,
    /// Spec: Package.publish_command, resolved; the directory it runs in.
    command: String,
    path: String,
}

/// Every package whose current version is tagged, with its in-repo
/// dependencies among those packages. A candidate without a publish
/// command is an error before anything runs.
fn candidates(repo: &Repo) -> Result<BTreeMap<String, Candidate>, Error> {
    let mut out = BTreeMap::new();
    for p in &repo.packages {
        let cfg = &repo.config.packages[&p.name];
        if cfg.release == ReleaseMode::None {
            continue;
        }
        let tag = repo.config.tag_for(&p.name, &p.version);
        if !git::ref_exists(&repo.root, &tag) {
            continue;
        }
        let command = repo.config.publish_command(&p.name).ok_or_else(|| {
            format!("{}: no publish command: set `registry` or `publish` on the package, or `[publish]` defaults", p.name)
        })?;
        out.insert(
            p.name.clone(),
            Candidate {
                version: p.version.clone(),
                tag,
                deps: p.dependencies.clone(),
                command,
                path: cfg.path.clone(),
            },
        );
    }
    let names: BTreeSet<String> = out.keys().cloned().collect();
    for c in out.values_mut() {
        c.deps.retain(|d| names.contains(d));
    }
    Ok(out)
}

fn run_command(repo: &Repo, name: &str, c: &Candidate, first: bool) -> (bool, String, String) {
    let command = c
        .command
        .replace("{package}", name)
        .replace("{version}", &c.version.to_string())
        .replace("{tag}", &c.tag);
    let registry = format!("{:?}", repo.config.registry_of(name)).to_lowercase();
    let out = Command::new("sh")
        .arg("-c")
        .arg(&command)
        .current_dir(repo.root.join(&c.path))
        .env("VERWALTER_PACKAGE", name)
        .env("VERWALTER_REGISTRY", registry)
        .env("VERWALTER_ROOT", &repo.root)
        .env("VERWALTER_VERSION", c.version.to_string())
        .env("VERWALTER_TAG", &c.tag)
        .env("VERWALTER_PRERELEASE", c.version.is_prerelease().to_string())
        .env("VERWALTER_FIRST_RELEASE", first.to_string())
        .output();
    match out {
        Ok(o) => (
            o.status.success(),
            String::from_utf8_lossy(&o.stdout).into_owned() + &String::from_utf8_lossy(&o.stderr),
            o.status.to_string(),
        ),
        Err(e) => (false, String::new(), e.to_string()),
    }
}

/// Run the publish command for every tagged package. Returns one outcome
/// per package, in the order they finished.
pub fn run(repo: &Repo, options: &Options, http: Http) -> Result<Vec<Outcome>, Error> {
    let candidates = candidates(repo)?;
    let jobs = options.jobs.unwrap_or(repo.config.publish.jobs).max(1);
    let mut done: BTreeMap<String, &'static str> = BTreeMap::new();
    let mut outcomes = Vec::new();
    let mut running: BTreeSet<String> = BTreeSet::new();
    let (tx, rx) = mpsc::channel::<(String, bool, String, String)>();

    std::thread::scope(|scope| -> Result<(), Error> {
        loop {
            // Block whatever depends on a failure, then start what is ready.
            let mut progressed = true;
            while progressed {
                progressed = false;
                for (name, c) in &candidates {
                    if done.contains_key(name) || running.contains(name) {
                        continue;
                    }
                    if let Some(dep) = c.deps.iter().find(|d| matches!(done.get(*d).copied(), Some("failed" | "blocked"))) {
                        done.insert(name.clone(), "blocked");
                        outcomes.push(Outcome {
                            package: name.clone(),
                            version: c.version.to_string(),
                            tag: c.tag.clone(),
                            status: "blocked",
                            reason: format!("{dep} did not publish"),
                        });
                        progressed = true;
                    }
                }
            }
            let mut resolved = false;
            for (name, c) in &candidates {
                if running.len() >= jobs {
                    break;
                }
                if done.contains_key(name) || running.contains(name) {
                    continue;
                }
                if !c.deps.iter().all(|d| matches!(done.get(d).copied(), Some("published" | "exists" | "dry-run"))) {
                    continue;
                }
                let on_registry = registry::exists(registry::version_url(&repo.config, name, &c.version), http)?;
                if on_registry == Some(true) {
                    done.insert(name.clone(), "exists");
                    outcomes.push(Outcome {
                        package: name.clone(),
                        version: c.version.to_string(),
                        tag: c.tag.clone(),
                        status: "exists",
                        reason: "already on the registry".into(),
                    });
                    resolved = true;
                    continue;
                }
                let first = match registry::exists(registry::package_url(&repo.config, name), http)? {
                    Some(has_any) => !has_any,
                    None => crate::release::first_release_by_tags(repo, name, &c.version)?,
                };
                if options.dry_run {
                    println!("would run in {}: {}", c.path, c.command);
                    done.insert(name.clone(), "dry-run");
                    outcomes.push(Outcome {
                        package: name.clone(),
                        version: c.version.to_string(),
                        tag: c.tag.clone(),
                        status: "dry-run",
                        reason: String::new(),
                    });
                    resolved = true;
                    continue;
                }
                running.insert(name.clone());
                let tx = tx.clone();
                scope.spawn(move || {
                    let (ok, output, status) = run_command(repo, name, c, first);
                    let _ = tx.send((name.clone(), ok, output, status));
                });
            }
            if running.is_empty() {
                if resolved {
                    continue;
                }
                // Nothing running and nothing startable: anything left is
                // waiting on a dependency that never resolves (a cycle).
                for (name, c) in &candidates {
                    if !done.contains_key(name) {
                        done.insert(name.clone(), "blocked");
                        outcomes.push(Outcome {
                            package: name.clone(),
                            version: c.version.to_string(),
                            tag: c.tag.clone(),
                            status: "blocked",
                            reason: "dependency cycle".into(),
                        });
                    }
                }
                return Ok(());
            }
            let (name, ok, output, status) = rx.recv().map_err(|e| e.to_string())?;
            running.remove(&name);
            let c = &candidates[&name];
            println!("::group::{name} {} ({})", c.version, if ok { "published" } else { "failed" });
            print!("{output}");
            println!("::endgroup::");
            done.insert(name.clone(), if ok { "published" } else { "failed" });
            outcomes.push(Outcome {
                package: name.clone(),
                version: c.version.to_string(),
                tag: c.tag.clone(),
                status: if ok { "published" } else { "failed" },
                reason: if ok { String::new() } else { status },
            });
        }
    })?;
    Ok(outcomes)
}

/// Spec: WorkflowPublishHandoff. Appends to `$GITHUB_OUTPUT` when set.
/// Returns the summary for stdout.
pub fn write_outputs(outcomes: &[Outcome]) -> Result<String, Error> {
    let names = |status: &str| -> Vec<&str> {
        outcomes
            .iter()
            .filter(|o| o.status == status)
            .map(|o| o.package.as_str())
            .collect()
    };
    if let Ok(path) = std::env::var("GITHUB_OUTPUT") {
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new().append(true).create(true).open(path)?;
        writeln!(f, "published={}", serde_json::to_string(&names("published"))?)?;
        writeln!(f, "failed={}", serde_json::to_string(&names("failed"))?)?;
        writeln!(f, "blocked={}", serde_json::to_string(&names("blocked"))?)?;
        writeln!(f, "outcomes={}", serde_json::to_string(outcomes)?)?;
    }
    let w = outcomes.iter().map(|o| o.package.len()).max().unwrap_or(0);
    Ok(outcomes
        .iter()
        .map(|o| format!("{:w$}  {}  {}  {}", o.package, o.version, o.status, o.reason).trim_end().to_string())
        .collect::<Vec<_>>()
        .join("\n"))
}

/// True when any outcome is a failure or a block.
pub fn failed(outcomes: &[Outcome]) -> bool {
    outcomes.iter().any(|o| matches!(o.status, "failed" | "blocked"))
}
