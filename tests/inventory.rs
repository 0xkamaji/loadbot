//! The exact GUI read projection, exercised without Tauri or a user's real state.
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};

use loadbot::{
    catalog::Runner,
    config::{self, CatalogSource, LocalConfig},
    interaction::{Notice, OperationContext, Unattended},
    launcher::{self, EntrySource},
    operations,
    paths::Paths,
    process::Event,
    recipe::{
        InvocationBehavior, RecipeArgument, RecipeDefinition, RecipeProgram, StoredInvocation,
        WorkingDirectory,
    },
    shortcuts::{self, Shortcut},
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

fn installed_tool(paths: &Paths, catalog: &str, tool: &str, url: &str) -> PathBuf {
    let directory = paths.tool(catalog, tool).unwrap();
    fs::create_dir_all(&directory).unwrap();
    for args in [vec!["init"], vec!["config", "remote.origin.url", url]] {
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
    directory
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
fn inventory_exposes_recipe_definitions_without_legacy_path_or_runner_fields() {
    let root = tempfile::tempdir().unwrap();
    let paths = paths(root.path());
    catalog(
        &paths,
        "recipes",
        r#"version = 1
[tools.demo]
type = "git"
url = "https://example.invalid/tool.git"
[tools.demo.commands.triage]
description = "Triage"
[tools.demo.commands.triage.recipe]
version = 1
behavior = "run"
program = { type = "interpreter", runner = "python" }
working_directory = { type = "project-root" }
[[tools.demo.commands.triage.recipe.arguments]]
type = "project-path"
path = "triage.py"
"#,
    );
    let mut policy = Unattended;
    let mut context = OperationContext::new(&mut policy);
    let projects = launcher::read_project_inventory(&paths, &mut context).unwrap();
    let value = serde_json::to_value(&projects[0].entries[0]).unwrap();
    assert!(value.get("path").is_none());
    assert!(value.get("runner").is_none());
    assert_eq!(value["recipe"]["version"], 1);
    assert_eq!(value["recipe"]["behavior"], "run");
    assert_eq!(value["recipe"]["arguments"][0]["type"], "project-path");
}

#[test]
fn existing_execution_entry_points_refuse_both_recipe_behaviors_without_spawning() {
    let root = tempfile::tempdir().unwrap();
    let paths = paths(root.path());
    let shortcut_path = paths.shortcuts().unwrap();
    fs::create_dir_all(shortcut_path.parent().unwrap()).unwrap();
    for behavior in [InvocationBehavior::Run, InvocationBehavior::Launch] {
        let name = match behavior {
            InvocationBehavior::Run => "run-recipe",
            InvocationBehavior::Launch => "launch-recipe",
        };
        let recipe = RecipeDefinition {
            version: 1,
            behavior,
            program: RecipeProgram::Executable {
                name: "never-started".into(),
            },
            working_directory: WorkingDirectory::ProjectRoot,
            arguments: vec![],
        };
        shortcuts::save(
            &shortcut_path,
            name,
            Shortcut::with_invocation(
                "personal".into(),
                "demo".into(),
                StoredInvocation::Recipe(recipe),
            )
            .unwrap(),
        )
        .unwrap();
    }
    let mut policy = Unattended;
    let mut context = OperationContext::new(&mut policy);
    let starts = Arc::new(Mutex::new(Vec::new()));
    let received = starts.clone();
    context.process.observer = Some(Arc::new(move |event| {
        if let Event::Starting { program, .. } = event {
            received.lock().unwrap().push(program);
        }
    }));
    for name in ["run-recipe", "launch-recipe"] {
        let error =
            launcher::run_shortcut_from(&paths, &shortcut_path, name, &mut context).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("Recipe execution is not implemented")
        );
    }
    assert!(starts.lock().unwrap().is_empty());
    assert!(!paths.tools().exists());
}

#[test]
fn pre_gui_management_installation_is_shared_by_catalog_and_inventory_queries() {
    let root = tempfile::tempdir().unwrap();
    let paths = paths(root.path());
    // This is the CLI-supported configuration/catalog format that predates the
    // Phase 4B GUI operations. Do not use catalog_add/tool_add to construct it.
    catalog(
        &paths,
        "existing",
        r#"version = 1
[tools.known-project]
type = "git"
url = "https://example.invalid/known-project.git"
[tools.known-project.commands.known-shortcut]
path = "scripts/known.sh"
description = "Existing shortcut"
runner = "sh"
"#,
    );
    let mut local = config::load(&paths.config()).unwrap();
    local.default_catalog = Some("existing".into());
    config::save(&paths.config(), &local).unwrap();

    let before = snapshot(root.path());
    let mut policy = Unattended;
    let mut context = OperationContext::new(&mut policy);
    let catalogs = operations::catalog_list(&paths, &mut context).unwrap();
    let inventory = launcher::read_project_inventory(&paths, &mut context).unwrap();

    assert_eq!(catalogs.len(), 1);
    assert_eq!(catalogs[0].name, "existing");
    assert!(catalogs[0].default);
    assert_eq!(catalogs[0].state, operations::CatalogState::Installed);
    assert_eq!(inventory.len(), 1);
    assert_eq!(inventory[0].catalog, "existing");
    assert_eq!(inventory[0].tool, "known-project");
    assert_eq!(inventory[0].entries[0].name, "known-shortcut");
    assert_eq!(
        snapshot(root.path()),
        before,
        "startup queries must not migrate or rewrite existing state"
    );
}

#[test]
fn shared_shortcut_add_validates_project_target_duplicates_and_persists_atomically() {
    let root = tempfile::tempdir().unwrap();
    let paths = paths(root.path());
    let url = "https://example.invalid/tool.git";
    catalog(
        &paths,
        "alpha",
        &format!("version = 1\n[tools.demo]\ntype = 'git'\nurl = '{url}'\n"),
    );
    let project = installed_tool(&paths, "alpha", "demo", url);
    fs::create_dir_all(project.join("scripts")).unwrap();
    fs::write(project.join("scripts/run.py"), "print('safe')\n").unwrap();
    let mut policy = Unattended;
    let mut context = OperationContext::new(&mut policy);

    let created = operations::shortcut_add(
        &paths,
        "alpha",
        "demo",
        "run",
        "scripts/run.py",
        Some("Run it".into()),
        Some(Runner::Python),
        &mut context,
    )
    .unwrap();
    assert_eq!(created.name, "run");
    let saved = loadbot::shortcuts::load(&paths.shortcuts().unwrap()).unwrap();
    assert_eq!(
        saved.shortcuts["run"].description.as_deref(),
        Some("Run it")
    );
    assert_eq!(
        saved.shortcuts["run"].legacy().unwrap().runner,
        Some(Runner::Python)
    );
    let before = fs::read(paths.shortcuts().unwrap()).unwrap();

    for (name, target, expected) in [
        ("run", "scripts/run.py", "already exists"),
        ("unsafe", "../outside", "must not escape"),
        ("missing", "scripts/missing.py", "missing or is not a file"),
    ] {
        let error = operations::shortcut_add(
            &paths,
            "alpha",
            "demo",
            name,
            target,
            None,
            None,
            &mut context,
        )
        .unwrap_err();
        assert!(format!("{error:#}").contains(expected), "{error:#}");
        assert_eq!(fs::read(paths.shortcuts().unwrap()).unwrap(), before);
    }
}

#[test]
fn recipe_shortcut_create_and_update_are_atomic_and_never_migrate_legacy_entries() {
    let root = tempfile::tempdir().unwrap();
    let paths = paths(root.path());
    let url = "https://example.invalid/tool.git";
    catalog(
        &paths,
        "alpha",
        &format!("version = 1\n[tools.demo]\ntype = 'git'\nurl = '{url}'\n"),
    );
    let project = installed_tool(&paths, "alpha", "demo", url);
    fs::create_dir_all(project.join("scripts")).unwrap();
    fs::write(project.join("scripts/tool.py"), "print('safe')\n").unwrap();
    let mut policy = Unattended;
    let mut context = OperationContext::new(&mut policy);
    let recipe = RecipeDefinition {
        version: 1,
        behavior: InvocationBehavior::Run,
        program: RecipeProgram::Interpreter {
            runner: Runner::Python,
        },
        working_directory: WorkingDirectory::ProjectRoot,
        arguments: vec![
            RecipeArgument::ProjectPath {
                path: "scripts/tool.py".into(),
            },
            RecipeArgument::Input {
                id: "format".into(),
                label: "Format".into(),
                kind: loadbot::recipe::InputKind::Text,
                required: false,
                default: Some("text".into()),
                prefix: Some("--format".into()),
            },
            RecipeArgument::Switch {
                id: "verbose".into(),
                label: "Verbose".into(),
                value: "--verbose".into(),
                default: false,
            },
        ],
    };

    operations::shortcut_add_recipe(
        &paths,
        "alpha",
        "demo",
        "recipe",
        Some("before".into()),
        recipe.clone(),
        &mut context,
    )
    .unwrap();
    let after_recipe_create = fs::read(paths.shortcuts().unwrap()).unwrap();
    let duplicate = operations::shortcut_add_recipe(
        &paths,
        "alpha",
        "demo",
        "recipe",
        None,
        recipe.clone(),
        &mut context,
    )
    .unwrap_err();
    assert!(format!("{duplicate:#}").contains("already exists"));
    assert_eq!(
        fs::read(paths.shortcuts().unwrap()).unwrap(),
        after_recipe_create
    );
    operations::shortcut_add(
        &paths,
        "alpha",
        "demo",
        "legacy",
        "scripts/tool.py",
        None,
        Some(Runner::Python),
        &mut context,
    )
    .unwrap();
    operations::shortcut_add_recipe(
        &paths,
        "alpha",
        "demo",
        "launch-app",
        None,
        RecipeDefinition {
            version: 1,
            behavior: InvocationBehavior::Launch,
            program: RecipeProgram::ProjectFile {
                path: "scripts/tool.py".into(),
            },
            working_directory: WorkingDirectory::TargetParent,
            arguments: vec![],
        },
        &mut context,
    )
    .unwrap();
    let mut updated = recipe;
    updated.behavior = InvocationBehavior::Launch;
    updated.program = RecipeProgram::Executable {
        name: "tool-runner".into(),
    };
    updated.working_directory = WorkingDirectory::ProjectRelative {
        path: "scripts".into(),
    };
    updated.arguments = vec![
        RecipeArgument::Switch {
            id: "verbose".into(),
            label: "Detailed output".into(),
            value: "--verbose".into(),
            default: true,
        },
        RecipeArgument::Literal {
            value: "one value".into(),
        },
        RecipeArgument::ProjectPath {
            path: "scripts/tool.py".into(),
        },
    ];
    operations::shortcut_update_recipe(
        &paths,
        "alpha",
        "demo",
        "recipe",
        Some("after".into()),
        updated.clone(),
        &mut context,
    )
    .unwrap();

    let saved = shortcuts::load(&paths.shortcuts().unwrap()).unwrap();
    assert_eq!(
        saved.shortcuts["recipe"].invocation.as_recipe(),
        Some(&updated)
    );
    assert_eq!(
        saved.shortcuts["recipe"].description.as_deref(),
        Some("after")
    );
    assert_eq!(
        saved.shortcuts["legacy"].legacy().unwrap().runner,
        Some(Runner::Python)
    );
    assert_eq!(
        saved.shortcuts["launch-app"]
            .invocation
            .as_recipe()
            .unwrap()
            .behavior,
        InvocationBehavior::Launch
    );
    let serialized = fs::read_to_string(paths.shortcuts().unwrap()).unwrap();
    let recipe_value: toml::Value = toml::from_str(&serialized).unwrap();
    assert!(recipe_value["shortcuts"]["recipe"].get("path").is_none());
    assert!(recipe_value["shortcuts"]["recipe"].get("runner").is_none());

    let before = fs::read(paths.shortcuts().unwrap()).unwrap();
    let error = operations::shortcut_update_recipe(
        &paths,
        "alpha",
        "demo",
        "legacy",
        None,
        updated,
        &mut context,
    )
    .unwrap_err();
    assert!(format!("{error:#}").contains("cannot be edited as a Recipe"));
    assert_eq!(fs::read(paths.shortcuts().unwrap()).unwrap(), before);

    for (name, invalid) in [
        (
            "invalid-program",
            RecipeDefinition {
                version: 1,
                behavior: InvocationBehavior::Run,
                program: RecipeProgram::Executable {
                    name: "cargo run".into(),
                },
                working_directory: WorkingDirectory::ProjectRoot,
                arguments: vec![],
            },
        ),
        (
            "missing-path",
            RecipeDefinition {
                version: 1,
                behavior: InvocationBehavior::Run,
                program: RecipeProgram::ProjectFile {
                    path: "scripts/missing.py".into(),
                },
                working_directory: WorkingDirectory::ProjectRoot,
                arguments: vec![],
            },
        ),
    ] {
        assert!(
            operations::shortcut_add_recipe(
                &paths,
                "alpha",
                "demo",
                name,
                None,
                invalid,
                &mut context,
            )
            .is_err()
        );
        assert_eq!(fs::read(paths.shortcuts().unwrap()).unwrap(), before);
    }
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
