#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use anyhow::Context;
use loadbot::{
    interaction::{OperationContext, Unattended},
    launcher::{self, Project},
    paths::Paths,
};
use std::path::Path;
use std::process::{Command, Stdio};

#[derive(Debug, serde::Serialize)]
struct DesktopError {
    message: String,
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
            message: format!("{error:#}"),
        })
    })
    .await
    .map_err(|error| DesktopError {
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
            message: format!("{error:#}"),
        })
    })
    .await
    .map_err(|error| DesktopError {
        message: format!("project-folder worker failed: {error}"),
    })?
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
            open_loadbot_project
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
}
