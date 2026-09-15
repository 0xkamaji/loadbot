use std::process::Command;

use anyhow::{Context, Result, bail};

use super::args::SetupArgs;
use super::gui::source_root;

pub fn run(arguments: SetupArgs) -> Result<()> {
    let root = source_root()?;
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

    #[cfg(windows)]
    let mut command = {
        let shell = if command_available("pwsh") {
            "pwsh"
        } else {
            "powershell"
        };
        let mut command = Command::new(shell);
        command.args(["-NoProfile", "-File"]);
        command.arg(root.join("setup.ps1"));
        command
    };
    #[cfg(not(windows))]
    let mut command = {
        let mut command = Command::new("sh");
        command.arg(root.join("setup.sh"));
        command
    };
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
