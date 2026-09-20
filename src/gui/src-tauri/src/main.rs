#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use anyhow::Context;
use loadbot::{
    catalog::Runner,
    interaction::{Interaction, Notice, OperationContext, ToolOperationStage, Unattended},
    launcher::{self, Project},
    operations::{self, CatalogState, ShortcutHelpRequest, ShortcutIdentity},
    paths::Paths,
    recipe::RecipeDefinition,
};
use std::fs;
#[cfg(unix)]
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use tauri::Manager;
use tauri::ipc::Channel;
use tauri_plugin_dialog::DialogExt;

const WORKSPACE_LAYOUT_FILE: &str = "workspace-layout-v1.json";
const MAX_WORKSPACE_LAYOUT_BYTES: usize = 4096;

#[derive(Debug, serde::Serialize)]
struct DesktopError {
    kind: &'static str,
    message: String,
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct CatalogContext {
    name: String,
    url: String,
    writable: bool,
    state: &'static str,
    default: bool,
}

#[derive(Debug, serde::Serialize)]
struct ProjectIdentity {
    catalog: String,
    tool: String,
}

#[derive(Debug, serde::Serialize)]
struct CatalogIdentity {
    catalog: String,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
enum BackendActivity {
    Progress {
        stage: &'static str,
        catalog: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        tool: Option<String>,
        detail: Option<String>,
    },
    Log {
        stream: &'static str,
        text: String,
    },
}

struct DesktopInteraction {
    activity: Option<Channel<BackendActivity>>,
}

impl Interaction for DesktopInteraction {
    fn notice(&mut self, notice: &Notice) {
        if let (Some(channel), Some(activity)) = (&self.activity, backend_activity(notice)) {
            let _ = channel.send(activity);
        }
    }
}

fn backend_activity(notice: &Notice) -> Option<BackendActivity> {
    Some(match notice {
        Notice::CatalogSyncStarted { name } => BackendActivity::Progress {
            stage: "validating",
            catalog: name.clone(),
            tool: None,
            detail: None,
        },
        Notice::CatalogSyncRepositoryChecked { name } => BackendActivity::Progress {
            stage: "repository-checked",
            catalog: name.clone(),
            tool: None,
            detail: None,
        },
        Notice::CatalogSyncUpdateStarted { name } => BackendActivity::Progress {
            stage: "updating-repository",
            catalog: name.clone(),
            tool: None,
            detail: None,
        },
        Notice::CatalogCurrent { name, new_commit } => BackendActivity::Progress {
            stage: "current",
            catalog: name.clone(),
            tool: None,
            detail: Some(new_commit.clone()),
        },
        Notice::CatalogSynced {
            name,
            old_commit,
            new_commit,
        } => BackendActivity::Progress {
            stage: "updated",
            catalog: name.clone(),
            tool: None,
            detail: Some(format!("{old_commit} → {new_commit}")),
        },
        Notice::ToolOperationStage {
            operation: _,
            stage,
            name,
            catalog_name,
        } => BackendActivity::Progress {
            stage: match stage {
                ToolOperationStage::ValidatingCheckout => "validating-checkout",
                ToolOperationStage::CloningProject => "cloning-project",
                ToolOperationStage::ValidatingFreshCheckout => "validating-fresh-checkout",
                ToolOperationStage::FetchingAndUpdating => "fetching-and-updating",
                ToolOperationStage::RemovingCheckout => "removing-checkout",
                ToolOperationStage::ReplacingCheckout => "replacing-checkout",
            },
            catalog: catalog_name.clone(),
            tool: Some(name.clone()),
            detail: None,
        },
        _ => return None,
    })
}

fn backend_process_activity(event: loadbot::process::Event) -> Option<BackendActivity> {
    use loadbot::process::{Event, Stream};
    let log = |stream, text| BackendActivity::Log { stream, text };
    Some(match event {
        Event::Starting {
            program,
            arguments,
            directory,
        } => {
            let command = std::iter::once(program)
                .chain(arguments)
                .map(|part| quote_log_argument(&part.to_string_lossy()))
                .collect::<Vec<_>>()
                .join(" ");
            let text = directory.map_or(command.clone(), |path| {
                format!("{command}\nworking directory: {}", path.display())
            });
            log("command", text)
        }
        Event::Output { stream, bytes } => log(
            match stream {
                Stream::Stdout => "stdout",
                Stream::Stderr => "stderr",
            },
            String::from_utf8_lossy(&bytes).into_owned(),
        ),
        Event::Exited { status, .. } => log("system", format!("process exited with {status}")),
        Event::Cancelled { .. } => log("system", "process cancelled".into()),
        Event::Failed { diagnostic } => log("system", diagnostic),
        Event::OperationStarted | Event::OperationFinished { .. } | Event::Started { .. } => {
            return None;
        }
    })
}

fn quote_log_argument(value: &str) -> String {
    if !value.is_empty()
        && value
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || "-._/:=@".contains(character))
    {
        value.to_owned()
    } else {
        format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
    }
}

#[tauri::command]
async fn read_loadbot_inventory() -> Result<Vec<Project>, DesktopError> {
    // Inventory and catalog context deliberately share the same path discovery
    // helper, so the installed GUI resolves exactly the state used by the CLI.
    run_loadbot_worker("inventory query", move |paths, context| {
        launcher::read_project_inventory(paths, context)
    })
    .await
}

#[tauri::command]
async fn open_loadbot_project(catalog: String, tool: String) -> Result<(), DesktopError> {
    tauri::async_runtime::spawn_blocking(move || {
        let result = (|| -> anyhow::Result<()> {
            let paths = Paths::discover()?;
            let mut policy = Unattended;
            let mut context = OperationContext::background(&mut policy);
            let directory =
                launcher::resolve_project_directory(&paths, &catalog, &tool, &mut context)?;
            open_directory(&directory)
        })();
        result.map_err(|error| DesktopError {
            kind: "operation",
            message: format!("{error:#}"),
        })
    })
    .await
    .map_err(|error| DesktopError {
        kind: "worker",
        message: format!("project-folder worker failed: {error}"),
    })?
}

#[tauri::command]
async fn open_loadbot_project_terminal(catalog: String, tool: String) -> Result<(), DesktopError> {
    tauri::async_runtime::spawn_blocking(move || {
        let result = (|| -> anyhow::Result<()> {
            let paths = Paths::discover()?;
            let mut policy = Unattended;
            let mut context = OperationContext::background(&mut policy);
            let directory =
                launcher::resolve_project_directory(&paths, &catalog, &tool, &mut context)?;
            open_terminal(&directory)
        })();
        result.map_err(|error| DesktopError {
            kind: "operation",
            message: format!("{error:#}"),
        })
    })
    .await
    .map_err(|error| DesktopError {
        kind: "worker",
        message: format!("project-terminal worker failed: {error}"),
    })?
}

#[tauri::command]
async fn read_loadbot_catalogs() -> Result<Vec<CatalogContext>, DesktopError> {
    run_loadbot_worker("catalog query", move |paths, context| {
        Ok(operations::catalog_list(paths, context)?
            .into_iter()
            .map(|catalog| CatalogContext {
                name: catalog.name,
                url: catalog.source.url,
                writable: catalog.source.writable,
                state: match catalog.state {
                    CatalogState::Missing => "missing",
                    CatalogState::Installed => "installed",
                    CatalogState::Mismatch => "mismatch",
                },
                default: catalog.default,
            })
            .collect())
    })
    .await
}

#[tauri::command]
async fn add_loadbot_catalog(
    name: String,
    url: String,
    writable: bool,
) -> Result<CatalogIdentity, DesktopError> {
    run_loadbot_worker("catalog add", move |paths, context| {
        operations::catalog_add(paths, &name, url, writable, context)?;
        Ok(CatalogIdentity { catalog: name })
    })
    .await
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
async fn add_loadbot_project(
    catalog: String,
    name: String,
    url: String,
    revision: Option<String>,
    commit: bool,
    push: bool,
) -> Result<ProjectIdentity, DesktopError> {
    let identity = ProjectIdentity {
        catalog: catalog.clone(),
        tool: name.clone(),
    };
    run_loadbot_worker("project add", move |paths, context| {
        operations::tool_add(paths, &catalog, &name, url, revision, commit, push, context)?;
        Ok(identity)
    })
    .await
}

async fn project_operation(
    label: &'static str,
    catalog: String,
    tool: String,
    activity: Channel<BackendActivity>,
    operation: fn(
        &Paths,
        &str,
        Option<&str>,
        &mut OperationContext<'_>,
    ) -> anyhow::Result<loadbot::interaction::MutationOutcome>,
) -> Result<ProjectIdentity, DesktopError> {
    let identity = ProjectIdentity {
        catalog: catalog.clone(),
        tool: tool.clone(),
    };
    run_loadbot_worker_with_activity(label, Some(activity), move |paths, context| {
        operation(paths, &tool, Some(&catalog), context)?;
        Ok(identity)
    })
    .await
}

#[tauri::command]
async fn pull_loadbot_project(
    catalog: String,
    tool: String,
    on_activity: Channel<BackendActivity>,
) -> Result<ProjectIdentity, DesktopError> {
    project_operation(
        "project pull",
        catalog,
        tool,
        on_activity,
        operations::tool_pull,
    )
    .await
}

#[tauri::command]
async fn update_loadbot_project(
    catalog: String,
    tool: String,
    on_activity: Channel<BackendActivity>,
) -> Result<ProjectIdentity, DesktopError> {
    project_operation(
        "project update",
        catalog,
        tool,
        on_activity,
        operations::tool_update,
    )
    .await
}

#[tauri::command]
async fn remove_loadbot_project(
    catalog: String,
    tool: String,
    on_activity: Channel<BackendActivity>,
) -> Result<ProjectIdentity, DesktopError> {
    project_operation(
        "project remove",
        catalog,
        tool,
        on_activity,
        operations::tool_remove,
    )
    .await
}

#[tauri::command]
async fn reinstall_loadbot_project(
    catalog: String,
    tool: String,
    on_activity: Channel<BackendActivity>,
) -> Result<ProjectIdentity, DesktopError> {
    project_operation(
        "project reinstall",
        catalog,
        tool,
        on_activity,
        operations::tool_reinstall,
    )
    .await
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
async fn add_loadbot_shortcut(
    catalog: String,
    tool: String,
    name: String,
    path: String,
    description: Option<String>,
    runner: Option<Runner>,
) -> Result<ShortcutIdentity, DesktopError> {
    run_loadbot_worker("shortcut add", move |paths, context| {
        operations::shortcut_add(
            paths,
            &catalog,
            &tool,
            &name,
            &path,
            description,
            runner,
            context,
        )
    })
    .await
}

#[tauri::command]
async fn add_loadbot_recipe_shortcut(
    catalog: String,
    tool: String,
    name: String,
    description: Option<String>,
    recipe: RecipeDefinition,
) -> Result<ShortcutIdentity, DesktopError> {
    run_loadbot_worker("Recipe shortcut add", move |paths, context| {
        operations::shortcut_add_recipe(paths, &catalog, &tool, &name, description, recipe, context)
    })
    .await
}

#[tauri::command]
async fn update_loadbot_recipe_shortcut(
    catalog: String,
    tool: String,
    name: String,
    description: Option<String>,
    recipe: RecipeDefinition,
) -> Result<ShortcutIdentity, DesktopError> {
    run_loadbot_worker("Recipe shortcut update", move |paths, context| {
        operations::shortcut_update_recipe(
            paths,
            &catalog,
            &tool,
            &name,
            description,
            recipe,
            context,
        )
    })
    .await
}

#[tauri::command]
async fn delete_loadbot_shortcuts(
    identities: Vec<ShortcutIdentity>,
) -> Result<usize, DesktopError> {
    run_loadbot_worker("shortcut delete", move |paths, context| {
        operations::shortcut_delete_many(paths, &identities, context)
    })
    .await
}

async fn choose_project_path(
    app: tauri::AppHandle,
    catalog: String,
    tool: String,
    kind: operations::ProjectPathKind,
) -> Result<Option<String>, DesktopError> {
    let root_catalog = catalog.clone();
    let root_tool = tool.clone();
    let root = run_loadbot_worker("project path lookup", move |paths, context| {
        operations::installed_tool_path(paths, &root_tool, &root_catalog, context)
    })
    .await?;
    let selected = tauri::async_runtime::spawn_blocking(move || {
        let picker = app.dialog().file().set_directory(root);
        match kind {
            operations::ProjectPathKind::File => picker.blocking_pick_file(),
            operations::ProjectPathKind::Directory => picker.blocking_pick_folder(),
        }
        .map(|path| path.into_path())
        .transpose()
    })
    .await
    .map_err(|error| DesktopError {
        kind: "worker",
        message: format!("project picker worker failed: {error}"),
    })?
    .map_err(|error| DesktopError {
        kind: "operation",
        message: format!("selected path is not a native filesystem path: {error}"),
    })?;
    let Some(selected) = selected else {
        return Ok(None);
    };
    run_loadbot_worker("project path validation", move |paths, context| {
        operations::portable_project_path(paths, &catalog, &tool, &selected, kind, context)
            .map(Some)
    })
    .await
}

#[tauri::command]
async fn choose_loadbot_project_file(
    app: tauri::AppHandle,
    catalog: String,
    tool: String,
) -> Result<Option<String>, DesktopError> {
    choose_project_path(app, catalog, tool, operations::ProjectPathKind::File).await
}

#[tauri::command]
async fn choose_loadbot_project_directory(
    app: tauri::AppHandle,
    catalog: String,
    tool: String,
) -> Result<Option<String>, DesktopError> {
    choose_project_path(app, catalog, tool, operations::ProjectPathKind::Directory).await
}

#[tauri::command]
async fn view_loadbot_shortcut_help(
    request: ShortcutHelpRequest,
) -> Result<launcher::HelpResult, DesktopError> {
    run_loadbot_worker("shortcut help", move |paths, context| {
        operations::shortcut_help(paths, &request, context)
    })
    .await
}

#[tauri::command]
async fn sync_loadbot_catalog(
    catalog: String,
    on_activity: Channel<BackendActivity>,
) -> Result<(), DesktopError> {
    run_loadbot_worker_with_activity("catalog sync", Some(on_activity), move |paths, context| {
        operations::catalog_sync(paths, &catalog, context)?;
        Ok(())
    })
    .await
}

async fn run_loadbot_worker<T, F>(label: &'static str, operation: F) -> Result<T, DesktopError>
where
    T: Send + 'static,
    F: FnOnce(&Paths, &mut OperationContext<'_>) -> anyhow::Result<T> + Send + 'static,
{
    run_loadbot_worker_with_activity(label, None, operation).await
}

async fn run_loadbot_worker_with_activity<T, F>(
    label: &'static str,
    activity: Option<Channel<BackendActivity>>,
    operation: F,
) -> Result<T, DesktopError>
where
    T: Send + 'static,
    F: FnOnce(&Paths, &mut OperationContext<'_>) -> anyhow::Result<T> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(move || {
        let result = (|| -> anyhow::Result<T> {
            let paths = Paths::discover()?;
            let process_activity = activity.clone();
            let mut policy = DesktopInteraction { activity };
            let mut context = OperationContext::background(&mut policy);
            if let Some(channel) = process_activity {
                context.process.observer = Some(Arc::new(move |event| {
                    if let Some(activity) = backend_process_activity(event) {
                        let _ = channel.send(activity);
                    }
                }));
            }
            context.run(|context| operation(&paths, context)).result
        })();
        result.map_err(|error| DesktopError {
            kind: if error
                .downcast_ref::<loadbot::process::Cancelled>()
                .is_some()
            {
                "cancelled"
            } else {
                "operation"
            },
            message: format!("{error:#}"),
        })
    })
    .await
    .map_err(|error| DesktopError {
        kind: "worker",
        message: format!("{label} worker failed: {error}"),
    })?
}

#[tauri::command]
fn read_loadbot_workspace_layout(app: tauri::AppHandle) -> Result<Option<String>, DesktopError> {
    let result = workspace_layout_path(&app).and_then(|path| read_workspace_layout(&path));
    result.map_err(desktop_error)
}

#[tauri::command]
fn write_loadbot_workspace_layout(
    app: tauri::AppHandle,
    contents: String,
) -> Result<(), DesktopError> {
    let result =
        workspace_layout_path(&app).and_then(|path| write_workspace_layout(&path, &contents));
    result.map_err(desktop_error)
}

fn desktop_error(error: anyhow::Error) -> DesktopError {
    DesktopError {
        kind: "storage",
        message: format!("{error:#}"),
    }
}

fn workspace_layout_path<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> anyhow::Result<PathBuf> {
    Ok(app.path().app_local_data_dir()?.join(WORKSPACE_LAYOUT_FILE))
}

fn read_workspace_layout(path: &Path) -> anyhow::Result<Option<String>> {
    if fs::symlink_metadata(path).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
        anyhow::bail!("refusing symlink workspace layout {}", path.display());
    }
    match fs::read_to_string(path) {
        Ok(contents) if contents.len() <= MAX_WORKSPACE_LAYOUT_BYTES => Ok(Some(contents)),
        Ok(_) => anyhow::bail!("workspace layout is unexpectedly large"),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error)
            .with_context(|| format!("could not read workspace layout {}", path.display())),
    }
}

fn write_workspace_layout(path: &Path, contents: &str) -> anyhow::Result<()> {
    if contents.len() > MAX_WORKSPACE_LAYOUT_BYTES {
        anyhow::bail!("workspace layout is unexpectedly large");
    }
    let parent = path.parent().context("workspace layout has no parent")?;
    fs::create_dir_all(parent)?;
    if fs::symlink_metadata(path).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
        anyhow::bail!("refusing symlink workspace layout {}", path.display());
    }
    let mut temporary = tempfile::Builder::new()
        .prefix(".loadbot-workspace-")
        .tempfile_in(parent)?;
    temporary.write_all(contents.as_bytes())?;
    temporary.as_file().sync_all()?;
    temporary.persist(path).map_err(|error| error.error)?;
    #[cfg(unix)]
    File::open(parent)?.sync_all()?;
    Ok(())
}

fn open_directory(path: &Path) -> anyhow::Result<()> {
    let mut command = directory_open_command(path);
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .with_context(|| {
            format!(
                "could not open project directory {} with the system file manager",
                path.display()
            )
        })?;
    Ok(())
}

fn directory_open_command(path: &Path) -> Command {
    #[cfg(target_os = "windows")]
    let mut command = Command::new("explorer.exe");
    #[cfg(target_os = "linux")]
    let mut command = Command::new("xdg-open");
    #[cfg(not(any(target_os = "windows", target_os = "linux")))]
    let mut command = Command::new("false");
    command.arg(path);
    command
}

fn open_terminal(path: &Path) -> anyhow::Result<()> {
    let mut failures = Vec::new();
    for mut command in terminal_open_commands(path) {
        let program = command.get_program().to_string_lossy().into_owned();
        match command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        {
            Ok(_) => return Ok(()),
            Err(error) => failures.push(format!("{program}: {error}")),
        }
    }
    anyhow::bail!(
        "could not open a terminal in {} ({})",
        path.display(),
        failures.join("; ")
    )
}

fn terminal_open_commands(path: &Path) -> Vec<Command> {
    #[cfg(target_os = "windows")]
    let commands = vec![{
        let mut command = Command::new("cmd.exe");
        command.arg("/K");
        command
    }];
    #[cfg(target_os = "linux")]
    let commands = [
        "x-terminal-emulator",
        "gnome-terminal",
        "konsole",
        "xfce4-terminal",
        "xterm",
    ]
    .map(Command::new)
    .into_iter()
    .collect::<Vec<_>>();
    #[cfg(not(any(target_os = "windows", target_os = "linux")))]
    let commands = vec![Command::new("false")];
    commands
        .into_iter()
        .map(|mut command| {
            command.current_dir(path);
            command
        })
        .collect()
}

fn main() {
    // The same native host and qualified semantic capabilities serve Windows and Linux.
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            read_loadbot_inventory,
            read_loadbot_catalogs,
            open_loadbot_project,
            open_loadbot_project_terminal,
            add_loadbot_catalog,
            add_loadbot_project,
            pull_loadbot_project,
            update_loadbot_project,
            remove_loadbot_project,
            reinstall_loadbot_project,
            add_loadbot_shortcut,
            add_loadbot_recipe_shortcut,
            update_loadbot_recipe_shortcut,
            delete_loadbot_shortcuts,
            choose_loadbot_project_file,
            choose_loadbot_project_directory,
            view_loadbot_shortcut_help,
            sync_loadbot_catalog,
            read_loadbot_workspace_layout,
            write_loadbot_workspace_layout
        ])
        .run(tauri::generate_context!())
        .expect("could not launch the Loadbot desktop window");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_folder_is_one_literal_argument_without_a_shell() {
        let path = Path::new("project with spaces;and-metacharacters");
        let command = directory_open_command(path);
        #[cfg(target_os = "windows")]
        assert_eq!(command.get_program(), "explorer.exe");
        #[cfg(target_os = "linux")]
        assert_eq!(command.get_program(), "xdg-open");
        assert_eq!(command.get_args().collect::<Vec<_>>(), [path.as_os_str()]);
    }

    #[test]
    fn project_terminal_uses_the_project_as_its_working_directory() {
        let path = Path::new("project with spaces;and-metacharacters");
        let commands = terminal_open_commands(path);
        #[cfg(target_os = "windows")]
        {
            let command = &commands[0];
            assert_eq!(command.get_program(), "cmd.exe");
            assert_eq!(command.get_args().collect::<Vec<_>>(), ["/K"]);
        }
        #[cfg(target_os = "linux")]
        assert_eq!(
            commands
                .iter()
                .map(|command| command.get_program())
                .collect::<Vec<_>>(),
            [
                "x-terminal-emulator",
                "gnome-terminal",
                "konsole",
                "xfce4-terminal",
                "xterm"
            ]
        );
        assert!(
            commands
                .iter()
                .all(|command| command.get_current_dir() == Some(path))
        );
    }

    #[test]
    fn native_activity_maps_typed_core_sync_notices_without_cli_text() {
        let checked = backend_activity(&Notice::CatalogSyncRepositoryChecked {
            name: "personal".into(),
        })
        .unwrap();
        let BackendActivity::Progress {
            stage,
            catalog,
            detail,
            ..
        } = checked
        else {
            panic!("expected progress activity")
        };
        assert_eq!(stage, "repository-checked");
        assert_eq!(catalog, "personal");
        assert!(detail.is_none());

        let updated = backend_activity(&Notice::CatalogSynced {
            name: "personal".into(),
            old_commit: "abc".into(),
            new_commit: "def".into(),
        })
        .unwrap();
        let BackendActivity::Progress { stage, detail, .. } = updated else {
            panic!("expected progress activity")
        };
        assert_eq!(stage, "updated");
        assert_eq!(detail.as_deref(), Some("abc → def"));

        let project = backend_activity(&Notice::ToolOperationStage {
            operation: loadbot::interaction::ToolOperation::Reinstall,
            stage: ToolOperationStage::ReplacingCheckout,
            name: "demo".into(),
            catalog_name: "personal".into(),
        })
        .unwrap();
        let BackendActivity::Progress {
            stage,
            catalog,
            tool,
            detail,
        } = project
        else {
            panic!("expected progress activity")
        };
        assert_eq!(stage, "replacing-checkout");
        assert_eq!(catalog, "personal");
        assert_eq!(tool.as_deref(), Some("demo"));
        assert!(detail.is_none());
    }

    #[test]
    fn native_activity_maps_real_process_commands_and_output_as_verbose_logs() {
        let command = backend_process_activity(loadbot::process::Event::Starting {
            program: "git".into(),
            arguments: ["fetch", "origin"].into_iter().map(Into::into).collect(),
            directory: Some(PathBuf::from("catalog path")),
        })
        .unwrap();
        let BackendActivity::Log { stream, text } = command else {
            panic!("expected log activity")
        };
        assert_eq!(stream, "command");
        assert_eq!(text, "git fetch origin\nworking directory: catalog path");

        let stderr = backend_process_activity(loadbot::process::Event::Output {
            stream: loadbot::process::Stream::Stderr,
            bytes: b"Permission denied (publickey).\n".to_vec(),
        })
        .unwrap();
        let BackendActivity::Log { stream, text } = stderr else {
            panic!("expected log activity")
        };
        assert_eq!(stream, "stderr");
        assert_eq!(text, "Permission denied (publickey).\n");
    }

    #[test]
    fn workspace_layout_is_gui_local_atomic_and_opaque() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join(WORKSPACE_LAYOUT_FILE);
        assert_eq!(read_workspace_layout(&path).unwrap(), None);
        let first = r#"{"version":1,"projects":320,"shortcuts":240,"terminal":180}"#;
        write_workspace_layout(&path, first).unwrap();
        assert_eq!(
            read_workspace_layout(&path).unwrap().as_deref(),
            Some(first)
        );
        let malformed = "not json; validation belongs to the presentation layer";
        write_workspace_layout(&path, malformed).unwrap();
        assert_eq!(
            read_workspace_layout(&path).unwrap().as_deref(),
            Some(malformed)
        );
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
    }
}
