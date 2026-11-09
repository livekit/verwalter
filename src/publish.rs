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

use anyhow::{Context, Result};
use serde::Serialize;

use crate::{
    config::ReleaseMode,
    git, github,
    registry::{self, Http},
    repo::Repo,
};

#[derive(Debug, Clone, Default)]
pub struct Options {
    /// Overrides `[publish].jobs`.
    pub jobs: Option<usize>,
    pub dry_run: bool,
    /// Spec: WorkflowRunsPublish.releases. The packages this run released,
    /// from the `release` step's output. None publishes every tagged
    /// package, for a re-run by hand; the CLI maps an empty list to None.
    pub only: Option<BTreeSet<String>>,
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
    /// The npm dist-tag the version belongs under: `next`, `latest-1`,
    /// else `latest`. Set for every registry as `VERWALTER_NPM_TAG`; only an
    /// npm command has a use for it.
    dist_tag: String,
    /// The packages a `[publish].workspace` command covers; empty for a
    /// package of its own.
    members: Vec<(String, crate::Version, String)>,
}

/// Name of the synthetic candidate for `[publish].workspace`.
const WORKSPACE: &str = "[workspace]";

/// pnpm finds its workspace by walking up from the package directory to
/// the nearest `pnpm-workspace.yaml`, which need not be at the repo root.
fn in_pnpm_workspace(root: &std::path::Path, path: &str) -> bool {
    let mut dir = root.join(path);
    loop {
        if dir.join("pnpm-workspace.yaml").exists() {
            return true;
        }
        if dir == root || !dir.pop() || !dir.starts_with(root) {
            return false;
        }
    }
}

/// Every package whose current version is tagged, or only those in
/// `only`, with its in-repo dependencies among those packages. A
/// candidate without a publish command is an error before anything runs.
fn candidates(repo: &Repo, only: Option<&BTreeSet<String>>) -> Result<BTreeMap<String, Candidate>> {
    let mut out = BTreeMap::new();
    for p in &repo.packages {
        let cfg = &repo.config.packages[&p.name];
        if cfg.release.mode == ReleaseMode::None || only.is_some_and(|o| !o.contains(&p.name)) {
            continue;
        }
        // Spec: Package.publish_command. `registry = "none"` set on the
        // package itself, with no command, says there is nothing to
        // publish: a GitHub-release-only package. The default `none`
        // with nothing configured anywhere is still an error below.
        if cfg.publish.registry == Some(crate::registry::Registry::None)
            && cfg.publish.command.is_none()
        {
            continue;
        }
        let tag = repo.config.tag_for(&p.name, &p.version);
        if !git::ref_exists(&repo.root, &tag) {
            continue;
        }
        let released = crate::prerelease::released_versions(repo, &p.name)?;
        let npm_tag = crate::version::npm_dist_tag(&p.version, &released);
        let dist_tag = npm_tag.clone().unwrap_or_else(|| "latest".into());
        if repo.config.in_workspace_publish(&p.name) {
            let ws = out
                .entry(WORKSPACE.to_string())
                .or_insert_with(|| Candidate {
                    version: p.version.clone(),
                    tag: String::new(),
                    deps: Vec::new(),
                    command: repo.config.publish.workspace.clone().unwrap_or_default(),
                    path: ".".into(),
                    // ponytail: the first member decides; an rc PR moves every member.
                    dist_tag: dist_tag.clone(),
                    members: Vec::new(),
                });
            ws.deps.extend(p.dependencies.iter().cloned());
            ws.members.push((p.name.clone(), p.version.clone(), tag));
            continue;
        }
        // Spec: Registry goproxy. The proxy request is the publish: an
        // empty command marks it.
        let goproxy = repo.config.registry_of(&p.name) == registry::Registry::Goproxy;
        let command = repo.config.publish_command(&p.name, in_pnpm_workspace(&repo.root, &cfg.path), npm_tag.as_deref()).or_else(|| goproxy.then(String::new)).with_context(|| {
            format!("{}: no publish command: set `publish.registry` or `publish.command` on the package, or in `[publish]`", p.name)
        })?;
        out.insert(
            p.name.clone(),
            Candidate {
                version: p.version.clone(),
                tag,
                deps: p.dependencies.clone(),
                command,
                path: cfg.path.clone(),
                dist_tag,
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

fn outcome(
    package: &str,
    version: &crate::Version,
    tag: &str,
    status: &'static str,
    reason: impl Into<String>,
) -> Outcome {
    Outcome {
        package: package.to_string(),
        version: version.to_string(),
        tag: tag.to_string(),
        status,
        reason: reason.into(),
    }
}

/// Per-member outcomes after the workspace command: the registry says
/// what is there; without a registry the command's exit status does.
fn workspace_outcomes(repo: &Repo, c: &Candidate, ok: bool, http: Http) -> Result<Vec<Outcome>> {
    let mut out = Vec::new();
    for (name, version, tag) in &c.members {
        let status =
            match registry::exists(registry::version_url(&repo.config, name, version), http)? {
                Some(true) => "published",
                Some(false) => "failed",
                None if ok => "published",
                None => "failed",
            };
        let reason = if status == "failed" {
            "not on the registry after the workspace command"
        } else {
            ""
        };
        out.push(outcome(name, version, tag, status, reason));
    }
    Ok(out)
}

fn run_command(repo: &Repo, name: &str, c: &Candidate, first: bool) -> (bool, String, String) {
    let command = c
        .command
        .replace("{package}", name)
        .replace("{version}", &c.version.to_string())
        .replace("{tag}", &c.tag)
        .replace("{npm_tag}", &c.dist_tag);
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
        .env("VERWALTER_NPM_TAG", &c.dist_tag)
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

/// Spec: PublishFlipsDrafts. A package is finished when the registry has
/// it and its GitHub release, if it is a draft, is published. A flip that
/// fails turns the outcome into a failure, so a re-run comes back: the
/// registry then says `exists`, and the flip is tried again.
fn finish(repo: &Repo, gh: &dyn github::Client, o: &mut Outcome) {
    if !matches!(o.status, "published" | "exists")
        || repo.config.packages[&o.package].release.mode != ReleaseMode::TagAndRelease
    {
        return;
    }
    let draft = match crate::release::view(gh, &o.tag) {
        Ok(state) => state.is_some_and(|s| s.draft),
        Err(e) => {
            o.status = "failed";
            o.reason = format!(
                "on the registry, but the release {} could not be read: {e}",
                o.tag
            );
            return;
        }
    };
    if !draft {
        return;
    }
    match gh.execute(&["release", "edit", &o.tag, "--draft=false"]) {
        Ok(_) => println!("{}: published the draft release {}", o.package, o.tag),
        Err(e) => {
            o.status = "failed";
            o.reason = format!(
                "on the registry, but the draft release {} could not be published: {e}",
                o.tag
            );
        }
    }
}

/// Run the publish command for every tagged package. Returns one outcome
/// per package, in the order they finished. Then flips the draft release
/// of every package the registry has (`finish`).
pub fn run(
    repo: &Repo,
    options: &Options,
    http: Http,
    gh: &dyn github::Client,
) -> Result<Vec<Outcome>> {
    let candidates = candidates(repo, options.only.as_ref())?;
    let jobs = options.jobs.unwrap_or(repo.config.publish.jobs).max(1);
    let mut done: BTreeMap<String, &'static str> = BTreeMap::new();
    let mut outcomes = Vec::new();
    let mut running: BTreeSet<String> = BTreeSet::new();
    let (tx, rx) = mpsc::channel::<(String, bool, String, String)>();

    std::thread::scope(|scope| -> Result<()> {
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
                        outcomes.push(outcome(
                            name,
                            &c.version,
                            &c.tag,
                            "blocked",
                            format!("{dep} did not publish"),
                        ));
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
                            outcomes.push(outcome(m, v, t, "exists", "already on the registry"));
                        }
                        resolved = true;
                        continue;
                    }
                } else if c.command.is_empty() {
                    // goproxy: asking for the version makes the proxy fetch it.
                    let url = registry::version_url(&repo.config, name, &c.version);
                    let (status, reason) = if options.dry_run {
                        ("dry-run", format!("GET {}", url.unwrap_or_default()))
                    } else {
                        match registry::exists(url, http)? {
                            Some(true) => ("published", String::new()),
                            _ => (
                                "failed",
                                "the Go module proxy does not serve the tag yet: re-run publish"
                                    .to_string(),
                            ),
                        }
                    };
                    done.insert(name.clone(), status);
                    outcomes.push(outcome(name, &c.version, &c.tag, status, reason));
                    resolved = true;
                    continue;
                } else {
                    let on_registry = registry::exists(
                        registry::version_url(&repo.config, name, &c.version),
                        http,
                    )?;
                    if on_registry == Some(true) {
                        done.insert(name.clone(), "exists");
                        outcomes.push(outcome(
                            name,
                            &c.version,
                            &c.tag,
                            "exists",
                            "already on the registry",
                        ));
                        resolved = true;
                        continue;
                    }
                }
                let first = name != WORKSPACE
                    && crate::release::first_release(repo, name, &c.version, http)?;
                if options.dry_run {
                    println!("would run in {}: {}", c.path, c.command);
                    done.insert(name.clone(), "dry-run");
                    let own = [(name.clone(), c.version.clone(), c.tag.clone())];
                    let units = if c.members.is_empty() {
                        &own[..]
                    } else {
                        &c.members
                    };
                    for (m, v, t) in units {
                        done.insert(m.clone(), "dry-run");
                        // The reason carries the command, so a dry run
                        // shows what each package would run.
                        outcomes.push(outcome(m, v, t, "dry-run", &c.command));
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
                        outcomes.push(outcome(
                            name,
                            &c.version,
                            &c.tag,
                            "blocked",
                            "dependency cycle",
                        ));
                    }
                }
                return Ok(());
            }
            let (name, ok, output, status) = rx.recv()?;
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
            let reason = if ok { String::new() } else { status };
            outcomes.push(outcome(
                &name,
                &c.version,
                &c.tag,
                if ok { "published" } else { "failed" },
                reason,
            ));
        }
    })?;
    for o in &mut outcomes {
        finish(repo, gh, o);
    }
    Ok(outcomes)
}

/// Spec: WorkflowPublishHandoff. Appends to `$GITHUB_OUTPUT` when set.
/// Returns the summary for stdout.
pub fn write_outputs(outcomes: &[Outcome]) -> Result<String> {
    let names = |status: &str| {
        serde_json::to_string(
            &outcomes
                .iter()
                .filter(|o| o.status == status)
                .map(|o| o.package.as_str())
                .collect::<Vec<_>>(),
        )
    };
    crate::github::append_output(&format!(
        "published={}\nfailed={}\nblocked={}\noutcomes={}\n",
        names("published")?,
        names("failed")?,
        names("blocked")?,
        serde_json::to_string(outcomes)?
    ))?;
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
