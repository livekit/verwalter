use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
    process::ExitCode,
};

use clap::Parser;
use verwalter::{
    branches, check, github, plan::plan, prerelease, release, repo::Repo, status,
    version::BumpLevel, workspace, Error,
};

/// Changeset-based versioning for single packages and monorepos.
// args_override_self: a repeated option takes its last value, as before clap.
// allow_hyphen_values: `--summary -x` takes `-x` as the value, as before clap.
#[derive(Parser)]
#[command(args_override_self = true)]
struct Cli {
    #[command(subcommand)]
    command: Command,
    /// The repository checkout [default: .]
    // A Vec, not one value: a repeated checkout takes the last, as before clap.
    #[arg(global = true)]
    checkout: Vec<PathBuf>,
    /// Config file [default: <checkout>/verwalter.toml]
    #[arg(long, global = true, allow_hyphen_values = true)]
    config: Option<PathBuf>,
    /// Print every write instead of running it
    // Global although plan, branches and changeset ignore it: the action
    // appends `--dry-run` to any command.
    #[arg(long, global = true)]
    dry_run: bool,
}

#[derive(clap::Subcommand)]
enum Command {
    /// Per package: version, tag, GitHub release, pending changesets, open PRs, next step
    Status {
        /// Git remote
        #[arg(long, default_value = "origin", allow_hyphen_values = true)]
        remote: String,
    },
    /// Print the release plan: next versions, dependencies first
    Plan,
    /// Create the release branch locally
    Branches {
        /// Ref to branch from [default: the base branch]
        #[arg(long, allow_hyphen_values = true)]
        base: Option<String>,
    },
    /// Push the release branch and open or update the PR
    Pr {
        /// Ref to branch from [default: <remote>/<base branch>]
        #[arg(long, allow_hyphen_values = true)]
        base: Option<String>,
        /// Git remote
        #[arg(long, default_value = "origin", allow_hyphen_values = true)]
        remote: String,
    },
    /// Write a changeset for the packages the branch changes and no pending changeset covers
    Changeset {
        /// Bump level
        #[arg(long, default_value_t = BumpLevel::Patch, allow_hyphen_values = true)]
        level: BumpLevel,
        /// Exactly these packages, not the changed ones
        #[arg(long = "package", value_name = "NAME", allow_hyphen_values = true)]
        packages: Vec<String>,
        /// Ref to diff against [default: <remote>/<base branch>, else the base branch]
        #[arg(long, allow_hyphen_values = true)]
        base: Option<String>,
        /// Changeset summary [default: the last commit subject]
        #[arg(long, allow_hyphen_values = true)]
        summary: Option<String>,
        /// Git remote
        #[arg(long, default_value = "origin", allow_hyphen_values = true)]
        remote: String,
    },
    /// Tag and create GitHub releases for every untagged version with a changelog entry
    Release {
        /// Asset to upload to drafts or the named package
        #[arg(long = "asset", value_name = "FILE", allow_hyphen_values = true)]
        assets: Vec<String>,
        /// Release only this package
        #[arg(long, value_name = "NAME", allow_hyphen_values = true)]
        package: Option<String>,
        /// Publish the draft release
        #[arg(long)]
        publish: bool,
        /// Delete this tag's GitHub release and the tag
        #[arg(long, value_name = "TAG", allow_hyphen_values = true)]
        abandon: Option<String>,
        /// Git remote
        #[arg(long, default_value = "origin", allow_hyphen_values = true)]
        remote: String,
    },
    /// Changeset and API check for a contributor PR, with comment
    Check {
        /// Pull request number
        #[arg(long, value_name = "NUMBER", allow_hyphen_values = true)]
        pr: u64,
        /// Git remote
        #[arg(long, default_value = "origin", allow_hyphen_values = true)]
        remote: String,
    },
    /// Run each package's publish command, dependencies first
    Publish {
        /// The `releases` output of `release`: only these packages [default: every tagged package]
        #[arg(long, value_name = "JSON", value_parser = releases, allow_hyphen_values = true)]
        releases: Option<BTreeSet<String>>,
        /// Packages to publish at once
        #[arg(long, value_name = "N", allow_hyphen_values = true)]
        jobs: Option<usize>,
    },
    /// Open an rc PR for every package with a pending changeset
    PreRelease {
        /// Branch to open the rc PR from [default: the base branch]
        #[arg(long, value_name = "BRANCH", allow_hyphen_values = true)]
        target: Option<String>,
        /// Git remote
        #[arg(long, default_value = "origin", allow_hyphen_values = true)]
        remote: String,
    },
}

/// A JSON list of objects with a `package`, or of names. Empty means
/// nothing to publish.
fn releases(text: &str) -> Result<BTreeSet<String>, serde_json::Error> {
    #[derive(serde::Deserialize)]
    #[serde(untagged)]
    enum Item {
        Named { package: String },
        Name(String),
    }
    if text.trim().is_empty() {
        return Ok(BTreeSet::new());
    }
    let items: Vec<Item> = serde_json::from_str(text)?;
    Ok(items
        .into_iter()
        .map(|i| match i {
            Item::Named { package } | Item::Name(package) => package,
        })
        .collect())
}

fn main() -> ExitCode {
    match run(Cli::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run(cli: Cli) -> Result<(), Error> {
    let Cli {
        command,
        checkout,
        config,
        dry_run,
    } = cli;
    let root = checkout
        .last()
        .cloned()
        .unwrap_or_else(|| PathBuf::from("."));
    // Commands run in package paths see VERWALTER_ROOT: it must be
    // absolute, or `$VERWALTER_ROOT/dist` resolves inside the package.
    let root = root
        .canonicalize()
        .map_err(|e| format!("{}: {e}", root.display()))?;
    let config_path = config.unwrap_or_else(|| root.join(verwalter::repo::CONFIG_FILE));
    // Spec: ChangesetCheckOnContributorPullRequest. `check` reads the
    // config only: a broken changeset on the PR is its finding, not a
    // reason to fail before it runs.
    if let Command::Check { pr, remote } = &command {
        let config = verwalter::config::Config::load(&root, &config_path)?;
        let findings = workspace::check(&root, &config);
        if !findings.is_empty() {
            return Err(workspace_error(&findings));
        }
        let mut gh = gh(&root, remote, dry_run);
        let c = check::run(&root, &config, *pr, &mut gh)?;
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
        repo.config.versioning.dependent_bump_level,
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
    match command {
        Command::Changeset {
            level,
            packages,
            base,
            summary,
            remote,
        } => {
            let base = base.unwrap_or_else(|| {
                let remote_base = format!("{remote}/{}", repo.config.git.base_branch);
                if verwalter::git::ref_exists(&repo.root, &remote_base) {
                    remote_base
                } else {
                    repo.config.git.base_branch.clone()
                }
            });
            match check::write_changeset(&repo, &base, level, summary.as_deref(), &packages)? {
                Some(path) => println!(
                    "{}",
                    path.strip_prefix(&repo.root).unwrap_or(&path).display()
                ),
                None => println!("every changed package has a pending changeset"),
            }
            Ok(())
        }
        Command::Plan => {
            print_plan(&repo, &releases);
            Ok(())
        }
        Command::Branches { base } => {
            let base = base.unwrap_or_else(|| repo.config.git.base_branch.clone());
            match branches::create(&repo, &releases, &base)? {
                Some(b) => println!("{b}  on {}", repo.config.git.base_branch),
                None => println!("nothing to release"),
            }
            Ok(())
        }
        Command::Pr { base, remote } => {
            let base = base.unwrap_or_else(|| format!("{remote}/{}", repo.config.git.base_branch));
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
        Command::Status { remote } => {
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
        Command::PreRelease { target, remote } => {
            let target = target.unwrap_or_else(|| repo.config.git.base_branch.clone());
            github::attribute(&mut repo, &mut gh(&root, &remote, dry_run));
            let mut gh = gh(&repo.root, &remote, dry_run);
            let (releases, pr) = prerelease::open(&repo, &target, &remote, dry_run, &mut gh)
                .map_err(|e| format!("{e}\nresume with: verwalter pre-release --target {target}  (the rc branch and PR are updated in place)"))?;
            for r in &releases {
                println!(
                    "#{}  {}  {} {} -> {}  on {}",
                    pr.number, pr.head, r.package, r.from, r.to, pr.base
                );
            }
            Ok(())
        }
        Command::Publish { releases, jobs } => {
            let options = verwalter::publish::Options {
                jobs,
                dry_run,
                only: releases,
            };
            let outcomes = verwalter::publish::run(&repo, &options, &verwalter::registry::curl)?;
            println!("{}", verwalter::publish::write_outputs(&outcomes)?);
            if verwalter::publish::failed(&outcomes) {
                return Err("not every package was published".into());
            }
            Ok(())
        }
        Command::Release {
            assets,
            package,
            publish,
            abandon,
            remote,
        } => {
            let mut gh = gh(&repo.root, &remote, dry_run);
            if let Some(tag) = abandon {
                for r in release::abandon(&repo, &remote, &tag, dry_run, &mut gh)? {
                    println!("removed {r}");
                }
                return Ok(());
            }
            let options = release::Options {
                assets,
                package,
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
        Command::Check { .. } => unreachable!("check returned above"),
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
