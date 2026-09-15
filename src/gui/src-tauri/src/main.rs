#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use loadbot::{
    interaction::{OperationContext, Unattended},
    launcher::{self, Project},
    paths::Paths,
};

#[derive(Debug, serde::Serialize)]
struct InventoryReadError {
    message: String,
}

#[tauri::command]
async fn read_loadbot_inventory() -> Result<Vec<Project>, InventoryReadError> {
    // Paths and the synchronous, non-Send operation context are created on the worker.
    // No caller-supplied path, shell string, or mutation/execution command is accepted.
    tauri::async_runtime::spawn_blocking(|| {
        let result = (|| -> anyhow::Result<Vec<Project>> {
            let paths = Paths::discover()?;
            let mut policy = Unattended;
            let mut context = OperationContext::new(&mut policy);
            context.process.terminal = false;
            context
                .run(|context| launcher::read_project_inventory(&paths, context))
                .result
        })();
        result.map_err(|error| InventoryReadError {
            message: format!("{error:#}"),
        })
    })
    .await
    .map_err(|error| InventoryReadError {
        message: format!("inventory worker failed: {error}"),
    })?
}

fn main() {
    // The same native host and one read-only query serve Windows and Linux.
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![read_loadbot_inventory])
        .run(tauri::generate_context!())
        .expect("could not launch the Loadbot desktop window");
}
