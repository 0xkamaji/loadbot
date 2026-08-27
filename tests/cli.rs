use std::ffi::{OsStr, OsString};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use tempfile::TempDir;

struct Repository {
    source: PathBuf,
    remote: PathBuf,
}

struct Fixture {
    _temporary: TempDir,
    home: PathBuf,
    catalog: Repository,
    tool: Repository,
}

impl Fixture {
    fn new() -> Option<Self> {
        if !git_available() {
            eprintln!("skipping integration test: Git is not available");
            return None;
        }
        let temporary = TempDir::new().unwrap();
        let catalog = create_repository(
            temporary.path(),
            "catalog",
            Some("version = 1\n\n[tools]\n"),
        );
        let tool = create_repository(temporary.path(), "tool", Some("initial\n"));
        Some(Self {
            home: temporary.path().join("loadbot-home"),
            _temporary: temporary,
            catalog,
            tool,
        })
    }

    fn loadbot<I, S>(&self, arguments: I) -> Output
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        Command::new(env!("CARGO_BIN_EXE_loadbot"))
            .env("LOADBOT_HOME", &self.home)
            .args(arguments)
            .output()
            .unwrap()
    }

    fn add_catalog(&self, name: &str, writable: bool) -> Output {
        let mut arguments = vec![
            OsString::from("catalog"),
            OsString::from("add"),
            OsString::from(name),
            self.catalog.remote.as_os_str().to_owned(),
        ];
        if writable {
            arguments.push(OsString::from("--writable"));
        }
        self.loadbot(arguments)
    }

    fn add_tool(&self, name: &str, catalog: &str, extra: &[&str]) -> Output {
        let mut arguments = vec![
            OsString::from("add"),
            OsString::from(name),
            self.tool.remote.as_os_str().to_owned(),
            OsString::from("--revision"),
            OsString::from("main"),
            OsString::from("--catalog"),
            OsString::from(catalog),
        ];
        arguments.extend(extra.iter().map(OsString::from));
        self.loadbot(arguments)
    }

    fn configure_catalog_identity(&self, name: &str) {
        let directory = self.home.join("catalogs").join(name);
        git(["config", "user.name", "Loadbot Tests"], Some(&directory));
        git(
            ["config", "user.email", "loadbot@example.test"],
            Some(&directory),
        );
    }
}

#[test]
fn catalog_registration_clones_and_sets_the_first_default() {
    let Some(fixture) = Fixture::new() else {
        return;
    };
    let output = fixture.add_catalog("personal", true);
    assert_success_ref(&output);

    let config = fs::read_to_string(fixture.home.join("config.toml")).unwrap();
    let config: toml::Value = toml::from_str(&config).unwrap();
    assert_eq!(config["default_catalog"].as_str(), Some("personal"));
    assert_eq!(
        config["catalogs"]["personal"]["writable"].as_bool(),
        Some(true)
    );
    assert!(fixture.home.join("catalogs/personal/.git").is_dir());
    assert!(!config.as_table().unwrap().contains_key("tools"));

    let list = fixture.loadbot(["catalog", "list"]);
    assert_success_ref(&list);
    assert!(
        stdout(&list).contains("personal\tinstalled\twritable\tyes"),
        "{}",
        stdout(&list)
    );
    let path = fixture.loadbot(["catalog", "path", "personal"]);
    assert_success_ref(&path);
    assert_eq!(
        stdout(&path),
        fixture.home.join("catalogs/personal").display().to_string()
    );
    let status = fixture.loadbot(["catalog", "status", "personal"]);
    assert_success_ref(&status);
    assert!(stdout(&status).contains("Catalog file: valid"));
}

#[test]
fn catalog_sync_fast_forwards_and_refuses_dirty_worktrees() {
    let Some(fixture) = Fixture::new() else {
        return;
    };
    assert_success(fixture.add_catalog("personal", true));
    fs::write(
        fixture.catalog.source.join("catalog.toml"),
        "version = 1\nnote = \"updated\"\n\n[tools]\n",
    )
    .unwrap();
    commit_and_push(&fixture.catalog.source, "catalog update");

    let sync = fixture.loadbot(["catalog", "sync", "personal"]);
    assert_success_ref(&sync);
    assert!(stdout(&sync).contains("synchronized catalog 'personal'"));

    fs::write(
        fixture.home.join("catalogs/personal/local.txt"),
        "do not remove\n",
    )
    .unwrap();
    let dirty = fixture.loadbot(["catalog", "sync", "personal"]);
    assert!(!dirty.status.success());
    assert!(stderr(&dirty).contains("working tree has local changes"));
    assert!(fixture.home.join("catalogs/personal/local.txt").is_file());
}

#[test]
fn catalog_url_mismatch_is_refused() {
    let Some(fixture) = Fixture::new() else {
        return;
    };
    assert_success(fixture.add_catalog("personal", false));
    let installed = fixture.home.join("catalogs/personal");
    git(
        [
            "remote",
            "set-url",
            "origin",
            "https://example.test/other.git",
        ],
        Some(&installed),
    );

    let output = fixture.loadbot(["catalog", "sync", "personal"]);
    assert!(!output.status.success());
    assert!(stderr(&output).contains("not the configured Git repository"));
}

#[test]
fn failed_catalog_registration_can_be_repaired_without_poisoning_other_catalogs() {
    let Some(fixture) = Fixture::new() else {
        return;
    };
    assert_success(fixture.add_catalog("personal", true));
    assert_success(fixture.add_tool("demo", "personal", &[]));
    let missing_remote = fixture._temporary.path().join("missing.git");
    let failed = fixture.loadbot([
        OsStr::new("catalog"),
        OsStr::new("add"),
        OsStr::new("broken"),
        missing_remote.as_os_str(),
    ]);
    assert!(!failed.status.success());

    let list = fixture.loadbot(["list"]);
    assert_success_ref(&list);
    assert!(stdout(&list).contains("demo\tpersonal"));
    assert!(stderr(&list).contains("skipping catalog 'broken'"));

    let repaired = fixture.loadbot([
        OsStr::new("catalog"),
        OsStr::new("add"),
        OsStr::new("broken"),
        fixture.catalog.remote.as_os_str(),
    ]);
    assert_success_ref(&repaired);
    assert!(fixture.home.join("catalogs/broken/.git").is_dir());
}

#[test]
fn unrelated_catalog_destination_is_never_overwritten() {
    let Some(fixture) = Fixture::new() else {
        return;
    };
    let destination = fixture.home.join("catalogs/personal");
    fs::create_dir_all(&destination).unwrap();
    fs::write(destination.join("keep.txt"), "keep\n").unwrap();

    let output = fixture.add_catalog("personal", true);
    assert!(!output.status.success());
    assert!(stderr(&output).contains("not a Git repository"));
    assert_eq!(
        fs::read_to_string(destination.join("keep.txt")).unwrap(),
        "keep\n"
    );
    assert!(!fixture.home.join("config.toml").exists());
}

#[cfg(unix)]
#[test]
fn catalog_symlink_destination_is_not_followed() {
    use std::os::unix::fs::symlink;

    let Some(fixture) = Fixture::new() else {
        return;
    };
    assert_success(fixture.add_catalog("personal", true));
    let destination = fixture.home.join("catalogs/personal");
    fs::remove_dir_all(&destination).unwrap();
    symlink(&fixture.catalog.source, &destination).unwrap();

    let sync = fixture.loadbot(["catalog", "sync", "personal"]);
    assert!(!sync.status.success());
    assert!(stderr(&sync).contains("not a Git repository"));
    let status = fixture.loadbot(["catalog", "status", "personal"]);
    assert_success_ref(&status);
    assert!(
        stdout(&status).contains("Catalog file: unavailable"),
        "{}",
        stdout(&status)
    );
}

#[test]
fn tool_add_writes_only_writable_catalogs_and_never_pushes_implicitly() {
    let Some(fixture) = Fixture::new() else {
        return;
    };
    assert_success(fixture.add_catalog("personal", true));
    let remote_before = bare_main_commit(&fixture.catalog.remote);

    let added = fixture.add_tool("demo", "personal", &[]);
    assert_success_ref(&added);
    let catalog = fs::read_to_string(fixture.home.join("catalogs/personal/catalog.toml")).unwrap();
    assert!(catalog.contains("[tools.demo]"));
    assert_eq!(bare_main_commit(&fixture.catalog.remote), remote_before);

    fixture.configure_catalog_identity("personal");
    let retry = fixture.add_tool("demo", "personal", &["--commit", "--push"]);
    assert_success_ref(&retry);
    assert_ne!(bare_main_commit(&fixture.catalog.remote), remote_before);

    let readonly_fixture = Fixture::new().unwrap();
    assert_success(readonly_fixture.add_catalog("public", false));
    let refused = readonly_fixture.add_tool("demo", "public", &[]);
    assert!(!refused.status.success());
    assert!(stderr(&refused).contains("read-only"));
}

#[test]
fn explicit_commit_and_push_use_the_catalog_repository() {
    let Some(fixture) = Fixture::new() else {
        return;
    };
    assert_success(fixture.add_catalog("personal", true));
    fixture.configure_catalog_identity("personal");
    let remote_before = bare_main_commit(&fixture.catalog.remote);
    let catalog_directory = fixture.home.join("catalogs/personal");
    fs::write(
        catalog_directory.join("unrelated.txt"),
        "staged user work\n",
    )
    .unwrap();
    git(["add", "unrelated.txt"], Some(&catalog_directory));

    let output = fixture.add_tool("demo", "personal", &["--commit", "--push"]);
    assert_success_ref(&output);
    assert!(stdout(&output).contains("committed catalog change"));
    assert!(stdout(&output).contains("pushed catalog 'personal'"));
    assert_ne!(bare_main_commit(&fixture.catalog.remote), remote_before);
    let message = git_text(["log", "-1", "--pretty=%s"], Some(&catalog_directory));
    assert_eq!(message, "Add demo to Loadbot catalog");
    let committed_files = git_text(
        ["show", "--pretty=", "--name-only", "HEAD"],
        Some(&catalog_directory),
    );
    assert_eq!(committed_files, "catalog.toml");
    let staged_files = git_text(
        ["diff", "--cached", "--name-only"],
        Some(&catalog_directory),
    );
    assert_eq!(staged_files, "unrelated.txt");
}

#[test]
fn direct_tool_add_uses_a_writable_default_catalog() {
    let Some(fixture) = Fixture::new() else {
        return;
    };
    assert_success(fixture.add_catalog("personal", true));
    let output = fixture.loadbot([
        OsStr::new("add"),
        OsStr::new("demo"),
        fixture.tool.remote.as_os_str(),
        OsStr::new("--revision"),
        OsStr::new("main"),
    ]);
    assert_success_ref(&output);
    assert!(
        fs::read_to_string(fixture.home.join("catalogs/personal/catalog.toml"))
            .unwrap()
            .contains("[tools.demo]")
    );
}

#[test]
fn tool_listing_aggregates_catalogs_and_requires_qualification_for_duplicates() {
    let Some(fixture) = Fixture::new() else {
        return;
    };
    assert_success(fixture.add_catalog("personal", true));
    assert_success(fixture.add_tool("demo", "personal", &[]));

    let second = create_repository(
        fixture._temporary.path(),
        "second-catalog",
        Some(&format!(
            "version = 1\n\n[tools.demo]\ntype = \"git\"\nurl = {:?}\nrevision = \"main\"\n",
            fixture.tool.remote.display().to_string()
        )),
    );
    let add_second = fixture.loadbot([
        OsStr::new("catalog"),
        OsStr::new("add"),
        OsStr::new("public"),
        second.remote.as_os_str(),
    ]);
    assert_success(add_second);

    let list = fixture.loadbot(["list"]);
    assert_success_ref(&list);
    assert_eq!(stdout(&list).matches("demo\t").count(), 2);
    assert_eq!(stdout(&list).matches("\tyes").count(), 2);

    let ambiguous = fixture.loadbot(["path", "demo"]);
    assert!(!ambiguous.status.success());
    assert!(stderr(&ambiguous).contains("ambiguous across catalogs"));
    let qualified = fixture.loadbot(["path", "demo", "--catalog", "personal"]);
    assert_success_ref(&qualified);
    assert_eq!(
        stdout(&qualified),
        fixture.home.join("tools/demo").display().to_string()
    );
}

#[test]
fn tool_clone_status_update_and_dirty_refusal_remain_safe() {
    let Some(fixture) = Fixture::new() else {
        return;
    };
    assert_success(fixture.add_catalog("personal", true));
    assert_success(fixture.add_tool("demo", "personal", &[]));
    assert_success(fixture.loadbot(["pull", "demo"]));

    let status = fixture.loadbot(["status", "demo"]);
    assert_success_ref(&status);
    assert!(stdout(&status).contains("Catalog: personal"));
    assert!(stdout(&status).contains("Working tree: clean"));

    fs::write(fixture.tool.source.join("update.txt"), "update\n").unwrap();
    commit_and_push(&fixture.tool.source, "tool update");
    let update = fixture.loadbot(["update", "demo"]);
    assert_success_ref(&update);
    assert!(fixture.home.join("tools/demo/update.txt").is_file());

    let local = fixture.home.join("tools/demo/local.txt");
    fs::write(&local, "keep\n").unwrap();
    let dirty = fixture.loadbot(["update", "demo"]);
    assert!(!dirty.status.success());
    assert!(stderr(&dirty).contains("working tree has local changes"));
    assert!(local.is_file());
}

#[test]
fn unrelated_tool_destination_is_never_overwritten() {
    let Some(fixture) = Fixture::new() else {
        return;
    };
    assert_success(fixture.add_catalog("personal", true));
    assert_success(fixture.add_tool("demo", "personal", &[]));
    let destination = fixture.home.join("tools/demo");
    fs::create_dir_all(&destination).unwrap();
    fs::write(destination.join("keep.txt"), "keep\n").unwrap();

    let output = fixture.loadbot(["pull", "demo"]);
    assert!(!output.status.success());
    assert!(stderr(&output).contains("not a Git repository"));
    assert_eq!(
        fs::read_to_string(destination.join("keep.txt")).unwrap(),
        "keep\n"
    );
}

#[test]
fn legacy_configuration_is_detected_and_explicitly_migrated() {
    let Some(fixture) = Fixture::new() else {
        return;
    };
    fs::create_dir_all(&fixture.home).unwrap();
    fs::write(
        fixture.home.join("config.toml"),
        format!(
            "version = 1\n\n[tools.demo]\ntype = \"git\"\nurl = {:?}\nrevision = \"main\"\n",
            fixture.tool.remote.display().to_string()
        ),
    )
    .unwrap();
    let detected = fixture.loadbot(["list"]);
    assert!(!detected.status.success());
    assert!(stderr(&detected).contains("catalog migrate"));

    let empty_catalog = create_repository(fixture._temporary.path(), "migration", None);
    let migrated = fixture.loadbot([
        OsStr::new("catalog"),
        OsStr::new("migrate"),
        OsStr::new("personal"),
        empty_catalog.remote.as_os_str(),
    ]);
    assert_success_ref(&migrated);
    let local = fs::read_to_string(fixture.home.join("config.toml")).unwrap();
    assert!(local.contains("[catalogs.personal]"));
    assert!(!local.contains("[tools.demo]"));
    let portable = fs::read_to_string(fixture.home.join("catalogs/personal/catalog.toml")).unwrap();
    assert!(portable.contains("[tools.demo]"));
}

#[test]
fn migration_refuses_existing_catalog_file_and_cleans_its_new_clone() {
    let Some(fixture) = Fixture::new() else {
        return;
    };
    fs::create_dir_all(&fixture.home).unwrap();
    let legacy = format!(
        "version = 1\n\n[tools.demo]\ntype = \"git\"\nurl = {:?}\n",
        fixture.tool.remote.display().to_string()
    );
    fs::write(fixture.home.join("config.toml"), &legacy).unwrap();

    let output = fixture.loadbot([
        OsStr::new("catalog"),
        OsStr::new("migrate"),
        OsStr::new("personal"),
        fixture.catalog.remote.as_os_str(),
    ]);
    assert!(!output.status.success());
    assert!(stderr(&output).contains("already exists"));
    assert!(!fixture.home.join("catalogs/personal").exists());
    assert_eq!(
        fs::read_to_string(fixture.home.join("config.toml")).unwrap(),
        legacy
    );
}

#[test]
fn incomplete_noninteractive_commands_fail_without_waiting() {
    let Some(fixture) = Fixture::new() else {
        return;
    };
    for arguments in [
        vec!["catalog", "add"],
        vec!["catalog", "sync"],
        vec!["catalog", "status"],
        vec!["catalog", "path"],
        vec!["add"],
        vec!["pull"],
        vec!["update"],
        vec!["status"],
        vec!["path"],
    ] {
        let output = fixture.loadbot(arguments);
        assert!(!output.status.success());
        assert!(stderr(&output).contains("interactive terminal"));
    }
}

#[test]
fn read_only_commands_do_not_create_loadbot_home() {
    let temporary = TempDir::new().unwrap();
    let home = temporary.path().join("missing-home");
    for arguments in [["catalog", "list"].as_slice(), ["list"].as_slice()] {
        let output = Command::new(env!("CARGO_BIN_EXE_loadbot"))
            .env("LOADBOT_HOME", &home)
            .args(arguments)
            .output()
            .unwrap();
        assert!(output.status.success(), "{}", stderr(&output));
        assert!(!home.exists());
    }
}

fn create_repository(base: &Path, name: &str, initial: Option<&str>) -> Repository {
    let source = base.join(format!("{name}-source"));
    let remote = base.join(format!("{name}.git"));
    fs::create_dir(&source).unwrap();
    git(["init", "--initial-branch", "main"], Some(&source));
    git(["config", "user.name", "Loadbot Tests"], Some(&source));
    git(
        ["config", "user.email", "loadbot@example.test"],
        Some(&source),
    );
    if let Some(contents) = initial {
        let file = if name.contains("catalog") {
            "catalog.toml"
        } else {
            "README.md"
        };
        fs::write(source.join(file), contents).unwrap();
        git(["add", file], Some(&source));
        git(["commit", "-m", "initial"], Some(&source));
    } else {
        git(["commit", "--allow-empty", "-m", "initial"], Some(&source));
    }
    git(
        [
            OsStr::new("init"),
            OsStr::new("--bare"),
            OsStr::new("--initial-branch"),
            OsStr::new("main"),
            remote.as_os_str(),
        ],
        None,
    );
    git(
        [
            OsStr::new("remote"),
            OsStr::new("add"),
            OsStr::new("origin"),
            remote.as_os_str(),
        ],
        Some(&source),
    );
    git(["push", "-u", "origin", "main"], Some(&source));
    Repository { source, remote }
}

fn commit_and_push(source: &Path, message: &str) {
    git(["add", "."], Some(source));
    git(["commit", "-m", message], Some(source));
    git(["push", "origin", "main"], Some(source));
}

fn bare_main_commit(remote: &Path) -> String {
    git_text(["rev-parse", "refs/heads/main"], Some(remote))
}

fn git_available() -> bool {
    Command::new("git")
        .arg("--version")
        .output()
        .is_ok_and(|output| output.status.success())
}

fn git<I, S>(arguments: I, directory: Option<&Path>)
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let output = git_output(arguments, directory);
    assert!(
        output.status.success(),
        "Git failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn git_text<I, S>(arguments: I, directory: Option<&Path>) -> String
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let output = git_output(arguments, directory);
    assert!(output.status.success(), "Git failed: {}", stderr(&output));
    stdout(&output)
}

fn git_output<I, S>(arguments: I, directory: Option<&Path>) -> Output
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let mut command = Command::new("git");
    command.args(arguments);
    if let Some(directory) = directory {
        command.current_dir(directory);
    }
    command.output().unwrap()
}

fn assert_success(output: Output) {
    assert_success_ref(&output);
}

fn assert_success_ref(output: &Output) {
    assert!(output.status.success(), "{}", stderr(output));
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).trim().to_owned()
}
