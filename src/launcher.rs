use std::collections::BTreeSet;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus};

use anyhow::{Context, Result, bail};

use crate::interactive::Prompt;
use crate::operations;
use crate::paths::{self, Paths};
use crate::shortcuts::{self, Shortcut};

const BROWSE_TOOLS: &str = "Browse installed tools...";
const EXIT: &str = "Exit";

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

pub fn run_shortcut(paths: &Paths, name: &str) -> Result<()> {
    run_shortcut_from(paths, &paths.shortcuts()?, name)
}

fn run_shortcut_from(paths: &Paths, shortcut_path: &Path, name: &str) -> Result<()> {
    paths::validate_name(name).context("invalid shortcut name")?;
    let shortcut_file = shortcuts::load(shortcut_path)?;
    let shortcut = shortcut_file
        .shortcuts
        .get(name)
        .with_context(|| format!("shortcut '{name}' does not exist"))?;
    let target = resolve_target(paths, shortcut).with_context(|| broken_message(name, shortcut))?;
    launch_file(&target)
}

pub fn run_interactive<P: Prompt>(paths: &Paths, prompt: &mut P) -> Result<()> {
    run_interactive_from(paths, prompt, &paths.shortcuts()?)
}

fn run_interactive_from<P: Prompt>(
    paths: &Paths,
    prompt: &mut P,
    shortcut_path: &Path,
) -> Result<()> {
    let names = shortcuts::shortcut_names(shortcut_path)?;
    match select_initial_action(prompt, &names)? {
        InitialAction::Shortcut(name) => {
            return run_shortcut_from(paths, shortcut_path, &name);
        }
        InitialAction::Browse => {}
        InitialAction::Exit => return Ok(()),
        InitialAction::Cancelled => return cancelled(),
    }
    browse_installed_tools(paths, prompt)
}

fn browse_installed_tools<P: Prompt>(paths: &Paths, prompt: &mut P) -> Result<()> {
    let tools = operations::installed_tools(paths)?;
    if tools.is_empty() {
        bail!("no installed tools are available; run 'loadbot pull TOOL' first");
    }
    let catalogs: Vec<_> = tools
        .iter()
        .map(|tool| tool.catalog.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();

    loop {
        let Some(catalog) = prompt.select("Select catalog:", &catalogs)? else {
            return cancelled();
        };
        let mut tool_choices = vec!["../ Back".to_owned()];
        tool_choices.extend(
            tools
                .iter()
                .filter(|tool| tool.catalog == catalog)
                .map(|tool| tool.name.clone()),
        );
        let Some(tool) = prompt.select("Select tool:", &tool_choices)? else {
            return cancelled();
        };
        if tool == "../ Back" {
            continue;
        }

        let root = operations::installed_tool_path(paths, &tool, &catalog)?;
        let Some(relative) = browse(prompt, &tool, &root)? else {
            continue;
        };
        let target = safe_target(&root, &relative)?;
        prompt.message(&format!(
            "Selected:\n{}",
            shortcuts::portable_path(&relative)?
        ))?;
        launch_file(&target)?;
        if prompt.confirm("Save as a Loadbot shortcut?", false)? == Some(true) {
            save_shortcut(prompt, paths, &catalog, &tool, &relative)?;
        }
        return Ok(());
    }
}

fn select_initial_action<P: Prompt>(prompt: &mut P, names: &[String]) -> Result<InitialAction> {
    let mut choices = names.to_vec();
    choices.push(BROWSE_TOOLS.to_owned());
    choices.push(EXIT.to_owned());
    let Some(selection) = prompt.select("Select:", &choices)? else {
        return Ok(InitialAction::Cancelled);
    };
    match selection.as_str() {
        BROWSE_TOOLS => Ok(InitialAction::Browse),
        EXIT => Ok(InitialAction::Exit),
        _ => Ok(InitialAction::Shortcut(selection)),
    }
}

fn browse<P: Prompt>(prompt: &mut P, tool: &str, root: &Path) -> Result<Option<PathBuf>> {
    let mut relative = PathBuf::new();
    loop {
        let directory = root.join(&relative);
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
                label: if file_type.is_dir() {
                    format!("{name}/")
                } else {
                    name
                },
                path: entry.path(),
                is_directory: file_type.is_dir(),
            });
        }
        entries.sort_by(|left, right| {
            right
                .is_directory
                .cmp(&left.is_directory)
                .then_with(|| left.label.cmp(&right.label))
        });

        let back = if relative.as_os_str().is_empty() {
            "../ Back to tools"
        } else {
            "../"
        };
        let mut choices = vec![back.to_owned()];
        choices.extend(entries.iter().map(|entry| entry.label.clone()));
        let label = if relative.as_os_str().is_empty() {
            format!("Browse {tool}:")
        } else {
            format!("Browse {}:", shortcuts::portable_path(&relative)?)
        };
        let Some(selection) = prompt.select(&label, &choices)? else {
            return cancelled().map(|()| None);
        };
        if selection == back {
            if relative.pop() {
                continue;
            }
            return Ok(None);
        }
        let selected = entries
            .iter()
            .find(|entry| entry.label == selection)
            .context("invalid file browser selection")?;
        let selected_relative = selected
            .path
            .strip_prefix(root)
            .context("selected file escaped its tool repository")?
            .to_owned();
        if selected.is_directory {
            relative = selected_relative;
        } else {
            return Ok(Some(selected_relative));
        }
    }
}

fn save_shortcut<P: Prompt>(
    prompt: &mut P,
    paths: &Paths,
    catalog: &str,
    tool: &str,
    relative: &Path,
) -> Result<()> {
    let default = relative
        .file_stem()
        .and_then(|name| name.to_str())
        .filter(|name| paths::validate_name(name).is_ok());
    let label = match default {
        Some(default) => format!("Shortcut name [{default}]:"),
        None => "Shortcut name:".to_owned(),
    };
    let Some(name) = prompt.input(&label, default)? else {
        return cancelled();
    };
    paths::validate_name(&name).context("invalid shortcut name")?;
    let shortcut = Shortcut::new(
        catalog.to_owned(),
        tool.to_owned(),
        shortcuts::portable_path(relative)?,
    )?;
    shortcuts::save(&paths.shortcuts()?, &name, shortcut)?;
    prompt.message(&format!("saved shortcut '{name}'"))
}

fn resolve_target(paths: &Paths, shortcut: &Shortcut) -> Result<PathBuf> {
    let root = operations::installed_tool_path(paths, &shortcut.tool, &shortcut.catalog)?;
    let relative = shortcuts::relative_path(&shortcut.path)?;
    safe_target(&root, &relative)
}

fn safe_target(root: &Path, relative: &Path) -> Result<PathBuf> {
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

fn launch_file(target: &Path) -> Result<()> {
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

fn run_command(mut command: Command, target: &Path) -> Result<()> {
    if let Some(parent) = target.parent() {
        command.current_dir(parent);
    }
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

fn cancelled() -> Result<()> {
    eprintln!("cancelled");
    Ok(())
}

struct BrowserEntry {
    label: String,
    path: PathBuf,
    is_directory: bool,
}

#[derive(Debug, PartialEq, Eq)]
enum InitialAction {
    Shortcut(String),
    Browse,
    Exit,
    Cancelled,
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;

    use super::*;

    #[derive(Default)]
    struct FakePrompt {
        selections: VecDeque<Option<String>>,
        displayed: Vec<Vec<String>>,
    }

    impl Prompt for FakePrompt {
        fn input(&mut self, _label: &str, _default: Option<&str>) -> Result<Option<String>> {
            unreachable!()
        }

        fn confirm(&mut self, _label: &str, _default: bool) -> Result<Option<bool>> {
            unreachable!()
        }

        fn select(&mut self, _label: &str, choices: &[String]) -> Result<Option<String>> {
            self.displayed.push(choices.to_vec());
            Ok(self.selections.pop_front().flatten())
        }

        fn message(&mut self, _message: &str) -> Result<()> {
            Ok(())
        }
    }

    #[test]
    fn browser_reads_one_directory_at_a_time_and_can_navigate_up() {
        let temporary = tempfile::TempDir::new().unwrap();
        let root = temporary.path();
        fs::create_dir(root.join("recipes")).unwrap();
        fs::write(root.join("recipes/run.sh"), "exit 0\n").unwrap();
        fs::write(root.join("README.md"), "demo\n").unwrap();
        let mut prompt = FakePrompt {
            selections: [
                Some("recipes/".to_owned()),
                Some("../".to_owned()),
                Some("recipes/".to_owned()),
                Some("run.sh".to_owned()),
            ]
            .into(),
            ..FakePrompt::default()
        };

        let selected = browse(&mut prompt, "demo", root).unwrap().unwrap();
        assert_eq!(selected, PathBuf::from("recipes/run.sh"));
        assert!(!prompt.displayed[0].contains(&"run.sh".to_owned()));
        assert!(prompt.displayed[1].contains(&"run.sh".to_owned()));
        assert_eq!(prompt.displayed[2], prompt.displayed[0]);
    }

    #[test]
    fn initial_menu_shows_shortcuts_and_keeps_browser_reachable() {
        let names = vec!["bn-triage".to_owned(), "print-strings".to_owned()];
        let mut shortcut_prompt = FakePrompt {
            selections: [Some("print-strings".to_owned())].into(),
            ..FakePrompt::default()
        };
        assert_eq!(
            select_initial_action(&mut shortcut_prompt, &names).unwrap(),
            InitialAction::Shortcut("print-strings".to_owned())
        );
        assert_eq!(
            shortcut_prompt.displayed[0],
            ["bn-triage", "print-strings", BROWSE_TOOLS, EXIT]
        );

        let mut browse_prompt = FakePrompt {
            selections: [Some(BROWSE_TOOLS.to_owned())].into(),
            ..FakePrompt::default()
        };
        assert_eq!(
            select_initial_action(&mut browse_prompt, &names).unwrap(),
            InitialAction::Browse
        );
    }

    #[test]
    fn zero_shortcuts_offer_browse_and_exit() {
        let mut prompt = FakePrompt {
            selections: [Some(BROWSE_TOOLS.to_owned())].into(),
            ..FakePrompt::default()
        };

        assert_eq!(
            select_initial_action(&mut prompt, &[]).unwrap(),
            InitialAction::Browse
        );
        assert_eq!(prompt.displayed[0], [BROWSE_TOOLS, EXIT]);

        let mut exit_prompt = FakePrompt {
            selections: [Some(EXIT.to_owned())].into(),
            ..FakePrompt::default()
        };
        assert_eq!(
            select_initial_action(&mut exit_prompt, &[]).unwrap(),
            InitialAction::Exit
        );
    }

    #[test]
    fn broken_shortcuts_remain_visible_and_use_normal_execution_errors() {
        let temporary = tempfile::TempDir::new().unwrap();
        let shortcut_path = temporary.path().join("shortcuts.toml");
        fs::write(
            &shortcut_path,
            r#"version = 1

[shortcuts.broken-tool]
catalog = "missing-catalog"
tool = "demo"
path = "run.sh"
"#,
        )
        .unwrap();
        let paths = Paths::with_root(temporary.path().join("loadbot-home"));
        let mut prompt = FakePrompt {
            selections: [Some("broken-tool".to_owned())].into(),
            ..FakePrompt::default()
        };

        let error = run_interactive_from(&paths, &mut prompt, &shortcut_path).unwrap_err();
        assert!(prompt.displayed[0].contains(&"broken-tool".to_owned()));
        assert!(format!("{error:#}").contains("shortcut 'broken-tool' is broken"));
        assert!(format!("{error:#}").contains("catalog 'missing-catalog' is not configured"));
    }

    #[cfg(unix)]
    #[test]
    fn target_symlinks_cannot_escape_the_tool_root() {
        use std::os::unix::fs::symlink;

        let temporary = tempfile::TempDir::new().unwrap();
        let root = temporary.path().join("tool");
        let outside = temporary.path().join("outside.sh");
        fs::create_dir(&root).unwrap();
        fs::write(&outside, "exit 0\n").unwrap();
        symlink(&outside, root.join("escape.sh")).unwrap();

        assert!(safe_target(&root, Path::new("escape.sh")).is_err());
    }
}
