use std::{path::PathBuf, process::ExitCode};

use verwalter::{branches, check, github, plan::plan, prerelease, release, repo::Repo, status, version::BumpLevel};

const USAGE: &str = "usage: verwalter <status|plan|branches|pr> [--config <file>] [--base <ref>] [--remote <name>] [--dry-run] [<checkout>]\n       verwalter release [--asset <file>]... [--package <name>] [--publish] [--abandon <tag>] [--remote <name>] [--dry-run] [<checkout>]\n       verwalter check --pr <number> [--config <file>] [--dry-run] [<checkout>]\n       verwalter pre-release <package> [--target <branch>] [--level major|minor|patch] [--remote <name>] [--dry-run] [<checkout>]";

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut args = std::env::args().skip(1);
    let Some(command) = args.next() else { return Err(USAGE.into()) };
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
            "--dry-run" => dry_run = true,
            "--package" => only = Some(args.next().ok_or(USAGE)?),
            "--publish" => publish = true,
            "--level" => {
                level = Some(match args.next().ok_or(USAGE)?.as_str() {
                    "major" => BumpLevel::Major,
                    "minor" => BumpLevel::Minor,
                    "patch" => BumpLevel::Patch,
                    _ => return Err(USAGE.into()),
                })
            }
            _ => root = PathBuf::from(arg),
        }
    }
    let repo = match config {
        Some(c) => Repo::load_with_config(&root, &c)?,
        None => Repo::load(&root)?,
    };
    let releases = plan(&repo.packages, &repo.changesets, repo.config.dependent_bump_level);
    match command.as_str() {
        "plan" => {
            print_plan(&repo, &releases);
            Ok(())
        }
        "branches" => {
            let base = base.unwrap_or_else(|| repo.config.base_branch.clone());
            for b in branches::create(&repo, &releases, &base)? {
                println!("{}  on {}", b.name, b.target);
            }
            Ok(())
        }
        "pr" => {
            let base = base.unwrap_or_else(|| format!("{remote}/{}", repo.config.base_branch));
            let branches = branches::create(&repo, &releases, &base)?;
            let mut gh = gh(&repo, &remote, dry_run);
            let prs = github::sync(&repo, &releases, &branches, &remote, dry_run, &mut gh)
                .map_err(|e| format!("{e}\nresume with: verwalter pr  (branches are regenerated, PRs updated in place; --dry-run shows the steps)"))?;
            for pr in prs {
                println!("#{}  {}  on {}", pr.number, pr.head, pr.base);
            }
            Ok(())
        }
        "status" => {
            let mut gh = gh(&repo, &remote, dry_run);
            let rows = status::status(&repo, &remote, &mut gh)?;
            let w = rows.iter().map(|r| r.package.len()).max().unwrap_or(0);
            for r in &rows {
                let pending = if r.pending_changesets.is_empty() { String::new() } else { format!("  {} pending", r.pending_changesets.len()) };
                let prs = [r.release_pr.map(|n| format!("  PR #{n}")), r.rc_pr.map(|n| format!("  rc PR #{n}"))].into_iter().flatten().collect::<String>();
                println!("{:w$}  {}  tag:{}  release:{}{}{}{}", r.package, r.version, r.tag_state, r.release, if r.assets > 0 { format!("({} assets)", r.assets) } else { String::new() }, pending, prs);
                println!("{:w$}  -> {}", "", r.next_step);
            }
            Ok(())
        }
        "pre-release" => {
            let package = package.ok_or(USAGE)?;
            let target = target.unwrap_or_else(|| repo.config.base_branch.clone());
            let mut gh = gh(&repo, &remote, dry_run);
            let (r, pr) = prerelease::open(&repo, &package, &target, level, &remote, dry_run, &mut gh)
                .map_err(|e| format!("{e}\nresume with: verwalter pre-release {package} --target {target}  (the rc branch and PR are updated in place)"))?;
            println!("#{}  {}  {} -> {}  on {}", pr.number, pr.head, r.from, r.to, pr.base);
            Ok(())
        }
        "check" => {
            let pr = pr.ok_or(USAGE)?;
            let mut gh = gh(&repo, &remote, dry_run);
            let c = check::run(&repo.root, &repo.config, pr, &mut gh)?;
            println!("{:?}: affected {:?}, covered {:?}", c.status, c.affected, c.covered);
            if c.status == check::Status::Failed {
                return Err(format!("changeset missing for: {}", c.missing().join(", ")).into());
            }
            Ok(())
        }
        "release" => {
            let mut gh = gh(&repo, &remote, dry_run);
            if let Some(tag) = abandon {
                for r in release::abandon(&repo, &remote, &tag, dry_run, &mut gh)? {
                    println!("removed {r}");
                }
                return Ok(());
            }
            let options = release::Options { assets, package: only, publish, dry_run };
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

type GhRunner<'a> = Box<dyn FnMut(&[&str]) -> Result<String, github::Error> + 'a>;

/// The gh runner, with every write printed instead of run under `--dry-run`.
fn gh<'a>(repo: &'a Repo, remote: &str, dry_run: bool) -> GhRunner<'a> {
    let inner = github::gh_cli(&repo.root, remote);
    if dry_run {
        Box::new(github::dry_run_gh(inner))
    } else {
        Box::new(inner)
    }
}

fn print_plan(repo: &Repo, releases: &[verwalter::plan::PlannedRelease]) {
    println!("{} packages, {} pending changesets, {} releases", repo.packages.len(), repo.changesets.len(), releases.len());
    if releases.is_empty() {
        return;
    }
    let width = releases.iter().map(|r| r.package.len()).max().unwrap_or(0);
    for r in releases {
        let on = r.stacked_on.as_deref().map_or(String::new(), |p| format!("  on {p}"));
        let via = if r.changesets.is_empty() {
            format!("deps: {}", r.bumped_dependencies.join(", "))
        } else {
            r.changesets.join(", ")
        };
        println!("{:width$}  {} -> {}  {:<5} {on}  [{via}]", r.package, r.from, r.to, format!("{:?}", r.level).to_lowercase());
    }
}
