#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // The host owns native decorations and application/window lifetime. Phase 1
    // registers no backend commands: the frontend receives a fixture adapter.
    tauri::Builder::default()
        .run(tauri::generate_context!())
        .expect("could not launch the Loadbot desktop window");
}
