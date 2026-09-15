use std::env;
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};

const SOURCE_ENV: &str = "LOADBOT_SOURCE";
const GUI_ENV: &str = "LOADBOT_GUI_PATH";

pub fn run(development: bool) -> Result<()> {
    if development {
        run_development(&source_root()?)
    } else {
        launch_installed(&installed_gui_path()?)
    }
}

pub(super) fn source_root() -> Result<PathBuf> {
    if let Some(path) = env::var_os(SOURCE_ENV).filter(|value| !value.is_empty()) {
        return validate_source(PathBuf::from(path));
    }
    if let Ok(path) = env::current_dir()
        && source_gui(&path).is_file()
    {
        return Ok(path);
    }
    validate_source(PathBuf::from(env!("CARGO_MANIFEST_DIR"))).with_context(|| {
        format!(
            "Loadbot source checkout was not found. Run from the checkout or set {SOURCE_ENV} to its path."
        )
    })
}

fn validate_source(path: PathBuf) -> Result<PathBuf> {
    let path = if path.is_absolute() {
        path
    } else {
        env::current_dir()
            .context("could not determine the current directory")?
            .join(path)
    };
    if !path.join("Cargo.toml").is_file() || !source_gui(&path).is_file() {
        bail!("{} is not a Loadbot source checkout", path.display());
    }
    Ok(path)
}

fn source_gui(root: &Path) -> PathBuf {
    root.join("src").join("gui").join("package.json")
}

fn installed_gui_path() -> Result<PathBuf> {
    if let Some(path) = env::var_os(GUI_ENV).filter(|value| !value.is_empty()) {
        return Ok(PathBuf::from(path));
    }
    let executable =
        env::current_exe().context("could not locate the installed Loadbot executable")?;
    let directory = executable
        .parent()
        .context("installed Loadbot executable has no parent directory")?;
    Ok(directory.join(format!("loadbot-desktop{}", env::consts::EXE_SUFFIX)))
}

fn launch_installed(path: &Path) -> Result<()> {
    if !path.is_file() {
        bail!(
            "Loadbot GUI is not installed at {}. Run `loadbot setup` and choose GUI or CLI + GUI.",
            path.display()
        );
    }
    Command::new(path)
        .stdin(Stdio::null())
        .spawn()
        .with_context(|| format!("could not launch Loadbot GUI at {}", path.display()))?;
    Ok(())
}

fn run_development(root: &Path) -> Result<()> {
    require_command(
        "cargo",
        "GUI development requires Cargo/Rust. Run `loadbot setup` to configure prerequisites.",
    )?;
    require_node()?;
    require_command(
        "npm",
        "GUI development requires Node.js/npm. Install Node.js LTS or run `loadbot setup`.",
    )?;
    let gui = root.join("src").join("gui");
    if !gui.join("src-tauri/Cargo.toml").is_file() {
        bail!(
            "GUI development source is incomplete: {} is missing",
            gui.join("src-tauri/Cargo.toml").display()
        );
    }
    #[cfg(target_os = "linux")]
    require_linux_native_dependencies()?;
    ensure_frontend_dependencies(&gui)?;
    let status = Command::new("npm")
        .current_dir(&gui)
        .args([OsStr::new("run"), OsStr::new("desktop")])
        .status()
        .context("could not start npm for the Loadbot GUI development environment")?;
    if !status.success() {
        bail!("Loadbot GUI development environment exited with {status}");
    }
    Ok(())
}

fn require_node() -> Result<()> {
    let output = Command::new("node").arg("--version").output().map_err(|_| {
        anyhow::anyhow!("GUI development requires Node.js 22.12 or newer. Install Node.js LTS or run `loadbot setup`.")
    })?;
    let version = String::from_utf8_lossy(&output.stdout);
    let mut parts = version.trim().trim_start_matches('v').split('.');
    let parsed = parts
        .next()
        .and_then(|major| major.parse::<u32>().ok())
        .zip(parts.next().and_then(|minor| minor.parse::<u32>().ok()));
    if !output.status.success()
        || !matches!(parsed, Some((major, minor)) if major > 22 || (major == 22 && minor >= 12))
    {
        bail!(
            "GUI development requires Node.js 22.12 or newer; found {}",
            version.trim()
        );
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn require_linux_native_dependencies() -> Result<()> {
    let base = Command::new("pkg-config")
        .args([
            "--exists",
            "webkit2gtk-4.1",
            "gtk+-3.0",
            "librsvg-2.0",
            "openssl",
        ])
        .status();
    let indicator = ["ayatana-appindicator3-0.1", "appindicator3-0.1"]
        .into_iter()
        .any(|package| {
            Command::new("pkg-config")
                .args(["--exists", package])
                .status()
                .is_ok_and(|status| status.success())
        });
    if !matches!(base, Ok(status) if status.success()) || !indicator {
        bail!(
            "GUI development requires the native Tauri WebKitGTK 4.1 build dependencies. Run `loadbot setup` and choose GUI or CLI + GUI."
        );
    }
    Ok(())
}

fn require_command(command: &str, guidance: &str) -> Result<()> {
    match Command::new(command)
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
    {
        Ok(status) if status.success() => Ok(()),
        _ => bail!(guidance.to_owned()),
    }
}

fn ensure_frontend_dependencies(gui: &Path) -> Result<()> {
    let lockfile = gui.join("package-lock.json");
    let node_modules = gui.join("node_modules");
    let marker = node_modules.join(".loadbot-package-lock.json");
    let expected = fs::read(&lockfile)
        .with_context(|| format!("could not read frontend lockfile {}", lockfile.display()))?;
    let current = fs::read(&marker).ok();
    let required_api = node_modules
        .join("@tauri-apps")
        .join("api")
        .join("package.json");
    if current.as_deref() == Some(expected.as_slice()) && required_api.is_file() {
        return Ok(());
    }

    eprintln!(
        "Frontend dependencies are missing or stale; restoring package-lock.json with `npm ci`..."
    );
    let status = Command::new("npm")
        .current_dir(gui)
        .arg(OsStr::new("ci"))
        .status()
        .context("could not run npm ci for GUI development dependencies")?;
    if !status.success() {
        bail!("`npm ci` failed with {status}; frontend dependencies were not repaired");
    }
    fs::write(&marker, expected).with_context(|| {
        format!(
            "could not record frontend dependency state at {}",
            marker.display()
        )
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn installed_gui_is_a_sibling_not_a_shell_command() {
        let executable =
            Path::new("install root").join(format!("loadbot{}", env::consts::EXE_SUFFIX));
        let expected = executable
            .parent()
            .unwrap()
            .join(format!("loadbot-desktop{}", env::consts::EXE_SUFFIX));
        assert_eq!(expected.file_stem(), Some(OsStr::new("loadbot-desktop")));
        assert!(!expected.to_string_lossy().contains("fixture"));
    }

    #[test]
    fn lockfile_marker_requires_exact_bytes_and_tauri_api() {
        let temporary = tempfile::TempDir::new().unwrap();
        let gui = temporary.path();
        fs::write(gui.join("package-lock.json"), b"lock-v2").unwrap();
        let modules = gui.join("node_modules");
        fs::create_dir_all(modules.join("@tauri-apps/api")).unwrap();
        fs::write(modules.join("@tauri-apps/api/package.json"), b"{}").unwrap();
        fs::write(modules.join(".loadbot-package-lock.json"), b"lock-v1").unwrap();
        assert_ne!(
            fs::read(gui.join("package-lock.json")).unwrap(),
            fs::read(modules.join(".loadbot-package-lock.json")).unwrap()
        );
        fs::copy(
            gui.join("package-lock.json"),
            modules.join(".loadbot-package-lock.json"),
        )
        .unwrap();
        assert_eq!(
            fs::read(gui.join("package-lock.json")).unwrap(),
            fs::read(modules.join(".loadbot-package-lock.json")).unwrap()
        );
    }
}
