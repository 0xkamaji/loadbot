use crate::catalog::ResolvedTool;
use crate::interaction::OperationContext;
use crate::{
    catalog::Runner,
    operations,
    paths::{self, Paths},
    recipe::StoredInvocation,
    shortcuts::{self, Shortcut},
};
use anyhow::{Context, Result, bail};
use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Output};
use std::time::Duration;
#[derive(Debug)]
pub struct ChildExit {
    code: i32,
    target: PathBuf,
}

impl ChildExit {
    pub fn code(&self) -> i32 {
        self.code
    }
}

impl fmt::Display for ChildExit {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{} exited with status {}",
            self.target.display(),
            self.code
        )
    }
}

impl std::error::Error for ChildExit {}

pub fn run_shortcut(paths: &Paths, name: &str, context: &mut OperationContext<'_>) -> Result<()> {
    run_shortcut_from(paths, &paths.shortcuts()?, name, context)
}

pub fn run_shortcut_from(
    paths: &Paths,
    shortcut_path: &Path,
    name: &str,
    context: &mut OperationContext<'_>,
) -> Result<()> {
    paths::validate_name(name).context("invalid shortcut name")?;
    let shortcut_file = shortcuts::load(shortcut_path)?;
    let shortcut = shortcut_file
        .shortcuts
        .get(name)
        .with_context(|| format!("shortcut '{name}' does not exist"))?;
    let legacy = shortcut
        .legacy()
        .context("structured Recipe execution is not implemented")?;
    let root = paths.tool(&shortcut.catalog, &shortcut.tool)?;
    let _repository_lease = context.lease(&root)?;
    let target = resolve_target(paths, shortcut, &legacy.path, context)
        .with_context(|| broken_message(name, shortcut))?;
    match legacy.runner {
        Some(runner) => {
            let root =
                operations::installed_tool_path(paths, &shortcut.tool, &shortcut.catalog, context)?;
            launch_with_runner_in(&target, &root, runner, context)
        }
        None => launch_file_in(&target, context),
    }
}

fn resolve_target(
    paths: &Paths,
    shortcut: &Shortcut,
    path: &str,
    context: &mut OperationContext<'_>,
) -> Result<PathBuf> {
    let root = operations::installed_tool_path(paths, &shortcut.tool, &shortcut.catalog, context)?;
    let relative = shortcuts::relative_path(path)?;
    safe_target(&root, &relative)
}

pub fn safe_target(root: &Path, relative: &Path) -> Result<PathBuf> {
    let root = fs::canonicalize(root)
        .with_context(|| format!("could not resolve tool directory {}", root.display()))?;
    let target = root.join(relative);
    if !target.is_file() {
        bail!("referenced path is missing or is not a file");
    }
    let target = fs::canonicalize(&target)
        .with_context(|| format!("could not resolve selected file {}", target.display()))?;
    if !target.starts_with(&root) {
        bail!("referenced path escapes its tool repository");
    }
    Ok(target)
}

pub fn launch_file(target: &Path) -> Result<()> {
    launch_file_in(
        target,
        &mut OperationContext::new(&mut crate::interaction::Unattended),
    )
}

pub fn launch_file_in(target: &Path, context: &mut OperationContext<'_>) -> Result<()> {
    context.process.cancellation.check()?;
    if is_native_executable(target)? {
        return run_command(Command::new(target), target, context);
    }

    let extension = target
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let interpreters: &[&str] = match extension.as_str() {
        "py" if cfg!(windows) => &["python", "python3"],
        "py" => &["python3", "python"],
        "sh" => &["sh"],
        "ps1" => &["pwsh", "powershell"],
        _ => {
            bail!(
                "Loadbot located {}, but it is not executable and has no supported script extension",
                target.display()
            )
        }
    };
    for interpreter in interpreters {
        let mut command = Command::new(interpreter);
        let working_directory = target.parent().unwrap_or_else(|| Path::new("."));
        script_argument(&mut command, target, working_directory, interpreter)?;
        match run_command(command, target, context) {
            Err(error)
                if error
                    .downcast_ref::<std::io::Error>()
                    .is_some_and(|error| error.kind() == std::io::ErrorKind::NotFound) => {}
            result => return result,
        }
    }
    bail!(
        "Loadbot located {}, but no supported interpreter was found in PATH",
        target.display()
    )
}

pub fn launch_with_runner(target: &Path, working_directory: &Path, runner: Runner) -> Result<()> {
    launch_with_runner_in(
        target,
        working_directory,
        runner,
        &mut OperationContext::new(&mut crate::interaction::Unattended),
    )
}

pub fn launch_with_runner_in(
    target: &Path,
    working_directory: &Path,
    runner: Runner,
    context: &mut OperationContext<'_>,
) -> Result<()> {
    context.process.cancellation.check()?;
    if runner == Runner::Direct {
        return run_command_in(Command::new(target), target, working_directory, context);
    }
    let executables = runner.executable_candidates();
    for executable in executables {
        let mut command = Command::new(executable);
        script_argument(&mut command, target, working_directory, executable)?;
        match run_command_in(command, target, working_directory, context) {
            Err(error)
                if error
                    .downcast_ref::<std::io::Error>()
                    .is_some_and(|error| error.kind() == std::io::ErrorKind::NotFound) => {}
            result => return result,
        }
    }
    bail!(
        "runner '{}' is not available in PATH for {}",
        runner.as_str(),
        target.display()
    )
}

fn script_argument(
    command: &mut Command,
    target: &Path,
    working_directory: &Path,
    interpreter: &str,
) -> Result<()> {
    #[cfg(windows)]
    if matches!(interpreter, "sh" | "bash") {
        // Rust searches an explicitly supplied child PATH before System32.
        // Merely inheriting PATH lets the WSL bash.exe win over Git Bash even
        // when Git is first on PATH. Preserve its value, but honor that order.
        if let Some(path) = std::env::var_os("PATH") {
            command.env("PATH", path);
        }
        // Windows canonical paths use the verbatim namespace, which POSIX
        // shells cannot open. Use a relative POSIX path from the existing child
        // cwd instead of guessing a Git/MSYS drive or WSL mount mapping.
        // Canonicalize both sides before deriving it; never undo safe_target's
        // repository containment validation or change the child's cwd.
        let directory = fs::canonicalize(working_directory)?;
        let target = fs::canonicalize(target)?;
        let relative = target
            .strip_prefix(&directory)
            .context("shell script must be within its working directory")?;
        let mut argument = std::ffi::OsString::from(".");
        for component in relative.components() {
            argument.push("/");
            argument.push(component.as_os_str());
        }
        command.arg(argument);
        return Ok(());
    }
    #[cfg(not(windows))]
    let _ = (working_directory, interpreter);
    command.arg(target);
    Ok(())
}

const HELP_CAPTURE_LIMIT: usize = 256 * 1024;
const HELP_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HelpResult {
    pub command_attempted: Vec<String>,
    pub stdout: String,
    pub stderr: String,
    pub exit_status: Option<i32>,
    pub detected_help_flag: Option<String>,
}

/// Probe one validated project target without a shell or command-string parsing.
/// Non-zero exits still count when the program returned useful help text.
pub fn view_help(
    target: &Path,
    working_directory: &Path,
    runner: Runner,
    context: &mut OperationContext<'_>,
) -> Result<HelpResult> {
    view_help_with_timeout(target, working_directory, runner, HELP_TIMEOUT, context)
}

fn view_help_with_timeout(
    target: &Path,
    working_directory: &Path,
    runner: Runner,
    timeout: Duration,
    context: &mut OperationContext<'_>,
) -> Result<HelpResult> {
    context.process.cancellation.check()?;
    let mut last = None;
    for flag in ["--help", "-h"] {
        let (output, command_attempted) =
            run_help_attempt(target, working_directory, runner, flag, timeout, context)?;
        let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
        let useful = !stdout.trim().is_empty() || !stderr.trim().is_empty();
        let result = HelpResult {
            command_attempted,
            stdout,
            stderr,
            exit_status: output.status.code(),
            detected_help_flag: useful.then(|| flag.to_owned()),
        };
        if useful {
            return Ok(result);
        }
        last = Some(result);
    }
    Ok(last.expect("the fixed help flag list is non-empty"))
}

fn run_help_attempt(
    target: &Path,
    working_directory: &Path,
    runner: Runner,
    flag: &str,
    timeout: Duration,
    context: &mut OperationContext<'_>,
) -> Result<(Output, Vec<String>)> {
    if runner == Runner::Direct {
        let mut command = Command::new(target);
        command.arg(flag).current_dir(working_directory);
        let attempted = rendered_command(&command);
        let output = crate::process::execute_with_timeout(
            &mut command,
            crate::process::Mode::Capture {
                limit: HELP_CAPTURE_LIMIT,
            },
            &context.process,
            timeout,
        )
        .with_context(|| format!("could not inspect help for {}", target.display()))?;
        return Ok((output, attempted));
    }

    for executable in runner.executable_candidates() {
        let mut command = Command::new(executable);
        script_argument(&mut command, target, working_directory, executable)?;
        command.arg(flag).current_dir(working_directory);
        let attempted = rendered_command(&command);
        match crate::process::execute_with_timeout(
            &mut command,
            crate::process::Mode::Capture {
                limit: HELP_CAPTURE_LIMIT,
            },
            &context.process,
            timeout,
        ) {
            Err(error)
                if error
                    .downcast_ref::<std::io::Error>()
                    .is_some_and(|error| error.kind() == std::io::ErrorKind::NotFound) => {}
            Ok(output) => return Ok((output, attempted)),
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("could not inspect help for {}", target.display()));
            }
        }
    }
    bail!(
        "runner '{}' is not available in PATH for {}",
        runner.as_str(),
        target.display()
    )
}

fn rendered_command(command: &Command) -> Vec<String> {
    std::iter::once(command.get_program())
        .chain(command.get_args())
        .map(|value| value.to_string_lossy().into_owned())
        .collect()
}

#[cfg(all(test, windows))]
mod shell_path_tests {
    use super::*;

    #[test]
    fn only_posix_shells_receive_relative_arguments() {
        let temporary = tempfile::TempDir::new().unwrap();
        let root = temporary.path().join("tool with spaces");
        let relative = Path::new("scripts with spaces").join("run.sh");
        fs::create_dir_all(root.join("scripts with spaces")).unwrap();
        fs::write(root.join(&relative), "exit 0\n").unwrap();
        let target = safe_target(&root, &relative).unwrap();
        assert!(matches!(
            target.components().next(),
            Some(std::path::Component::Prefix(prefix)) if prefix.kind().is_verbatim()
        ));

        for interpreter in ["sh", "bash", "python", "pwsh", "powershell"] {
            let mut command = Command::new(interpreter);
            script_argument(&mut command, &target, &root, interpreter).unwrap();
            let expected = if matches!(interpreter, "sh" | "bash") {
                std::ffi::OsStr::new("./scripts with spaces/run.sh")
            } else {
                target.as_os_str()
            };
            assert_eq!(command.get_args().collect::<Vec<_>>(), [expected]);
        }
    }
}

fn run_command(command: Command, target: &Path, context: &mut OperationContext<'_>) -> Result<()> {
    let working_directory = target.parent().unwrap_or_else(|| Path::new("."));
    run_command_in(command, target, working_directory, context)
}

fn run_command_in(
    mut command: Command,
    target: &Path,
    working_directory: &Path,
    context: &mut OperationContext<'_>,
) -> Result<()> {
    command.current_dir(working_directory);
    let output = crate::process::execute(&mut command, context.tool_mode, &context.process)
        .with_context(|| {
            format!(
                "could not launch {}; the required executable may not be available",
                target.display()
            )
        })?;
    successful_status(output.status, target)
}

fn successful_status(status: ExitStatus, target: &Path) -> Result<()> {
    if !status.success() {
        let Some(code) = status.code() else {
            bail!("{} was terminated without an exit code", target.display());
        };
        return Err(ChildExit {
            code,
            target: target.to_owned(),
        }
        .into());
    }
    Ok(())
}

#[cfg(unix)]
fn is_native_executable(path: &Path) -> Result<bool> {
    use std::os::unix::fs::PermissionsExt;

    Ok(fs::metadata(path)?.permissions().mode() & 0o111 != 0)
}

#[cfg(windows)]
fn is_native_executable(path: &Path) -> Result<bool> {
    Ok(path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            matches!(
                extension.to_ascii_lowercase().as_str(),
                "exe" | "com" | "bat" | "cmd"
            )
        }))
}

fn broken_message(name: &str, shortcut: &Shortcut) -> String {
    let invocation = match &shortcut.invocation {
        StoredInvocation::Legacy(legacy) => format!("path: {}", legacy.path),
        StoredInvocation::Recipe(_) => "invocation: recipe".to_owned(),
    };
    format!(
        "shortcut '{name}' is broken:\n\ncatalog: {}\ntool: {}\n{}",
        shortcut.catalog, shortcut.tool, invocation
    )
}

/// Launch a catalog or personal command after validating its managed tool and path.
pub fn launch_command(
    paths: &Paths,
    catalog: &str,
    tool: &str,
    path: &str,
    runner: Option<Runner>,
    source: EntrySource,
    context: &mut OperationContext<'_>,
) -> Result<()> {
    let _repository_lease = context.lease(&paths.tool(catalog, tool)?)?;
    let root = operations::installed_tool_path(paths, tool, catalog, context)?;
    let relative = shortcuts::relative_path(path)?;
    let target = safe_target(&root, &relative)?;
    match runner {
        Some(runner) => launch_with_runner_in(&target, &root, runner, context),
        None if source == EntrySource::Catalog => {
            launch_with_runner_in(&target, &root, Runner::Direct, context)
        }
        None => launch_file_in(&target, context),
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct ProjectKey {
    tool: String,
    catalog: String,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Project {
    pub tool: String,
    pub catalog: String,
    pub installed: bool,
    pub entries: Vec<ProjectEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectEntry {
    pub name: String,
    pub description: Option<String>,
    pub invocation: StoredInvocation,
    pub source: EntrySource,
}

impl serde::Serialize for ProjectEntry {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(serde::Serialize)]
        struct Wire<'a> {
            name: &'a str,
            #[serde(skip_serializing_if = "Option::is_none")]
            path: Option<&'a str>,
            #[serde(skip_serializing_if = "Option::is_none")]
            description: Option<&'a str>,
            #[serde(skip_serializing_if = "Option::is_none")]
            runner: Option<Runner>,
            #[serde(skip_serializing_if = "Option::is_none")]
            recipe: Option<&'a crate::recipe::RecipeDefinition>,
            source: EntrySource,
        }
        let legacy = self.invocation.as_legacy();
        Wire {
            name: &self.name,
            path: legacy.map(|legacy| legacy.path.as_str()),
            description: self.description.as_deref(),
            runner: legacy.and_then(|legacy| legacy.runner),
            recipe: self.invocation.as_recipe(),
            source: self.source,
        }
        .serialize(serializer)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum EntrySource {
    Catalog,
    Personal,
}

/// Read the existing launcher inventory without writes, synchronization, or launch checks.
///
/// Unlike the CLI's warning-plus-partial inventory, this complete-snapshot query fails
/// if `all_tools` skips any catalog. The original typed notices remain in `context`.
/// Missing optional configuration/shortcut files retain their normal empty semantics.
pub fn read_project_inventory(
    paths: &Paths,
    context: &mut OperationContext<'_>,
) -> Result<Vec<Project>> {
    let notice_start = context.notices.len();
    let tools = operations::tool_list(paths, context)?;
    let skipped: Vec<_> = context.notices[notice_start..]
        .iter()
        .filter_map(|notice| match notice {
            crate::interaction::Notice::SkippedCatalog { name, diagnostic } => {
                Some(format!("catalog '{name}': {diagnostic}"))
            }
            _ => None,
        })
        .collect();
    if !skipped.is_empty() {
        bail!(
            "could not read complete Loadbot inventory: {}",
            skipped.join("; ")
        );
    }
    let shortcut_file = shortcuts::load(&paths.shortcuts()?)?;
    Ok(project_inventory_with_status(&tools, &shortcut_file))
}

/// Resolve an installed project's real directory from its qualified Loadbot identity.
///
/// The caller decides how to present the directory. This function performs no OS open,
/// process launch, catalog synchronization, or mutation.
pub fn resolve_project_directory(
    paths: &Paths,
    catalog: &str,
    tool: &str,
    context: &mut OperationContext<'_>,
) -> Result<PathBuf> {
    let directory = operations::installed_tool_path(paths, tool, catalog, context)?;
    let directory = fs::canonicalize(&directory).with_context(|| {
        format!(
            "could not resolve project directory {}",
            directory.display()
        )
    })?;
    if !directory.is_dir() {
        bail!("resolved project path is not a directory");
    }
    Ok(directory)
}

pub fn project_inventory(
    tools: &[ResolvedTool],
    shortcut_file: &shortcuts::ShortcutFile,
) -> Vec<Project> {
    let mut projects = BTreeMap::<ProjectKey, Vec<ProjectEntry>>::new();
    for tool in tools {
        let key = ProjectKey {
            tool: tool.name.clone(),
            catalog: tool.catalog.clone(),
        };
        // Configured projects remain navigable before they gain their first command
        // or personal shortcut. This keeps the inventory authoritative after add.
        projects.entry(key.clone()).or_default();
        for (name, command) in &tool.definition.commands {
            projects.entry(key.clone()).or_default().push(ProjectEntry {
                name: name.clone(),
                description: command.description.clone(),
                invocation: command.invocation.clone(),
                source: EntrySource::Catalog,
            });
        }
    }
    for (name, shortcut) in &shortcut_file.shortcuts {
        projects
            .entry(ProjectKey {
                tool: shortcut.tool.clone(),
                catalog: shortcut.catalog.clone(),
            })
            .or_default()
            .push(ProjectEntry {
                name: name.clone(),
                description: shortcut.description.clone(),
                invocation: shortcut.invocation.clone(),
                source: EntrySource::Personal,
            });
    }

    projects
        .into_iter()
        .map(|(key, mut entries)| {
            entries.sort_by(|left, right| {
                left.name
                    .cmp(&right.name)
                    .then_with(|| left.source.cmp(&right.source))
            });
            Project {
                tool: key.tool,
                catalog: key.catalog,
                installed: false,
                entries,
            }
        })
        .collect()
}

fn project_inventory_with_status(
    tools: &[operations::ToolSummary],
    shortcut_file: &shortcuts::ShortcutFile,
) -> Vec<Project> {
    let installed = tools
        .iter()
        .map(|summary| {
            (
                (summary.tool.catalog.clone(), summary.tool.name.clone()),
                summary.installed,
            )
        })
        .collect::<BTreeMap<_, _>>();
    let resolved = tools
        .iter()
        .map(|summary| summary.tool.clone())
        .collect::<Vec<_>>();
    project_inventory(&resolved, shortcut_file)
        .into_iter()
        .map(|project| Project {
            installed: installed
                .get(&(project.catalog.clone(), project.tool.clone()))
                .copied()
                .unwrap_or(false),
            ..project
        })
        .collect()
}

#[derive(Debug)]
pub struct BrowserEntry {
    pub name: String,
    pub path: PathBuf,
    pub is_directory: bool,
}

/// Enumerate only the selected directory; symlinks and special files are omitted.
pub fn browse_directory(root: &Path, relative: &Path) -> Result<Vec<BrowserEntry>> {
    if !relative.as_os_str().is_empty() {
        shortcuts::portable_path(relative)?;
    }
    // Resolve the root once, then reject symlinks at every selected component.
    // Filtering the returned entries alone does not protect direct API callers.
    let canonical_root = fs::canonicalize(root)
        .with_context(|| format!("could not resolve tool directory {}", root.display()))?;
    if fs::symlink_metadata(root)?.file_type().is_symlink() {
        bail!("refusing to browse a symlink tool directory");
    }
    let mut selected = canonical_root.clone();
    for component in relative.components() {
        selected.push(component);
        let metadata = fs::symlink_metadata(&selected)
            .with_context(|| format!("could not inspect directory {}", selected.display()))?;
        if metadata.file_type().is_symlink() {
            bail!("refusing to browse a symlink directory");
        }
        if !metadata.is_dir() {
            bail!("selected path is not a directory");
        }
    }
    let canonical_directory = fs::canonicalize(&selected)?;
    if !canonical_directory.starts_with(&canonical_root) {
        bail!("selected directory escapes its tool repository");
    }
    let directory = root.join(relative);
    let mut entries = Vec::new();
    for entry in fs::read_dir(&directory)
        .with_context(|| format!("could not browse {}", directory.display()))?
    {
        let entry = entry?;
        let file_type = entry.file_type()?;
        if file_type.is_symlink() || (!file_type.is_dir() && !file_type.is_file()) {
            continue;
        }
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        entries.push(BrowserEntry {
            name,
            path: entry.path(),
            is_directory: file_type.is_dir(),
        });
    }
    entries.sort_by(|left, right| {
        right
            .is_directory
            .cmp(&left.is_directory)
            .then_with(|| left.name.cmp(&right.name))
    });
    Ok(entries)
}
