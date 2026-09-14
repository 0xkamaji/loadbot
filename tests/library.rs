//! Public API tests use only normal library dependencies. They can also be run
//! directly with rustc when the environment cannot fetch Cargo's dev dependencies.
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use loadbot::{
    catalog, config, git,
    interaction::{Notice, OperationContext, Unattended},
    launcher, operations,
    paths::Paths,
    shortcuts,
};

static NEXT: AtomicU64 = AtomicU64::new(0);
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "loadbot-library-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        Self(root)
    }
    fn paths(&self) -> Paths {
        Paths::with_directories(self.0.join("data"), self.0.join("configuration")).unwrap()
    }
    fn repository(&self, name: &str) -> PathBuf {
        let path = self.0.join(name);
        fs::create_dir(&path).unwrap();
        git_command(&path, &["init", "--initial-branch", "main"]);
        git_command(&path, &["config", "user.name", "Loadbot Test"]);
        git_command(&path, &["config", "user.email", "test@example.test"]);
        path
    }
    fn installed_catalog(&self) -> Paths {
        let source = self.repository("catalog-source");
        catalog::save(
            &source.join("catalog.toml"),
            &catalog::CatalogFile::default(),
        )
        .unwrap();
        git_command(&source, &["add", "catalog.toml"]);
        git_command(&source, &["commit", "-m", "initial"]);
        let paths = self.paths();
        let mut policy = Unattended;
        let mut context = OperationContext::new(&mut policy);
        let report = context.run(|context| {
            operations::catalog_add(
                &paths,
                "personal",
                source.display().to_string(),
                true,
                context,
            )
        });
        let outcome = report.result.unwrap();
        assert!(
            matches!(&outcome.notices[0], Notice::CatalogRegistered { name } if name == "personal")
        );
        assert!(
            matches!(&report.notices[0], Notice::CatalogRegistered { name } if name == "personal")
        );
        assert!(
            matches!(&report.notices[1], Notice::CatalogInstalled { name, .. } if name == "personal")
        );
        paths
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn git_command(path: &Path, args: &[&str]) {
    let output = Command::new("git")
        .arg("-C")
        .arg(path)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn empty_public_queries_need_no_terminal_and_create_nothing() {
    let fixture = Fixture::new();
    let paths = fixture.paths();
    let mut policy = Unattended;
    let mut context = OperationContext::new(&mut policy);
    assert!(
        operations::catalog_list(&paths, &mut context)
            .unwrap()
            .is_empty()
    );
    assert!(
        operations::tool_list(&paths, &mut context)
            .unwrap()
            .is_empty()
    );
    assert!(
        shortcuts::load(&paths.shortcuts().unwrap())
            .unwrap()
            .shortcuts
            .is_empty()
    );
    assert!(!paths.config().exists());
    assert!(!paths.shortcuts().unwrap().parent().unwrap().exists());
    assert_eq!(fs::read_dir(&fixture.0).unwrap().count(), 0);
}

#[test]
fn public_operations_return_structured_paths_status_and_shortcuts() {
    let fixture = Fixture::new();
    let paths = fixture.installed_catalog();
    let source = fixture.repository("tool-source");
    fs::write(source.join("README.md"), "tool").unwrap();
    git_command(&source, &["add", "."]);
    git_command(&source, &["commit", "-m", "initial"]);
    let mut policy = Unattended;
    let mut context = OperationContext::new(&mut policy);
    operations::tool_add(
        &paths,
        "demo",
        "invalid",
        "".into(),
        None,
        false,
        false,
        &mut context,
    )
    .unwrap_err();
    operations::tool_add(
        &paths,
        "personal",
        "demo",
        source.display().to_string(),
        None,
        false,
        false,
        &mut context,
    )
    .unwrap();
    let before = operations::tool_status(&paths, "demo", Some("personal"), &mut context).unwrap();
    assert!(!before.installed);
    assert!(before.repository.is_none());
    assert_eq!(
        before.path,
        operations::tool_path(&paths, "demo", Some("personal"), &mut context).unwrap()
    );
    operations::tool_pull(&paths, "demo", Some("personal"), &mut context).unwrap();
    let after = operations::tool_status(&paths, "demo", Some("personal"), &mut context).unwrap();
    assert!(after.installed);
    assert_eq!(after.repository.unwrap().branch.as_deref(), Some("main"));
    let shortcut =
        shortcuts::Shortcut::new("personal".into(), "demo".into(), "README.md".into()).unwrap();
    let file = paths.shortcuts().unwrap();
    shortcuts::save(&file, "demo", shortcut.clone()).unwrap();
    assert_eq!(shortcuts::load(&file).unwrap().shortcuts["demo"], shortcut);
    shortcuts::remove(&file, "demo").unwrap();
    assert!(file.is_file());
    assert!(shortcuts::load(&file).unwrap().shortcuts.is_empty());
}

#[test]
fn reports_retain_warnings_and_partial_success_on_failure() {
    let fixture = Fixture::new();
    let paths = fixture.installed_catalog();
    let mut local = config::load(&paths.config()).unwrap();
    local.catalogs.insert(
        "missing".into(),
        config::CatalogSource::new("/missing".into(), false),
    );
    config::save(&paths.config(), &local).unwrap();
    let mut policy = Unattended;
    let mut context = OperationContext::new(&mut policy);
    let report = context.run(|context| operations::resolve_tool(&paths, "absent", None, context));
    assert!(
        report
            .result
            .unwrap_err()
            .to_string()
            .contains("not configured")
    );
    assert!(
        matches!(&report.notices[0], Notice::SkippedCatalog { name, diagnostic } if name == "missing" && diagnostic.contains("not installed"))
    );
    // Force Git's staging step to fail independently of machine/CI identity settings.
    fs::write(
        paths.catalog("personal").join(".git/index.lock"),
        "test lock",
    )
    .unwrap();
    let report = context.run(|context| {
        operations::tool_add(
            &paths,
            "personal",
            "saved",
            "local.git".into(),
            None,
            true,
            false,
            context,
        )
    });
    assert!(report.result.is_err());
    assert!(
        report
            .notices
            .iter()
            .any(|notice| matches!(notice, Notice::ToolAdded { name, .. } if name == "saved"))
    );
    assert!(
        catalog::load(&paths.catalog_file("personal"))
            .unwrap()
            .tools
            .contains_key("saved")
    );
}

#[test]
fn identity_decisions_are_typed_and_cancellation_is_preserved() {
    struct Choose(Option<usize>);
    impl loadbot::interaction::Interaction for Choose {
        fn can_choose(&self) -> bool {
            true
        }
        fn choose_identity(
            &mut self,
            identities: &[git::RotIdentity],
        ) -> anyhow::Result<Option<usize>> {
            assert_eq!(identities[1].alias, "github-work");
            Ok(self.0)
        }
    }
    let identities = vec![
        git::RotIdentity {
            alias: "github-home".into(),
            username: Some("home".into()),
            verification: "verified".into(),
        },
        git::RotIdentity {
            alias: "github-work".into(),
            username: Some("work".into()),
            verification: "verified".into(),
        },
    ];
    assert_eq!(
        git::select_verified_rot_identity(identities.clone(), &mut Choose(Some(1)))
            .unwrap()
            .alias,
        "github-work"
    );
    assert!(
        git::select_verified_rot_identity(identities.clone(), &mut Choose(None))
            .unwrap_err()
            .to_string()
            .contains("cancelled")
    );
    assert!(git::select_verified_rot_identity(identities, &mut Choose(Some(99))).is_err());
}

#[test]
fn shell_launchers_keep_spaces_working_directory_and_exit_code() {
    let fixture = Fixture::new();
    let root = fixture.0.join("tool with spaces");
    let scripts = root.join("scripts with spaces");
    fs::create_dir_all(&scripts).unwrap();
    let relative = Path::new("scripts with spaces").join("-run with spaces.sh");
    fs::write(
        root.join(&relative),
        "printf 'child stdout\\n'\nprintf 'child stderr\\n' >&2\nprintf 'ran' > cwd-marker\nexit 7\n",
    )
    .unwrap();
    // safe_target produces the verbatim Windows path that previously failed.
    let target = launcher::safe_target(&root, &relative).unwrap();
    for runner in [None, Some(catalog::Runner::Sh), Some(catalog::Runner::Bash)] {
        let error = match runner {
            None => launcher::launch_file(&target),
            Some(runner) => launcher::launch_with_runner(&target, &root, runner),
        }
        .unwrap_err();
        assert_eq!(
            error
                .downcast_ref::<launcher::ChildExit>()
                .unwrap_or_else(|| panic!("{runner:?}: {error:#}"))
                .code(),
            7
        );
        let expected_cwd = if runner.is_none() { &scripts } else { &root };
        let other_cwd = if runner.is_none() { &root } else { &scripts };
        assert_eq!(
            fs::read_to_string(expected_cwd.join("cwd-marker")).unwrap(),
            "ran"
        );
        assert!(!other_cwd.join("cwd-marker").exists());
        fs::remove_file(expected_cwd.join("cwd-marker")).unwrap();
    }
}

fn binary() -> PathBuf {
    option_env!("CARGO_BIN_EXE_loadbot")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("LOADBOT_TEST_BINARY").map(PathBuf::from))
        .expect("Cargo binary or LOADBOT_TEST_BINARY")
}
fn cli(fixture: &Fixture, args: &[&str]) -> std::process::Output {
    Command::new(binary())
        .env("LOADBOT_HOME", fixture.0.join("data"))
        .env("XDG_CONFIG_HOME", fixture.0.join("configuration-root"))
        .env("APPDATA", fixture.0.join("configuration-root"))
        .env(
            "LOADBOT_CONFIG_HOME",
            fixture.0.join("configuration-root/loadbot"),
        )
        .args(args)
        .output()
        .unwrap()
}

#[test]
fn cli_read_rendering_and_rot_json_remain_compatible() {
    let fixture = Fixture::new();
    let paths = fixture.installed_catalog();
    let mut policy = Unattended;
    let mut context = OperationContext::new(&mut policy);
    operations::tool_add(
        &paths,
        "personal",
        "demo",
        "missing.git".into(),
        None,
        false,
        false,
        &mut context,
    )
    .unwrap();
    let listed = cli(&fixture, &["list"]);
    assert!(listed.status.success());
    assert_eq!(
        String::from_utf8(listed.stdout).unwrap(),
        "demo\n  catalog  personal\n  type     git\n  state    missing\n"
    );
    assert!(listed.stderr.is_empty());
    let path = paths.tool("personal", "demo").unwrap();
    assert_eq!(
        String::from_utf8(cli(&fixture, &["path", "demo"]).stdout).unwrap(),
        format!("{}\n", path.display())
    );
    assert_eq!(
        String::from_utf8(cli(&fixture, &["status", "demo"]).stdout).unwrap(),
        format!(
            "Name: demo\nCatalog: personal\nPath: {}\nInstalled: no\nCatalog fetch URL: missing.git\nConfigured revision: (default)\nCurrent branch: -\nCurrent commit: -\nWorking tree: -\nFetch URL: -\nPush URL: -\n",
            path.display()
        )
    );
    assert_eq!(
        cli(&fixture, &["rot", "complete", "shortcut", ""]).stdout,
        b"[\"add\",\"list\",\"remove\"]\n"
    );
    let file = fixture.0.join("configuration-root/loadbot/shortcuts.toml");
    let mut shortcut =
        shortcuts::Shortcut::new("personal".into(), "demo".into(), "missing.sh".into()).unwrap();
    shortcut.description = Some("Description".into());
    shortcut.runner = Some(catalog::Runner::Sh);
    shortcuts::save(&file, "demo", shortcut).unwrap();
    assert_eq!(cli(&fixture, &["shortcut", "list"]).stdout, b"Name: demo\nCatalog: personal\nTool: demo\nPath: missing.sh\nDescription: Description\nRunner: sh\n");
    assert_eq!(
        cli(&fixture, &["rot", "complete", "shortcut", "remove", ""]).stdout,
        b"[\"demo\"]\n"
    );
    let before = fs::read(&file).unwrap();
    for args in [vec![], vec!["shortcut"], vec!["shortcut", "remove", "demo"]] {
        let result = cli(&fixture, &args);
        assert!(!result.status.success());
        assert!(
            String::from_utf8(result.stderr)
                .unwrap()
                .contains("interactive terminal")
        );
        assert_eq!(fs::read(&file).unwrap(), before);
    }
}

#[cfg(target_os = "linux")]
#[test]
fn menus_delegate_and_cancel_without_changing_saved_definitions() {
    use std::io::Write;
    use std::process::Stdio;
    if !Command::new("script")
        .arg("--version")
        .output()
        .is_ok_and(|output| output.status.success())
    {
        eprintln!("skipping: script is unavailable");
        return;
    }
    let fixture = Fixture::new();
    let file = fixture.0.join("configuration-root/loadbot/shortcuts.toml");
    shortcuts::save(
        &file,
        "demo",
        shortcuts::Shortcut::new("personal".into(), "broken".into(), "missing.sh".into()).unwrap(),
    )
    .unwrap();
    let terminal = |input: &[u8]| {
        let mut child = Command::new("script")
            .args(["-q", "-e", "-c", binary().to_str().unwrap(), "/dev/null"])
            .env("LOADBOT_HOME", fixture.0.join("data"))
            .env("XDG_CONFIG_HOME", fixture.0.join("configuration-root"))
            .env(
                "LOADBOT_CONFIG_HOME",
                fixture.0.join("configuration-root/loadbot"),
            )
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child.stdin.take().unwrap().write_all(input).unwrap();
        child.wait_with_output().unwrap()
    };
    let before = fs::read(&file).unwrap();
    let listing = terminal(b"8\n2\n");
    assert!(listing.status.success());
    let text = String::from_utf8(listing.stdout)
        .unwrap()
        .replace("\r\n", "\n");
    assert!(text.contains("8. Manage shortcuts"));
    assert!(text.contains("Shortcuts:"));
    assert!(
        text.ends_with(&String::from_utf8(cli(&fixture, &["shortcut", "list"]).stdout).unwrap())
    );
    for input in [
        b"10\n".as_slice(),
        b"q\n",
        b"8\n4\n",
        b"8\n3\nq\n",
        b"8\n3\n1\nn\n",
    ] {
        assert!(terminal(input).status.success());
        assert_eq!(fs::read(&file).unwrap(), before);
    }
    let removed = terminal(b"8\n3\n1\ny\n");
    assert!(removed.status.success());
    assert!(shortcuts::load(&file).unwrap().shortcuts.is_empty());
    assert!(!fixture.0.join("data").exists());
}

#[test]
fn saving_validates_public_records_before_touching_storage() {
    let fixture = Fixture::new();
    let existing = fixture.0.join("shortcuts.toml");
    let original = "version = 1\ncustom = 'keep'\n\n[shortcuts.old]\ncatalog = 'personal'\ntool = 'demo'\npath = 'run.sh'\nfuture = 42\n";
    fs::write(&existing, original).unwrap();
    for field in ["catalog", "tool"] {
        for direct in [true, false] {
            let mut shortcut = if direct {
                shortcuts::Shortcut {
                    catalog: if field == "catalog" {
                        "../outside"
                    } else {
                        "personal"
                    }
                    .into(),
                    tool: if field == "tool" {
                        "../outside"
                    } else {
                        "demo"
                    }
                    .into(),
                    path: "run.sh".into(),
                    description: None,
                    runner: None,
                    extra: Default::default(),
                }
            } else {
                shortcuts::Shortcut::new("personal".into(), "demo".into(), "run.sh".into()).unwrap()
            };
            if !direct {
                if field == "catalog" {
                    shortcut.catalog = "../outside".into();
                } else {
                    shortcut.tool = "../outside".into();
                }
            }
            for path in [&existing, &fixture.0.join("missing/shortcuts.toml")] {
                let error = shortcuts::save(path, "new", shortcut.clone()).unwrap_err();
                assert!(
                    error
                        .to_string()
                        .contains(&format!("invalid shortcut {field}"))
                );
                assert_eq!(fs::read(&existing).unwrap(), original.as_bytes());
                assert!(!fixture.0.join("missing").exists());
            }
        }
    }
    let valid =
        shortcuts::Shortcut::new("personal".into(), "demo".into(), "new.sh".into()).unwrap();
    shortcuts::save(&existing, "new", valid).unwrap();
    let loaded = shortcuts::load(&existing).unwrap();
    assert_eq!(loaded.extra["custom"].as_str(), Some("keep"));
    assert_eq!(
        loaded.shortcuts["old"].extra["future"].as_integer(),
        Some(42)
    );
    assert_eq!(loaded.shortcuts.len(), 2);
}

#[test]
fn public_browser_validates_directories_and_preserves_order() {
    let fixture = Fixture::new();
    let root = fixture.0.join("root");
    fs::create_dir_all(root.join("nested/deeper/z-directory")).unwrap();
    fs::write(root.join("nested/deeper/b.txt"), "b").unwrap();
    fs::write(root.join("nested/deeper/a.txt"), "a").unwrap();
    let entries = launcher::browse_directory(&root, Path::new("nested/deeper")).unwrap();
    assert_eq!(
        entries
            .iter()
            .map(|entry| entry.name.as_str())
            .collect::<Vec<_>>(),
        ["z-directory", "a.txt", "b.txt"]
    );
    assert!(entries[0].is_directory);
    assert_eq!(entries[1].path, root.join("nested/deeper/a.txt"));
    for relative in [
        Path::new("../root"),
        Path::new("nested/../../root"),
        root.as_path(),
    ] {
        assert!(launcher::browse_directory(&root, relative).is_err());
    }
}

#[cfg(any(unix, windows))]
#[test]
fn public_browser_rejects_selected_and_intermediate_directory_symlinks() {
    let fixture = Fixture::new();
    let root = fixture.0.join("root");
    let outside = fixture.0.join("outside");
    fs::create_dir(&root).unwrap();
    fs::create_dir_all(outside.join("nested")).unwrap();
    fs::write(outside.join("nested/secret.txt"), "outside").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(&outside, root.join("escape")).unwrap();
    #[cfg(windows)]
    if let Err(error) = std::os::windows::fs::symlink_dir(&outside, root.join("escape")) {
        if error.raw_os_error() == Some(1314) {
            eprintln!("symlink fixture requires Windows Developer Mode or symlink privilege");
            return;
        }
        panic!("could not create directory symlink: {error}");
    }
    assert!(
        launcher::browse_directory(&root, Path::new(""))
            .unwrap()
            .is_empty()
    );
    for relative in ["escape", "escape/nested"] {
        assert!(launcher::browse_directory(&root, Path::new(relative)).is_err());
    }
}

#[test]
fn explicit_configuration_override_isolates_cli_and_completion() {
    let fixture = Fixture::new();
    let isolated = fixture.0.join("isolated");
    let platform = fixture.0.join("platform");
    let shortcut =
        shortcuts::Shortcut::new("personal".into(), "demo".into(), "run.sh".into()).unwrap();
    shortcuts::save(
        &isolated.join("shortcuts.toml"),
        "isolated",
        shortcut.clone(),
    )
    .unwrap();
    shortcuts::save(
        &platform.join("loadbot/shortcuts.toml"),
        "platform",
        shortcut,
    )
    .unwrap();
    let before = fs::read(platform.join("loadbot/shortcuts.toml")).unwrap();
    for (args, expected) in [
        (vec!["rot", "complete", "run", ""], "[\"isolated\"]\n"),
        (
            vec!["shortcut", "remove", "isolated", "--yes"],
            "removed shortcut 'isolated'\n",
        ),
    ] {
        let output = Command::new(binary())
            .env("LOADBOT_HOME", fixture.0.join("data"))
            .env("LOADBOT_CONFIG_HOME", &isolated)
            .env("XDG_CONFIG_HOME", &platform)
            .env("APPDATA", &platform)
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(output.stdout, expected.as_bytes());
    }
    assert!(
        shortcuts::load(&isolated.join("shortcuts.toml"))
            .unwrap()
            .shortcuts
            .is_empty()
    );
    assert_eq!(
        fs::read(platform.join("loadbot/shortcuts.toml")).unwrap(),
        before
    );
}

#[test]
fn catalog_query_keeps_progress_output_before_a_later_error() {
    let fixture = Fixture::new();
    let paths = fixture.paths();
    fs::create_dir_all(paths.catalog("second")).unwrap();
    fs::write(
        paths.config(),
        "version = 1\n[catalogs.first]\nurl = 'first.git'\n[catalogs.second]\nurl = 'second.git'\n",
    )
    .unwrap();
    let empty_path = fixture.0.join("no-programs");
    fs::create_dir(&empty_path).unwrap();
    for (args, expected) in [
        (
            vec!["catalog", "list"],
            "NAME\tSTATE\tACCESS\tDEFAULT\tURL\nfirst\tmissing\tread-only\tno\tfirst.git\n"
                .to_owned(),
        ),
        (
            vec!["catalog", "status", "second"],
            format!(
                "Name: second\nPath: {}\nCatalog fetch URL: second.git\nWritable: no\n",
                paths.catalog("second").display()
            ),
        ),
    ] {
        let output = Command::new(binary())
            .env("LOADBOT_HOME", fixture.0.join("data"))
            .env("LOADBOT_CONFIG_HOME", fixture.0.join("configuration"))
            .env("PATH", &empty_path)
            .args(args)
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert_eq!(output.stdout, expected.as_bytes());
        assert!(String::from_utf8_lossy(&output.stderr).contains("Git"));
    }
}

#[test]
fn cli_owns_empty_list_wording_and_spacing_between_tools() {
    let fixture = Fixture::new();
    for (args, expected) in [
        (vec!["catalog", "list"], "no catalogs configured\n"),
        (vec!["list"], "no tools configured in registered catalogs\n"),
    ] {
        let output = cli(&fixture, &args);
        assert!(output.status.success());
        assert_eq!(output.stdout, expected.as_bytes());
        assert!(output.stderr.is_empty());
    }
    assert!(!fixture.0.join("data").exists());
    let paths = fixture.installed_catalog();
    git_command(
        &paths.catalog("personal"),
        &["config", "user.name", "Loadbot Test"],
    );
    git_command(
        &paths.catalog("personal"),
        &["config", "user.email", "test@example.test"],
    );
    let mut policy = Unattended;
    let mut context = OperationContext::new(&mut policy);
    for name in ["alpha", "beta"] {
        operations::tool_add(
            &paths,
            "personal",
            name,
            "missing.git".into(),
            None,
            true,
            false,
            &mut context,
        )
        .unwrap();
    }
    let output = cli(&fixture, &["list"]);
    assert!(output.status.success());
    assert_eq!(output.stdout, b"alpha\n  catalog  personal\n  type     git\n  state    missing\n\nbeta\n  catalog  personal\n  type     git\n  state    missing\n");
    assert!(output.stderr.is_empty());
}
