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

    pub fn shortcuts(&self) -> Result<PathBuf> {
        Ok(absolute(
            dirs::config_dir()
                .context("could not determine the operating system's configuration directory")?
                .join("loadbot"),
        )?
        .join("shortcuts.toml"))
    }

    pub fn tools(&self) -> PathBuf {
        self.root.join("tools")
    }

    pub fn catalogs(&self) -> PathBuf {
        self.root.join("catalogs")
    }

    pub fn catalog(&self, name: &str) -> PathBuf {
        self.catalogs().join(name)
    }

    pub fn catalog_file(&self, name: &str) -> PathBuf {
        self.catalog(name).join("catalog.toml")
    }

    pub fn tool(&self, catalog: &str, name: &str) -> Result<PathBuf> {
        validate_name(catalog).context("invalid catalog name")?;
        validate_name(name)?;
        Ok(self.tools().join(catalog).join(name))
    }

    #[cfg(test)]
    pub fn with_root(root: PathBuf) -> Self {
        Self { root }
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
        || name.ends_with('.')
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
        || is_windows_reserved(name)
    {
        bail!("invalid tool name '{name}': use only ASCII letters, numbers, '_', '-', and '.'");
    }
    Ok(())
}

fn is_windows_reserved(name: &str) -> bool {
    let stem = name.split('.').next().unwrap_or(name).to_ascii_uppercase();
    matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || stem
            .strip_prefix("COM")
            .or_else(|| stem.strip_prefix("LPT"))
            .is_some_and(|number| {
                matches!(number, "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9")
            })
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
            "tool.",
            "CON",
            "nul.txt",
            "COM1",
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

    #[test]
    fn tool_paths_require_safe_catalog_and_tool_names() {
        let paths = Paths {
            root: PathBuf::from("/loadbot"),
        };

        assert_eq!(
            paths.tool("personal", "re-toolkit").unwrap(),
            PathBuf::from("/loadbot/tools/personal/re-toolkit")
        );
        assert!(paths.tool("../outside", "re-toolkit").is_err());
        assert!(paths.tool("personal", "../outside").is_err());
    }
}
