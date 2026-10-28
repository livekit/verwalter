//! The few git operations the tool needs, over the `git` CLI.
//! ponytail: no libgit2/gitoxide; CI and maintainers have git.

use std::{
    path::Path,
    process::{Command, Output},
};

use crate::Error;

fn output(root: &Path, args: &[&str]) -> Result<Output, Error> {
    Ok(Command::new("git").arg("-C").arg(root).args(args).output()?)
}

/// Run git and return trimmed stdout; a non-zero exit is an error.
pub fn run(root: &Path, args: &[&str]) -> Result<String, Error> {
    let out = output(root, args)?;
    if !out.status.success() {
        return Err(format!("git {}: {}", args.join(" "), String::from_utf8_lossy(&out.stderr).trim()).into());
    }
    Ok(String::from_utf8(out.stdout)?.trim().to_string())
}

/// A git command that changes the remote or the refs. With `dry_run` it
/// is printed instead of run.
pub fn write(root: &Path, dry_run: bool, args: &[&str]) -> Result<String, Error> {
    if dry_run {
        println!("dry-run: git {}", args.join(" "));
        return Ok(String::new());
    }
    run(root, args)
}

pub fn is_repo(root: &Path) -> bool {
    output(root, &["rev-parse", "--git-dir"]).map(|o| o.status.success()).unwrap_or(false)
}

pub fn ref_exists(root: &Path, name: &str) -> bool {
    output(root, &["rev-parse", "--verify", "--quiet", &format!("{name}^{{commit}}")]).map(|o| o.status.success()).unwrap_or(false)
}

/// Whether `ancestor` is reachable from `descendant`.
pub fn is_ancestor(root: &Path, ancestor: &str, descendant: &str) -> bool {
    output(root, &["merge-base", "--is-ancestor", ancestor, descendant]).map(|o| o.status.success()).unwrap_or(false)
}

/// The commit that added `path`, as seen from `rev`.
pub fn added_in(root: &Path, rev: &str, path: &str) -> Option<String> {
    run(root, &["log", "--diff-filter=A", "--format=%H", "-n", "1", rev, "--", path]).ok().filter(|s| !s.is_empty())
}

/// `-c user.name/user.email` arguments when the checkout has no identity,
/// so commits work on a bare CI runner.
pub fn identity_args(root: &Path) -> Vec<&'static str> {
    match run(root, &["config", "user.email"]) {
        Ok(email) if !email.is_empty() => Vec::new(),
        _ => vec!["-c", "user.name=verwalter", "-c", "user.email=verwalter@users.noreply.github.com"],
    }
}
