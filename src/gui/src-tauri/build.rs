fn main() {
    let mut attributes =
        tauri_build::Attributes::new().app_manifest(tauri_build::AppManifest::new().commands(&[
            "start_loadbot_interactive_session",
            "send_loadbot_interactive_input",
            "terminate_loadbot_interactive_session",
            "read_loadbot_inventory",
            "read_loadbot_catalogs",
            "open_loadbot_project",
            "open_loadbot_project_terminal",
            "add_loadbot_catalog",
            "add_loadbot_project",
            "pull_loadbot_project",
            "update_loadbot_project",
            "inspect_loadbot_project_push",
            "push_loadbot_project",
            "commit_and_push_loadbot_project",
            "remove_loadbot_project",
            "reinstall_loadbot_project",
            "add_loadbot_shortcut",
            "add_loadbot_recipe_shortcut",
            "update_loadbot_recipe_shortcut",
            "delete_loadbot_shortcuts",
            "choose_loadbot_project_file",
            "choose_loadbot_project_directory",
            "view_loadbot_shortcut_help",
            "sync_loadbot_catalog",
            "read_loadbot_workspace_layout",
            "write_loadbot_workspace_layout",
        ]));
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        // Windows resources require an ICO container. Embed the approved 100x100
        // PNG byte-for-byte, without resampling or generating replacement artwork.
        let source = "../loadbot-gui-assets/assets/branding/loadbot-header.png";
        println!("cargo:rerun-if-changed={source}");
        let png = std::fs::read(source).expect("approved Loadbot header PNG is missing");
        let mut ico = vec![0, 0, 1, 0, 1, 0, 100, 100, 0, 0, 1, 0, 32, 0];
        ico.extend_from_slice(&(png.len() as u32).to_le_bytes());
        ico.extend_from_slice(&22_u32.to_le_bytes());
        ico.extend_from_slice(&png);
        let path =
            std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap()).join("loadbot.ico");
        std::fs::write(&path, ico).expect("could not write Windows icon container");
        attributes = attributes
            .windows_attributes(tauri_build::WindowsAttributes::new().window_icon_path(path));
    }
    tauri_build::try_build(attributes).expect("could not prepare the Loadbot desktop host");
}
