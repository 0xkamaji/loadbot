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

#[derive(Debug, Clone, PartialEq)]
pub struct Shortcut {
    pub catalog: String,
    pub tool: String,
    pub invocation: crate::recipe::StoredInvocation,
    pub description: Option<String>,
    pub extra: BTreeMap<String, toml::Value>,
}

#[derive(Serialize, Deserialize)]
struct ShortcutWire {
    catalog: String,
    tool: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    runner: Option<Runner>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    recipe: Option<crate::recipe::RecipeDefinition>,
    #[serde(flatten)]
    extra: BTreeMap<String, toml::Value>,
}

impl Serialize for Shortcut {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let (path, runner, recipe) = self.invocation.parts();
        ShortcutWire {
            catalog: self.catalog.clone(),
            tool: self.tool.clone(),
            path,
            description: self.description.clone(),
            runner,
            recipe,
            extra: self.extra.clone(),
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Shortcut {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let wire = ShortcutWire::deserialize(deserializer)?;
        let invocation =
            crate::recipe::StoredInvocation::from_parts(wire.path, wire.runner, wire.recipe)
                .map_err(serde::de::Error::custom)?;
        Ok(Self {
            catalog: wire.catalog,
            tool: wire.tool,
            invocation,
            description: wire.description,
            extra: wire.extra,
        })
    }
}

impl Shortcut {
    pub fn new(catalog: String, tool: String, path: String) -> Result<Self> {
        Self::with_invocation(
            catalog,
            tool,
            crate::recipe::StoredInvocation::legacy(path, None),
        )
    }

    pub fn with_invocation(
        catalog: String,
        tool: String,
        invocation: crate::recipe::StoredInvocation,
    ) -> Result<Self> {
        let shortcut = Self {
            catalog,
            tool,
            invocation,
            description: None,
            extra: BTreeMap::new(),
        };
        shortcut.validate()?;
        Ok(shortcut)
    }

    /// Validate the complete record, including fields changed after construction.
    pub fn validate(&self) -> Result<()> {
        self.validate_names()?;
        match &self.invocation {
            crate::recipe::StoredInvocation::Legacy(legacy) => {
                relative_path(&legacy.path)?;
            }
            crate::recipe::StoredInvocation::Recipe(recipe) => recipe.validate()?,
        }
        Ok(())
    }

    pub fn legacy(&self) -> Option<&crate::recipe::LegacyInvocation> {
        self.invocation.as_legacy()
    }

    pub fn set_legacy_runner(&mut self, runner: Option<Runner>) -> Result<()> {
        let legacy = self
            .invocation
            .as_legacy()
            .context("cannot set a legacy runner on a structured recipe")?;
        self.invocation = crate::recipe::StoredInvocation::legacy(legacy.path.clone(), runner);
        Ok(())
    }

    fn validate_names(&self) -> Result<()> {
        paths::validate_name(&self.catalog).context("invalid shortcut catalog")?;
        paths::validate_name(&self.tool).context("invalid shortcut tool")?;
        Ok(())
    }
}

pub fn load(path: &Path) -> Result<ShortcutFile> {
    reject_symlink(path)?;
    let Some(contents) = crate::persistence::read_optional(path)? else {
        return Ok(ShortcutFile::default());
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
        shortcut.validate_names()?;
        match &shortcut.invocation {
            crate::recipe::StoredInvocation::Legacy(legacy) => relative_path(&legacy.path)
                .with_context(|| format!("shortcut '{name}' contains an unsafe path"))
                .map(|_| ())?,
            crate::recipe::StoredInvocation::Recipe(recipe) => recipe
                .validate()
                .with_context(|| format!("shortcut '{name}' contains an invalid recipe"))?,
        }
    }
    Ok(shortcuts)
}

pub fn save(path: &Path, name: &str, shortcut: Shortcut) -> Result<()> {
    paths::validate_name(name).context("invalid shortcut name")?;
    shortcut.validate()?;
    let _lease = crate::persistence::Lease::acquire(path)?;
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
    remove_matching(path, name, None)
}

/// Apply a confirmed removal only if that exact definition still exists.
pub fn remove_if_matches(path: &Path, name: &str, expected: &Shortcut) -> Result<()> {
    remove_matching(path, name, Some(expected))
}

fn remove_matching(path: &Path, name: &str, expected: Option<&Shortcut>) -> Result<()> {
    paths::validate_name(name).context("invalid shortcut name")?;
    let _lease = crate::persistence::Lease::acquire(path)?;
    let mut file = load(path)?;
    if expected.is_some_and(|expected| file.shortcuts.get(name) != Some(expected)) {
        return Err(crate::persistence::Busy {
            resource: path.to_owned(),
        }
        .into());
    }
    if file.shortcuts.remove(name).is_none() {
        bail!("shortcut '{name}' does not exist");
    }
    reject_symlink(path)?;
    config::save_toml(path, &file)
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
        assert!(contents.contains("path = \"recipes/print_strings.py\""));
        assert!(!contents.contains("[shortcuts.print-strings.recipe]"));
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
recipe_hint = { version = 1, arguments = ["--check"] }
"#,
        )
        .unwrap();

        let loaded = load(&path).unwrap();
        assert_eq!(loaded.shortcuts["legacy"].description, None);
        assert_eq!(loaded.shortcuts["legacy"].legacy().unwrap().runner, None);
        assert_eq!(
            loaded.shortcuts["audit"].description.as_deref(),
            Some("Run the audit")
        );
        assert_eq!(
            loaded.shortcuts["audit"].legacy().unwrap().runner,
            Some(Runner::Bash)
        );
        assert_eq!(
            loaded.shortcuts["audit"].extra["future"].as_str(),
            Some("kept")
        );
        assert_eq!(
            loaded.shortcuts["audit"].extra["recipe_hint"]
                .get("arguments")
                .and_then(toml::Value::as_array)
                .and_then(|arguments| arguments.first())
                .and_then(toml::Value::as_str),
            Some("--check")
        );

        let new = Shortcut::new("personal".into(), "demo".into(), "new.sh".into()).unwrap();
        save(&path, "new", new).unwrap();
        let after_save = load(&path).unwrap();
        assert_eq!(
            after_save.shortcuts["audit"].extra["recipe_hint"],
            loaded.shortcuts["audit"].extra["recipe_hint"]
        );
        assert_eq!(
            after_save.shortcuts["audit"].legacy(),
            loaded.shortcuts["audit"].legacy()
        );
        assert!(
            after_save.shortcuts["audit"]
                .invocation
                .as_recipe()
                .is_none()
        );
    }

    #[test]
    fn personal_recipe_saves_atomically_without_legacy_fields_and_preserves_metadata() {
        use crate::recipe::{
            InvocationBehavior, RecipeArgument, RecipeDefinition, RecipeProgram, StoredInvocation,
            WorkingDirectory,
        };

        let temporary = tempfile::TempDir::new().unwrap();
        let path = temporary.path().join("shortcuts.toml");
        let recipe = RecipeDefinition {
            version: 1,
            behavior: InvocationBehavior::Launch,
            program: RecipeProgram::ProjectFile {
                path: "bin/tool.exe".into(),
            },
            working_directory: WorkingDirectory::TargetParent,
            arguments: vec![RecipeArgument::Literal {
                value: "one value".into(),
            }],
        };
        let mut shortcut = Shortcut::with_invocation(
            "personal".into(),
            "demo".into(),
            StoredInvocation::Recipe(recipe.clone()),
        )
        .unwrap();
        shortcut
            .extra
            .insert("future".into(), toml::Value::String("kept".into()));
        save(&path, "launch", shortcut).unwrap();

        let serialized = fs::read_to_string(&path).unwrap();
        let value: toml::Value = toml::from_str(&serialized).unwrap();
        let entry = &value["shortcuts"]["launch"];
        assert!(entry.get("path").is_none());
        assert!(entry.get("runner").is_none());
        let loaded = load(&path).unwrap();
        assert_eq!(
            loaded.shortcuts["launch"].invocation.as_recipe(),
            Some(&recipe)
        );
        assert_eq!(
            loaded.shortcuts["launch"].extra["future"].as_str(),
            Some("kept")
        );
        #[derive(Deserialize)]
        #[allow(dead_code)]
        struct OldShortcut {
            catalog: String,
            tool: String,
            path: String,
        }
        assert!(
            toml::from_str::<OldShortcut>(&toml::to_string(&loaded.shortcuts["launch"]).unwrap())
                .is_err()
        );

        let before = fs::read(&path).unwrap();
        let mixed = serialized.replace(
            "tool = \"demo\"",
            "tool = \"demo\"\npath = \"bin/tool.exe\"",
        );
        fs::write(&path, mixed).unwrap();
        let error = load(&path).unwrap_err();
        assert!(format!("{error:#}").contains("both a legacy path and a recipe"));
        fs::write(&path, before).unwrap();
        assert_eq!(
            load(&path).unwrap().shortcuts["launch"]
                .invocation
                .as_recipe(),
            Some(&recipe)
        );
    }
}
