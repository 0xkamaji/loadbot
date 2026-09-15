#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use anyhow::Context;
use loadbot::{
    catalog::Runner,
    interaction::{OperationContext, Unattended},
    launcher::{self, Project},
    operations::{self, CatalogState, ShortcutIdentity},
    paths::Paths,
};
use std::fs;
#[cfg(unix)]
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use tauri::Manager;

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

#[tauri::command]
async fn read_loadbot_inventory() -> Result<Vec<Project>, DesktopError> {
    // Paths and the synchronous, non-Send operation context are created on the worker.
    // No caller-supplied path, shell string, or mutation/execution command is accepted.
    tauri::async_runtime::spawn_blocking(move || {
        let result = (|| -> anyhow::Result<Vec<Project>> {
            let paths = Paths::discover()?;
            let mut policy = Unattended;
            let mut context = OperationContext::new(&mut policy);
            context.process.terminal = false;
            context
                .run(|context| launcher::read_project_inventory(&paths, context))
                .result
        })();
        result.map_err(|error| DesktopError {
            kind: "operation",
            message: format!("{error:#}"),
        })
    })
    .await
    .map_err(|error| DesktopError {
        kind: "worker",
        message: format!("inventory worker failed: {error}"),
    })?
}

#[tauri::command]
async fn open_loadbot_project(catalog: String, tool: String) -> Result<(), DesktopError> {
    tauri::async_runtime::spawn_blocking(move || {
        let result = (|| -> anyhow::Result<()> {
            let paths = Paths::discover()?;
            let mut policy = Unattended;
            let mut context = OperationContext::new(&mut policy);
            context.process.terminal = false;
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
async fn sync_loadbot_catalog(catalog: String) -> Result<(), DesktopError> {
    run_loadbot_worker("catalog sync", move |paths, context| {
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
    tauri::async_runtime::spawn_blocking(move || {
        let result = (|| -> anyhow::Result<T> {
            let paths = Paths::discover()?;
            let mut policy = Unattended;
            let mut context = OperationContext::new(&mut policy);
            context.process.terminal = false;
            context.run(|context| operation(&paths, context)).result
        })();
        result.map_err(|error| DesktopError {
            kind: "operation",
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

fn main() {
    // The same native host and qualified semantic capabilities serve Windows and Linux.
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            read_loadbot_inventory,
            read_loadbot_catalogs,
            open_loadbot_project,
            add_loadbot_catalog,
            add_loadbot_project,
            add_loadbot_shortcut,
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
