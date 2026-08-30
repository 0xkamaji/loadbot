use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::catalog::Runner;
use crate::config;
use crate::paths;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ShortcutFile {
    pub version: u32,
    #[serde(default)]
    pub shortcuts: BTreeMap<String, Shortcut>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, toml::Value>,
}

impl Default for ShortcutFile {
    fn default() -> Self {
        Self {
            version: 1,
            shortcuts: BTreeMap::new(),
            extra: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Shortcut {
    pub catalog: String,
    pub tool: String,
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runner: Option<Runner>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, toml::Value>,
}

impl Shortcut {
    pub fn new(catalog: String, tool: String, path: String) -> Result<Self> {
        paths::validate_name(&catalog).context("invalid shortcut catalog")?;
        paths::validate_name(&tool).context("invalid shortcut tool")?;
        relative_path(&path)?;
        Ok(Self {
            catalog,
            tool,
            path,
            description: None,
            runner: None,
            extra: BTreeMap::new(),
        })
    }
}

pub fn load(path: &Path) -> Result<ShortcutFile> {
    reject_symlink(path)?;
    let contents = match fs::read_to_string(path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(ShortcutFile::default());
        }
        Err(error) => {
            return Err(error).with_context(|| format!("could not read {}", path.display()));
        }
    };
    let shortcuts: ShortcutFile =
        toml::from_str(&contents).with_context(|| format!("could not parse {}", path.display()))?;
    if shortcuts.version != 1 {
        bail!(
            "unsupported shortcut version {} in {}",
            shortcuts.version,
            path.display()
        );
    }
    for (name, shortcut) in &shortcuts.shortcuts {
        paths::validate_name(name).context("invalid shortcut name")?;
        paths::validate_name(&shortcut.catalog).context("invalid shortcut catalog")?;
        paths::validate_name(&shortcut.tool).context("invalid shortcut tool")?;
        relative_path(&shortcut.path)
            .with_context(|| format!("shortcut '{name}' contains an unsafe path"))?;
    }
    Ok(shortcuts)
}

pub fn save(path: &Path, name: &str, shortcut: Shortcut) -> Result<()> {
    paths::validate_name(name).context("invalid shortcut name")?;
    relative_path(&shortcut.path)?;
    let mut shortcuts = load(path)?;
    if shortcuts.shortcuts.contains_key(name) {
        bail!("shortcut '{name}' already exists");
    }
    shortcuts.shortcuts.insert(name.to_owned(), shortcut);
    reject_symlink(path)?;
    config::save_toml(path, &shortcuts)
}

pub fn shortcut_names(path: &Path) -> Result<Vec<String>> {
    Ok(load(path)?.shortcuts.into_keys().collect())
}

pub fn relative_path(value: &str) -> Result<PathBuf> {
    if value.is_empty() || value.starts_with('/') || value.contains(['\\', ':']) {
        bail!("shortcut path must be a portable relative path inside its tool repository");
    }
    let mut path = PathBuf::new();
    for component in value.split('/') {
        if component.is_empty() || matches!(component, "." | "..") {
            bail!("shortcut path must not escape its tool repository");
        }
        path.push(component);
    }
    Ok(path)
}

pub fn portable_path(path: &Path) -> Result<String> {
    let mut components = Vec::new();
    for component in path.components() {
        let std::path::Component::Normal(component) = component else {
            bail!("shortcut path must be relative to its tool repository");
        };
        let component = component
            .to_str()
            .context("shortcut path is not valid UTF-8")?;
        if component.contains(['\\', ':']) {
            bail!("shortcut path is not portable across supported platforms");
        }
        components.push(component);
    }
    let value = components.join("/");
    relative_path(&value)?;
    Ok(value)
}

fn reject_symlink(path: &Path) -> Result<()> {
    if fs::symlink_metadata(path).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
        bail!("refusing symlink shortcut file {}", path.display());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shortcuts_round_trip_without_absolute_paths() {
        let temporary = tempfile::TempDir::new().unwrap();
        let path = temporary.path().join("shortcuts.toml");
        let shortcut = Shortcut::new(
            "personal".to_owned(),
            "re-toolkit".to_owned(),
            "recipes/print_strings.py".to_owned(),
        )
        .unwrap();

        save(&path, "print-strings", shortcut.clone()).unwrap();
        assert_eq!(load(&path).unwrap().shortcuts["print-strings"], shortcut);
        let contents = fs::read_to_string(path).unwrap();
        assert!(!contents.contains(temporary.path().to_str().unwrap()));
    }

    #[test]
    fn rejects_traversal_and_duplicate_names() {
        for path in [
            "",
            "../outside",
            "recipes/../../outside",
            "/tmp/file",
            "C:\\file",
            "recipes\\file.py",
        ] {
            assert!(relative_path(path).is_err(), "accepted {path:?}");
        }

        let temporary = tempfile::TempDir::new().unwrap();
        let path = temporary.path().join("shortcuts.toml");
        let shortcut = Shortcut::new(
            "personal".to_owned(),
            "demo".to_owned(),
            "run.sh".to_owned(),
        )
        .unwrap();
        save(&path, "demo", shortcut.clone()).unwrap();
        assert!(save(&path, "demo", shortcut).is_err());
    }

    #[test]
    fn shortcut_names_are_alphabetical() {
        let temporary = tempfile::TempDir::new().unwrap();
        let path = temporary.path().join("shortcuts.toml");
        fs::write(
            &path,
            r#"version = 1

[shortcuts.print-strings]
catalog = "personal"
tool = "demo"
path = "print.py"

[shortcuts.bn-triage]
catalog = "personal"
tool = "demo"
path = "triage.py"
"#,
        )
        .unwrap();

        assert_eq!(
            shortcut_names(&path).unwrap(),
            ["bn-triage", "print-strings"]
        );
    }

    #[test]
    fn version_one_shortcuts_accept_optional_metadata_and_preserve_unknown_fields() {
        let temporary = tempfile::TempDir::new().unwrap();
        let path = temporary.path().join("shortcuts.toml");
        fs::write(
            &path,
            r#"version = 1

[shortcuts.legacy]
catalog = "personal"
tool = "demo"
path = "legacy.sh"

[shortcuts.audit]
catalog = "personal"
tool = "demo"
path = "audit.sh"
description = "Run the audit"
runner = "bash"
future = "kept"
"#,
        )
        .unwrap();

        let loaded = load(&path).unwrap();
        assert_eq!(loaded.shortcuts["legacy"].description, None);
        assert_eq!(loaded.shortcuts["legacy"].runner, None);
        assert_eq!(
            loaded.shortcuts["audit"].description.as_deref(),
            Some("Run the audit")
        );
        assert_eq!(loaded.shortcuts["audit"].runner, Some(Runner::Bash));
        assert_eq!(
            loaded.shortcuts["audit"].extra["future"].as_str(),
            Some("kept")
        );
    }
}
