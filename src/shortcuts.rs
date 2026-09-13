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

/// Remove only one definition, preserving metadata and the valid file when empty.
pub fn remove(path: &Path, name: &str) -> Result<()> {
    paths::validate_name(name).context("invalid shortcut name")?;
    let mut file = load(path)?;
    if file.shortcuts.remove(name).is_none() {
        bail!("shortcut '{name}' does not exist");
    }
    reject_symlink(path)?;
    config::save_toml(path, &file)
}

fn details(name: &str, shortcut: &Shortcut) -> String {
    let mut text = format!(
        "Name: {name}\nCatalog: {}\nTool: {}\nPath: {}",
        shortcut.catalog, shortcut.tool, shortcut.path
    );
    if let Some(description) = &shortcut.description {
        text.push_str(&format!("\nDescription: {description}"));
    }
    if let Some(runner) = shortcut.runner {
        text.push_str(&format!("\nRunner: {}", runner.as_str()));
    }
    text
}

pub fn list(path: &Path) -> Result<String> {
    let file = load(path)?;
    if file.shortcuts.is_empty() {
        return Ok("No shortcuts are saved.".to_owned());
    }
    Ok(file
        .shortcuts
        .iter()
        .map(|(name, shortcut)| details(name, shortcut))
        .collect::<Vec<_>>()
        .join("\n\n"))
}

pub fn remove_with_prompt<P: crate::interactive::Prompt>(
    path: &Path,
    name: Option<String>,
    yes: bool,
    prompt: &mut P,
) -> Result<Option<String>> {
    if yes && name.is_none() {
        bail!("--yes requires an explicit shortcut name");
    }
    if let Some(name) = &name {
        paths::validate_name(name).context("invalid shortcut name")?;
    }
    let file = load(path)?;
    let name = match name {
        Some(name) => name,
        None => {
            let choices: Vec<_> = file.shortcuts.keys().cloned().collect();
            if choices.is_empty() {
                prompt.message("No shortcuts are saved.")?;
                return Ok(None);
            }
            let Some(name) = prompt.select("Select a shortcut:", &choices)? else {
                return Ok(None);
            };
            name
        }
    };
    paths::validate_name(&name).context("invalid shortcut name")?;
    let shortcut = file
        .shortcuts
        .get(&name)
        .with_context(|| format!("shortcut '{name}' does not exist"))?;
    if !yes {
        prompt.message(&details(&name, shortcut))?;
        if prompt.confirm("Remove this shortcut?", false)? != Some(true) {
            return Ok(None);
        }
    }
    remove(path, &name)?;
    Ok(Some(name))
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

    fn fixture(path: &Path) {
        fs::write(path, r#"version = 1
future = "top-level"
[shortcuts.zebra]
catalog = "missing"
tool = "uninstalled"
path = "missing.sh"
[shortcuts.alpha]
catalog = "personal"
tool = "demo"
path = "alpha.sh"
description = "Alpha description"
runner = "bash"
future = "entry"
"#).unwrap();
    }

    #[test]
    fn listing_is_sorted_read_only_and_does_not_resolve_targets() {
        let temp = tempfile::TempDir::new().unwrap();
        let path = temp.path().join("shortcuts.toml");
        assert_eq!(list(&path).unwrap(), "No shortcuts are saved.");
        assert!(!path.exists());
        fixture(&path);
        let before = fs::read(&path).unwrap();
        let text = list(&path).unwrap();
        assert!(text.find("Name: alpha").unwrap() < text.find("Name: zebra").unwrap());
        assert!(text.contains("Catalog: personal\nTool: demo\nPath: alpha.sh\nDescription: Alpha description\nRunner: bash"));
        assert_eq!(fs::read(&path).unwrap(), before);
    }

    #[test]
    fn removal_preserves_metadata_and_retains_final_empty_file() {
        let temp = tempfile::TempDir::new().unwrap();
        let path = temp.path().join("shortcuts.toml");
        fixture(&path);
        let before = load(&path).unwrap();
        remove(&path, "zebra").unwrap();
        let after = load(&path).unwrap();
        assert_eq!(after.shortcuts.len(), 1);
        assert_eq!(after.shortcuts["alpha"], before.shortcuts["alpha"]);
        assert_eq!(after.extra, before.extra);
        assert_eq!(after.version, 1);
        remove(&path, "alpha").unwrap();
        assert!(path.is_file());
        let empty = load(&path).unwrap();
        assert!(empty.shortcuts.is_empty());
        assert_eq!(empty.extra, before.extra);
        assert_eq!(empty.version, 1);
        assert_eq!(list(&path).unwrap(), "No shortcuts are saved.");
    }

    struct RemovalPrompt {
        selection: Option<String>,
        confirmation: Option<bool>,
        selections: usize,
        messages: Vec<String>,
    }

    impl crate::interactive::Prompt for RemovalPrompt {
        fn input(&mut self, _: &str, _: Option<&str>) -> Result<Option<String>> {
            unreachable!()
        }
        fn select(&mut self, label: &str, choices: &[String]) -> Result<Option<String>> {
            assert_eq!(label, "Select a shortcut:");
            assert_eq!(choices, &["alpha", "zebra"]);
            self.selections += 1;
            Ok(self.selection.clone())
        }
        fn confirm(&mut self, label: &str, default: bool) -> Result<Option<bool>> {
            assert_eq!(label, "Remove this shortcut?");
            assert!(!default);
            assert!(self.messages.last().unwrap().starts_with("Name: alpha\n"));
            Ok(self.confirmation)
        }
        fn message(&mut self, text: &str) -> Result<()> {
            self.messages.push(text.to_owned());
            Ok(())
        }
    }

    #[test]
    fn removal_selection_confirmation_and_named_forms() {
        for (named, yes, selection, confirmation, removed) in [
            (false, false, Some("alpha"), Some(true), true),
            (true, false, None, Some(true), true),
            (true, true, None, None, true),
            (false, false, Some("alpha"), Some(false), false),
            (false, false, Some("alpha"), None, false),
            (false, false, None, None, false),
        ] {
            let temp = tempfile::TempDir::new().unwrap();
            let path = temp.path().join("shortcuts.toml");
            fixture(&path);
            let before = fs::read(&path).unwrap();
            let mut prompt = RemovalPrompt {
                selection: selection.map(str::to_owned), confirmation,
                selections: 0, messages: Vec::new(),
            };
            let result = remove_with_prompt(&path, named.then(|| "alpha".to_owned()), yes, &mut prompt).unwrap();
            assert_eq!(result.is_some(), removed);
            assert_eq!(prompt.selections, usize::from(!named));
            if !removed {
                assert_eq!(fs::read(&path).unwrap(), before);
            } else {
                assert!(!load(&path).unwrap().shortcuts.contains_key("alpha"));
            }
        }
    }

    #[test]
    fn invalid_or_missing_removal_does_not_write() {
        let temp = tempfile::TempDir::new().unwrap();
        let path = temp.path().join("shortcuts.toml");
        fixture(&path);
        let before = fs::read(&path).unwrap();
        for name in ["absent", "../invalid"] {
            assert!(remove(&path, name).is_err());
            assert_eq!(fs::read(&path).unwrap(), before);
        }
        fs::write(&path, "not valid toml").unwrap();
        assert!(remove(&path, "alpha").is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "not valid toml");
    }

    #[cfg(unix)]
    #[test]
    fn removal_and_listing_refuse_symlink_files() {
        let temp = tempfile::TempDir::new().unwrap();
        let target = temp.path().join("target.toml");
        let link = temp.path().join("shortcuts.toml");
        fixture(&target);
        let before = fs::read(&target).unwrap();
        std::os::unix::fs::symlink(&target, &link).unwrap();
        assert!(remove(&link, "alpha").unwrap_err().to_string().contains("symlink"));
        assert!(list(&link).is_err());
        assert_eq!(fs::read(&target).unwrap(), before);
    }

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
