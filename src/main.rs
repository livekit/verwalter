use std::{
    path::{Path, PathBuf},
    process::ExitCode,
};

use verwalter::{
    branches, check, github, plan::plan, prerelease, release, repo::Repo, status,
    version::BumpLevel, workspace, Error,
};

const USAGE: &str = "usage: verwalter <status|plan|branches|pr> [--config <file>] [--base <ref>] [--remote <name>] [--dry-run] [<checkout>]\n       verwalter changeset [--level major|minor|patch] [--package <name>]... [--base <ref>] [--summary <text>] [<checkout>]\n       verwalter release [--asset <file>]... [--package <name>] [--publish] [--abandon <tag>] [--remote <name>] [--dry-run] [<checkout>]\n       verwalter check --pr <number> [--config <file>] [--dry-run] [<checkout>]\n       verwalter publish [--releases <json>] [--jobs <n>] [--dry-run] [<checkout>]\n       verwalter pre-release <package> [--target <branch>] [--level major|minor|patch] [--remote <name>] [--dry-run] [<checkout>]";

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), Error> {
    let mut args = std::env::args().skip(1);
    let Some(command) = args.next() else {
        return Err(USAGE.into());
    };
    let mut package = None;
    if command == "pre-release" {
        package = Some(args.next().ok_or(USAGE)?);
    }
    let mut config = None;
    let mut base = None;
    let mut remote = String::from("origin");
    let mut pr: Option<u64> = None;
    let mut assets: Vec<String> = Vec::new();
    let mut abandon: Option<String> = None;
    let mut target: Option<String> = None;
    let mut summary: Option<String> = None;
    let mut packages: Vec<String> = Vec::new();
    let mut jobs: Option<usize> = None;
    let mut releases_only: Option<std::collections::BTreeSet<String>> = None;
    let mut level: Option<BumpLevel> = None;
    let mut dry_run = false;
    let mut only: Option<String> = None;
    let mut publish = false;
    let mut root = PathBuf::from(".");
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--config" => config = Some(PathBuf::from(args.next().ok_or(USAGE)?)),
            "--base" => base = Some(args.next().ok_or(USAGE)?),
            "--remote" => remote = args.next().ok_or(USAGE)?,
            "--pr" => pr = Some(args.next().ok_or(USAGE)?.parse()?),
            "--asset" => assets.push(args.next().ok_or(USAGE)?),
            "--abandon" => abandon = Some(args.next().ok_or(USAGE)?),
            "--target" => target = Some(args.next().ok_or(USAGE)?),
            "--summary" => summary = Some(args.next().ok_or(USAGE)?),
            "--jobs" => jobs = Some(args.next().ok_or(USAGE)?.parse()?),
            // The `releases` output of `release`: a JSON list of objects
            // with a `package`, or of names. Empty means nothing to publish.
            "--releases" => {
                #[derive(serde::Deserialize)]
                #[serde(untagged)]
                enum Item {
                    Named { package: String },
                    Name(String),
                }
                let text = args.next().ok_or(USAGE)?;
                let items: Vec<Item> = if text.trim().is_empty() {
                    Vec::new()
                } else {
                    serde_json::from_str(&text)?
                };
                releases_only = Some(
                    items
                        .into_iter()
                        .map(|i| match i {
                            Item::Named { package } | Item::Name(package) => package,
                        })
                        .collect(),
                );
            }
            "--dry-run" => dry_run = true,
            "--package" => {
                let p = args.next().ok_or(USAGE)?;
                only = Some(p.clone());
                packages.push(p);
            }
            "--publish" => publish = true,
            "--level" => level = Some(args.next().ok_or(USAGE)?.parse().map_err(|()| USAGE)?),
            _ => root = PathBuf::from(arg),
        }
    }
    // Commands run in package paths see VERWALTER_ROOT: it must be
    // absolute, or `$VERWALTER_ROOT/dist` resolves inside the package.
    let root = root
        .canonicalize()
        .map_err(|e| format!("{}: {e}", root.display()))?;
    let config_path = config.unwrap_or_else(|| root.join(verwalter::repo::CONFIG_FILE));
    // Spec: ChangesetCheckOnContributorPullRequest. `check` reads the
    // config only: a broken changeset on the PR is its finding, not a
    // reason to fail before it runs.
    if command == "check" {
        let config = verwalter::config::Config::parse(
            &std::fs::read_to_string(&config_path)
                .map_err(|e| format!("{}: {e}", config_path.display()))?,
        )?;
        let findings = workspace::check(&root, &config);
        if !findings.is_empty() {
            return Err(workspace_error(&findings));
        }
        let pr = pr.ok_or(USAGE)?;
        let mut gh = gh(&root, &remote, dry_run);
        let c = check::run(&root, &config, pr, &mut gh)?;
        println!(
            "{:?}: affected {:?}, covered {:?}",
            c.status, c.affected, c.covered
        );
        if c.status == check::Status::Failed {
            let mut why = Vec::new();
            if !c.missing().is_empty() {
                why.push(format!("changeset missing for: {}", c.missing().join(", ")));
            }
            for f in &c.api {
                why.push(match f.required {
                    Some(r) => format!(
                        "{}: changeset declares {}, the API check requires {r}",
                        f.package, f.claimed
                    ),
                    None => format!("{}: the API check did not run: {}", f.package, f.output),
                });
            }
            return Err(why.join("\n").into());
        }
        return Ok(());
    }
    let mut repo = Repo::load_with_config(&root, &config_path)?;
    let releases = plan(
        &repo.packages,
        &repo.changesets,
        repo.config.dependent_bump_level,
    );
    // Spec: WorkspaceConsistency. `check` fails on it, the others warn.
    for f in &workspace::check(&repo.root, &repo.config) {
        eprintln!("warning: {f}");
    }
    for w in repo
        .warnings
        .iter()
        .chain(&verwalter::plan::fixed_group_drift(&repo.packages))
    {
        eprintln!("warning: {w}");
    }
    match command.as_str() {
        "changeset" => {
            let base = base.unwrap_or_else(|| {
                let remote_base = format!("{remote}/{}", repo.config.base_branch);
                if verwalter::git::ref_exists(&repo.root, &remote_base) {
                    remote_base
                } else {
                    repo.config.base_branch.clone()
                }
            });
            match check::write_changeset(
                &repo,
                &base,
                level.unwrap_or(BumpLevel::Patch),
                summary.as_deref(),
                &packages,
            )? {
                Some(path) => println!(
                    "{}",
                    path.strip_prefix(&repo.root).unwrap_or(&path).display()
                ),
                None => println!("every changed package has a pending changeset"),
            }
            Ok(())
        }
        "plan" => {
            print_plan(&repo, &releases);
            Ok(())
        }
        "branches" => {
            let base = base.unwrap_or_else(|| repo.config.base_branch.clone());
            match branches::create(&repo, &releases, &base)? {
                Some(b) => println!("{b}  on {}", repo.config.base_branch),
                None => println!("nothing to release"),
            }
            Ok(())
        }
        "pr" => {
            let base = base.unwrap_or_else(|| format!("{remote}/{}", repo.config.base_branch));
            github::attribute(&mut repo, &mut gh(&root, &remote, dry_run));
            let branch = branches::create(&repo, &releases, &base)?;
            let mut gh = gh(&repo.root, &remote, dry_run);
            let pr = github::sync(&repo, &releases, branch.as_deref(), &remote, dry_run, &mut gh)
                .map_err(|e| format!("{e}\nresume with: verwalter pr  (the branch is regenerated, the PR updated in place; --dry-run shows the steps)"))?;
            match pr {
                Some(pr) => println!("#{}  {}  on {}", pr.number, pr.head, pr.base),
                None => println!("nothing to release"),
            }
            Ok(())
        }
        "status" => {
            let mut gh = gh(&repo.root, &remote, dry_run);
            let rows = status::status(&repo, &remote, &mut gh)?;
            let w = rows.iter().map(|r| r.package.len()).max().unwrap_or(0);
            for r in &rows {
                let pending = if r.pending_changesets.is_empty() {
                    String::new()
                } else {
                    format!("  {} pending", r.pending_changesets.len())
                };
                let prs = [
                    r.release_pr.map(|n| format!("  PR #{n}")),
                    r.rc_pr.map(|n| format!("  rc PR #{n}")),
                ]
                .into_iter()
                .flatten()
                .collect::<String>();
                println!(
                    "{:w$}  {}  tag:{}  release:{}{}{}{}{}",
                    r.package,
                    r.version,
                    r.tag_state,
                    r.release,
                    if r.assets > 0 {
                        format!("({} assets)", r.assets)
                    } else {
                        String::new()
                    },
                    match r.on_registry {
                        Some(true) => "  registry:yes",
                        Some(false) => "  registry:no",
                        None => "",
                    },
                    pending,
                    prs
                );
                println!("{:w$}  -> {}", "", r.next_step);
            }
            Ok(())
        }
        "pre-release" => {
            let package = package.ok_or(USAGE)?;
            let target = target.unwrap_or_else(|| repo.config.base_branch.clone());
            github::attribute(&mut repo, &mut gh(&root, &remote, dry_run));
            let mut gh = gh(&repo.root, &remote, dry_run);
            let (releases, pr) = prerelease::open(&repo, &package, &target, level, &remote, dry_run, &mut gh)
                .map_err(|e| format!("{e}\nresume with: verwalter pre-release {package} --target {target}  (the rc branch and PR are updated in place)"))?;
            for r in &releases {
                println!(
                    "#{}  {}  {} {} -> {}  on {}",
                    pr.number, pr.head, r.package, r.from, r.to, pr.base
                );
            }
            Ok(())
        }
        "publish" => {
            let options = verwalter::publish::Options {
                jobs,
                dry_run,
                only: releases_only,
            };
            let outcomes = verwalter::publish::run(&repo, &options, &verwalter::registry::curl)?;
            println!("{}", verwalter::publish::write_outputs(&outcomes)?);
            if verwalter::publish::failed(&outcomes) {
                return Err("not every package was published".into());
            }
            Ok(())
        }
        "release" => {
            let mut gh = gh(&repo.root, &remote, dry_run);
            if let Some(tag) = abandon {
                for r in release::abandon(&repo, &remote, &tag, dry_run, &mut gh)? {
                    println!("removed {r}");
                }
                return Ok(());
            }
            let options = release::Options {
                assets,
                package: only,
                publish,
                dry_run,
            };
            let released = release::run(&repo, &remote, &options, &mut gh)?;
            for r in &released {
                println!("{}  {}  {}  {}", r.package, r.version, r.tag, r.status);
            }
            println!("{}", release::write_outputs(&released)?);
            Ok(())
        }
        _ => Err(USAGE.into()),
    }
}

fn workspace_error(findings: &[workspace::Finding]) -> Error {
    format!(
        "verwalter.toml does not match the workspace:\n  {}",
        findings
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n  ")
    )
    .into()
}

type GhRunner<'a> = Box<dyn FnMut(&[&str]) -> Result<String, Error> + 'a>;

/// The gh runner, with every write printed instead of run under `--dry-run`.
fn gh<'a>(root: &'a Path, remote: &str, dry_run: bool) -> GhRunner<'a> {
    let inner = github::gh_cli(root, remote);
    if dry_run {
        Box::new(github::dry_run_gh(inner))
    } else {
        Box::new(inner)
    }
}

fn print_plan(repo: &Repo, releases: &[verwalter::plan::PlannedRelease]) {
    println!(
        "{} packages, {} pending changesets, {} releases in one PR",
        repo.packages.len(),
        repo.changesets.len(),
        releases.len()
    );
    if releases.is_empty() {
        return;
    }
    let width = releases.iter().map(|r| r.package.len()).max().unwrap_or(0);
    for r in releases {
        let via = if !r.changesets.is_empty() {
            r.changesets.join(", ")
        } else if !r.bumped_dependencies.is_empty() {
            format!("deps: {}", r.bumped_dependencies.join(", "))
        } else if r.from.is_prerelease() {
            "promotes the pre-release".to_string()
        } else {
            format!("fixed with {}", r.fixed_with.join(", "))
        };
        println!(
            "{:width$}  {} -> {}  {:<5}  [{via}]",
            r.package,
            r.from,
            r.to,
            r.level.to_string()
        );
    }
}
