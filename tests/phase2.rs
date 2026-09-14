use std::fs;
use std::io::{BufRead, Read, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};

use loadbot::{config, persistence::{Busy, Lease}, process::{self, Cancellation, Control, Event, Mode, Stream}, shortcuts::{self, Shortcut}};

fn worker(role: &str) -> Command {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command.args(["--exact", "controlled_child", "--nocapture"])
        .env("LOADBOT_PHASE2_CHILD", role);
    command
}

#[test]
fn controlled_child() {
    let Ok(role) = std::env::var("LOADBOT_PHASE2_CHILD") else { return; };
    match role.as_str() {
        "lease" => {
            let resource = std::env::var_os("LOADBOT_PHASE2_RESOURCE").unwrap();
            let _lease = Lease::acquire(Path::new(&resource)).unwrap();
            println!("READY");
            std::io::stdout().flush().unwrap();
            let mut input = [0];
            std::io::stdin().read_exact(&mut input).unwrap();
            // Deliberately bypass destructors to exercise OS lock release.
            std::process::exit(7);
        }
        "config" | "shortcut" => {
            let resource = std::env::var_os("LOADBOT_PHASE2_RESOURCE").unwrap();
            let name = std::env::var("LOADBOT_PHASE2_NAME").unwrap();
            let result = if role == "config" {
                config::update(Path::new(&resource), |config| {
                    config.extra.insert(name, toml::Value::Boolean(true));
                    Ok(())
                })
            } else {
                shortcuts::save(Path::new(&resource), &name, Shortcut::new("personal".into(), "demo".into(), "run.sh".into()).unwrap())
            };
            match result {
                Ok(()) => std::process::exit(0),
                Err(error) if error.downcast_ref::<Busy>().is_some() => std::process::exit(23),
                Err(error) => panic!("{error:#}"),
            }
        }
        "large" => {
            let thread = std::thread::spawn(|| {
                for _ in 0..1024 { std::io::stderr().write_all(&[b'e'; 8192]).unwrap(); }
            });
            for _ in 0..1024 { std::io::stdout().write_all(&[b'o'; 8192]).unwrap(); }
            thread.join().unwrap();
        }
        "descendant" => {
            println!("DESCENDANT:{}", std::process::id());
            std::io::stdout().flush().unwrap();
            loop { std::thread::park(); }
        }
        "tree" => {
            let mut descendant = worker("descendant").spawn().unwrap();
            descendant.wait().unwrap();
        }
        "exit" => std::process::exit(7),
        _ => panic!("unknown worker"),
    }
}

fn hold(resource: &Path) -> std::process::Child {
    let mut child = worker("lease").env("LOADBOT_PHASE2_RESOURCE", resource)
        .stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().unwrap();
    let mut reader = std::io::BufReader::new(child.stdout.take().unwrap());
    let mut line = String::new();
    loop {
        assert_ne!(reader.read_line(&mut line).unwrap(), 0);
        if line.contains("READY") { break; }
        line.clear();
    }
    child
}

#[test]
fn concurrent_document_transactions_are_busy_then_preserve_both_updates() {
    let temporary = tempfile::tempdir().unwrap();
    for role in ["config", "shortcut"] {
        let path = temporary.path().join(format!("{role}.toml"));
        let mut holder = hold(&path);
        let edit = |name: &str| worker(role).env("LOADBOT_PHASE2_RESOURCE", &path)
            .env("LOADBOT_PHASE2_NAME", name).status().unwrap().code().unwrap();
        assert_eq!(edit("first"), 23);
        holder.stdin.take().unwrap().write_all(b"x").unwrap();
        assert_eq!(holder.wait().unwrap().code(), Some(7));
        assert_eq!(edit("first"), 0);
        assert_eq!(edit("second"), 0);
        let contents = fs::read_to_string(&path).unwrap();
        assert!(contents.contains("first"));
        assert!(contents.contains("second"));
        // Atomic replacement did not change the lock's identity.
        let _lease = Lease::acquire(&path).unwrap();
        assert_eq!(edit("third"), 23);
    }
}

#[test]
fn repository_mutation_conflicts_and_process_exit_releases_lease() {
    let temporary = tempfile::tempdir().unwrap();
    let paths = loadbot::paths::Paths::with_root(temporary.path().join("data"));
    let resource = paths.catalog("personal");
    let mut holder = hold(&resource);
    let mut interaction = loadbot::interaction::Unattended;
    let mut context = loadbot::interaction::OperationContext::new(&mut interaction);
    let error = loadbot::operations::catalog_add(&paths, "personal", "local.git".into(), true, &mut context).unwrap_err();
    assert!(error.downcast_ref::<Busy>().is_some(), "{error:#}");
    assert!(!paths.config().exists());
    holder.kill().unwrap();
    holder.wait().unwrap();
    let _lease = Lease::acquire(&resource).unwrap();
}

#[test]
fn failed_transaction_and_recovery_keep_existing_data() {
    let temporary = tempfile::tempdir().unwrap();
    let path = temporary.path().join("config.toml");
    config::save(&path, &config::LocalConfig::default()).unwrap();
    let before = fs::read(&path).unwrap();
    let result: anyhow::Result<()> = config::update(&path, |_| anyhow::bail!("injected edit failure"));
    assert!(result.is_err());
    assert_eq!(fs::read(&path).unwrap(), before);
    config::update(&path, |config| { config.extra.insert("kept".into(), true.into()); Ok(()) }).unwrap();
    let backup = path.with_extension("toml.loadbot-backup");
    fs::rename(&path, &backup).unwrap();
    assert!(config::load(&path).unwrap_err().to_string().contains("recovery required"));
    assert!(config::save(&path, &config::LocalConfig::default()).is_err());
    assert!(!path.exists());
    // Supported recovery is explicit inspection/restoration, never defaulting.
    fs::rename(&backup, &path).unwrap();
    assert_eq!(config::load(&path).unwrap().extra["kept"].as_bool(), Some(true));
    let abandoned = temporary.path().join(".loadbot-write-abandoned");
    fs::write(&abandoned, "incomplete = ").unwrap();
    config::update(&path, |_| Ok(())).unwrap();
    assert!(abandoned.exists());
}

#[test]
fn confirmed_removal_cannot_delete_a_replaced_definition() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("shortcuts.toml");
    let original = Shortcut::new("personal".into(), "demo".into(), "old.sh".into()).unwrap();
    shortcuts::save(&path, "demo", original.clone()).unwrap();
    shortcuts::remove(&path, "demo").unwrap();
    let replacement = Shortcut::new("personal".into(), "demo".into(), "new.sh".into()).unwrap();
    shortcuts::save(&path, "demo", replacement.clone()).unwrap();
    let error = shortcuts::remove_if_matches(&path, "demo", &original).unwrap_err();
    assert!(error.downcast_ref::<Busy>().is_some());
    assert_eq!(shortcuts::load(&path).unwrap().shortcuts["demo"], replacement);
}

#[test]
fn cancelled_catalog_inspection_is_not_downgraded_to_a_skipped_catalog() {
    use loadbot::interaction::{OperationContext, OperationStatus, Unattended};
    let root = tempfile::tempdir().unwrap();
    let paths = loadbot::paths::Paths::with_root(root.path().join("data"));
    fs::create_dir_all(paths.catalog("personal")).unwrap();
    config::update(&paths.config(), |config| {
        config.catalogs.insert("personal".into(), config::CatalogSource::new("local.git".into(), true));
        Ok(())
    }).unwrap();
    let mut unattended = Unattended;
    let mut context = OperationContext::new(&mut unattended);
    let cancellation = context.process.cancellation.clone();
    context.process.observer = Some(Arc::new(move |event| {
        if matches!(event, Event::Starting { .. }) { cancellation.cancel(); }
    }));
    let report = context.run(|context| loadbot::operations::all_tools(&paths, context));
    assert_eq!(report.status(), OperationStatus::Cancelled);
    assert!(!report.notices.iter().any(|notice| matches!(notice, loadbot::interaction::Notice::SkippedCatalog { .. })));
}

#[test]
fn executor_reports_spawn_exit_and_streams_large_simultaneous_output() {
    let mut absent = Command::new("loadbot-test-executable-that-does-not-exist");
    assert!(process::execute(&mut absent, Mode::Stream, &Control::default()).is_err());
    assert_eq!(process::execute(&mut worker("exit"), Mode::Stream, &Control::default()).unwrap().status.code(), Some(7));
    let counts = Arc::new(Mutex::new([0usize; 2]));
    let totals = counts.clone();
    let control = Control {
        observer: Some(Arc::new(move |event| {
            if let Event::Output { stream, bytes } = event {
                totals.lock().unwrap()[usize::from(stream == Stream::Stderr)] += bytes.len();
            }
        })),
        ..Control::default()
    };
    let output = process::execute(&mut worker("large"), Mode::Stream, &control).unwrap();
    assert!(output.status.success());
    assert!(output.stdout.is_empty() && output.stderr.is_empty());
    assert!(counts.lock().unwrap().iter().all(|count| *count >= 8 * 1024 * 1024));
    assert!(process::execute(&mut worker("large"), Mode::Capture { limit: 1024 }, &Control::default()).unwrap_err().to_string().contains("capture limit"));
}

#[test]
fn cancellation_stops_and_reaps_process_tree_and_preserves_output() {
    let cancellation = Cancellation::default();
    let request = cancellation.clone();
    let received = Arc::new(Mutex::new(Vec::new()));
    let bytes = received.clone();
    let control = Control {
        cancellation,
        terminal: false,
        observer: Some(Arc::new(move |event| {
            if let Event::Output { bytes: chunk, .. } = event {
                let mut bytes = bytes.lock().unwrap();
                bytes.extend(chunk);
                if String::from_utf8_lossy(&bytes).lines().any(|line| line.starts_with("DESCENDANT:")) {
                    request.cancel();
                }
            }
        })),
    };
    let error = process::execute(&mut worker("tree"), Mode::Stream, &control).unwrap_err();
    assert!(error.downcast_ref::<process::Cancelled>().is_some(), "{error:#}");
    let text = String::from_utf8(received.lock().unwrap().clone()).unwrap();
    let pid: u32 = text.lines().find_map(|line| line.strip_prefix("DESCENDANT:")).unwrap().parse().unwrap();
    #[cfg(target_os = "linux")]
    if let Ok(stat) = fs::read_to_string(format!("/proc/{pid}/stat")) {
        assert_eq!(stat.rsplit_once(") ").unwrap().1.split_whitespace().next(), Some("Z"));
    }
    #[cfg(windows)]
    unsafe {
        use windows_sys::Win32::{Foundation::{CloseHandle, WAIT_OBJECT_0}, System::Threading::{OpenProcess, PROCESS_SYNCHRONIZE, WaitForSingleObject}};
        let handle = OpenProcess(PROCESS_SYNCHRONIZE, 0, pid);
        if !handle.is_null() { assert_eq!(WaitForSingleObject(handle, 0), WAIT_OBJECT_0); CloseHandle(handle); }
    }
    let error = process::execute(&mut worker("exit"), Mode::Stream, &control).unwrap_err();
    assert!(error.downcast_ref::<process::Cancelled>().is_some());
}

#[test]
fn cancellation_after_definition_save_reports_partial_work_without_rollback() {
    use loadbot::{catalog, interaction::{Interaction, Notice, OperationContext, OperationStatus}, operations, paths::Paths};
    let temporary = tempfile::tempdir().unwrap();
    let paths = Paths::with_root(temporary.path().join("data"));
    let repository = paths.catalog("personal");
    fs::create_dir_all(&repository).unwrap();
    let git = |args: &[&str]| {
        let output = Command::new("git").current_dir(&repository).args(args)
            .env("GIT_CONFIG_NOSYSTEM", "1").env("GIT_CONFIG_GLOBAL", temporary.path().join("absent"))
            .env("GIT_AUTHOR_NAME", "Loadbot Test").env("GIT_AUTHOR_EMAIL", "loadbot@example.test")
            .env("GIT_COMMITTER_NAME", "Loadbot Test").env("GIT_COMMITTER_EMAIL", "loadbot@example.test")
            .output().unwrap();
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    };
    git(&["init"]);
    git(&["config", "user.name", "Loadbot Test"]);
    git(&["config", "user.email", "loadbot@example.test"]);
    git(&["remote", "add", "origin", "local-catalog.git"]);
    catalog::save(&paths.catalog_file("personal"), &catalog::CatalogFile::default()).unwrap();
    git(&["add", "catalog.toml"]);
    git(&["commit", "-m", "Initial catalog"]);
    config::update(&paths.config(), |local| {
        local.catalogs.insert("personal".into(), config::CatalogSource::new("local-catalog.git".into(), true));
        Ok(())
    }).unwrap();
    struct CancelAfterSave(Cancellation);
    impl Interaction for CancelAfterSave {
        fn notice(&mut self, notice: &Notice) {
            if matches!(notice, Notice::ToolAdded { .. }) { self.0.cancel(); }
        }
    }
    let cancellation = Cancellation::default();
    let mut interaction = CancelAfterSave(cancellation.clone());
    let mut context = OperationContext::new(&mut interaction);
    context.process.cancellation = cancellation;
    let report = context.run(|context| operations::tool_add(&paths, "personal", "demo", "local-tool.git".into(), None, true, false, context));
    assert_eq!(report.status(), OperationStatus::Cancelled);
    assert!(report.is_partial());
    assert!(report.notices.iter().any(|notice| matches!(notice, Notice::ToolAdded { .. })));
    assert!(catalog::load(&paths.catalog_file("personal")).unwrap().tools.contains_key("demo"));
    assert!(Lease::acquire(&repository).is_ok());

    // Retrying can commit the already saved definition. The nonexistent local
    // origin then fails to push; no network, credentials, or remote writes occur.
    let mut unattended = loadbot::interaction::Unattended;
    let mut retry = OperationContext::new(&mut unattended);
    let report = retry.run(|context| operations::tool_add(&paths, "personal", "demo", "local-tool.git".into(), None, true, true, context));
    assert_eq!(report.status(), OperationStatus::Failed);
    assert!(report.is_partial());
    assert!(report.notices.iter().any(|notice| matches!(notice, Notice::CatalogCommitted { .. })));
    assert!(format!("{:#}", report.result.unwrap_err()).contains("pushing the catalog failed"));
    assert!(!loadbot::git::path_has_changes(&repository, "catalog.toml").unwrap());
}
