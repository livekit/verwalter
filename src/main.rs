use std::{path::PathBuf, process::ExitCode};

use verwalter::{branches, github, plan::plan, repo::Repo};

const USAGE: &str = "usage: verwalter <plan|branches|pr> [--config <file>] [--base <ref>] [--remote <name>] [<checkout>]";

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
    let mut config = None;
    let mut base = None;
    let mut remote = String::from("origin");
    let mut root = PathBuf::from(".");
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--config" => config = Some(PathBuf::from(args.next().ok_or(USAGE)?)),
            "--base" => base = Some(args.next().ok_or(USAGE)?),
            "--remote" => remote = args.next().ok_or(USAGE)?,
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
            let mut gh = github::gh_cli(&repo.root);
            for pr in github::sync(&repo, &releases, &branches, &remote, &mut gh)? {
                println!("#{}  {}  on {}", pr.number, pr.head, pr.base);
            }
            Ok(())
        }
        _ => Err(USAGE.into()),
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
