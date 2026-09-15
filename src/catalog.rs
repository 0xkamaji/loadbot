use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::{config, paths};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CatalogFile {
    pub version: u32,
    #[serde(default)]
    pub tools: BTreeMap<String, ToolConfig>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, toml::Value>,
}

impl Default for CatalogFile {
    fn default() -> Self {
        Self {
            version: 1,
            tools: BTreeMap::new(),
            extra: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolConfig {
    #[serde(rename = "type")]
    pub source_type: SourceType,
    pub url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub revision: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub commands: BTreeMap<String, CommandConfig>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, toml::Value>,
}

impl ToolConfig {
    pub fn git(url: String, revision: Option<String>) -> Self {
        Self {
            source_type: SourceType::Git,
            url,
            revision,
            commands: BTreeMap::new(),
            extra: BTreeMap::new(),
        }
    }

    pub fn has_source(&self, other: &Self) -> bool {
        self.source_type == other.source_type
            && self.url == other.url
            && self.revision == other.revision
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct CommandConfig {
    pub invocation: crate::recipe::StoredInvocation,
    pub description: Option<String>,
    pub extra: BTreeMap<String, toml::Value>,
}

impl CommandConfig {
    pub fn legacy(path: String, runner: Option<Runner>) -> Self {
        Self {
            invocation: crate::recipe::StoredInvocation::legacy(path, runner),
            description: None,
            extra: BTreeMap::new(),
        }
    }

    pub fn legacy_invocation(&self) -> Option<&crate::recipe::LegacyInvocation> {
        self.invocation.as_legacy()
    }

    pub fn recipe(recipe: crate::recipe::RecipeDefinition) -> Result<Self> {
        recipe.validate()?;
        Ok(Self {
            invocation: crate::recipe::StoredInvocation::Recipe(recipe),
            description: None,
            extra: BTreeMap::new(),
        })
    }
}

#[derive(Serialize, Deserialize)]
struct CommandConfigWire {
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

impl Serialize for CommandConfig {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let (path, runner, recipe) = self.invocation.parts();
        CommandConfigWire {
            path,
            description: self.description.clone(),
            runner,
            recipe,
            extra: self.extra.clone(),
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for CommandConfig {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let wire = CommandConfigWire::deserialize(deserializer)?;
        let invocation =
            crate::recipe::StoredInvocation::from_parts(wire.path, wire.runner, wire.recipe)
                .map_err(serde::de::Error::custom)?;
        Ok(Self {
            invocation,
            description: wire.description,
            extra: wire.extra,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Runner {
    Direct,
    Bash,
    Sh,
    Python,
    Powershell,
}

impl Runner {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Direct => "direct",
            Self::Bash => "bash",
            Self::Sh => "sh",
            Self::Python => "python",
            Self::Powershell => "powershell",
        }
    }

    pub fn executable_candidates(self) -> &'static [&'static str] {
        match self {
            Self::Direct => &[],
            Self::Bash => &["bash"],
            Self::Sh => &["sh"],
            Self::Python if cfg!(windows) => &["python", "python3"],
            Self::Python => &["python3", "python"],
            Self::Powershell if cfg!(windows) => &["powershell", "pwsh"],
            Self::Powershell => &["pwsh", "powershell"],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SourceType {
    Git,
}

impl SourceType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Git => "git",
        }
    }
}

#[derive(Debug, Clone)]
pub struct ResolvedTool {
    pub name: String,
    pub catalog: String,
    pub definition: ToolConfig,
}

pub fn load(path: &Path) -> Result<CatalogFile> {
    reject_symlink(path)?;
    let contents = crate::persistence::read_optional(path)?
        .with_context(|| format!("could not read catalog file {}", path.display()))?;
    parse(&contents, path)
}

pub fn load_or_default(path: &Path) -> Result<CatalogFile> {
    reject_symlink(path)?;
    match crate::persistence::read_optional(path)? {
        Some(contents) => parse(&contents, path),
        None => Ok(CatalogFile::default()),
    }
}

pub fn save(path: &Path, catalog: &CatalogFile) -> Result<()> {
    validate(catalog, path)?;
    reject_symlink(path)?;
    config::save_toml(path, catalog)
}

/// Edit a catalog under its repository lease. Do not prompt or call repository
/// operations inside the callback. Whole-document `save` is a low-level writer.
pub fn update<T>(path: &Path, change: impl FnOnce(&mut CatalogFile) -> Result<T>) -> Result<T> {
    let repository = path
        .parent()
        .context("catalog path has no repository directory")?;
    let _lease = crate::persistence::Lease::acquire(repository)?;
    let mut catalog = load_or_default(path)?;
    let result = change(&mut catalog)?;
    save(path, &catalog)?;
    Ok(result)
}

fn reject_symlink(path: &Path) -> Result<()> {
    if fs::symlink_metadata(path).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
        bail!("refusing symlink catalog file {}", path.display());
    }
    Ok(())
}

fn parse(contents: &str, path: &Path) -> Result<CatalogFile> {
    let catalog: CatalogFile = toml::from_str(contents)
        .with_context(|| format!("could not parse catalog file {}", path.display()))?;
    validate(&catalog, path)?;
    Ok(catalog)
}

fn validate(catalog: &CatalogFile, path: &Path) -> Result<()> {
    if catalog.version != 1 {
        bail!(
            "unsupported catalog version {} in {}",
            catalog.version,
            path.display()
        );
    }
    for (tool_name, tool) in &catalog.tools {
        paths::validate_name(tool_name)
            .with_context(|| format!("catalog contains an unsafe tool name '{tool_name}'"))?;
        for (name, command) in &tool.commands {
            paths::validate_name(name)
                .with_context(|| format!("tool '{tool_name}' contains an unsafe command name"))?;
            match &command.invocation {
                crate::recipe::StoredInvocation::Legacy(legacy) => {
                    crate::shortcuts::relative_path(&legacy.path).with_context(|| {
                        format!("command '{name}' for tool '{tool_name}' contains an unsafe path")
                    })?;
                }
                crate::recipe::StoredInvocation::Recipe(recipe) => {
                    recipe.validate().with_context(|| {
                        format!(
                            "command '{name}' for tool '{tool_name}' contains an invalid recipe"
                        )
                    })?;
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_round_trips_and_preserves_unknown_fields() {
        let input = r#"
version = 1
owner = "team"

[tools.demo]
type = "git"
url = "https://example.test/demo.git"
revision = "main"
note = "keep me"
"#;
        let parsed: CatalogFile = toml::from_str(input).unwrap();
        let serialized = toml::to_string(&parsed).unwrap();
        let reparsed: CatalogFile = toml::from_str(&serialized).unwrap();

        assert_eq!(parsed, reparsed);
        assert_eq!(reparsed.extra["owner"].as_str(), Some("team"));
        assert_eq!(
            reparsed.tools["demo"].extra["note"].as_str(),
            Some("keep me")
        );
    }

    #[test]
    fn catalog_parses_commands_and_catalogs_without_commands() {
        let with_commands = parse(
            r#"version = 1

[tools.demo]
type = "git"
url = "https://example.test/demo.git"

[tools.demo.commands.audit]
path = "scripts/audit.sh"
description = "Audit the repository"
runner = "bash"
future = true
recipe_hint = { version = 1, arguments = ["--check"] }
"#,
            Path::new("catalog.toml"),
        )
        .unwrap();
        let command = &with_commands.tools["demo"].commands["audit"];
        assert_eq!(
            command.legacy_invocation().unwrap().path,
            "scripts/audit.sh"
        );
        assert_eq!(command.description.as_deref(), Some("Audit the repository"));
        assert_eq!(
            command.legacy_invocation().unwrap().runner,
            Some(Runner::Bash)
        );
        assert_eq!(command.extra["future"].as_bool(), Some(true));
        assert_eq!(
            command.extra["recipe_hint"]
                .get("arguments")
                .and_then(toml::Value::as_array)
                .and_then(|arguments| arguments.first())
                .and_then(toml::Value::as_str),
            Some("--check")
        );
        let serialized = toml::to_string(&with_commands).unwrap();
        let reparsed = parse(&serialized, Path::new("catalog.toml")).unwrap();
        assert_eq!(reparsed, with_commands);

        let without_commands = parse(
            "version = 1\n\n[tools.demo]\ntype = \"git\"\nurl = \"demo.git\"\n",
            Path::new("catalog.toml"),
        )
        .unwrap();
        assert!(without_commands.tools["demo"].commands.is_empty());
    }

    #[test]
    fn catalog_rejects_unsafe_command_paths_and_unsupported_runners() {
        for command in [
            "path = \"/tmp/run.sh\"",
            "path = \"scripts/../run.sh\"",
            "path = \"run.sh\"\nrunner = \"fish\"",
        ] {
            let input = format!(
                "version = 1\n\n[tools.demo]\ntype = \"git\"\nurl = \"demo.git\"\n\n[tools.demo.commands.run]\n{command}\n"
            );
            assert!(parse(&input, Path::new("catalog.toml")).is_err());
        }
    }

    #[test]
    fn catalog_recipe_round_trips_without_legacy_fields_and_mixed_forms_are_rejected() {
        let input = r#"version = 1

[tools.demo]
type = "git"
url = "demo.git"

[tools.demo.commands.triage]
description = "Triage a sample"
future = "preserved"

[tools.demo.commands.triage.recipe]
version = 1
behavior = "run"
program = { type = "interpreter", runner = "python" }
working_directory = { type = "project-root" }

[[tools.demo.commands.triage.recipe.arguments]]
type = "project-path"
path = "triage.py"
"#;
        let parsed = parse(input, Path::new("catalog.toml")).unwrap();
        let command = &parsed.tools["demo"].commands["triage"];
        assert!(command.legacy_invocation().is_none());
        assert_eq!(command.invocation.as_recipe().unwrap().version, 1);
        assert_eq!(command.extra["future"].as_str(), Some("preserved"));

        let serialized = toml::to_string(&parsed).unwrap();
        let value: toml::Value = toml::from_str(&serialized).unwrap();
        let entry = &value["tools"]["demo"]["commands"]["triage"];
        assert!(entry.get("path").is_none());
        assert!(entry.get("runner").is_none());
        assert_eq!(
            parse(&serialized, Path::new("catalog.toml")).unwrap(),
            parsed
        );

        #[derive(Deserialize)]
        #[allow(dead_code)]
        struct OldCommandConfig {
            path: String,
        }
        assert!(toml::from_str::<OldCommandConfig>(&toml::to_string(command).unwrap()).is_err());

        let mixed = input.replace(
            "description = \"Triage a sample\"",
            "description = \"Triage a sample\"\npath = \"triage.py\"",
        );
        let error = parse(&mixed, Path::new("catalog.toml")).unwrap_err();
        assert!(format!("{error:#}").contains("both a legacy path and a recipe"));
        let unsupported = input.replace(
            "version = 1\nbehavior = \"run\"",
            "version = 2\nbehavior = \"run\"",
        );
        let error = parse(&unsupported, Path::new("catalog.toml")).unwrap_err();
        assert!(format!("{error:#}").contains("unsupported recipe version 2"));
        let unsupported_runner = input.replace("runner = \"python\"", "runner = \"fish\"");
        assert!(parse(&unsupported_runner, Path::new("catalog.toml")).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn catalog_files_must_not_be_symlinks() {
        use std::os::unix::fs::symlink;

        let temporary = tempfile::TempDir::new().unwrap();
        let outside = temporary.path().join("outside.toml");
        let catalog_path = temporary.path().join("catalog.toml");
        fs::write(&outside, "version = 1\n").unwrap();
        symlink(outside, &catalog_path).unwrap();

        assert!(load(&catalog_path).is_err());
        assert!(save(&catalog_path, &CatalogFile::default()).is_err());
    }
}
