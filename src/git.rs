use std::ffi::{OsStr, OsString};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use anyhow::{Context, Result, bail};

#[derive(Debug)]
pub struct RepositoryStatus {
    pub branch: Option<String>,
    pub commit: String,
    pub dirty: bool,
    pub origin: Option<String>,
}

pub fn clone_repository(url: &str, revision: Option<&str>, destination: &Path) -> Result<()> {
    let mut arguments = vec![OsString::from("clone")];
    if let Some(revision) = revision {
        arguments.push(OsString::from("--branch"));
        arguments.push(OsString::from(revision));
    }
    arguments.push(OsString::from("--"));
    arguments.push(OsString::from(url));
    arguments.push(destination.as_os_str().to_owned());
    checked_output(arguments)?;
    Ok(())
}

pub fn is_expected_repository(path: &Path, configured_url: &str) -> Result<bool> {
    if !path.is_dir() || !is_repository(path)? {
        return Ok(false);
    }
    let Some(origin) = origin_url(path)? else {
        return Ok(false);
    };
    Ok(urls_match(&origin, configured_url))
}

pub fn is_repository(path: &Path) -> Result<bool> {
    let output = raw_output([
        OsStr::new("-C"),
        path.as_os_str(),
        OsStr::new("rev-parse"),
        OsStr::new("--show-toplevel"),
    ])?;
    if !output.status.success() {
        return Ok(false);
    }

    let reported = PathBuf::from(String::from_utf8_lossy(&output.stdout).trim());
    let actual =
        fs::canonicalize(path).with_context(|| format!("could not resolve {}", path.display()))?;
    let reported = fs::canonicalize(reported).context("Git reported an invalid repository root")?;
    Ok(actual == reported)
}

pub fn status(path: &Path) -> Result<RepositoryStatus> {
    let branch_output = raw_output([
        OsStr::new("-C"),
        path.as_os_str(),
        OsStr::new("symbolic-ref"),
        OsStr::new("--quiet"),
        OsStr::new("--short"),
        OsStr::new("HEAD"),
    ])?;
    let branch = branch_output
        .status
        .success()
        .then(|| stdout_text(&branch_output));
    let commit = query(path, &["rev-parse", "--short", "HEAD"])?;
    let porcelain = query(path, &["status", "--porcelain", "--untracked-files=normal"])?;

    Ok(RepositoryStatus {
        branch,
        commit,
        dirty: !porcelain.is_empty(),
        origin: origin_url(path)?,
    })
}

pub fn update(path: &Path, configured_revision: Option<&str>) -> Result<(String, String)> {
    let current = status(path)?;
    if current.dirty {
        bail!("working tree has local changes");
    }
    let branch = current.branch.context(
        "repository is detached; this version can only safely update checked-out branches",
    )?;
    if let Some(revision) = configured_revision
        && revision != branch
    {
        bail!(
            "configured revision '{revision}' is not the checked-out branch '{branch}'; this version only updates branches"
        );
    }

    query(path, &["fetch", "origin"])?;
    let target = format!("origin/{branch}");
    query(path, &["merge", "--ff-only", "--", &target])?;
    let new_commit = query(path, &["rev-parse", "--short", "HEAD"])?;
    Ok((current.commit, new_commit))
}

fn origin_url(path: &Path) -> Result<Option<String>> {
    let output = raw_output([
        OsStr::new("-C"),
        path.as_os_str(),
        OsStr::new("remote"),
        OsStr::new("get-url"),
        OsStr::new("origin"),
    ])?;
    Ok(output.status.success().then(|| stdout_text(&output)))
}

fn query(path: &Path, arguments: &[&str]) -> Result<String> {
    let mut command_arguments = vec![OsString::from("-C"), path.as_os_str().to_owned()];
    command_arguments.extend(arguments.iter().map(|argument| OsString::from(*argument)));
    Ok(stdout_text(&checked_output(command_arguments)?))
}

fn checked_output<I, S>(arguments: I) -> Result<Output>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let output = raw_output(arguments)?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        if stderr.is_empty() {
            bail!("Git command failed with status {}", output.status);
        }
        bail!("Git command failed: {stderr}");
    }
    Ok(output)
}

fn raw_output<I, S>(arguments: I) -> Result<Output>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    Command::new("git")
        .args(arguments)
        .output()
        .map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                anyhow::anyhow!("Git is required but was not found in PATH")
            } else {
                anyhow::anyhow!("could not execute Git: {error}")
            }
        })
}

fn stdout_text(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

// Keep URL equivalence deliberately narrow until real-world cases require more.
fn urls_match(actual: &str, configured: &str) -> bool {
    normalize_url(actual) == normalize_url(configured)
}

fn normalize_url(url: &str) -> String {
    let normalized = url.trim().trim_end_matches('/');
    normalized
        .strip_suffix(".git")
        .unwrap_or(normalized)
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_comparison_ignores_git_suffix_and_trailing_slash() {
        assert!(urls_match(
            "https://github.com/owner/repo.git/",
            "https://github.com/owner/repo"
        ));
        assert!(!urls_match(
            "https://github.com/owner/repo",
            "git@github.com:owner/repo"
        ));
    }
}
