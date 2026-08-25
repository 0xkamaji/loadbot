use std::env;
use std::path::PathBuf;

use anyhow::{Context, Result, bail};

#[derive(Debug, Clone)]
pub struct Paths {
    root: PathBuf,
}

impl Paths {
    pub fn discover() -> Result<Self> {
        let root = match env::var_os("LOADBOT_HOME") {
            Some(value) if !value.is_empty() => PathBuf::from(value),
            _ => dirs::data_local_dir()
                .context("could not determine the operating system's local data directory")?
                .join("loadbot"),
        };
        let root = absolute(root)?;
        Ok(Self { root })
    }

    pub fn config(&self) -> PathBuf {
        self.root.join("config.toml")
    }

    pub fn tools(&self) -> PathBuf {
        self.root.join("tools")
    }

    pub fn tool(&self, name: &str) -> PathBuf {
        self.tools().join(name)
    }
}

fn absolute(path: PathBuf) -> Result<PathBuf> {
    if path.is_absolute() {
        return Ok(path);
    }
    Ok(env::current_dir()
        .context("could not determine the current directory")?
        .join(path))
}

pub fn validate_name(name: &str) -> Result<()> {
    if name.is_empty()
        || name == "."
        || name == ".."
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
    {
        bail!("invalid tool name '{name}': use only ASCII letters, numbers, '_', '-', and '.'");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_traversal_and_separators() {
        for name in [
            "",
            ".",
            "..",
            "../tool",
            "tool/name",
            "tool\\name",
            "/tmp/tool",
        ] {
            assert!(validate_name(name).is_err(), "accepted {name:?}");
        }
    }

    #[test]
    fn accepts_conservative_names() {
        for name in ["re-toolbox", "tool_name", "tool.name", "Tool123"] {
            assert!(validate_name(name).is_ok(), "rejected {name:?}");
        }
    }
}
