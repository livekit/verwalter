use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, anyhow, bail, ensure};
use clap::Parser;
use verwalter::{
    branches, check, git, github, plan::plan, prerelease, release, repo::Repo, status,
    version::BumpLevel, workspace,
};

/// Changeset-based versioning for single packages and monorepos.
#[derive(Parser)]
#[command(version)]
struct Cli {
    /// The repository checkout [default: the root of the git repository containing the current directory]
    #[arg(global = true, value_parser = checkout)]
    checkout: Option<PathBuf>,
    /// Config file [default: <checkout>/verwalter.toml]
    #[arg(long, global = true)]
    config: Option<PathBuf>,
    /// Git remote
    #[arg(long, global = true, default_value = "origin")]
    remote: String,
    /// Print every write instead of running it
    #[arg(long, global = true)]
    dry_run: bool,

    #[command(subcommand)]
    command: Command,
}

#[derive(clap::Subcommand)]
enum Command {
    /// Per package: version, tag, GitHub release, pending changesets, open PRs, next step
    Status {
        /// Exit non-zero when the current version of a package is not completely released
        #[arg(long)]
        check: bool,
        /// With --check: ask again every 15 seconds for this long, until every package is complete. `90s` or `5m`
        #[arg(long, value_name = "TIME", value_parser = duration, requires = "check")]
        wait: Option<std::time::Duration>,
        /// Print one JSON list in place of the table
        #[arg(long)]
        json: bool,
        /// Only this package
        #[arg(long, value_name = "NAME")]
        package: Option<String>,
    },
    /// Print the release plan: next versions, dependencies first
    Plan,
    /// Create the release branch locally
    Branches {
        /// Ref to branch from [default: the base branch]
        #[arg(long)]
        base: Option<String>,
    },
    /// Push the release branch and open or update the PR
    Pr {
        /// Ref to branch from [default: <remote>/<base branch>]
        #[arg(long)]
        base: Option<String>,
    },
    /// Write a changeset for the packages the branch changes and no pending changeset covers
    Changeset {
        /// Bump level
        #[arg(long, default_value_t = BumpLevel::Patch)]
        level: BumpLevel,
        /// Exactly these packages, not the changed ones
        #[arg(long = "package", value_name = "NAME")]
        packages: Vec<String>,
        /// Ref to diff against [default: <remote>/<base branch>, else the base branch]
        #[arg(long)]
        base: Option<String>,
        /// Changeset summary [default: the last commit subject]
        #[arg(long, allow_hyphen_values = true)]
        summary: Option<String>,
    },
    /// Tag and create GitHub releases for every untagged version with a changelog entry
    Release {
        /// Asset to upload to drafts or the named package: a file, or every file in a directory
        #[arg(long = "asset", value_name = "FILE|DIR")]
        assets: Vec<String>,
        /// Release only this package
        #[arg(long, value_name = "NAME")]
        package: Option<String>,
        /// Publish the draft release
        #[arg(long)]
        publish: bool,
        /// Upload assets even when HEAD changed a package's files since its tag. Without it, the release fails: build the assets from the tag instead
        #[arg(long)]
        allow_changed_source: bool,
        /// Delete this tag's GitHub release and the tag
        #[arg(long, value_name = "TAG", conflicts_with_all = ["assets", "package", "publish"])]
        abandon: Option<String>,
    },
    /// Changeset and API check for a contributor PR, with comment
    Check {
        /// Pull request number
        #[arg(long, value_name = "NUMBER")]
        pr: u64,
    },
    /// Run each package's publish command, dependencies first
    Publish {
        /// The `releases` output of `release`: only these packages. An empty list, from a re-run by hand after a run that released nothing, counts as absent [default: every tagged package]
        #[arg(long, value_name = "JSON", value_parser = releases)]
        releases: Option<BTreeSet<String>>,
        /// Packages to publish at once
        #[arg(long, value_name = "N")]
        jobs: Option<usize>,
        /// Publish even when HEAD changed a package's files since its tag. Without it, such a package fails: check out the tag instead
        #[arg(long)]
        allow_changed_source: bool,
        /// How long to wait for the registry to show a version after its command passed: `90s`, `5m`, or `0` for no wait. npm scans a new version before it shows it
        #[arg(long, value_name = "TIME", value_parser = duration, default_value = "5m")]
        wait: std::time::Duration,
    },
    /// Open an rc PR for every package with a pending changeset
    PreRelease {
        /// Branch to open the rc PR from [default: the base branch]
        #[arg(long, value_name = "BRANCH")]
        target: Option<String>,
    },
}

/// A file as the checkout is most often a glob that the shell expanded
/// after `--asset`: say so instead of reading the file as the checkout.
fn checkout(arg: &str) -> Result<PathBuf, String> {
    if Path::new(arg).is_file() {
        return Err("the <checkout> is one directory. `--asset` takes one file or one directory, and a glob such as `--asset dist/*` expands to several arguments: pass `--asset dist`".into());
    }
    Ok(PathBuf::from(arg))
}

/// A JSON list of objects with a `package`, or of names. Empty means
/// nothing to publish.
/// Spec: WorkflowRunsPublish.releases. An empty list reaches `publish`
/// only from a re-run by hand after a run that released nothing, and
/// then means: every tagged package, which the registry check narrows
/// to the versions it lacks. Publishing nothing and exiting 0 would pass
/// for a successful re-run.
fn only(releases: Option<BTreeSet<String>>) -> Option<BTreeSet<String>> {
    releases.filter(|r| !r.is_empty())
}

/// `90s`, `5m`, or a plain number of seconds.
fn duration(text: &str) -> Result<std::time::Duration, String> {
    let (digits, per) = match text.strip_suffix('m') {
        Some(d) => (d, 60),
        None => (text.strip_suffix('s').unwrap_or(text), 1),
    };
    let n: u64 = digits
        .parse()
        .map_err(|_| format!("{text}: use seconds or minutes, such as 90s or 5m"))?;
    Ok(std::time::Duration::from_secs(n * per))
}

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

// Returning the error prints `Error: ` with its `Caused by:` chain (and a
// backtrace under RUST_BACKTRACE=1), and exits 1.
fn main() -> Result<()> {
    run(Cli::parse())
}

fn run(cli: Cli) -> Result<()> {
    let Cli {
        command,
        checkout: root,
        config,
        remote,
        dry_run,
    } = cli;
    let root = match root {
        Some(root) => root,
        None => git::run(Path::new("."), &["rev-parse", "--show-toplevel"])?.into(),
    };
    // Commands run in package paths see VERWALTER_ROOT: it must be
    // absolute, or `$VERWALTER_ROOT/dist` resolves inside the package.
    let root = root
        .canonicalize()
        .with_context(|| root.display().to_string())?;
    let config_path = config.unwrap_or_else(|| root.join(verwalter::repo::CONFIG_FILE));
    // Under `--dry-run`, every gh write is printed instead of run.
    let client = github::SystemClient::new(&root, &remote);
    let gh: &dyn github::Client = if dry_run {
        &github::DryRunClient::new(client)
    } else {
        &client
    };
    // Spec: ChangesetCheckOnContributorPullRequest. `check` reads the
    // config only: a broken changeset on the PR is its finding, not a
    // reason to fail before it runs.
    if let Command::Check { pr } = &command {
        let config = verwalter::config::Config::load(&root, &config_path)?;
        let findings = workspace::check(&root, &config);
        if !findings.is_empty() {
            return Err(workspace_error(&findings));
        }
        let c = check::run(&root, &config, *pr, gh)?;
        println!(
            "{:?}: affected {:?}, covered {:?}",
            c.status, c.affected, c.covered
        );
        if c.failed() {
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
            for f in &c.dry_run {
                why.push(format!(
                    "{}: publish dry run failed: {}\n{}",
                    f.packages.join(", "),
                    f.command,
                    f.output
                ));
            }
            bail!(why.join("\n"));
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
            // Spec: GoMajorVersion. `branches` fails on it when it writes
            // the go.mod; the plan says so first.
            let go: Vec<String> = releases
                .iter()
                .filter_map(|r| {
                    let module = repo.config.packages[&r.package].go_module.as_deref()?;
                    verwalter::handler::go_major_error(module, &r.to)
                })
                .collect();
            ensure!(go.is_empty(), go.join("\n"));
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
        Command::Pr { base } => {
            let base = base.unwrap_or_else(|| format!("{remote}/{}", repo.config.git.base_branch));
            github::attribute(&mut repo, gh);
            let branch = branches::create(&repo, &releases, &base)?;
            let pr = github::sync(&repo, &releases, branch.as_deref(), &remote, dry_run, gh)
                .map_err(|e| anyhow!("{e:#}\nresume with: verwalter pr  (the branch is regenerated, the PR updated in place; --dry-run shows the steps)"))?;
            match pr {
                Some(pr) => println!("#{}  {}  on {}", pr.number, pr.head, pr.base),
                None => println!("nothing to release"),
            }
            Ok(())
        }
        Command::Status {
            check,
            wait,
            json,
            package,
        } => {
            // Spec: surface ReleaseStatus. --wait asks again until every
            // package is complete or the time is over: a registry can
            // show a new version minutes after the publish.
            let deadline = std::time::Instant::now() + wait.unwrap_or_default();
            let rows = loop {
                let mut rows = status::status(&repo, &remote, gh)?;
                if let Some(only) = &package {
                    rows.retain(|r| &r.package == only);
                    ensure!(!rows.is_empty(), "{only}: no such package");
                }
                let left = deadline.saturating_duration_since(std::time::Instant::now());
                if left.is_zero() || rows.iter().all(|r| r.incomplete.is_none()) {
                    break rows;
                }
                eprintln!("not complete yet, asking again");
                std::thread::sleep(verwalter::publish::POLL.min(left));
            };
            if json {
                println!("{}", serde_json::to_string(&rows)?);
            }
            let w = rows.iter().map(|r| r.package.len()).max().unwrap_or(0);
            for r in rows.iter().filter(|_| !json) {
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
            // Spec: surface ReleaseStatus, the end-state check.
            if check {
                for r in &rows {
                    if let Some(why) = &r.incomplete {
                        eprintln!(
                            "{} {} is not completely released: {why}",
                            r.package, r.version
                        );
                    }
                }
                ensure!(
                    rows.iter().all(|r| r.incomplete.is_none()),
                    "not every package is completely released"
                );
            }
            Ok(())
        }
        Command::PreRelease { target } => {
            let target = target.unwrap_or_else(|| repo.config.git.base_branch.clone());
            github::attribute(&mut repo, gh);
            let (releases, pr) = prerelease::open(&repo, &target, &remote, dry_run, gh)
                .map_err(|e| anyhow!("{e:#}\nresume with: verwalter pre-release --target {target}  (the rc branch and PR are updated in place)"))?;
            for r in &releases {
                println!(
                    "#{}  {}  {} {} -> {}  on {}",
                    pr.number, pr.head, r.package, r.from, r.to, pr.base
                );
            }
            Ok(())
        }
        Command::Publish {
            releases,
            jobs,
            allow_changed_source,
            wait,
        } => {
            let options = verwalter::publish::Options {
                jobs,
                dry_run,
                only: only(releases),
                allow_changed_source,
                wait,
            };
            let outcomes =
                verwalter::publish::run(&repo, &options, &verwalter::registry::curl, gh)?;
            println!("{}", verwalter::publish::write_outputs(&outcomes)?);
            if verwalter::publish::failed(&outcomes) {
                bail!("not every package was published");
            }
            Ok(())
        }
        Command::Release {
            assets,
            package,
            publish,
            allow_changed_source,
            abandon,
        } => {
            if let Some(tag) = abandon {
                for r in release::abandon(&repo, &remote, &tag, dry_run, gh)? {
                    println!("removed {r}");
                }
                return Ok(());
            }
            let options = release::Options {
                assets,
                package,
                publish,
                allow_changed_source,
                dry_run,
            };
            let released = release::run(&repo, &remote, &options, gh)?;
            for r in &released {
                println!("{}  {}  {}  {}", r.package, r.version, r.tag, r.status);
            }
            println!("{}", release::write_outputs(&released)?);
            Ok(())
        }
        Command::Check { .. } => unreachable!("check returned above"),
    }
}

fn workspace_error(findings: &[workspace::Finding]) -> anyhow::Error {
    anyhow!(
        "verwalter.toml does not match the workspace:\n  {}",
        findings
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n  ")
    )
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

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn cli_definition() {
        Cli::command().debug_assert();
    }

    // Spec: WorkflowPublishesInDependencyOrder. An empty release list
    // counts as absent: every tagged package.
    #[test]
    fn empty_releases_list_means_every_tagged_package() {
        assert_eq!(only(Some(releases("[]").unwrap())), None);
        assert_eq!(only(Some(releases("").unwrap())), None);
        assert_eq!(only(None), None);
        let one = releases(r#"[{"package":"a","version":"1.0.0"}]"#).unwrap();
        assert_eq!(only(Some(one.clone())), Some(one));
    }
}
