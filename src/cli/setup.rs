use std::env;
use std::fs;
use std::process::Command;

use anyhow::{Context, Result, bail};

use super::args::SetupArgs;

#[cfg(not(windows))]
const SETUP_SCRIPT: &str = include_str!("../../setup.sh");
#[cfg(windows)]
const SETUP_SCRIPT: &str = include_str!("../../setup.ps1");
const GUI_ICON: &[u8] =
    include_bytes!("../gui/loadbot-gui-assets/assets/branding/loadbot-header.png");

pub fn run(arguments: SetupArgs) -> Result<()> {
    let mode = if arguments.cli {
        Some("cli")
    } else if arguments.gui {
        Some("gui")
    } else if arguments.all {
        Some("all")
    } else if arguments.repair {
        Some("repair")
    } else {
        None
    };
    let executable = env::current_exe().context("could not locate the Loadbot executable")?;
    let executable_directory = executable
        .parent()
        .context("the Loadbot executable has no parent directory")?;
    let desktop = executable_directory.join(format!("loadbot-desktop{}", env::consts::EXE_SUFFIX));
    let temporary = tempfile::Builder::new()
        .prefix("loadbot-release-setup-")
        .tempdir()
        .context("could not create a temporary directory for Loadbot setup")?;
    #[cfg(not(windows))]
    let script = temporary.path().join("setup.sh");
    #[cfg(windows)]
    let script = temporary.path().join("setup.ps1");
    let icon = temporary.path().join("loadbot-header.png");
    fs::write(&script, SETUP_SCRIPT).context("could not write the temporary setup script")?;
    fs::write(&icon, GUI_ICON).context("could not write the temporary GUI icon")?;

    #[cfg(windows)]
    let mut command = {
        let shell = if command_available("pwsh") {
            "pwsh"
        } else {
            "powershell"
        };
        let mut command = Command::new(shell);
        command.args(["-NoProfile", "-File"]);
        command.arg(&script);
        command
    };
    #[cfg(not(windows))]
    let mut command = {
        let mut command = Command::new("sh");
        command.arg(&script);
        command
    };
    command
        .env("LOADBOT_INTERNAL_RELEASE_SETUP", "1")
        .env("LOADBOT_RELEASE_CLI", &executable)
        .env("LOADBOT_RELEASE_GUI", &desktop)
        .env("LOADBOT_RELEASE_ICON", &icon);
    if let Some(mode) = mode {
        #[cfg(windows)]
        command.arg(format!("-{mode}"));
        #[cfg(not(windows))]
        command.arg(format!("--{mode}"));
    }
    let status = command
        .status()
        .context("could not start the Loadbot setup bootstrap")?;
    if !status.success() {
        bail!("Loadbot setup exited with {status}");
    }
    Ok(())
}

#[cfg(windows)]
fn command_available(command: &str) -> bool {
    Command::new(command)
        .arg("-NoProfile")
        .arg("-Command")
        .arg("exit 0")
        .status()
        .is_ok_and(|status| status.success())
}
