use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::{config, paths, shortcuts};

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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CommandConfig {
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runner: Option<Runner>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, toml::Value>,
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
    let contents = fs::read_to_string(path)
        .with_context(|| format!("could not read catalog file {}", path.display()))?;
    parse(&contents, path)
}

pub fn load_or_default(path: &Path) -> Result<CatalogFile> {
    reject_symlink(path)?;
    match fs::read_to_string(path) {
        Ok(contents) => parse(&contents, path),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(CatalogFile::default()),
        Err(error) => {
            Err(error).with_context(|| format!("could not read catalog file {}", path.display()))
        }
    }
}

pub fn save(path: &Path, catalog: &CatalogFile) -> Result<()> {
    reject_symlink(path)?;
    config::save_toml(path, catalog)
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
            shortcuts::relative_path(&command.path).with_context(|| {
                format!("command '{name}' for tool '{tool_name}' contains an unsafe path")
            })?;
        }
    }
    Ok(catalog)
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
"#,
            Path::new("catalog.toml"),
        )
        .unwrap();
        let command = &with_commands.tools["demo"].commands["audit"];
        assert_eq!(command.path, "scripts/audit.sh");
        assert_eq!(command.description.as_deref(), Some("Audit the repository"));
        assert_eq!(command.runner, Some(Runner::Bash));
        assert_eq!(command.extra["future"].as_bool(), Some(true));

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
