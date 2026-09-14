use std::path::Path;
use anyhow::{Context, Result, bail};
use loadbot::{paths, shortcuts::{load, remove, Shortcut}};
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

pub fn remove_with_prompt<P: super::menus::Prompt>(
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

#[cfg(test)]
mod tests {
use super::*;
use std::fs;
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

    impl super::super::menus::Prompt for RemovalPrompt {
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

}
