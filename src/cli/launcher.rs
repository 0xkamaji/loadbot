#[cfg(all(test, unix))]
use loadbot::launcher::launch_with_runner;
use loadbot::launcher::{EntrySource, launch_file, safe_target};
use std::collections::{BTreeMap, BTreeSet};
#[cfg(test)]
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use super::menus::Prompt;
use super::operations;
use loadbot::paths::{self, Paths};
use loadbot::shortcuts;
#[cfg(test)]
use loadbot::shortcuts::Shortcut;
use loadbot::{catalog::ResolvedTool, catalog::Runner};

const BROWSE_TOOLS: &str = "Browse installed tools...";
const BACK: &str = "../ Back";
const EXIT: &str = "Exit";

pub fn run_shortcut(paths: &Paths, name: &str) -> Result<()> {
    super::output::with_context(|context| loadbot::launcher::run_shortcut(paths, name, context))
}

pub fn run_interactive<P: Prompt>(paths: &Paths, prompt: &mut P) -> Result<()> {
    run_interactive_from(paths, prompt, &paths.shortcuts()?)
}

fn run_interactive_from<P: Prompt>(
    paths: &Paths,
    prompt: &mut P,
    shortcut_path: &Path,
) -> Result<()> {
    let projects = project_inventory(
        &operations::all_tools(paths)?,
        &shortcuts::load(shortcut_path)?,
    );
    loop {
        match select_project_action(prompt, &projects)? {
            ProjectAction::Project(index) => {
                if run_project_menu(paths, prompt, &projects[index])? {
                    return Ok(());
                }
            }
            ProjectAction::Browse => return run_selected_file(paths, prompt),
            ProjectAction::Exit => return Ok(()),
            ProjectAction::Cancelled => return cancelled(),
        }
    }
}

pub fn add_shortcut<P: Prompt>(paths: &Paths, prompt: &mut P) -> Result<()> {
    let Some(selected) = select_installed_file(paths, prompt)? else {
        return Ok(());
    };
    create_shortcut_for_paths(
        prompt,
        paths,
        &selected.catalog,
        &selected.tool,
        &selected.relative,
    )?;
    Ok(())
}

pub fn add_shortcuts_for_tool<P: Prompt>(
    paths: &Paths,
    prompt: &mut P,
    catalog: &str,
    tool: &str,
) -> Result<()> {
    let root = operations::installed_tool_path(paths, tool, catalog)?;
    loop {
        let Some(relative) = browse(prompt, &root, "Select file:")? else {
            return Ok(());
        };
        if !create_shortcut_for_paths(prompt, paths, catalog, tool, &relative)? {
            return Ok(());
        }
        if prompt.confirm("Add another shortcut?", false)? != Some(true) {
            return Ok(());
        }
    }
}

#[cfg(test)]
fn add_shortcuts_for_tool_from<P: Prompt>(
    prompt: &mut P,
    shortcut_path: &Path,
    catalog: &str,
    tool: &str,
    root: &Path,
) -> Result<()> {
    loop {
        let Some(relative) = browse(prompt, root, "Select file:")? else {
            return Ok(());
        };
        if !create_shortcut(prompt, shortcut_path, catalog, tool, &relative)? {
            return Ok(());
        }
        if prompt.confirm("Add another shortcut?", false)? != Some(true) {
            return Ok(());
        }
    }
}

fn run_selected_file<P: Prompt>(paths: &Paths, prompt: &mut P) -> Result<()> {
    let Some(selected) = select_installed_file(paths, prompt)? else {
        return Ok(());
    };
    let target = safe_target(&selected.root, &selected.relative)?;
    prompt.message(&format!(
        "Selected:\n{}",
        shortcuts::portable_path(&selected.relative)?
    ))?;
    launch_file(&target)?;
    if prompt.confirm("Save as a Loadbot shortcut?", false)? == Some(true)
        && let Some(name) = save_shortcut_for_paths(
            prompt,
            paths,
            &selected.catalog,
            &selected.tool,
            &selected.relative,
        )?
    {
        prompt.message(&format!("saved shortcut '{name}'"))?;
    }
    Ok(())
}

fn select_installed_file<P: Prompt>(paths: &Paths, prompt: &mut P) -> Result<Option<SelectedFile>> {
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
            return cancelled().map(|()| None);
        };
        let mut tool_choices = vec!["../ Back".to_owned()];
        tool_choices.extend(
            tools
                .iter()
                .filter(|tool| tool.catalog == catalog)
                .map(|tool| tool.name.clone()),
        );
        let Some(tool) = prompt.select("Select tool:", &tool_choices)? else {
            return cancelled().map(|()| None);
        };
        if tool == "../ Back" {
            continue;
        }

        let root = operations::installed_tool_path(paths, &tool, &catalog)?;
        let Some(relative) = browse(prompt, &root, &format!("Browse {tool}:"))? else {
            continue;
        };
        return Ok(Some(SelectedFile {
            catalog,
            tool,
            root,
            relative,
        }));
    }
}

fn select_project_action<P: Prompt>(prompt: &mut P, projects: &[Project]) -> Result<ProjectAction> {
    let mut choices: Vec<_> = projects
        .iter()
        .map(|project| project.label.clone())
        .collect();
    choices.push(BROWSE_TOOLS.to_owned());
    choices.push(EXIT.to_owned());
    let Some(selection) = prompt.select("Select project:", &choices)? else {
        return Ok(ProjectAction::Cancelled);
    };
    match selection.as_str() {
        BROWSE_TOOLS => Ok(ProjectAction::Browse),
        EXIT => Ok(ProjectAction::Exit),
        _ => projects
            .iter()
            .position(|project| project.label == selection)
            .map(ProjectAction::Project)
            .context("invalid project selection"),
    }
}

fn run_project_menu<P: Prompt>(paths: &Paths, prompt: &mut P, project: &Project) -> Result<bool> {
    let mut choices: Vec<_> = project
        .entries
        .iter()
        .map(|entry| entry.label.clone())
        .collect();
    choices.push(BACK.to_owned());
    choices.push(EXIT.to_owned());
    let Some(selection) = prompt.select(&format!("Run from {}:", project.label), &choices)? else {
        cancelled()?;
        return Ok(true);
    };
    if selection == BACK {
        return Ok(false);
    }
    if selection == EXIT {
        return Ok(true);
    }
    let entry = project
        .entries
        .iter()
        .find(|entry| entry.label == selection)
        .context("invalid project command selection")?;
    launch_entry(paths, project, entry).with_context(|| match entry.source {
        EntrySource::Catalog => format!(
            "shared command '{}' is broken:\n\ncatalog: {}\ntool: {}\npath: {}",
            entry.name, project.catalog, project.tool, entry.path
        ),
        EntrySource::Personal => format!(
            "shortcut '{}' is broken:\n\ncatalog: {}\ntool: {}\npath: {}",
            entry.name, project.catalog, project.tool, entry.path
        ),
    })?;
    Ok(true)
}

fn launch_entry(paths: &Paths, project: &Project, entry: &ProjectEntry) -> Result<()> {
    super::output::with_context(|context| {
        loadbot::launcher::launch_command(
            paths,
            &project.catalog,
            &project.tool,
            &entry.path,
            entry.runner,
            entry.source,
            context,
        )
    })
}

fn project_inventory(
    tools: &[ResolvedTool],
    shortcut_file: &shortcuts::ShortcutFile,
) -> Vec<Project> {
    let projects: BTreeMap<_, _> = loadbot::launcher::project_inventory(tools, shortcut_file)
        .into_iter()
        .map(|project| {
            let key = ProjectKey {
                tool: project.tool,
                catalog: project.catalog,
            };
            let entries = project
                .entries
                .into_iter()
                .map(|entry| ProjectEntry {
                    name: entry.name,
                    label: String::new(),
                    path: entry.path,
                    description: entry.description,
                    runner: entry.runner,
                    source: entry.source,
                })
                .collect::<Vec<_>>();
            (key, entries)
        })
        .collect();
    let duplicate_names: BTreeSet<_> = projects
        .keys()
        .filter(|key| {
            projects
                .keys()
                .filter(|other| other.tool == key.tool)
                .count()
                > 1
        })
        .map(|key| key.tool.clone())
        .collect();
    projects
        .into_iter()
        .map(|(key, mut entries)| {
            let conflicts: BTreeSet<_> = entries
                .iter()
                .filter(|entry| {
                    entries
                        .iter()
                        .filter(|other| other.name == entry.name)
                        .count()
                        > 1
                })
                .map(|entry| entry.name.clone())
                .collect();
            for entry in &mut entries {
                let qualify =
                    conflicts.contains(&entry.name) || matches!(entry.name.as_str(), BACK | EXIT);
                entry.label = if qualify {
                    format!(
                        "{} [{}]",
                        entry.name,
                        match entry.source {
                            EntrySource::Catalog => "shared",
                            EntrySource::Personal => "personal",
                        }
                    )
                } else {
                    entry.name.clone()
                };
                if let Some(description) = &entry.description {
                    entry.label.push_str(" - ");
                    entry.label.push_str(description);
                }
            }
            let qualify = duplicate_names.contains(&key.tool)
                || matches!(key.tool.as_str(), BROWSE_TOOLS | EXIT);
            Project {
                label: if qualify {
                    format!("{} ({})", key.tool, key.catalog)
                } else {
                    key.tool.clone()
                },
                tool: key.tool,
                catalog: key.catalog,
                entries,
            }
        })
        .collect()
}

fn browse<P: Prompt>(prompt: &mut P, root: &Path, root_label: &str) -> Result<Option<PathBuf>> {
    let mut relative = PathBuf::new();
    loop {
        let mut entries: Vec<_> = loadbot::launcher::browse_directory(root, &relative)?
            .into_iter()
            .map(|entry| BrowserEntry {
                label: if entry.is_directory {
                    format!("{}/", entry.name)
                } else {
                    entry.name
                },
                path: entry.path,
                is_directory: entry.is_directory,
            })
            .collect();

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
            root_label.to_owned()
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

#[cfg(test)]
fn save_shortcut<P: Prompt>(
    prompt: &mut P,
    shortcut_path: &Path,
    catalog: &str,
    tool: &str,
    relative: &Path,
) -> Result<Option<String>> {
    let Some(name) = shortcut_name(prompt, relative)? else {
        return Ok(None);
    };
    let shortcut = Shortcut::new(
        catalog.to_owned(),
        tool.to_owned(),
        shortcuts::portable_path(relative)?,
    )?;
    shortcuts::save(shortcut_path, &name, shortcut)?;
    Ok(Some(name))
}

fn shortcut_name<P: Prompt>(prompt: &mut P, relative: &Path) -> Result<Option<String>> {
    let default = relative
        .file_stem()
        .and_then(|name| name.to_str())
        .filter(|name| paths::validate_name(name).is_ok());
    let label = match default {
        Some(default) => format!("Shortcut name [{default}]:"),
        None => "Shortcut name:".to_owned(),
    };
    let Some(name) = prompt.input(&label, default)? else {
        cancelled()?;
        return Ok(None);
    };
    paths::validate_name(&name).context("invalid shortcut name")?;
    Ok(Some(name))
}

fn save_shortcut_for_paths<P: Prompt>(
    prompt: &mut P,
    paths: &Paths,
    catalog: &str,
    tool: &str,
    relative: &Path,
) -> Result<Option<String>> {
    let Some(name) = shortcut_name(prompt, relative)? else {
        return Ok(None);
    };
    operations::shortcut_add(
        paths,
        catalog,
        tool,
        &name,
        &shortcuts::portable_path(relative)?,
        None,
        None,
    )?;
    Ok(Some(name))
}

#[cfg(test)]
fn create_shortcut<P: Prompt>(
    prompt: &mut P,
    shortcut_path: &Path,
    catalog: &str,
    tool: &str,
    relative: &Path,
) -> Result<bool> {
    let Some(name) = save_shortcut(prompt, shortcut_path, catalog, tool, relative)? else {
        return Ok(false);
    };
    prompt.message(&format!("Shortcut '{name}' saved."))?;
    Ok(true)
}

fn create_shortcut_for_paths<P: Prompt>(
    prompt: &mut P,
    paths: &Paths,
    catalog: &str,
    tool: &str,
    relative: &Path,
) -> Result<bool> {
    let Some(name) = save_shortcut_for_paths(prompt, paths, catalog, tool, relative)? else {
        return Ok(false);
    };
    prompt.message(&format!("Shortcut '{name}' saved."))?;
    Ok(true)
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

struct SelectedFile {
    catalog: String,
    tool: String,
    root: PathBuf,
    relative: PathBuf,
}

#[derive(Debug, PartialEq, Eq)]
enum ProjectAction {
    Project(usize),
    Browse,
    Exit,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct ProjectKey {
    tool: String,
    catalog: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Project {
    label: String,
    tool: String,
    catalog: String,
    entries: Vec<ProjectEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ProjectEntry {
    name: String,
    label: String,
    path: String,
    description: Option<String>,
    runner: Option<Runner>,
    source: EntrySource,
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;

    use super::*;

    #[derive(Default)]
    struct FakePrompt {
        inputs: VecDeque<Option<String>>,
        confirmations: VecDeque<Option<bool>>,
        selections: VecDeque<Option<String>>,
        displayed: Vec<Vec<String>>,
        input_requests: Vec<(String, Option<String>)>,
        confirmation_requests: Vec<(String, bool)>,
        selection_labels: Vec<String>,
        messages: Vec<String>,
    }

    impl Prompt for FakePrompt {
        fn input(&mut self, label: &str, default: Option<&str>) -> Result<Option<String>> {
            self.input_requests
                .push((label.to_owned(), default.map(str::to_owned)));
            Ok(self.inputs.pop_front().flatten())
        }

        fn confirm(&mut self, label: &str, default: bool) -> Result<Option<bool>> {
            self.confirmation_requests.push((label.to_owned(), default));
            Ok(self.confirmations.pop_front().flatten())
        }

        fn select(&mut self, label: &str, choices: &[String]) -> Result<Option<String>> {
            self.selection_labels.push(label.to_owned());
            self.displayed.push(choices.to_vec());
            Ok(self.selections.pop_front().flatten())
        }

        fn message(&mut self, message: &str) -> Result<()> {
            self.messages.push(message.to_owned());
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

        let selected = browse(&mut prompt, root, "Browse demo:").unwrap().unwrap();
        assert_eq!(selected, PathBuf::from("recipes/run.sh"));
        assert!(!prompt.displayed[0].contains(&"run.sh".to_owned()));
        assert!(prompt.displayed[1].contains(&"run.sh".to_owned()));
        assert_eq!(prompt.displayed[2], prompt.displayed[0]);
    }

    #[test]
    fn shortcut_save_uses_file_stem_and_portable_relative_path() {
        let temporary = tempfile::TempDir::new().unwrap();
        let shortcut_path = temporary.path().join("shortcuts.toml");
        let mut prompt = FakePrompt {
            inputs: [Some("dotfiles".to_owned())].into(),
            ..FakePrompt::default()
        };

        let name = save_shortcut(
            &mut prompt,
            &shortcut_path,
            "personal",
            "dotfiles",
            Path::new("recipes/install_dotfiles.sh"),
        )
        .unwrap();

        assert_eq!(name.as_deref(), Some("dotfiles"));
        assert_eq!(
            prompt.input_requests,
            [(
                "Shortcut name [install_dotfiles]:".to_owned(),
                Some("install_dotfiles".to_owned())
            )]
        );
        let saved = shortcuts::load(&shortcut_path).unwrap();
        assert_eq!(saved.shortcuts["dotfiles"].catalog, "personal");
        assert_eq!(saved.shortcuts["dotfiles"].tool, "dotfiles");
        assert_eq!(
            saved.shortcuts["dotfiles"].path,
            "recipes/install_dotfiles.sh"
        );
        assert!(
            !fs::read_to_string(shortcut_path)
                .unwrap()
                .contains(temporary.path().to_str().unwrap())
        );
    }

    #[test]
    fn known_tool_flow_saves_multiple_shortcuts_without_running_files() {
        let temporary = tempfile::TempDir::new().unwrap();
        let root = temporary.path().join("tool");
        let marker = temporary.path().join("executed");
        fs::create_dir(&root).unwrap();
        fs::write(
            root.join("install_dotfiles.sh"),
            format!("touch {:?}\n", marker),
        )
        .unwrap();
        fs::write(root.join("update.sh"), format!("touch {:?}\n", marker)).unwrap();
        let shortcut_path = temporary.path().join("shortcuts.toml");
        let mut prompt = FakePrompt {
            inputs: [Some("dotfiles".to_owned()), Some("update".to_owned())].into(),
            confirmations: [Some(true), Some(false)].into(),
            selections: [
                Some("install_dotfiles.sh".to_owned()),
                Some("update.sh".to_owned()),
            ]
            .into(),
            ..FakePrompt::default()
        };

        add_shortcuts_for_tool_from(&mut prompt, &shortcut_path, "personal", "dotfiles", &root)
            .unwrap();

        let saved = shortcuts::load(&shortcut_path).unwrap();
        assert_eq!(saved.shortcuts["dotfiles"].path, "install_dotfiles.sh");
        assert_eq!(saved.shortcuts["update"].path, "update.sh");
        assert!(
            saved
                .shortcuts
                .values()
                .all(|shortcut| shortcut.catalog == "personal" && shortcut.tool == "dotfiles")
        );
        assert_eq!(prompt.selection_labels, ["Select file:", "Select file:"]);
        assert_eq!(
            prompt.confirmation_requests,
            [
                ("Add another shortcut?".to_owned(), false),
                ("Add another shortcut?".to_owned(), false)
            ]
        );
        assert_eq!(
            prompt.messages,
            ["Shortcut 'dotfiles' saved.", "Shortcut 'update' saved."]
        );
        assert!(!marker.exists());
    }

    #[test]
    fn project_menu_is_first_and_keeps_browser_reachable() {
        let projects = vec![Project {
            label: "demo".to_owned(),
            tool: "demo".to_owned(),
            catalog: "personal".to_owned(),
            entries: Vec::new(),
        }];
        let mut project_prompt = FakePrompt {
            selections: [Some("demo".to_owned())].into(),
            ..FakePrompt::default()
        };
        assert_eq!(
            select_project_action(&mut project_prompt, &projects).unwrap(),
            ProjectAction::Project(0)
        );
        assert_eq!(project_prompt.displayed[0], ["demo", BROWSE_TOOLS, EXIT]);

        let mut browse_prompt = FakePrompt {
            selections: [Some(BROWSE_TOOLS.to_owned())].into(),
            ..FakePrompt::default()
        };
        assert_eq!(
            select_project_action(&mut browse_prompt, &projects).unwrap(),
            ProjectAction::Browse
        );
    }

    #[test]
    fn zero_shortcuts_offer_browse_and_exit() {
        let mut prompt = FakePrompt {
            selections: [Some(BROWSE_TOOLS.to_owned())].into(),
            ..FakePrompt::default()
        };

        assert_eq!(
            select_project_action(&mut prompt, &[]).unwrap(),
            ProjectAction::Browse
        );
        assert_eq!(prompt.displayed[0], [BROWSE_TOOLS, EXIT]);

        let mut exit_prompt = FakePrompt {
            selections: [Some(EXIT.to_owned())].into(),
            ..FakePrompt::default()
        };
        assert_eq!(
            select_project_action(&mut exit_prompt, &[]).unwrap(),
            ProjectAction::Exit
        );
    }

    #[test]
    fn inventory_groups_projects_qualifies_duplicates_and_marks_name_conflicts() {
        use loadbot::catalog::{CommandConfig, SourceType, ToolConfig};

        let command = |description: Option<&str>| CommandConfig {
            path: "scripts/audit.sh".to_owned(),
            description: description.map(str::to_owned),
            runner: Some(Runner::Bash),
            extra: BTreeMap::new(),
        };
        let tool = |catalog: &str, commands: BTreeMap<String, CommandConfig>| ResolvedTool {
            name: "demo".to_owned(),
            catalog: catalog.to_owned(),
            definition: ToolConfig {
                source_type: SourceType::Git,
                url: "demo.git".to_owned(),
                revision: None,
                commands,
                extra: BTreeMap::new(),
            },
        };
        let tools = [
            tool(
                "personal",
                BTreeMap::from([
                    ("audit".to_owned(), command(Some("Shared audit"))),
                    ("build".to_owned(), command(None)),
                ]),
            ),
            tool(
                "public",
                BTreeMap::from([("scan".to_owned(), command(None))]),
            ),
        ];
        let mut shortcut_file = shortcuts::ShortcutFile::default();
        let mut shortcut = Shortcut::new(
            "personal".to_owned(),
            "demo".to_owned(),
            "local/audit.sh".to_owned(),
        )
        .unwrap();
        shortcut.description = Some("My audit".to_owned());
        shortcut_file.shortcuts.insert("audit".to_owned(), shortcut);

        let projects = project_inventory(&tools, &shortcut_file);

        assert_eq!(projects.len(), 2);
        assert_eq!(projects[0].label, "demo (personal)");
        assert_eq!(projects[1].label, "demo (public)");
        assert_eq!(
            projects[0]
                .entries
                .iter()
                .map(|entry| entry.label.as_str())
                .collect::<Vec<_>>(),
            [
                "audit [shared] - Shared audit",
                "audit [personal] - My audit",
                "build"
            ]
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
            selections: [Some("demo".to_owned()), Some("broken-tool".to_owned())].into(),
            ..FakePrompt::default()
        };

        let error = run_interactive_from(&paths, &mut prompt, &shortcut_path).unwrap_err();
        assert!(prompt.displayed[0].contains(&"demo".to_owned()));
        assert!(prompt.displayed[1].contains(&"broken-tool".to_owned()));
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

    #[cfg(unix)]
    #[test]
    fn explicit_bash_runner_runs_non_executable_script_with_bash_from_tool_root() {
        let temporary = tempfile::TempDir::new().unwrap();
        let root = temporary.path();
        fs::create_dir(root.join("scripts")).unwrap();
        let script = root.join("scripts/run.sh");
        fs::write(&script, "[[ -d scripts ]] && printf bash > runner-result\n").unwrap();

        launch_with_runner(&script, root, Runner::Bash).unwrap();

        assert_eq!(
            fs::read_to_string(root.join("runner-result")).unwrap(),
            "bash"
        );
    }

    #[cfg(unix)]
    #[test]
    fn direct_runner_does_not_guess_an_interpreter_for_non_executable_scripts() {
        let temporary = tempfile::TempDir::new().unwrap();
        let script = temporary.path().join("run.sh");
        fs::write(&script, "exit 0\n").unwrap();

        let error = launch_with_runner(&script, temporary.path(), Runner::Direct).unwrap_err();

        assert!(format!("{error:#}").contains("could not launch"));
    }
}
