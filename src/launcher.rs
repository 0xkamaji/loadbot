use crate::catalog::ResolvedTool;
use crate::interaction::OperationContext;
use crate::{
    catalog::Runner,
    operations,
    paths::{self, Paths},
    shortcuts::{self, Shortcut},
};
use anyhow::{Context, Result, bail};
use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
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
    let target =
        resolve_target(paths, shortcut, context).with_context(|| broken_message(name, shortcut))?;
    match shortcut.runner {
        Some(runner) => {
            let root =
                operations::installed_tool_path(paths, &shortcut.tool, &shortcut.catalog, context)?;
            launch_with_runner(&target, &root, runner)
        }
        None => launch_file(&target),
    }
}

fn resolve_target(
    paths: &Paths,
    shortcut: &Shortcut,
    context: &mut OperationContext<'_>,
) -> Result<PathBuf> {
    let root = operations::installed_tool_path(paths, &shortcut.tool, &shortcut.catalog, context)?;
    let relative = shortcuts::relative_path(&shortcut.path)?;
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
    if is_native_executable(target)? {
        return run_command(Command::new(target), target);
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
        command.arg(target);
        match run_command(command, target) {
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
    if runner == Runner::Direct {
        return run_command_in(Command::new(target), target, working_directory);
    }
    let executables: &[&str] = match runner {
        Runner::Direct => unreachable!(),
        Runner::Bash => &["bash"],
        Runner::Sh => &["sh"],
        Runner::Python if cfg!(windows) => &["python", "python3"],
        Runner::Python => &["python3", "python"],
        Runner::Powershell if cfg!(windows) => &["powershell", "pwsh"],
        Runner::Powershell => &["pwsh", "powershell"],
    };
    for executable in executables {
        let mut command = Command::new(executable);
        command.arg(target);
        match run_command_in(command, target, working_directory) {
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

fn run_command(command: Command, target: &Path) -> Result<()> {
    let working_directory = target.parent().unwrap_or_else(|| Path::new("."));
    run_command_in(command, target, working_directory)
}

fn run_command_in(mut command: Command, target: &Path, working_directory: &Path) -> Result<()> {
    command
        .current_dir(working_directory)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    let status = command.status().with_context(|| {
        format!(
            "could not launch {}; the required executable may not be available",
            target.display()
        )
    })?;
    successful_status(status, target)
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
    format!(
        "shortcut '{name}' is broken:\n\ncatalog: {}\ntool: {}\npath: {}",
        shortcut.catalog, shortcut.tool, shortcut.path
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
    let root = operations::installed_tool_path(paths, tool, catalog, context)?;
    let relative = shortcuts::relative_path(path)?;
    let target = safe_target(&root, &relative)?;
    match runner {
        Some(runner) => launch_with_runner(&target, &root, runner),
        None if source == EntrySource::Catalog => {
            launch_with_runner(&target, &root, Runner::Direct)
        }
        None => launch_file(&target),
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct ProjectKey {
    tool: String,
    catalog: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Project {
    pub tool: String,
    pub catalog: String,
    pub entries: Vec<ProjectEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectEntry {
    pub name: String,
    pub path: String,
    pub description: Option<String>,
    pub runner: Option<Runner>,
    pub source: EntrySource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum EntrySource {
    Catalog,
    Personal,
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
        for (name, command) in &tool.definition.commands {
            projects.entry(key.clone()).or_default().push(ProjectEntry {
                name: name.clone(),
                path: command.path.clone(),
                description: command.description.clone(),
                runner: command.runner,
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
                path: shortcut.path.clone(),
                description: shortcut.description.clone(),
                runner: shortcut.runner,
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
                entries,
            }
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
