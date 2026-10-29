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
    /// The packages a `[publish].workspace` command covers; empty for a
    /// package of its own.
    members: Vec<(String, crate::Version, String)>,
}

/// Name of the synthetic candidate for `[publish].workspace`.
const WORKSPACE: &str = "[workspace]";

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
        if repo.config.in_workspace_publish(&p.name) {
            let ws = out
                .entry(WORKSPACE.to_string())
                .or_insert_with(|| Candidate {
                    version: p.version.clone(),
                    tag: String::new(),
                    deps: Vec::new(),
                    command: repo.config.publish.workspace.clone().unwrap_or_default(),
                    path: ".".into(),
                    members: Vec::new(),
                });
            ws.deps.extend(p.dependencies.iter().cloned());
            ws.members.push((p.name.clone(), p.version.clone(), tag));
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
                members: Vec::new(),
            },
        );
    }
    // Dependencies among the candidates only. A workspace member counts
    // as done once the workspace command ran, so dependents name the
    // member and wait for that; the workspace waits for what its members
    // need from outside.
    let mut names: BTreeSet<String> = out.keys().cloned().collect();
    if let Some(ws) = out.get(WORKSPACE) {
        names.extend(ws.members.iter().map(|m| m.0.clone()));
    }
    let members: BTreeSet<String> = out
        .get(WORKSPACE)
        .map(|ws| ws.members.iter().map(|m| m.0.clone()).collect())
        .unwrap_or_default();
    for (name, c) in out.iter_mut() {
        c.deps.retain(|d| names.contains(d));
        if name == WORKSPACE {
            c.deps.retain(|d| !members.contains(d));
            c.deps.sort();
            c.deps.dedup();
        }
    }
    Ok(out)
}

/// Per-member outcomes after the workspace command: the registry says
/// what is there; without a registry the command's exit status does.
fn workspace_outcomes(
    repo: &Repo,
    c: &Candidate,
    ok: bool,
    http: Http,
) -> Result<Vec<Outcome>, Error> {
    let mut out = Vec::new();
    for (name, version, tag) in &c.members {
        let status =
            match registry::exists(registry::version_url(&repo.config, name, version), http)? {
                Some(true) => "published",
                Some(false) => "failed",
                None if ok => "published",
                None => "failed",
            };
        out.push(Outcome {
            package: name.clone(),
            version: version.to_string(),
            tag: tag.clone(),
            status,
            reason: if status == "failed" {
                "not on the registry after the workspace command".into()
            } else {
                String::new()
            },
        });
    }
    Ok(out)
}

fn run_command(repo: &Repo, name: &str, c: &Candidate, first: bool) -> (bool, String, String) {
    let command = c
        .command
        .replace("{package}", name)
        .replace("{version}", &c.version.to_string())
        .replace("{tag}", &c.tag);
    let registry = format!(
        "{:?}",
        if name == WORKSPACE {
            repo.config.publish.registry
        } else {
            repo.config.registry_of(name)
        }
    )
    .to_lowercase();
    let out = Command::new("sh")
        .arg("-c")
        .arg(&command)
        .current_dir(repo.root.join(&c.path))
        .env("VERWALTER_PACKAGE", name)
        .env("VERWALTER_REGISTRY", registry)
        .env("VERWALTER_ROOT", &repo.root)
        .env("VERWALTER_VERSION", c.version.to_string())
        .env("VERWALTER_TAG", &c.tag)
        .env(
            "VERWALTER_PRERELEASE",
            c.version.is_prerelease().to_string(),
        )
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
                    if let Some(dep) = c
                        .deps
                        .iter()
                        .find(|d| matches!(done.get(*d).copied(), Some("failed" | "blocked")))
                    {
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
                if !c.deps.iter().all(|d| {
                    matches!(
                        done.get(d).copied(),
                        Some("published" | "exists" | "dry-run")
                    )
                }) {
                    continue;
                }
                if name == WORKSPACE {
                    // Skip the command only when every member is there.
                    let mut all = true;
                    for (m, v, _) in &c.members {
                        if registry::exists(registry::version_url(&repo.config, m, v), http)?
                            != Some(true)
                        {
                            all = false;
                            break;
                        }
                    }
                    if all {
                        done.insert(name.clone(), "exists");
                        for (m, v, t) in &c.members {
                            done.insert(m.clone(), "exists");
                            outcomes.push(Outcome {
                                package: m.clone(),
                                version: v.to_string(),
                                tag: t.clone(),
                                status: "exists",
                                reason: "already on the registry".into(),
                            });
                        }
                        resolved = true;
                        continue;
                    }
                } else {
                    let on_registry = registry::exists(
                        registry::version_url(&repo.config, name, &c.version),
                        http,
                    )?;
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
                }
                let first = if name == WORKSPACE {
                    false
                } else {
                    match registry::exists(registry::package_url(&repo.config, name), http)? {
                        Some(has_any) => !has_any,
                        None => crate::release::first_release_by_tags(repo, name, &c.version)?,
                    }
                };
                if options.dry_run {
                    println!("would run in {}: {}", c.path, c.command);
                    done.insert(name.clone(), "dry-run");
                    let units: Vec<(String, String, String)> = if c.members.is_empty() {
                        vec![(name.clone(), c.version.to_string(), c.tag.clone())]
                    } else {
                        c.members
                            .iter()
                            .map(|(m, v, t)| (m.clone(), v.to_string(), t.clone()))
                            .collect()
                    };
                    for (m, v, t) in units {
                        done.insert(m.clone(), "dry-run");
                        outcomes.push(Outcome {
                            package: m,
                            version: v,
                            tag: t,
                            status: "dry-run",
                            reason: String::new(),
                        });
                    }
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
            println!(
                "::group::{name} {} ({})",
                c.version,
                if ok { "published" } else { "failed" }
            );
            print!("{output}");
            println!("::endgroup::");
            done.insert(name.clone(), if ok { "published" } else { "failed" });
            if name == WORKSPACE {
                for o in workspace_outcomes(repo, c, ok, http)? {
                    done.insert(o.package.clone(), o.status);
                    outcomes.push(o);
                }
                continue;
            }
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
        let mut f = std::fs::OpenOptions::new()
            .append(true)
            .create(true)
            .open(path)?;
        writeln!(
            f,
            "published={}",
            serde_json::to_string(&names("published"))?
        )?;
        writeln!(f, "failed={}", serde_json::to_string(&names("failed"))?)?;
        writeln!(f, "blocked={}", serde_json::to_string(&names("blocked"))?)?;
        writeln!(f, "outcomes={}", serde_json::to_string(outcomes)?)?;
    }
    let w = outcomes.iter().map(|o| o.package.len()).max().unwrap_or(0);
    Ok(outcomes
        .iter()
        .map(|o| {
            format!(
                "{:w$}  {}  {}  {}",
                o.package, o.version, o.status, o.reason
            )
            .trim_end()
            .to_string()
        })
        .collect::<Vec<_>>()
        .join("\n"))
}

/// True when any outcome is a failure or a block.
pub fn failed(outcomes: &[Outcome]) -> bool {
    outcomes
        .iter()
        .any(|o| matches!(o.status, "failed" | "blocked"))
}
