use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use tempfile::TempDir;

struct Fixture {
    _temporary: TempDir,
    home: PathBuf,
    remote: PathBuf,
    source: PathBuf,
}

impl Fixture {
    fn new() -> Option<Self> {
        if Command::new("git").arg("--version").output().is_err() {
            eprintln!("skipping integration test: Git is not available");
            return None;
        }

        let temporary = TempDir::new().unwrap();
        let home = temporary.path().join("loadbot-home");
        let remote = temporary.path().join("remote.git");
        let source = temporary.path().join("source");
        fs::create_dir(&source).unwrap();

        git(["init", "--initial-branch", "main"], Some(&source));
        git(["config", "user.name", "Loadbot Tests"], Some(&source));
        git(
            ["config", "user.email", "loadbot@example.test"],
            Some(&source),
        );
        fs::write(source.join("README.md"), "initial\n").unwrap();
        git(["add", "README.md"], Some(&source));
        git(["commit", "-m", "initial"], Some(&source));
        git(
            [OsStr::new("init"), OsStr::new("--bare"), remote.as_os_str()],
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

        Some(Self {
            _temporary: temporary,
            home,
            remote,
            source,
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

    fn add(&self, name: &str) -> Output {
        self.loadbot([
            OsStr::new("add"),
            OsStr::new(name),
            self.remote.as_os_str(),
            OsStr::new("--revision"),
            OsStr::new("main"),
        ])
    }

    fn install(&self, name: &str) {
        assert_success(self.add(name));
        assert_success(self.loadbot(["pull", name]));
    }

    fn add_remote_commit(&self) {
        fs::write(self.source.join("new-file.txt"), "new commit\n").unwrap();
        git(["add", "new-file.txt"], Some(&self.source));
        git(["commit", "-m", "update"], Some(&self.source));
        git(["push", "origin", "main"], Some(&self.source));
    }
}

#[test]
fn add_creates_round_trippable_configuration_in_loadbot_home() {
    let Some(fixture) = Fixture::new() else {
        return;
    };
    assert_success(fixture.add("demo"));

    let config_path = fixture.home.join("config.toml");
    let contents = fs::read_to_string(config_path).unwrap();
    let config: toml::Value = toml::from_str(&contents).unwrap();
    assert_eq!(config["version"].as_integer(), Some(1));
    assert_eq!(config["tools"]["demo"]["type"].as_str(), Some("git"));
    assert_eq!(config["tools"]["demo"]["revision"].as_str(), Some("main"));
    assert!(!fixture.home.parent().unwrap().join("config.toml").exists());

    let second = fixture.add("demo");
    assert_success(second);
}

#[test]
fn pull_clones_and_path_prints_the_absolute_destination() {
    let Some(fixture) = Fixture::new() else {
        return;
    };
    fixture.install("demo");

    let destination = fixture.home.join("tools").join("demo");
    assert!(destination.join("README.md").is_file());
    let path = fixture.loadbot(["path", "demo"]);
    assert_success_ref(&path);
    assert_eq!(stdout(&path), destination.display().to_string());

    let second_pull = fixture.loadbot(["pull", "demo"]);
    assert_success_ref(&second_pull);
    assert!(stdout(&second_pull).contains("already installed"));
}

#[test]
fn list_distinguishes_missing_and_installed_tools() {
    let Some(fixture) = Fixture::new() else {
        return;
    };
    assert_success(fixture.add("missing"));
    fixture.install("installed");

    let output = fixture.loadbot(["list"]);
    assert_success_ref(&output);
    let text = stdout(&output);
    assert!(text.contains("missing\tgit\tmissing\tmain"), "{text}");
    assert!(text.contains("installed\tgit\tinstalled\tmain"), "{text}");
}

#[test]
fn status_detects_clean_and_dirty_repositories() {
    let Some(fixture) = Fixture::new() else {
        return;
    };
    fixture.install("demo");

    let clean = fixture.loadbot(["status", "demo"]);
    assert_success_ref(&clean);
    assert!(stdout(&clean).contains("Working tree: clean"));

    fs::write(
        fixture.home.join("tools/demo/untracked.txt"),
        "local change\n",
    )
    .unwrap();
    let dirty = fixture.loadbot(["status", "demo"]);
    assert_success_ref(&dirty);
    assert!(stdout(&dirty).contains("Working tree: dirty"));
}

#[test]
fn update_fast_forwards_to_a_new_remote_commit() {
    let Some(fixture) = Fixture::new() else {
        return;
    };
    fixture.install("demo");
    fixture.add_remote_commit();

    let output = fixture.loadbot(["update", "demo"]);
    assert_success_ref(&output);
    assert!(stdout(&output).contains("updated tool 'demo' from"));
    assert!(fixture.home.join("tools/demo/new-file.txt").is_file());
}

#[test]
fn update_refuses_a_dirty_working_tree() {
    let Some(fixture) = Fixture::new() else {
        return;
    };
    fixture.install("demo");
    let local_file = fixture.home.join("tools/demo/local.txt");
    fs::write(&local_file, "do not delete\n").unwrap();

    let output = fixture.loadbot(["update", "demo"]);
    assert!(!output.status.success());
    assert!(stderr(&output).contains("working tree has local changes"));
    assert!(local_file.is_file());
}

#[test]
fn pull_never_overwrites_an_unrelated_destination() {
    let Some(fixture) = Fixture::new() else {
        return;
    };
    assert_success(fixture.add("demo"));
    let destination = fixture.home.join("tools/demo");
    fs::create_dir_all(&destination).unwrap();
    let sentinel = destination.join("keep.txt");
    fs::write(&sentinel, "keep me\n").unwrap();

    let output = fixture.loadbot(["pull", "demo"]);
    assert!(!output.status.success());
    assert!(stderr(&output).contains("destination exists but is not a Git repository"));
    assert_eq!(fs::read_to_string(sentinel).unwrap(), "keep me\n");
}

fn git<I, S>(arguments: I, directory: Option<&Path>)
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let mut command = Command::new("git");
    command.args(arguments);
    if let Some(directory) = directory {
        command.current_dir(directory);
    }
    let output = command.output().unwrap();
    assert!(
        output.status.success(),
        "Git failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
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
