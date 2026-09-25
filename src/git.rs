use std::ffi::{OsStr, OsString};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use serde::Deserialize;

use crate::interaction::Interaction;

const ROT_IDENTITY_VERSION: u32 = 1;
const NETWORK_GIT_TIMEOUT: Duration = Duration::from_secs(5 * 60);

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct RotIdentity {
    pub alias: String,
    pub username: Option<String>,
    pub verification: String,
}

#[derive(Debug, Deserialize)]
struct RotIdentityDocument {
    version: u32,
    identities: Vec<RotIdentity>,
}

#[derive(Debug, Clone)]
pub struct RepositoryStatus {
    pub branch: Option<String>,
    pub commit: Option<String>,
    pub dirty: bool,
    pub origin: Option<String>,
    pub push_url: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ManagedCheckout {
    ExpectedTransport,
    EquivalentTransport,
    RequiresVerifiedAlias,
    Mismatch,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct GitHubRepository {
    owner: String,
    name: String,
}

/// Repository identity policy for managed checkouts.
///
/// Canonical GitHub transports are always recognized. Additional SSH hosts are
/// recognized only when they came from Rot's verified identity inventory.
#[derive(Debug, Clone, Default)]
pub(crate) struct ManagedRepositoryMatcher {
    verified_github_aliases: Vec<String>,
}

impl ManagedRepositoryMatcher {
    pub(crate) fn canonical() -> Self {
        Self::default()
    }

    pub(crate) fn from_verified_rot_identities(identities: &[RotIdentity]) -> Self {
        Self {
            verified_github_aliases: identities
                .iter()
                .filter(|identity| {
                    identity.verification == "verified"
                        && identity.username.is_some()
                        && valid_ssh_alias(&identity.alias)
                })
                .map(|identity| identity.alias.clone())
                .collect(),
        }
    }

    /// Answer whether a path is a managed checkout for the configured URL.
    /// Path/repository/origin validation and repository identity comparison are
    /// intentionally performed together so callers cannot accidentally omit one.
    pub(crate) fn checkout_match(
        &self,
        path: &Path,
        configured_url: &str,
    ) -> Result<ManagedCheckout> {
        if !path.is_dir() || !is_repository(path)? {
            return Ok(ManagedCheckout::Mismatch);
        }
        let Some(actual_url) = fetch_url(path)? else {
            return Ok(ManagedCheckout::Mismatch);
        };
        if normalize_url(&actual_url) == normalize_url(configured_url) {
            return Ok(ManagedCheckout::ExpectedTransport);
        }
        if github_https_read_url(configured_url).is_some()
            && github_https_repository(&actual_url).is_some()
            && github_urls_have_same_identity(&actual_url, configured_url, &[])
        {
            return Ok(ManagedCheckout::ExpectedTransport);
        }
        if github_urls_have_same_identity(
            &actual_url,
            configured_url,
            &self.verified_github_aliases,
        ) {
            Ok(ManagedCheckout::EquivalentTransport)
        } else if github_urls_may_match_verified_alias(&actual_url, configured_url) {
            Ok(ManagedCheckout::RequiresVerifiedAlias)
        } else {
            Ok(ManagedCheckout::Mismatch)
        }
    }

    pub(crate) fn is_managed_checkout(&self, path: &Path, configured_url: &str) -> Result<bool> {
        Ok(matches!(
            self.checkout_match(path, configured_url)?,
            ManagedCheckout::ExpectedTransport | ManagedCheckout::EquivalentTransport
        ))
    }
}

pub fn clone_repository(
    url: &str,
    revision: Option<&str>,
    destination: &Path,
    interaction: &mut dyn Interaction,
) -> Result<()> {
    let (arguments, read_url) = clone_arguments(url, revision, destination);
    if read_url == url {
        checked_network_output(arguments, url, interaction)?;
    } else {
        checked_read_network_output(
            arguments,
            &read_url,
            clone_arguments_for_url(url, revision, destination),
            url,
            interaction,
        )?;
    }
    configure_read_remote(destination, url)?;
    Ok(())
}

fn clone_arguments(
    configured_url: &str,
    revision: Option<&str>,
    destination: &Path,
) -> (Vec<OsString>, String) {
    let read_url =
        github_https_read_url(configured_url).unwrap_or_else(|| configured_url.to_owned());
    (
        clone_arguments_for_url(&read_url, revision, destination),
        read_url,
    )
}

fn clone_arguments_for_url(url: &str, revision: Option<&str>, destination: &Path) -> Vec<OsString> {
    let mut arguments = vec![OsString::from("clone")];
    if let Some(revision) = revision {
        arguments.push(OsString::from("--branch"));
        arguments.push(OsString::from(revision));
    }
    arguments.push(OsString::from("--"));
    arguments.push(OsString::from(url));
    arguments.push(destination.as_os_str().to_owned());
    arguments
}

pub fn is_repository(path: &Path) -> Result<bool> {
    if fs::symlink_metadata(path).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
        return Ok(false);
    }
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
    let commit = head_commit(path)?
        .map(|_| query(path, &["rev-parse", "--short", "HEAD"]))
        .transpose()?;
    let porcelain = query(path, &["status", "--porcelain", "--untracked-files=normal"])?;

    Ok(RepositoryStatus {
        branch,
        commit,
        dirty: !porcelain.is_empty(),
        origin: origin_url(path)?,
        push_url: push_url(path)?,
    })
}

/// Return whether HEAD contains commits that are not reachable from any
/// configured `origin` reference. Destructive project operations use this in
/// addition to the worktree status so locally committed work is preserved.
pub fn has_local_commits_not_on_origin(path: &Path) -> Result<bool> {
    let count = query(
        path,
        &["rev-list", "--count", "HEAD", "--not", "--remotes=origin"],
    )?;
    Ok(count
        .trim()
        .parse::<u64>()
        .context("Git returned an invalid commit count")?
        > 0)
}

pub fn current_branch(path: &Path) -> Result<Option<String>> {
    let output = raw_output([
        OsStr::new("-C"),
        path.as_os_str(),
        OsStr::new("symbolic-ref"),
        OsStr::new("--quiet"),
        OsStr::new("--short"),
        OsStr::new("HEAD"),
    ])?;
    Ok(output.status.success().then(|| stdout_text(&output)))
}

pub fn head_commit(path: &Path) -> Result<Option<String>> {
    let output = raw_output([
        OsStr::new("-C"),
        path.as_os_str(),
        OsStr::new("rev-parse"),
        OsStr::new("--verify"),
        OsStr::new("--quiet"),
        OsStr::new("HEAD^{commit}"),
    ])?;
    if output.status.success() {
        Ok(Some(stdout_text(&output)))
    } else if output.status.code() == Some(1) {
        Ok(None)
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        bail!("could not inspect repository HEAD: {stderr}")
    }
}

pub fn working_tree_changes(path: &Path) -> Result<String> {
    query(path, &["status", "--porcelain", "--untracked-files=normal"])
}

pub fn tracked_files(path: &Path) -> Result<Vec<String>> {
    Ok(query(path, &["ls-files"])?
        .lines()
        .map(str::to_owned)
        .collect())
}

pub fn origin_refs(
    path: &Path,
    interaction: &mut dyn Interaction,
) -> Result<Vec<(String, String)>> {
    let fetch = fetch_url(path)?;
    let push = push_url(path)?;
    let configured_url = match (fetch.as_deref(), push.as_deref()) {
        (Some(fetch), Some(push))
            if github_https_repository(fetch).is_some()
                && github_ssh_repository(push, &[]).is_some()
                && urls_match(fetch, push) =>
        {
            Some(push.to_owned())
        }
        _ => fetch,
    };
    if let Some(configured_url) = configured_url.as_deref() {
        configure_read_remote(path, configured_url)?;
    }
    let fetch_url = configured_remote_url(path, false)?;
    let refs = if let Some(ssh_url) = configured_url
        .as_deref()
        .filter(|url| github_https_read_url(url).is_some())
    {
        read_network_query(
            path,
            &["ls-remote", "--refs", "origin"],
            fetch_url.as_deref().unwrap_or(""),
            &["ls-remote", "--refs", ssh_url],
            ssh_url,
            interaction,
        )?
    } else {
        network_query(
            path,
            &["ls-remote", "--refs", "origin"],
            fetch_url.as_deref(),
            interaction,
        )?
    };
    refs.lines()
        .map(|line| {
            let (commit, reference) = line
                .split_once(char::is_whitespace)
                .context("Git returned an invalid origin ref")?;
            Ok((commit.to_owned(), reference.trim().to_owned()))
        })
        .collect()
}

pub fn origin_has_refs(path: &Path, interaction: &mut dyn Interaction) -> Result<bool> {
    Ok(!origin_refs(path, interaction)?.is_empty())
}

pub fn update(
    path: &Path,
    configured_url: &str,
    configured_revision: Option<&str>,
    interaction: &mut dyn Interaction,
) -> Result<(String, String)> {
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

    configure_read_remote(path, configured_url)?;

    let read_url = configured_remote_url(path, false)?;
    if github_https_read_url(configured_url).is_some() {
        read_network_query(
            path,
            &["fetch", "origin"],
            read_url.as_deref().unwrap_or(""),
            &[
                "fetch",
                configured_url,
                "+refs/heads/*:refs/remotes/origin/*",
            ],
            configured_url,
            interaction,
        )?;
    } else {
        network_query(path, &["fetch", "origin"], read_url.as_deref(), interaction)?;
    }
    let target = format!("origin/{branch}");
    interaction.process_control().cancellation.check()?;
    // User decisions during fetch release the operation lease. Recheck Git's
    // local preconditions before changing the worktree.
    let after_fetch = status(path)?;
    if after_fetch.dirty
        || after_fetch.branch.as_deref() != Some(&branch)
        || after_fetch.commit != current.commit
    {
        bail!("repository changed during fetch; retry the update");
    }
    controlled_query(path, &["merge", "--ff-only", "--", &target], interaction)?;
    let _completed_step = crate::process::critical_scope();
    let new_commit = query(path, &["rev-parse", "--short", "HEAD"])?;
    Ok((
        current
            .commit
            .context("repository has no commits and cannot be updated")?,
        new_commit,
    ))
}

pub fn commit_file(path: &Path, file: &str, message: &str) -> Result<String> {
    commit_file_with_interaction(path, file, message, &mut crate::interaction::Unattended)
}

pub fn commit_file_with_interaction(
    path: &Path,
    file: &str,
    message: &str,
    interaction: &mut dyn Interaction,
) -> Result<String> {
    controlled_query(path, &["add", "--", file], interaction)?;
    controlled_query(
        path,
        &["commit", "--only", "-m", message, "--", file],
        interaction,
    )?;
    let _completed_step = crate::process::critical_scope();
    query(path, &["rev-parse", "--short", "HEAD"])
}

fn controlled_query(
    path: &Path,
    arguments: &[&str],
    interaction: &mut dyn Interaction,
) -> Result<String> {
    let mut args = vec![OsString::from("-C"), path.as_os_str().to_owned()];
    args.extend(arguments.iter().map(OsString::from));
    let output = raw_output_control(args, &interaction.process_control())?;
    if !output.status.success() {
        bail!("{}", git_error_message(&output));
    }
    Ok(stdout_text(&output))
}

pub fn path_has_changes(path: &Path, file: &str) -> Result<bool> {
    Ok(!query(
        path,
        &[
            "status",
            "--porcelain",
            "--untracked-files=normal",
            "--",
            file,
        ],
    )?
    .is_empty())
}

pub fn push_origin(path: &Path, interaction: &mut dyn Interaction) -> Result<()> {
    network_query(
        path,
        &["push", "origin", "HEAD"],
        configured_remote_url(path, true)?.as_deref(),
        interaction,
    )?;
    Ok(())
}

pub fn origin_url(path: &Path) -> Result<Option<String>> {
    fetch_url(path)
}

pub fn fetch_url(path: &Path) -> Result<Option<String>> {
    configured_remote_url(path, false)
}

pub fn push_url(path: &Path) -> Result<Option<String>> {
    let output = raw_output([
        OsStr::new("-C"),
        path.as_os_str(),
        OsStr::new("config"),
        OsStr::new("--get"),
        OsStr::new("remote.origin.pushurl"),
    ])?;
    Ok(output.status.success().then(|| stdout_text(&output)))
}

pub fn verified_rot_identities() -> Result<Vec<RotIdentity>> {
    query_rot_identities()
}

pub fn select_verified_rot_identity<P: Interaction + ?Sized>(
    identities: Vec<RotIdentity>,
    prompt: &mut P,
) -> Result<RotIdentity> {
    select_rot_identity(identities, true, prompt)
}

pub fn github_ssh_push_url(canonical_url: &str, alias: &str) -> Option<String> {
    let repository = github_https_repository(canonical_url)?;
    valid_ssh_alias(alias)
        .then(|| format!("git@{alias}:{}/{}.git", repository.owner, repository.name))
}

pub fn set_push_url(path: &Path, url: &str) -> Result<()> {
    set_remote_url(path, "remote.origin.pushurl", url)
}

/// Recovery could not verify the original configuration. Callers must report
/// possible partial work and retain the checkout for inspection.
#[derive(Debug)]
pub struct RemoteRecoveryIncomplete {
    pub path: PathBuf,
    diagnostic: String,
}

impl std::fmt::Display for RemoteRecoveryIncomplete {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "remote configuration recovery is incomplete in {}; inspect Git configuration before retrying: {}",
            self.path.display(),
            self.diagnostic
        )
    }
}

impl std::error::Error for RemoteRecoveryIncomplete {}

pub fn reconcile_remote(path: &Path, fetch: &str, push: &str) -> Result<()> {
    crate::process::current_control().cancellation.check()?;
    // Once started, both writes, verification, and any recovery must finish
    // even if the caller cancels. The operation records success before checking
    // the caller's token again. The caller retains its repository lease.
    let _transaction = crate::process::critical_scope();
    let old_fetch = local_remote_urls(path, "remote.origin.url")?;
    let old_push = local_remote_urls(path, "remote.origin.pushurl")?;
    let result = (|| {
        set_remote_url(path, "remote.origin.pushurl", push)?;
        set_remote_url(path, "remote.origin.url", fetch)?;
        if local_remote_urls(path, "remote.origin.url")? != [fetch]
            || local_remote_urls(path, "remote.origin.pushurl")? != [push]
        {
            bail!("Git did not retain the requested fetch and push URLs");
        }
        Ok(())
    })();
    if let Err(error) = result {
        // Attempt both restorations, even if one fails, then verify actual
        // local values (including absence and multiple configured URLs).
        let mut failures = Vec::new();
        for (key, urls) in [
            ("remote.origin.url", &old_fetch),
            ("remote.origin.pushurl", &old_push),
        ] {
            if let Err(recovery) = restore_remote_url(path, key, urls) {
                failures.push(format!("{key}: {recovery:#}"));
            }
        }
        let mut restored = true;
        for (key, expected) in [
            ("remote.origin.url", &old_fetch),
            ("remote.origin.pushurl", &old_push),
        ] {
            match local_remote_urls(path, key) {
                Ok(actual) if &actual == expected => {}
                Ok(actual) => {
                    restored = false;
                    failures.push(format!(
                        "{key}: expected {expected:?}, remaining {actual:?}"
                    ));
                }
                Err(verification) => {
                    restored = false;
                    failures.push(format!("{key}: remaining state unknown: {verification:#}"));
                }
            }
        }
        if !restored {
            return Err(error.context(RemoteRecoveryIncomplete {
                path: path.to_owned(),
                diagnostic: failures.join("; "),
            }));
        }
        return Err(error.context(if failures.is_empty() {
            "original remote configuration was restored and verified".to_owned()
        } else {
            format!("original remote configuration verified unchanged despite recovery command failures: {}", failures.join("; "))
        }));
    }
    Ok(())
}

fn local_remote_urls(path: &Path, key: &str) -> Result<Vec<String>> {
    let output = raw_output([
        OsStr::new("-C"),
        path.as_os_str(),
        OsStr::new("config"),
        OsStr::new("--local"),
        OsStr::new("--null"),
        OsStr::new("--get-all"),
        OsStr::new(key),
    ])?;
    if output.status.code() == Some(1) {
        return Ok(Vec::new());
    }
    if !output.status.success() {
        bail!(
            "could not read {key}: Git exited with {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    let text = String::from_utf8(output.stdout).context("Git remote URL is not UTF-8")?;
    Ok(text.split_terminator('\0').map(str::to_owned).collect())
}

fn set_remote_url(path: &Path, key: &str, url: &str) -> Result<()> {
    checked_output([
        OsStr::new("-C"),
        path.as_os_str(),
        OsStr::new("config"),
        OsStr::new("--local"),
        OsStr::new("--replace-all"),
        OsStr::new(key),
        OsStr::new(url),
    ])?;
    Ok(())
}

fn restore_remote_url(path: &Path, key: &str, urls: &[String]) -> Result<()> {
    if let Some((first, rest)) = urls.split_first() {
        set_remote_url(path, key, first)?;
        for url in rest {
            checked_output([
                OsStr::new("-C"),
                path.as_os_str(),
                OsStr::new("config"),
                OsStr::new("--local"),
                OsStr::new("--add"),
                OsStr::new(key),
                OsStr::new(url),
            ])?;
        }
    } else {
        let output = raw_output([
            OsStr::new("-C"),
            path.as_os_str(),
            OsStr::new("config"),
            OsStr::new("--local"),
            OsStr::new("--unset-all"),
            OsStr::new(key),
        ])?;
        // Git returns 5 when the requested key is already absent.
        if !output.status.success() && output.status.code() != Some(5) {
            bail!(
                "could not remove {key}: Git exited with {}: {}",
                output.status,
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
    }
    Ok(())
}

fn github_repository_identity(url: &str, verified_aliases: &[String]) -> Option<GitHubRepository> {
    // Try HTTPS github.com
    if let Some(path) = url.strip_prefix("https://github.com/") {
        return github_repository_path(path);
    }
    // Try SSH git@github.com:
    if let Some(path) = url.strip_prefix("git@github.com:") {
        return github_repository_path(path);
    }
    // Try SSH ssh://git@github.com/ (without port)
    if let Some(path) = url.strip_prefix("ssh://git@github.com/") {
        // Reject URLs with port (e.g., ssh://git@github.com:2222/...)
        if path.contains(':') {
            return None;
        }
        return github_repository_path(path);
    }
    // Try Rot-managed SSH aliases
    if let Some(path) = url.strip_prefix("git@") {
        let (host, path) = path.split_once(':')?;
        if verified_aliases.iter().any(|alias| alias == host) {
            return github_repository_path(path);
        }
    }
    None
}

fn github_alias_candidate(url: &str) -> Option<GitHubRepository> {
    let path = url.strip_prefix("git@")?;
    let (host, path) = path.split_once(':')?;
    (host != "github.com" && valid_ssh_alias(host)).then(|| github_repository_path(path))?
}

fn github_urls_may_match_verified_alias(actual: &str, configured: &str) -> bool {
    let actual = github_repository_identity(actual, &[]).or_else(|| github_alias_candidate(actual));
    let configured =
        github_repository_identity(configured, &[]).or_else(|| github_alias_candidate(configured));
    matches!((actual, configured), (Some(actual), Some(configured))
        if actual.owner.eq_ignore_ascii_case(&configured.owner)
            && actual.name.eq_ignore_ascii_case(&configured.name))
}

fn github_https_repository(url: &str) -> Option<GitHubRepository> {
    github_repository_path(url.strip_prefix("https://github.com/")?)
}

fn github_https_read_url(url: &str) -> Option<String> {
    // Convert SSH GitHub URLs to HTTPS read URLs
    // Only accepts canonical github.com SSH forms (no aliases)
    // Rejects already-HTTPS URLs (they don't need conversion)
    if url.starts_with("https://github.com/") {
        return None;
    }
    let repository = github_ssh_repository(url, &[])?;
    if !repository
        .owner
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        || !repository
            .name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    {
        return None;
    }
    Some(format!(
        "https://github.com/{}/{}.git",
        repository.owner, repository.name
    ))
}

fn configure_read_remote(path: &Path, configured_url: &str) -> Result<()> {
    let Some(read_url) = github_https_read_url(configured_url) else {
        return Ok(());
    };
    let actual = fetch_url(path)?.context("repository has no origin URL")?;
    if !urls_match(&actual, configured_url) {
        bail!("repository origin is not the configured Git repository");
    }
    let configured_push = push_url(path)?;
    let push = configured_push
        .as_deref()
        .unwrap_or(configured_url)
        .to_owned();
    if normalize_url(&actual) != normalize_url(&read_url) || configured_push.is_none() {
        reconcile_remote(path, &read_url, &push)?;
    }
    Ok(())
}

fn github_ssh_repository(url: &str, verified_aliases: &[String]) -> Option<GitHubRepository> {
    github_repository_identity(url, verified_aliases)
}

fn github_repository_path(path: &str) -> Option<GitHubRepository> {
    let path = path.trim_end_matches('/');
    let path = path.strip_suffix(".git").unwrap_or(path);
    let (owner, name) = path.split_once('/')?;
    if owner.is_empty() || name.is_empty() || name.contains('/') {
        return None;
    }
    Some(GitHubRepository {
        owner: owner.to_owned(),
        name: name.to_owned(),
    })
}

fn query(path: &Path, arguments: &[&str]) -> Result<String> {
    let mut command_arguments = vec![OsString::from("-C"), path.as_os_str().to_owned()];
    command_arguments.extend(arguments.iter().map(|argument| OsString::from(*argument)));
    Ok(stdout_text(&checked_output(command_arguments)?))
}

fn network_query(
    path: &Path,
    arguments: &[&str],
    known_url: Option<&str>,
    interaction: &mut dyn Interaction,
) -> Result<String> {
    let mut command_arguments = vec![OsString::from("-C"), path.as_os_str().to_owned()];
    command_arguments.extend(arguments.iter().map(|argument| OsString::from(*argument)));
    Ok(stdout_text(&checked_network_output(
        command_arguments,
        known_url.unwrap_or(""),
        interaction,
    )?))
}

fn read_network_query(
    path: &Path,
    preferred_arguments: &[&str],
    preferred_url: &str,
    ssh_arguments: &[&str],
    ssh_url: &str,
    interaction: &mut dyn Interaction,
) -> Result<String> {
    let command_arguments = |arguments: &[&str]| {
        let mut command_arguments = vec![OsString::from("-C"), path.as_os_str().to_owned()];
        command_arguments.extend(arguments.iter().map(OsString::from));
        command_arguments
    };
    Ok(stdout_text(&checked_read_network_output(
        command_arguments(preferred_arguments),
        preferred_url,
        command_arguments(ssh_arguments),
        ssh_url,
        interaction,
    )?))
}

fn configured_remote_url(path: &Path, push: bool) -> Result<Option<String>> {
    if push {
        let output = raw_output([
            OsStr::new("-C"),
            path.as_os_str(),
            OsStr::new("config"),
            OsStr::new("--get"),
            OsStr::new("remote.origin.pushurl"),
        ])?;
        if output.status.success() {
            return Ok(Some(stdout_text(&output)));
        }
    }
    let output = raw_output([
        OsStr::new("-C"),
        path.as_os_str(),
        OsStr::new("config"),
        OsStr::new("--get"),
        OsStr::new("remote.origin.url"),
    ])?;
    Ok(output.status.success().then(|| stdout_text(&output)))
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

fn checked_network_output(
    arguments: Vec<OsString>,
    canonical_url: &str,
    interaction: &mut dyn Interaction,
) -> Result<Output> {
    let control = interaction.process_control();
    let original_arguments = arguments.clone();
    let without_interactive_executor = crate::process::Control {
        interactive_executor: None,
        ..control.clone()
    };
    let mut first_attempt = true;
    let result = checked_network_output_with(
        arguments,
        canonical_url,
        |arguments| {
            let attempt_control = if first_attempt {
                first_attempt = false;
                &without_interactive_executor
            } else {
                &control
            };
            raw_network_output_control(arguments, attempt_control)
        },
        query_rot_identities,
        interaction.can_choose(),
        interaction,
    );
    match result {
        Ok(output) => Ok(output),
        Err(original_error) if control.interactive_executor.is_some() => {
            let output = raw_interactive_network_output_control(&original_arguments, &control)?
                .expect("interactive executor was checked");
            if output.status.success() {
                Ok(output)
            } else {
                Err(original_error.context(format!(
                    "Interactive Git retry failed: {}",
                    git_error_message(&output)
                )))
            }
        }
        Err(error) => Err(error),
    }
}

fn checked_read_network_output(
    preferred_arguments: Vec<OsString>,
    preferred_url: &str,
    ssh_arguments: Vec<OsString>,
    ssh_url: &str,
    interaction: &mut dyn Interaction,
) -> Result<Output> {
    let control = interaction.process_control();
    checked_read_network_output_with(
        preferred_arguments,
        preferred_url,
        ssh_arguments,
        ssh_url,
        |arguments| raw_network_output_control(arguments, &control),
        query_rot_identities,
        interaction.can_choose(),
        interaction,
    )
}

#[allow(clippy::too_many_arguments)]
fn checked_read_network_output_with<G, I, P>(
    preferred_arguments: Vec<OsString>,
    preferred_url: &str,
    ssh_arguments: Vec<OsString>,
    ssh_url: &str,
    mut run_git: G,
    identities: I,
    interactive: bool,
    prompt: &mut P,
) -> Result<Output>
where
    G: FnMut(&[OsString]) -> Result<Output>,
    I: FnMut() -> Result<Vec<RotIdentity>>,
    P: Interaction + ?Sized,
{
    let preferred = run_git(&preferred_arguments)?;
    if preferred.status.success() {
        return Ok(preferred);
    }
    let preferred_error = git_error_message(&preferred);
    checked_network_output_with(
        ssh_arguments,
        ssh_url,
        |arguments| run_git(arguments),
        identities,
        interactive,
        prompt,
    )
    .map_err(|error| {
        anyhow::anyhow!(
            "HTTPS read from {preferred_url} failed: {preferred_error}\n\nSSH fallback failed: {error:#}"
        )
    })
}

fn checked_network_output_with<G, I, P>(
    arguments: Vec<OsString>,
    canonical_url: &str,
    mut run_git: G,
    mut identities: I,
    interactive: bool,
    prompt: &mut P,
) -> Result<Output>
where
    G: FnMut(&[OsString]) -> Result<Output>,
    I: FnMut() -> Result<Vec<RotIdentity>>,
    P: Interaction + ?Sized,
{
    let output = run_git(&arguments)?;
    if output.status.success() {
        return Ok(output);
    }
    let original_error = git_error_message(&output);
    if runtime_url_rewrite(canonical_url, "placeholder").is_none()
        || !is_public_key_auth_failure(&output)
    {
        bail!(original_error);
    }

    let identities =
        identities().map_err(|error| authentication_context(error, &original_error))?;
    let identity = select_rot_identity(identities, interactive, prompt)
        .map_err(|error| authentication_context(error, &original_error))?;
    let rewrite = runtime_url_rewrite(canonical_url, &identity.alias)
        .context("could not prepare the selected Rot SSH identity")?;

    let mut retry_arguments = vec![OsString::from("-c"), OsString::from(rewrite)];
    retry_arguments.extend(arguments);
    let retry = run_git(&retry_arguments)?;
    if !retry.status.success() {
        bail!(
            "{original_error}\n\nRetry with Rot-managed SSH identity '{}' failed: {}",
            identity.alias,
            git_error_message(&retry)
        );
    }
    Ok(retry)
}

fn authentication_context(error: anyhow::Error, original: &str) -> anyhow::Error {
    if error.downcast_ref::<crate::process::Cancelled>().is_some()
        || error
            .downcast_ref::<crate::process::CleanupIncomplete>()
            .is_some()
        || error.downcast_ref::<crate::persistence::Busy>().is_some()
    {
        error.context(original.to_owned())
    } else {
        anyhow::anyhow!("{original}\n\n{error}")
    }
}

fn query_rot_identities() -> Result<Vec<RotIdentity>> {
    let operation_id = crate::process::OperationId(rand::random());
    let output = crate::process::execute(
        Command::new("rot").args(["ssh", "identities", "--json"]),
        crate::process::Mode::Capture { limit: 1024 * 1024 },
        &crate::process::current_control(),
        operation_id,
    )
    .map_err(|error| {
        if error
            .downcast_ref::<std::io::Error>()
            .is_some_and(|io| io.kind() == std::io::ErrorKind::NotFound)
        {
            error.context(
                "Rot is not installed in PATH. Configure GitHub SSH normally or install Rot.",
            )
        } else {
            error.context("could not query Rot-managed SSH identities")
        }
    })?;
    if !output.status.success() {
        bail!("Rot could not inspect its managed GitHub SSH identities");
    }
    parse_rot_identities(&output.stdout)
}

fn parse_rot_identities(json: &[u8]) -> Result<Vec<RotIdentity>> {
    let document: RotIdentityDocument =
        serde_json::from_slice(json).context("Rot returned an invalid SSH identity document")?;
    if document.version != ROT_IDENTITY_VERSION {
        bail!(
            "Rot returned unsupported SSH identity document version {}",
            document.version
        );
    }

    let mut identities = Vec::new();
    for identity in document.identities {
        if identity.verification == "verified"
            && identity.username.is_some()
            && valid_ssh_alias(&identity.alias)
            && !identities
                .iter()
                .any(|existing: &RotIdentity| existing.alias == identity.alias)
        {
            identities.push(identity);
        }
    }
    Ok(identities)
}

fn select_rot_identity<P: Interaction + ?Sized>(
    identities: Vec<RotIdentity>,
    interactive: bool,
    prompt: &mut P,
) -> Result<RotIdentity> {
    if identities.is_empty() {
        bail!(
            "No verified Rot-managed GitHub SSH identity was found. Configure SSH through Rot, then retry."
        );
    }
    if identities.len() == 1 {
        return Ok(identities.into_iter().next().expect("one identity"));
    }
    if !interactive {
        let available = identities
            .iter()
            .map(|identity| identity.alias.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        bail!(
            "Multiple verified Rot-managed GitHub SSH identities are available ({available}). Re-run interactively to choose one."
        );
    }

    let index = prompt
        .choose_identity(&identities)?
        .ok_or(crate::process::Cancelled)
        .context("SSH identity selection was cancelled")?;
    if index >= identities.len() {
        anyhow::bail!("an invalid SSH identity was selected");
    }
    Ok(identities[index].clone())
}

fn runtime_url_rewrite(canonical_url: &str, alias: &str) -> Option<String> {
    if let Some(path) = canonical_url.strip_prefix("git@github.com:")
        && !path.is_empty()
    {
        return Some(format!("url.git@{alias}:{path}.insteadOf={canonical_url}"));
    }
    if let Some(path) = canonical_url.strip_prefix("ssh://git@github.com/")
        && !path.is_empty()
    {
        return Some(format!(
            "url.ssh://git@{alias}/{path}.insteadOf={canonical_url}"
        ));
    }
    None
}

fn valid_ssh_alias(alias: &str) -> bool {
    !alias.is_empty()
        && alias
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

fn is_public_key_auth_failure(output: &Output) -> bool {
    let stderr = String::from_utf8_lossy(&output.stderr).to_ascii_lowercase();
    stderr.contains("permission denied (publickey)")
        || (stderr.contains("publickey")
            && (stderr.contains("authentication failed")
                || stderr.contains("no supported authentication methods")))
}

fn git_error_message(output: &Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
    if stderr.is_empty() {
        format!("Git command failed with status {}", output.status)
    } else {
        format!("Git command failed: {stderr}")
    }
}

fn raw_output<I, S>(arguments: I) -> Result<Output>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    raw_output_control(arguments, &crate::process::current_control())
}

fn raw_output_control<I, S>(arguments: I, control: &crate::process::Control) -> Result<Output>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let operation_id = crate::process::OperationId(rand::random());
    crate::process::execute(
        Command::new("git").args(arguments),
        crate::process::Mode::Capture {
            limit: 4 * 1024 * 1024,
        },
        control,
        operation_id,
    )
    .context("could not execute Git (ensure Git is available in PATH)")
}

fn raw_network_output_control<I, S>(
    arguments: I,
    control: &crate::process::Control,
) -> Result<Output>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let noninteractive = matches!(control.policy, crate::process::ExecutionPolicy::Background);
    let arguments = arguments
        .into_iter()
        .map(|argument| argument.as_ref().to_owned())
        .collect::<Vec<_>>();
    let mut command = network_git_command(&arguments, noninteractive);
    let mode = crate::process::Mode::Capture {
        limit: 4 * 1024 * 1024,
    };
    let operation_id = crate::process::OperationId(rand::random());
    let output = if noninteractive {
        crate::process::execute_with_timeout(
            &mut command,
            mode,
            control,
            NETWORK_GIT_TIMEOUT,
            operation_id,
        )
    } else {
        crate::process::execute(&mut command, mode, control, operation_id)
    };
    let output = output.context(
        "could not execute network Git operation (ensure Git and SSH are available in PATH)",
    )?;
    if output.status.success() || !noninteractive {
        return Ok(output);
    }
    let Some(output) = raw_interactive_network_output_control(&arguments, control)? else {
        return Ok(output);
    };

    Ok(output)
}

fn raw_interactive_network_output_control(
    arguments: &[OsString],
    control: &crate::process::Control,
) -> Result<Option<Output>> {
    let Some(executor) = &control.interactive_executor else {
        return Ok(None);
    };
    // The background attempt deliberately disables prompting. If it cannot
    // complete, retry the exact backend-built command with terminal access.
    // This avoids guessing which credential helper or SSH implementation will
    // prompt, and keeps executable/argv ownership entirely in Rust.
    let mut command = crate::process::InteractiveCommand::new("git");
    command.args(arguments);
    let result = executor(command, crate::process::OperationId::random())
        .context("interactive Git execution failed")?;
    if result.cancelled {
        return Err(crate::process::Cancelled.into());
    }
    let code = if result.status.success() {
        0
    } else {
        result.status.code.max(1)
    };
    Ok(Some(Output {
        status: interactive_exit_status(code),
        stdout: Vec::new(),
        stderr: result.output,
    }))
}

#[cfg(unix)]
fn interactive_exit_status(code: u32) -> std::process::ExitStatus {
    use std::os::unix::process::ExitStatusExt;
    std::process::ExitStatus::from_raw((code.min(255) as i32) << 8)
}

#[cfg(windows)]
fn interactive_exit_status(code: u32) -> std::process::ExitStatus {
    use std::os::windows::process::ExitStatusExt;
    std::process::ExitStatus::from_raw(code)
}

fn network_git_command<I, S>(arguments: I, noninteractive: bool) -> Command
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let mut command = Command::new("git");
    command.args(arguments);
    if noninteractive {
        command
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GIT_SSH_COMMAND", "ssh -o BatchMode=yes")
            .env("SSH_ASKPASS_REQUIRE", "never")
            .env("GCM_INTERACTIVE", "Never");
    }
    command
}

fn stdout_text(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

fn urls_match(actual: &str, configured: &str) -> bool {
    if normalize_url(actual) == normalize_url(configured) {
        return true;
    }
    github_urls_have_same_identity(actual, configured, &[])
}

fn github_urls_have_same_identity(
    actual: &str,
    configured: &str,
    verified_aliases: &[String],
) -> bool {
    let actual = github_repository_identity(actual, verified_aliases);
    let configured = github_repository_identity(configured, verified_aliases);
    matches!((actual, configured), (Some(actual), Some(configured))
        if actual.owner.eq_ignore_ascii_case(&configured.owner)
            && actual.name.eq_ignore_ascii_case(&configured.name))
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
    #[cfg(unix)]
    use std::os::unix::process::ExitStatusExt;

    struct TestPrompt {
        selection: Option<String>,
        select_calls: usize,
        choices: Vec<String>,
    }

    impl TestPrompt {
        fn new(selection: Option<&str>) -> Self {
            Self {
                selection: selection.map(str::to_owned),
                select_calls: 0,
                choices: Vec::new(),
            }
        }
    }

    impl Interaction for TestPrompt {
        fn choose_identity(&mut self, identities: &[RotIdentity]) -> Result<Option<usize>> {
            self.select_calls += 1;
            self.choices = identities
                .iter()
                .map(|identity| {
                    format!(
                        "{} -> {}",
                        identity.alias,
                        identity.username.as_deref().unwrap()
                    )
                })
                .collect();
            Ok(self
                .selection
                .as_ref()
                .and_then(|selection| self.choices.iter().position(|choice| choice == selection)))
        }
    }

    fn identity(alias: &str, username: &str) -> RotIdentity {
        RotIdentity {
            alias: alias.to_owned(),
            username: Some(username.to_owned()),
            verification: "verified".to_owned(),
        }
    }

    #[cfg(unix)]
    fn command_output(success: bool, stderr: &str) -> Output {
        Output {
            status: std::process::ExitStatus::from_raw(if success { 0 } else { 128 << 8 }),
            stdout: Vec::new(),
            stderr: stderr.as_bytes().to_vec(),
        }
    }

    #[test]
    fn url_comparison_uses_canonical_github_identity_across_read_transports() {
        assert!(urls_match(
            "https://github.com/owner/repo.git/",
            "https://github.com/owner/repo"
        ));
        assert!(urls_match(
            "https://github.com/owner/repo",
            "git@github.com:owner/repo"
        ));
        assert!(urls_match(
            "ssh://git@github.com/OWNER/REPO.git",
            "https://github.com/owner/repo.git"
        ));
        assert!(!urls_match(
            "https://github.com/other/repo.git",
            "git@github.com:owner/repo.git"
        ));
        assert!(!urls_match(
            "git@gitlab.com:owner/repo.git",
            "https://github.com/owner/repo.git"
        ));
    }

    #[test]
    fn github_ssh_read_urls_are_narrowly_converted_to_https() {
        assert_eq!(
            github_https_read_url("git@github.com:OWNER/REPO.git").as_deref(),
            Some("https://github.com/OWNER/REPO.git")
        );
        assert_eq!(
            github_https_read_url("ssh://git@github.com/OWNER/REPO.git").as_deref(),
            Some("https://github.com/OWNER/REPO.git")
        );
        for unchanged in [
            "https://github.com/OWNER/REPO.git",
            "git@gitlab.com:OWNER/REPO.git",
            "git@github.example.com:OWNER/REPO.git",
            "ssh://git@github.com:2222/OWNER/REPO.git",
            "git@github.com:OWNER/nested/REPO.git",
            "git@github.com:OWNER/REPO.git?token=secret",
        ] {
            assert_eq!(github_https_read_url(unchanged), None, "{unchanged}");
        }
    }

    #[test]
    fn clone_arguments_use_the_derived_https_read_url() {
        let configured = "git@github.com:owner/repo.git";
        let destination = Path::new("checkout");
        let (arguments, read_url) = clone_arguments(configured, Some("main"), destination);

        assert_eq!(
            arguments,
            ["clone", "--branch", "main", "--", &read_url, "checkout"].map(OsString::from)
        );
        assert!(!arguments.iter().any(|argument| argument == configured));
    }

    #[test]
    fn github_repository_identity_accepts_only_approved_transports_and_aliases() {
        let configured = github_https_repository("https://github.com/Owner/Repo.git").unwrap();
        for actual in [
            "git@github.com:owner/repo.git",
            "ssh://git@github.com/OWNER/REPO.git",
            "git@github-work:Owner/Repo.git",
        ] {
            let parsed = github_ssh_repository(actual, &["github-work".to_owned()]).unwrap();
            assert!(parsed.owner.eq_ignore_ascii_case(&configured.owner));
            assert!(parsed.name.eq_ignore_ascii_case(&configured.name));
        }

        assert!(github_ssh_repository("git@unknown:Owner/Repo.git", &[]).is_none());
        assert!(github_ssh_repository("git@gitlab.com:Owner/Repo.git", &[]).is_none());
        assert!(github_https_repository("https://example.com/Owner/Repo.git").is_none());
    }

    #[test]
    fn github_repository_identity_keeps_owner_and_repository_boundaries() {
        let configured = github_https_repository("https://github.com/owner/repo.git").unwrap();
        let other_owner = github_ssh_repository("git@github.com:other/repo.git", &[]).unwrap();
        let other_repo = github_ssh_repository("git@github.com:owner/other.git", &[]).unwrap();

        assert_ne!(configured, other_owner);
        assert_ne!(configured, other_repo);
        assert!(github_repository_path("owner/nested/repo.git").is_none());
    }

    #[test]
    fn github_push_url_uses_the_verified_alias_and_canonical_path() {
        assert_eq!(
            github_ssh_push_url("https://github.com/0xkamaji/rot-tools.git", "github-kamaji"),
            Some("git@github-kamaji:0xkamaji/rot-tools.git".to_owned())
        );
        assert_eq!(
            github_ssh_push_url("git@github.com:0xkamaji/private.git", "github-kamaji"),
            None
        );
        assert_eq!(
            github_ssh_push_url("https://github.com/owner/repo.git", "bad alias"),
            None
        );
    }

    #[test]
    fn managed_checkout_matches_canonical_transports_and_only_verified_aliases() {
        if raw_output([OsStr::new("--version")]).is_err() {
            return;
        }
        let temporary = tempfile::tempdir().unwrap();
        let repository = temporary.path();
        checked_output([
            OsStr::new("init"),
            OsStr::new("--quiet"),
            repository.as_os_str(),
        ])
        .unwrap();
        let canonical = ManagedRepositoryMatcher::canonical();
        let verified = ManagedRepositoryMatcher::from_verified_rot_identities(&[identity(
            "github-work",
            "owner",
        )]);
        let configured = "https://github.com/owner/repo.git";

        set_remote_url(
            repository,
            "remote.origin.url",
            "git@github.com:OWNER/REPO.git",
        )
        .unwrap();
        assert!(
            canonical
                .is_managed_checkout(repository, configured)
                .unwrap()
        );

        set_remote_url(
            repository,
            "remote.origin.url",
            "git@github-work:owner/repo.git",
        )
        .unwrap();
        assert!(
            verified
                .is_managed_checkout(repository, configured)
                .unwrap()
        );
        assert!(
            !canonical
                .is_managed_checkout(repository, configured)
                .unwrap()
        );

        set_remote_url(
            repository,
            "remote.origin.url",
            "https://github.com/owner/repo.git",
        )
        .unwrap();
        assert!(
            verified
                .is_managed_checkout(repository, "git@github-work:OWNER/REPO.git")
                .unwrap()
        );
        assert!(
            !canonical
                .is_managed_checkout(repository, "git@github-work:owner/repo.git")
                .unwrap()
        );

        set_remote_url(
            repository,
            "remote.origin.url",
            "git@random-host:owner/repo.git",
        )
        .unwrap();
        assert!(
            !verified
                .is_managed_checkout(repository, configured)
                .unwrap()
        );

        set_remote_url(
            repository,
            "remote.origin.url",
            "git@github-work:owner/different.git",
        )
        .unwrap();
        assert!(
            !verified
                .is_managed_checkout(repository, configured)
                .unwrap()
        );
    }

    #[test]
    fn repository_match_accepts_https_fetch_for_configured_github_ssh() {
        if raw_output([OsStr::new("--version")]).is_err() {
            return;
        }
        let temporary = tempfile::tempdir().unwrap();
        let repository = temporary.path();
        checked_output([
            OsStr::new("init"),
            OsStr::new("--quiet"),
            repository.as_os_str(),
        ])
        .unwrap();
        set_remote_url(
            repository,
            "remote.origin.url",
            "https://github.com/owner/repo.git",
        )
        .unwrap();

        assert_eq!(
            ManagedRepositoryMatcher::canonical()
                .checkout_match(repository, "git@github.com:OWNER/REPO.git")
                .unwrap(),
            ManagedCheckout::ExpectedTransport
        );
        assert!(
            ManagedRepositoryMatcher::canonical()
                .is_managed_checkout(repository, "ssh://git@github.com/owner/repo.git")
                .unwrap()
        );
    }

    #[test]
    fn remote_reconciliation_changes_only_fetch_and_push_configuration() {
        if raw_output([OsStr::new("--version")]).is_err() {
            return;
        }
        let temporary = tempfile::tempdir().unwrap();
        let repository = temporary.path();
        checked_output([
            OsStr::new("init"),
            OsStr::new("--quiet"),
            repository.as_os_str(),
        ])
        .unwrap();
        set_remote_url(
            repository,
            "remote.origin.url",
            "git@github.com:owner/repo.git",
        )
        .unwrap();
        fs::write(repository.join("dirty.txt"), "preserve\n").unwrap();
        let before = working_tree_changes(repository).unwrap();

        reconcile_remote(
            repository,
            "https://github.com/owner/repo.git",
            "git@github.com:owner/repo.git",
        )
        .unwrap();

        assert_eq!(
            fetch_url(repository).unwrap().as_deref(),
            Some("https://github.com/owner/repo.git")
        );
        assert_eq!(
            push_url(repository).unwrap().as_deref(),
            Some("git@github.com:owner/repo.git")
        );
        assert_eq!(working_tree_changes(repository).unwrap(), before);
        assert_eq!(
            fs::read_to_string(repository.join("dirty.txt")).unwrap(),
            "preserve\n"
        );
    }

    #[test]
    fn github_ssh_remote_is_configured_for_https_reads_and_original_ssh_pushes() {
        if raw_output([OsStr::new("--version")]).is_err() {
            return;
        }
        let temporary = tempfile::tempdir().unwrap();
        let repository = temporary.path();
        checked_output([
            OsStr::new("init"),
            OsStr::new("--quiet"),
            repository.as_os_str(),
        ])
        .unwrap();
        let configured = "ssh://git@github.com/owner/repo.git";
        set_remote_url(repository, "remote.origin.url", configured).unwrap();

        configure_read_remote(repository, configured).unwrap();

        assert_eq!(
            fetch_url(repository).unwrap().as_deref(),
            Some("https://github.com/owner/repo.git")
        );
        assert_eq!(push_url(repository).unwrap().as_deref(), Some(configured));
        assert!(
            ManagedRepositoryMatcher::canonical()
                .is_managed_checkout(repository, configured)
                .unwrap()
        );
    }

    #[test]
    fn github_read_transport_preserves_an_existing_authenticated_push_url() {
        if raw_output([OsStr::new("--version")]).is_err() {
            return;
        }
        let temporary = tempfile::tempdir().unwrap();
        let repository = temporary.path();
        checked_output([
            OsStr::new("init"),
            OsStr::new("--quiet"),
            repository.as_os_str(),
        ])
        .unwrap();
        let configured = "git@github.com:owner/repo.git";
        set_remote_url(repository, "remote.origin.url", configured).unwrap();
        set_remote_url(
            repository,
            "remote.origin.pushurl",
            "git@github-work:owner/repo.git",
        )
        .unwrap();

        configure_read_remote(repository, configured).unwrap();

        assert_eq!(
            fetch_url(repository).unwrap().as_deref(),
            Some("https://github.com/owner/repo.git")
        );
        assert_eq!(
            push_url(repository).unwrap().as_deref(),
            Some("git@github-work:owner/repo.git")
        );
    }

    #[test]
    fn https_and_non_github_urls_never_receive_ssh_rewrites() {
        assert_eq!(
            runtime_url_rewrite("https://github.com/owner/repo.git", "github-work"),
            None
        );
        assert_eq!(
            runtime_url_rewrite("git@gitlab.com:owner/repo.git", "github-work"),
            None
        );
    }

    #[test]
    fn github_ssh_rewrites_are_command_scoped_and_leave_canonical_url_unchanged() {
        let canonical = "git@github.com:0xkamaji/private-repo.git";
        assert_eq!(
            runtime_url_rewrite(canonical, "github-kamaji"),
            Some(
                "url.git@github-kamaji:0xkamaji/private-repo.git.insteadOf=git@github.com:0xkamaji/private-repo.git"
                    .to_owned()
            )
        );
        assert_eq!(canonical, "git@github.com:0xkamaji/private-repo.git");

        assert_eq!(
            runtime_url_rewrite(
                "ssh://git@github.com/0xkamaji/private-repo.git",
                "github-kamaji"
            ),
            Some(
                "url.ssh://git@github-kamaji/0xkamaji/private-repo.git.insteadOf=ssh://git@github.com/0xkamaji/private-repo.git"
                    .to_owned()
            )
        );
    }

    #[test]
    fn rot_document_accepts_only_verified_safe_identities() {
        let identities = parse_rot_identities(
            br#"{"version":1,"future_field":true,"identities":[
                {"alias":"github-kamaji","username":"0xkamaji","verification":"verified","future_field":42},
                {"alias":"github-work","username":null,"verification":"unverified"},
                {"alias":"bad alias","username":"bad","verification":"verified"},
                {"alias":"github-kamaji","username":"duplicate","verification":"verified"}
            ]}"#,
        )
        .unwrap();

        assert_eq!(identities, vec![identity("github-kamaji", "0xkamaji")]);
    }

    #[test]
    fn unsupported_rot_contract_version_is_rejected() {
        let error = parse_rot_identities(br#"{"version":2,"identities":[]}"#).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("unsupported SSH identity document version 2")
        );
    }

    #[test]
    fn rot_document_accepts_empty_results_and_rejects_malformed_contracts() {
        assert!(
            parse_rot_identities(br#"{"version":1,"identities":[]}"#)
                .unwrap()
                .is_empty()
        );
        for document in [
            b"not JSON".as_slice(),
            b"[]",
            br#"{"identities":[]}"#,
            br#"{"version":1}"#,
            br#"{"version":1,"identities":[{"alias":"github-work"}]}"#,
        ] {
            assert!(parse_rot_identities(document).is_err());
        }
    }

    #[test]
    fn exactly_one_verified_identity_is_selected_without_prompting() {
        let mut prompt = TestPrompt::new(None);
        let selected = select_rot_identity(
            vec![identity("github-kamaji", "0xkamaji")],
            false,
            &mut prompt,
        )
        .unwrap();

        assert_eq!(selected.alias, "github-kamaji");
        assert_eq!(prompt.select_calls, 0);
    }

    #[test]
    fn multiple_identities_fail_without_prompting_non_interactively() {
        let mut prompt = TestPrompt::new(None);
        let error = select_rot_identity(
            vec![
                identity("github-kamaji", "0xkamaji"),
                identity("github-work", "work-account"),
            ],
            false,
            &mut prompt,
        )
        .unwrap_err();

        assert!(error.to_string().contains("Re-run interactively"));
        assert_eq!(prompt.select_calls, 0);
    }

    #[test]
    fn multiple_identities_use_a_friendly_interactive_chooser() {
        let mut prompt = TestPrompt::new(Some("github-work -> work-account"));
        let selected = select_rot_identity(
            vec![
                identity("github-kamaji", "0xkamaji"),
                identity("github-work", "work-account"),
            ],
            true,
            &mut prompt,
        )
        .unwrap();

        assert_eq!(selected.alias, "github-work");
        assert_eq!(prompt.select_calls, 1);
        assert_eq!(
            prompt.choices,
            vec!["github-kamaji -> 0xkamaji", "github-work -> work-account"]
        );
    }

    #[test]
    fn no_verified_identity_has_rot_setup_guidance() {
        let mut prompt = TestPrompt::new(None);
        let error = select_rot_identity(Vec::new(), false, &mut prompt).unwrap_err();
        assert!(error.to_string().contains("Configure SSH through Rot"));
        assert_eq!(prompt.select_calls, 0);
    }

    #[cfg(unix)]
    #[test]
    fn successful_public_ssh_operation_never_queries_rot() {
        let mut prompt = TestPrompt::new(None);
        let mut identity_queries = 0;
        let output = checked_network_output_with(
            vec![OsString::from("clone")],
            "git@github.com:owner/public.git",
            |_| Ok(command_output(true, "")),
            || {
                identity_queries += 1;
                Ok(Vec::new())
            },
            false,
            &mut prompt,
        )
        .unwrap();

        assert!(output.status.success());
        assert_eq!(identity_queries, 0);
        assert_eq!(prompt.select_calls, 0);
    }

    #[test]
    fn network_git_commands_are_noninteractive_without_disabling_host_verification() {
        let command = network_git_command(["fetch", "origin"], true);
        let environment = command
            .get_envs()
            .map(|(name, value)| {
                (
                    name.to_string_lossy().into_owned(),
                    value.map(|value| value.to_string_lossy().into_owned()),
                )
            })
            .collect::<std::collections::BTreeMap<_, _>>();
        assert_eq!(environment["GIT_TERMINAL_PROMPT"].as_deref(), Some("0"));
        assert_eq!(
            environment["GIT_SSH_COMMAND"].as_deref(),
            Some("ssh -o BatchMode=yes")
        );
        assert_eq!(environment["SSH_ASKPASS_REQUIRE"].as_deref(), Some("never"));
        assert_eq!(environment["GCM_INTERACTIVE"].as_deref(), Some("Never"));

        let configuration = command
            .get_args()
            .map(|argument| argument.to_string_lossy())
            .chain(environment.values().flatten().map(|value| value.into()))
            .collect::<Vec<_>>()
            .join(" ");
        assert!(!configuration.contains("StrictHostKeyChecking"));
        assert!(!configuration.contains("UserKnownHostsFile"));

        let interactive = network_git_command(["fetch", "origin"], false);
        assert!(interactive.get_envs().next().is_none());
    }

    #[test]
    fn noninteractive_network_executor_preserves_success_and_real_stderr() {
        let control = crate::process::Control {
            policy: crate::process::ExecutionPolicy::Background,
            ..crate::process::Control::default()
        };
        let success = raw_network_output_control(["--version"], &control).unwrap();
        assert!(success.status.success());
        assert!(stdout_text(&success).starts_with("git version"));

        let failure =
            raw_network_output_control(["--definitely-not-a-git-option"], &control).unwrap();
        assert!(!failure.status.success());
        let stderr = String::from_utf8_lossy(&failure.stderr);
        assert!(!stderr.trim().is_empty());
        assert!(git_error_message(&failure).contains(stderr.trim()));
    }

    #[cfg(unix)]
    #[test]
    fn anonymous_https_failure_retries_original_ssh_through_rot() {
        let mut prompt = TestPrompt::new(None);
        let configured = "git@github.com:owner/private.git";
        let (preferred, read_url) = clone_arguments(configured, None, Path::new("checkout"));
        let ssh = clone_arguments_for_url(configured, None, Path::new("checkout"));
        let mut calls = Vec::new();
        let output = checked_read_network_output_with(
            preferred.clone(),
            &read_url,
            ssh.clone(),
            configured,
            |arguments| {
                calls.push(arguments.to_vec());
                Ok(match calls.len() {
                    1 => command_output(false, "remote: Repository not found."),
                    2 => command_output(false, "Permission denied (publickey)."),
                    _ => command_output(true, ""),
                })
            },
            || Ok(vec![identity("github-work", "owner")]),
            false,
            &mut prompt,
        )
        .unwrap();

        assert!(output.status.success());
        assert_eq!(read_url, "https://github.com/owner/private.git");
        assert_eq!(calls[0], preferred);
        assert_eq!(calls[1], ssh);
        assert_eq!(calls[2][0], OsString::from("-c"));
        assert_eq!(
            calls[2][1],
            OsString::from(
                "url.git@github-work:owner/private.git.insteadOf=git@github.com:owner/private.git"
            )
        );
        assert_eq!(&calls[2][2..], ssh.as_slice());
    }

    #[cfg(unix)]
    #[test]
    fn anonymous_https_and_ssh_failures_are_both_preserved() {
        let mut prompt = TestPrompt::new(None);
        let error = checked_read_network_output_with(
            vec![OsString::from("clone"), OsString::from("https")],
            "https://github.com/owner/private.git",
            vec![OsString::from("clone"), OsString::from("ssh")],
            "git@github.com:owner/private.git",
            |arguments| {
                if arguments.last() == Some(&OsString::from("https")) {
                    Ok(command_output(false, "remote: Repository not found."))
                } else {
                    Ok(command_output(false, "Permission denied (publickey)."))
                }
            },
            || bail!("Rot is not installed"),
            false,
            &mut prompt,
        )
        .unwrap_err();

        let message = format!("{error:#}");
        assert!(message.contains("HTTPS read"));
        assert!(message.contains("Repository not found"));
        assert!(message.contains("SSH fallback failed"));
        assert!(message.contains("Permission denied (publickey)"));
        assert!(message.contains("Rot is not installed"));
    }

    #[cfg(unix)]
    #[test]
    fn all_network_operations_share_the_transient_rot_retry() {
        let operations = [
            vec![OsString::from("clone"), OsString::from("canonical")],
            vec![
                OsString::from("-C"),
                OsString::from("repo"),
                OsString::from("fetch"),
            ],
            vec![
                OsString::from("-C"),
                OsString::from("repo"),
                OsString::from("ls-remote"),
            ],
            vec![
                OsString::from("-C"),
                OsString::from("repo"),
                OsString::from("push"),
            ],
        ];
        for original in operations {
            let mut calls = Vec::new();
            let mut prompt = TestPrompt::new(None);
            checked_network_output_with(
                original.clone(),
                "git@github.com:owner/private.git",
                |arguments| {
                    calls.push(arguments.to_vec());
                    if calls.len() == 1 {
                        Ok(command_output(
                            false,
                            "git@github.com: Permission denied (publickey).",
                        ))
                    } else {
                        Ok(command_output(true, ""))
                    }
                },
                || Ok(vec![identity("github-kamaji", "0xkamaji")]),
                false,
                &mut prompt,
            )
            .unwrap();

            assert_eq!(calls[0], original);
            assert_eq!(calls[1][0], OsString::from("-c"));
            assert_eq!(
                calls[1][1],
                OsString::from(
                    "url.git@github-kamaji:owner/private.git.insteadOf=git@github.com:owner/private.git"
                )
            );
            assert_eq!(&calls[1][2..], original.as_slice());
        }
    }

    #[cfg(unix)]
    #[test]
    fn missing_rot_preserves_the_git_authentication_error_and_adds_guidance() {
        let mut prompt = TestPrompt::new(None);
        let error = checked_network_output_with(
            vec![OsString::from("clone")],
            "git@github.com:owner/private.git",
            |_| {
                Ok(command_output(
                    false,
                    "git@github.com: Permission denied (publickey).",
                ))
            },
            || bail!("Rot is not installed in PATH. Configure GitHub SSH normally or install Rot."),
            false,
            &mut prompt,
        )
        .unwrap_err();

        let message = error.to_string();
        assert!(message.contains("Permission denied (publickey)"));
        assert!(message.contains("Rot is not installed in PATH"));
    }

    #[cfg(unix)]
    #[test]
    fn fallback_is_limited_to_explicit_public_key_authentication_failures() {
        assert!(is_public_key_auth_failure(&command_output(
            false,
            "git@github.com: Permission denied (publickey)."
        )));
        assert!(!is_public_key_auth_failure(&command_output(
            false,
            "ERROR: Repository not found."
        )));
        assert!(!is_public_key_auth_failure(&command_output(
            false,
            "Could not resolve host: github.com"
        )));
    }

    #[test]
    fn canonical_fetch_and_push_urls_are_read_without_instead_of_expansion() {
        if raw_output([OsStr::new("--version")]).is_err() {
            return;
        }
        let temporary = tempfile::tempdir().unwrap();
        let repository = temporary.path();
        checked_output([
            OsStr::new("init"),
            OsStr::new("--quiet"),
            repository.as_os_str(),
        ])
        .unwrap();
        for (key, value) in [
            ("remote.origin.url", "git@github.com:owner/repo.git"),
            (
                "remote.origin.pushurl",
                "git@github.com:owner/repo-write.git",
            ),
            ("url.git@existing:.insteadOf", "git@github.com:"),
        ] {
            checked_output([
                OsStr::new("-C"),
                repository.as_os_str(),
                OsStr::new("config"),
                OsStr::new(key),
                OsStr::new(value),
            ])
            .unwrap();
        }

        assert_eq!(
            configured_remote_url(repository, false).unwrap().as_deref(),
            Some("git@github.com:owner/repo.git")
        );
        assert_eq!(
            configured_remote_url(repository, true).unwrap().as_deref(),
            Some("git@github.com:owner/repo-write.git")
        );
        assert_eq!(
            origin_url(repository).unwrap().as_deref(),
            Some("git@github.com:owner/repo.git")
        );
    }
}
