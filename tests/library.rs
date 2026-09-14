//! Public API tests use only normal library dependencies. They can also be run
//! directly with rustc when the environment cannot fetch Cargo's dev dependencies.
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use loadbot::{catalog, config, git, interaction::{Notice, OperationContext, Unattended}, operations, paths::Paths, shortcuts};

#[cfg(unix)]
use loadbot::launcher;

static NEXT: AtomicU64 = AtomicU64::new(0);
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("loadbot-library-{}-{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos(), NEXT.fetch_add(1, Ordering::Relaxed)));
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
        catalog::save(&source.join("catalog.toml"), &catalog::CatalogFile::default()).unwrap();
        git_command(&source, &["add", "catalog.toml"]);
        git_command(&source, &["commit", "-m", "initial"]);
        let paths = self.paths();
        let mut policy = Unattended;
        let mut context = OperationContext::new(&mut policy);
        let report = context.run(|context| operations::catalog_add(&paths, "personal", source.display().to_string(), true, context));
        let outcome = report.result.unwrap();
        assert!(matches!(&outcome.notices[0], Notice::CatalogRegistered { name } if name == "personal"));
        assert!(matches!(&report.notices[0], Notice::CatalogRegistered { name } if name == "personal"));
        assert!(matches!(&report.notices[1], Notice::CatalogInstalled { name, .. } if name == "personal"));
        paths
    }
}
impl Drop for Fixture {
    fn drop(&mut self) { let _ = fs::remove_dir_all(&self.0); }
}
fn git_command(path: &Path, args: &[&str]) {
    let output = Command::new("git").arg("-C").arg(path).args(args).output().unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
}

#[test]
fn empty_public_queries_need_no_terminal_and_create_nothing() {
    let fixture = Fixture::new();
    let paths = fixture.paths();
    let mut policy = Unattended;
    let mut context = OperationContext::new(&mut policy);
    assert!(operations::catalog_list(&paths, &mut context).unwrap().is_empty());
    assert!(operations::tool_list(&paths, &mut context).unwrap().is_empty());
    assert!(shortcuts::load(&paths.shortcuts().unwrap()).unwrap().shortcuts.is_empty());
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
    operations::tool_add(&paths, "demo", "invalid", "".into(), None, false, false, &mut context).unwrap_err();
    operations::tool_add(&paths, "personal", "demo", source.display().to_string(), None, false, false, &mut context).unwrap();
    let before = operations::tool_status(&paths, "demo", Some("personal"), &mut context).unwrap();
    assert!(!before.installed);
    assert!(before.repository.is_none());
    assert_eq!(before.path, operations::tool_path(&paths, "demo", Some("personal"), &mut context).unwrap());
    operations::tool_pull(&paths, "demo", Some("personal"), &mut context).unwrap();
    let after = operations::tool_status(&paths, "demo", Some("personal"), &mut context).unwrap();
    assert!(after.installed);
    assert_eq!(after.repository.unwrap().branch.as_deref(), Some("main"));
    let shortcut = shortcuts::Shortcut::new("personal".into(), "demo".into(), "README.md".into()).unwrap();
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
    local.catalogs.insert("missing".into(), config::CatalogSource::new("/missing".into(), false));
    config::save(&paths.config(), &local).unwrap();
    let mut policy = Unattended;
    let mut context = OperationContext::new(&mut policy);
    let report = context.run(|context| operations::resolve_tool(&paths, "absent", None, context));
    assert!(report.result.unwrap_err().to_string().contains("not configured"));
    assert!(matches!(&report.notices[0], Notice::SkippedCatalog { name, diagnostic } if name == "missing" && diagnostic.contains("not installed")));
    // Force Git's staging step to fail independently of machine/CI identity settings.
    fs::write(paths.catalog("personal").join(".git/index.lock"), "test lock").unwrap();
    let report = context.run(|context| operations::tool_add(&paths, "personal", "saved", "local.git".into(), None, true, false, context));
    assert!(report.result.is_err());
    assert!(report.notices.iter().any(|notice| matches!(notice, Notice::ToolAdded { name, .. } if name == "saved")));
    assert!(catalog::load(&paths.catalog_file("personal")).unwrap().tools.contains_key("saved"));
}

#[test]
fn identity_decisions_are_typed_and_cancellation_is_preserved() {
    struct Choose(Option<usize>);
    impl loadbot::interaction::Interaction for Choose {
        fn can_choose(&self) -> bool { true }
        fn choose_identity(&mut self, identities: &[git::RotIdentity]) -> anyhow::Result<Option<usize>> {
            assert_eq!(identities[1].alias, "github-work");
            Ok(self.0)
        }
    }
    let identities = vec![
        git::RotIdentity { alias: "github-home".into(), username: Some("home".into()), verification: "verified".into() },
        git::RotIdentity { alias: "github-work".into(), username: Some("work".into()), verification: "verified".into() },
    ];
    assert_eq!(git::select_verified_rot_identity(identities.clone(), &mut Choose(Some(1))).unwrap().alias, "github-work");
    assert!(git::select_verified_rot_identity(identities.clone(), &mut Choose(None)).unwrap_err().to_string().contains("cancelled"));
    assert!(git::select_verified_rot_identity(identities, &mut Choose(Some(99))).is_err());
}

#[cfg(unix)]
#[test]
fn launcher_keeps_child_exit_code_and_working_directory() {
    let fixture = Fixture::new();
    let root = fixture.0.join("tool");
    fs::create_dir_all(root.join("scripts")).unwrap();
    let script = root.join("scripts/run.sh");
    fs::write(&script, "pwd > cwd.txt\nprintf 'child stdout\\n'\nprintf 'child stderr\\n' >&2\nexit 7\n").unwrap();
    let error = launcher::launch_with_runner(&script, &root, catalog::Runner::Sh).unwrap_err();
    assert_eq!(error.downcast_ref::<launcher::ChildExit>().unwrap().code(), 7);
    assert_eq!(fs::read_to_string(root.join("cwd.txt")).unwrap().trim(), root.to_str().unwrap());
}

#[cfg(target_os = "linux")]
fn binary() -> PathBuf {
    option_env!("CARGO_BIN_EXE_loadbot").map(PathBuf::from)
        .or_else(|| std::env::var_os("LOADBOT_TEST_BINARY").map(PathBuf::from))
        .expect("Cargo binary or LOADBOT_TEST_BINARY")
}
#[cfg(target_os = "linux")]
fn cli(fixture: &Fixture, args: &[&str]) -> std::process::Output {
    Command::new(binary())
        .env("LOADBOT_HOME", fixture.0.join("data"))
        .env("XDG_CONFIG_HOME", fixture.0.join("configuration-root"))
        .env("APPDATA", fixture.0.join("configuration-root"))
        .args(args).output().unwrap()
}

#[cfg(target_os = "linux")]
#[test]
fn cli_read_rendering_and_rot_json_remain_compatible() {
    let fixture = Fixture::new();
    let paths = fixture.installed_catalog();
    let mut policy = Unattended;
    let mut context = OperationContext::new(&mut policy);
    operations::tool_add(&paths, "personal", "demo", "missing.git".into(), None, false, false, &mut context).unwrap();
    let listed = cli(&fixture, &["list"]);
    assert!(listed.status.success());
    assert_eq!(String::from_utf8(listed.stdout).unwrap(), "demo\n  catalog  personal\n  type     git\n  state    missing\n");
    assert!(listed.stderr.is_empty());
    let path = paths.tool("personal", "demo").unwrap();
    assert_eq!(String::from_utf8(cli(&fixture, &["path", "demo"]).stdout).unwrap(), format!("{}\n", path.display()));
    assert_eq!(String::from_utf8(cli(&fixture, &["status", "demo"]).stdout).unwrap(), format!("Name: demo\nCatalog: personal\nPath: {}\nInstalled: no\nCatalog fetch URL: missing.git\nConfigured revision: (default)\nCurrent branch: -\nCurrent commit: -\nWorking tree: -\nFetch URL: -\nPush URL: -\n", path.display()));
    assert_eq!(cli(&fixture, &["rot", "complete", "shortcut", ""]).stdout, b"[\"add\",\"list\",\"remove\"]\n");
    let file = fixture.0.join("configuration-root/loadbot/shortcuts.toml");
    let mut shortcut = shortcuts::Shortcut::new("personal".into(), "demo".into(), "missing.sh".into()).unwrap();
    shortcut.description = Some("Description".into());
    shortcut.runner = Some(catalog::Runner::Sh);
    shortcuts::save(&file, "demo", shortcut).unwrap();
    assert_eq!(cli(&fixture, &["shortcut", "list"]).stdout, b"Name: demo\nCatalog: personal\nTool: demo\nPath: missing.sh\nDescription: Description\nRunner: sh\n");
    assert_eq!(cli(&fixture, &["rot", "complete", "shortcut", "remove", ""]).stdout, b"[\"demo\"]\n");
    let before = fs::read(&file).unwrap();
    for args in [vec![], vec!["shortcut"], vec!["shortcut", "remove", "demo"]] {
        let result = cli(&fixture, &args);
        assert!(!result.status.success());
        assert!(String::from_utf8(result.stderr).unwrap().contains("interactive terminal"));
        assert_eq!(fs::read(&file).unwrap(), before);
    }
}

#[cfg(target_os = "linux")]
#[test]
fn menus_delegate_and_cancel_without_changing_saved_definitions() {
    use std::io::Write;
    use std::process::Stdio;
    if !Command::new("script").arg("--version").output().is_ok_and(|output| output.status.success()) {
        eprintln!("skipping: script is unavailable");
        return;
    }
    let fixture = Fixture::new();
    let file = fixture.0.join("configuration-root/loadbot/shortcuts.toml");
    shortcuts::save(&file, "demo", shortcuts::Shortcut::new("personal".into(), "broken".into(), "missing.sh".into()).unwrap()).unwrap();
    let terminal = |input: &[u8]| {
        let mut child = Command::new("script")
            .args(["-q", "-e", "-c", binary().to_str().unwrap(), "/dev/null"])
            .env("LOADBOT_HOME", fixture.0.join("data"))
            .env("XDG_CONFIG_HOME", fixture.0.join("configuration-root"))
            .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped())
            .spawn().unwrap();
        child.stdin.take().unwrap().write_all(input).unwrap();
        child.wait_with_output().unwrap()
    };
    let before = fs::read(&file).unwrap();
    let listing = terminal(b"8\n2\n");
    assert!(listing.status.success());
    let text = String::from_utf8(listing.stdout).unwrap().replace("\r\n", "\n");
    assert!(text.contains("8. Manage shortcuts"));
    assert!(text.contains("Shortcuts:"));
    assert!(text.ends_with(&String::from_utf8(cli(&fixture, &["shortcut", "list"]).stdout).unwrap()));
    for input in [b"10\n".as_slice(), b"q\n", b"8\n4\n", b"8\n3\nq\n", b"8\n3\n1\nn\n"] {
        assert!(terminal(input).status.success());
        assert_eq!(fs::read(&file).unwrap(), before);
    }
    let removed = terminal(b"8\n3\n1\ny\n");
    assert!(removed.status.success());
    assert!(shortcuts::load(&file).unwrap().shortcuts.is_empty());
    assert!(!fixture.0.join("data").exists());
}
