use std::ffi::{OsStr, OsString};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use anyhow::{Context, Result, bail};
use serde::Deserialize;

use crate::interactive::{Prompt, TerminalPrompt, terminal_is_interactive};

const ROT_IDENTITY_VERSION: u32 = 1;

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
struct RotIdentity {
    alias: String,
    username: Option<String>,
    verification: String,
}

#[derive(Debug, Deserialize)]
struct RotIdentityDocument {
    version: u32,
    identities: Vec<RotIdentity>,
}

#[derive(Debug)]
pub struct RepositoryStatus {
    pub branch: Option<String>,
    pub commit: Option<String>,
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
    checked_network_output(arguments, url)?;
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
    })
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

pub fn origin_refs(path: &Path) -> Result<Vec<(String, String)>> {
    network_query(
        path,
        &["ls-remote", "--refs", "origin"],
        configured_remote_url(path, false)?.as_deref(),
    )?
    .lines()
    .map(|line| {
        let (commit, reference) = line
            .split_once(char::is_whitespace)
            .context("Git returned an invalid origin ref")?;
        Ok((commit.to_owned(), reference.trim().to_owned()))
    })
    .collect()
}

pub fn origin_has_refs(path: &Path) -> Result<bool> {
    Ok(!origin_refs(path)?.is_empty())
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

    network_query(
        path,
        &["fetch", "origin"],
        configured_remote_url(path, false)?.as_deref(),
    )?;
    let target = format!("origin/{branch}");
    query(path, &["merge", "--ff-only", "--", &target])?;
    let new_commit = query(path, &["rev-parse", "--short", "HEAD"])?;
    Ok((
        current
            .commit
            .context("repository has no commits and cannot be updated")?,
        new_commit,
    ))
}

pub fn commit_file(path: &Path, file: &str, message: &str) -> Result<String> {
    query(path, &["add", "--", file])?;
    query(path, &["commit", "--only", "-m", message, "--", file])?;
    query(path, &["rev-parse", "--short", "HEAD"])
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

pub fn push_origin(path: &Path) -> Result<()> {
    network_query(
        path,
        &["push", "origin", "HEAD"],
        configured_remote_url(path, true)?.as_deref(),
    )?;
    Ok(())
}

pub fn origin_url(path: &Path) -> Result<Option<String>> {
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

fn network_query(path: &Path, arguments: &[&str], known_url: Option<&str>) -> Result<String> {
    let mut command_arguments = vec![OsString::from("-C"), path.as_os_str().to_owned()];
    command_arguments.extend(arguments.iter().map(|argument| OsString::from(*argument)));
    Ok(stdout_text(&checked_network_output(
        command_arguments,
        known_url.unwrap_or(""),
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

fn checked_network_output(arguments: Vec<OsString>, canonical_url: &str) -> Result<Output> {
    let mut prompt = TerminalPrompt;
    checked_network_output_with(
        arguments,
        canonical_url,
        |arguments| raw_output(arguments),
        query_rot_identities,
        terminal_is_interactive(),
        &mut prompt,
    )
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
    P: Prompt,
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
        identities().map_err(|error| anyhow::anyhow!("{original_error}\n\n{error}"))?;
    let identity = select_rot_identity(identities, interactive, prompt)
        .map_err(|error| anyhow::anyhow!("{original_error}\n\n{error}"))?;
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

fn query_rot_identities() -> Result<Vec<RotIdentity>> {
    let output = Command::new("rot")
        .args(["ssh", "identities", "--json"])
        .output()
        .map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                anyhow::anyhow!(
                    "Rot is not installed in PATH. Configure GitHub SSH normally or install Rot."
                )
            } else {
                anyhow::anyhow!("could not query Rot-managed SSH identities: {error}")
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

fn select_rot_identity<P: Prompt>(
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

    let choices = identities
        .iter()
        .map(|identity| {
            format!(
                "{} -> {}",
                identity.alias,
                identity.username.as_deref().expect("verified username")
            )
        })
        .collect::<Vec<_>>();
    let selected = prompt
        .select("Choose a Rot-managed GitHub SSH identity:", &choices)?
        .context("SSH identity selection was cancelled")?;
    let index = choices
        .iter()
        .position(|choice| choice == &selected)
        .context("an invalid SSH identity was selected")?;
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

    impl Prompt for TestPrompt {
        fn input(&mut self, _label: &str, _default: Option<&str>) -> Result<Option<String>> {
            unreachable!()
        }

        fn confirm(&mut self, _label: &str, _default: bool) -> Result<Option<bool>> {
            unreachable!()
        }

        fn select(&mut self, _label: &str, choices: &[String]) -> Result<Option<String>> {
            self.select_calls += 1;
            self.choices = choices.to_vec();
            Ok(self.selection.clone())
        }

        fn message(&mut self, _message: &str) -> Result<()> {
            unreachable!()
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
            br#"{"version":1,"identities":[
                {"alias":"github-kamaji","username":"0xkamaji","verification":"verified"},
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

    #[cfg(unix)]
    #[test]
    fn https_auth_failure_never_queries_rot() {
        let mut prompt = TestPrompt::new(None);
        let mut identity_queries = 0;
        let error = checked_network_output_with(
            vec![OsString::from("clone")],
            "https://github.com/owner/private.git",
            |_| {
                Ok(command_output(
                    false,
                    "git@github.com: Permission denied (publickey).",
                ))
            },
            || {
                identity_queries += 1;
                Ok(Vec::new())
            },
            false,
            &mut prompt,
        )
        .unwrap_err();

        assert!(error.to_string().contains("Permission denied"));
        assert_eq!(identity_queries, 0);
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
            Some("git@existing:owner/repo.git")
        );
    }
}
