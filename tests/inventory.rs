//! The exact GUI read projection, exercised without Tauri or a user's real state.
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};

use loadbot::{
    config::{self, CatalogSource, LocalConfig},
    interaction::{Notice, OperationContext, Unattended},
    launcher::{self, EntrySource},
    operations,
    paths::Paths,
    process::Event,
};

fn paths(root: &Path) -> Paths {
    Paths::with_directories(root.join("data with spaces"), root.join("configuration")).unwrap()
}

fn snapshot(root: &Path) -> BTreeMap<PathBuf, Option<Vec<u8>>> {
    fn visit(root: &Path, path: &Path, files: &mut BTreeMap<PathBuf, Option<Vec<u8>>>) {
        for entry in fs::read_dir(path).unwrap() {
            let path = entry.unwrap().path();
            let directory = path.is_dir();
            files.insert(
                path.strip_prefix(root).unwrap().to_owned(),
                if directory {
                    None
                } else {
                    Some(fs::read(&path).unwrap())
                },
            );
            if directory {
                visit(root, &path, files);
            }
        }
    }
    let mut files = BTreeMap::new();
    visit(root, root, &mut files);
    files
}

fn catalog(paths: &Paths, name: &str, contents: &str) {
    let directory = paths.catalog(name);
    fs::create_dir_all(&directory).unwrap();
    // No network: initialize a local checkout and configure a deliberately remote URL.
    // Inventory must only inspect that string, never contact its origin.
    let url = format!("https://example.invalid/{name}.git");
    // Inventory needs neither a branch name nor a commit. Keep fixture creation
    // compatible with Git versions that can perform the actual read queries.
    for args in [vec!["init"], vec!["config", "remote.origin.url", &url]] {
        let output = Command::new("git")
            .arg("-C")
            .arg(&directory)
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    fs::write(paths.catalog_file(name), contents).unwrap();
    let mut local = config::load(&paths.config()).unwrap();
    local
        .catalogs
        .insert(name.into(), CatalogSource::new(url, false));
    fs::write(paths.config(), toml::to_string(&local).unwrap()).unwrap();
}

fn observe(context: &mut OperationContext<'_>) -> Arc<Mutex<Vec<Vec<String>>>> {
    context.process.terminal = false;
    let commands = Arc::new(Mutex::new(Vec::new()));
    let received = commands.clone();
    context.process.observer = Some(Arc::new(move |event| {
        if let Event::Starting {
            program, arguments, ..
        } = event
        {
            assert_eq!(
                program, "git",
                "inventory must not spawn Loadbot, Rot, or a runner"
            );
            let args: Vec<_> = arguments
                .iter()
                .map(|arg| arg.to_str().unwrap().to_owned())
                .collect();
            assert_eq!(args[0], "-C");
            assert!(
                args[2..] == ["rev-parse", "--show-toplevel"]
                    || args[2..] == ["config", "--get", "remote.origin.url"],
                "unexpected inventory process: {args:?}"
            );
            received.lock().unwrap().push(args);
        }
    }));
    commands
}

#[test]
fn missing_optional_files_are_empty_and_create_nothing() {
    let root = tempfile::tempdir().unwrap();
    let before = snapshot(root.path());
    let mut policy = Unattended;
    let mut context = OperationContext::new(&mut policy);
    let commands = observe(&mut context);
    assert!(
        launcher::read_project_inventory(&paths(root.path()), &mut context)
            .unwrap()
            .is_empty()
    );
    assert!(commands.lock().unwrap().is_empty());
    assert_eq!(snapshot(root.path()), before);
}

#[test]
fn local_read_matches_launcher_semantics_and_the_shared_json_contract_without_writes() {
    let root = tempfile::tempdir().unwrap();
    let paths = paths(root.path());
    catalog(
        &paths,
        "alpha",
        r#"version = 1
[tools.demo]
type = "git"
url = "https://example.invalid/tool.git"
[tools.demo.commands.inspect]
path = "recipes/inspect file.py"
description = "Inspect π data"
runner = "python"
[tools.no-commands]
type = "git"
url = "https://example.invalid/unused.git"
"#,
    );
    catalog(
        &paths,
        "beta",
        r#"version = 1
[tools.demo]
type = "git"
url = "https://example.invalid/another-tool.git"
[tools.demo.commands.inspect]
path = "scripts/inspect.ps1"
runner = "powershell"
"#,
    );
    let shortcuts = paths.shortcuts().unwrap();
    fs::create_dir_all(shortcuts.parent().unwrap()).unwrap();
    fs::write(&shortcuts, "version = 1\n[shortcuts.inspect]\ncatalog = 'alpha'\ntool = 'demo'\npath = 'recipes/inspect file.py'\n").unwrap();
    let before = snapshot(root.path());
    let mut policy = Unattended;
    let mut context = OperationContext::new(&mut policy);
    let commands = observe(&mut context);
    let actual = launcher::read_project_inventory(&paths, &mut context).unwrap();
    let tools = operations::all_tools(&paths, &mut context).unwrap();
    assert_eq!(
        actual,
        launcher::project_inventory(&tools, &loadbot::shortcuts::load(&shortcuts).unwrap())
    );
    let expected: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/gui-inventory.json")).unwrap();
    assert_eq!(serde_json::to_value(&actual).unwrap(), expected);
    assert!(!commands.lock().unwrap().is_empty());
    assert_eq!(
        snapshot(root.path()),
        before,
        "even lock sidecars or Git metadata writes are forbidden"
    );
    assert!(
        !paths.tools().exists(),
        "inventory must not install or inspect tool contents"
    );
}

#[test]
fn qualified_project_identity_resolves_the_existing_managed_directory_without_writes() {
    let root = tempfile::tempdir().unwrap();
    let paths = paths(root.path());
    catalog(
        &paths,
        "alpha",
        "version = 1\n[tools.demo]\ntype = 'git'\nurl = 'https://example.invalid/tool.git'\n[tools.demo.commands.inspect]\npath = 'inspect.sh'\n",
    );
    let project = paths.tool("alpha", "demo").unwrap();
    fs::create_dir_all(&project).unwrap();
    for args in [
        vec!["init"],
        vec![
            "config",
            "remote.origin.url",
            "https://example.invalid/tool.git",
        ],
    ] {
        let output = Command::new("git")
            .arg("-C")
            .arg(&project)
            .args(args)
            .output()
            .unwrap();
        assert!(output.status.success());
    }
    let before = snapshot(root.path());
    let mut policy = Unattended;
    let mut context = OperationContext::new(&mut policy);

    assert_eq!(
        launcher::resolve_project_directory(&paths, "alpha", "demo", &mut context).unwrap(),
        fs::canonicalize(&project).unwrap()
    );
    assert!(launcher::resolve_project_directory(&paths, "beta", "demo", &mut context).is_err());
    assert_eq!(snapshot(root.path()), before);
}

#[test]
fn skipped_catalog_is_an_explicit_failure_not_a_partial_or_empty_success() {
    let root = tempfile::tempdir().unwrap();
    let paths = paths(root.path());
    catalog(
        &paths,
        "healthy",
        "version = 1\n[tools.demo]\ntype = 'git'\nurl = 'unused'\n[tools.demo.commands.inspect]\npath = 'inspect.sh'\n",
    );
    let mut local = config::load(&paths.config()).unwrap();
    local
        .catalogs
        .insert("missing".into(), CatalogSource::new("unused".into(), false));
    fs::write(paths.config(), toml::to_string(&local).unwrap()).unwrap();
    let before = snapshot(root.path());
    let mut policy = Unattended;
    let mut context = OperationContext::new(&mut policy);
    observe(&mut context);
    // This is the existing library's meaningful partial-result behavior.
    assert_eq!(
        operations::all_tools(&paths, &mut context).unwrap().len(),
        1
    );
    assert!(
        context
            .notices
            .iter()
            .any(|n| matches!(n, Notice::SkippedCatalog { name, .. } if name == "missing"))
    );
    let report = context.run(|context| launcher::read_project_inventory(&paths, context));
    assert!(
        report
            .result
            .unwrap_err()
            .to_string()
            .contains("catalog 'missing': catalog 'missing' is not installed")
    );
    assert_eq!(snapshot(root.path()), before);
    // Old notices from a prior operation must not poison a later complete read.
    local.catalogs.remove("missing");
    fs::write(paths.config(), toml::to_string(&local).unwrap()).unwrap();
    assert_eq!(
        launcher::read_project_inventory(&paths, &mut context)
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn malformed_config_shortcuts_and_catalogs_fail_without_repair_or_fallback() {
    let root = tempfile::tempdir().unwrap();
    let paths = paths(root.path());
    fs::create_dir_all(paths.config().parent().unwrap()).unwrap();
    let mut policy = Unattended;
    let mut context = OperationContext::new(&mut policy);
    observe(&mut context);
    fs::write(paths.config(), "not toml").unwrap();
    let before = snapshot(root.path());
    assert!(launcher::read_project_inventory(&paths, &mut context).is_err());
    assert_eq!(snapshot(root.path()), before);
    fs::write(
        paths.config(),
        toml::to_string(&LocalConfig::default()).unwrap(),
    )
    .unwrap();
    let shortcuts = paths.shortcuts().unwrap();
    fs::create_dir_all(shortcuts.parent().unwrap()).unwrap();
    fs::write(&shortcuts, "version = 99").unwrap();
    let before = snapshot(root.path());
    assert!(
        launcher::read_project_inventory(&paths, &mut context)
            .unwrap_err()
            .to_string()
            .contains("unsupported shortcut version")
    );
    assert_eq!(snapshot(root.path()), before);
    fs::remove_file(&shortcuts).unwrap();
    // A directory in place of a document is unreadable as text on both platforms.
    fs::create_dir(&shortcuts).unwrap();
    assert!(launcher::read_project_inventory(&paths, &mut context).is_err());
    fs::remove_dir(&shortcuts).unwrap();
    catalog(&paths, "broken", "version = 42");
    let before = snapshot(root.path());
    assert!(
        launcher::read_project_inventory(&paths, &mut context)
            .unwrap_err()
            .to_string()
            .contains("catalog 'broken'")
    );
    assert_eq!(snapshot(root.path()), before);
}

#[test]
fn broken_personal_references_remain_inventory_without_claiming_launchability() {
    let root = tempfile::tempdir().unwrap();
    let paths = paths(root.path());
    let file = paths.shortcuts().unwrap();
    fs::create_dir_all(file.parent().unwrap()).unwrap();
    fs::write(&file, "version = 1\n[shortcuts.orphan]\ncatalog = 'unregistered'\ntool = 'missing'\npath = 'missing.sh'\nrunner = 'bash'\n").unwrap();
    let before = snapshot(root.path());
    let mut policy = Unattended;
    let mut context = OperationContext::new(&mut policy);
    let commands = observe(&mut context);
    let inventory = launcher::read_project_inventory(&paths, &mut context).unwrap();
    assert_eq!(inventory[0].catalog, "unregistered");
    assert_eq!(inventory[0].tool, "missing");
    assert_eq!(inventory[0].entries[0].source, EntrySource::Personal);
    assert!(commands.lock().unwrap().is_empty());
    assert_eq!(snapshot(root.path()), before);
}
