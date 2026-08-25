use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Config {
    pub version: u32,
    #[serde(default)]
    pub tools: BTreeMap<String, ToolConfig>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, toml::Value>,
}

impl Default for Config {
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
    #[serde(flatten)]
    pub extra: BTreeMap<String, toml::Value>,
}

impl ToolConfig {
    pub fn git(url: String, revision: Option<String>) -> Self {
        Self {
            source_type: SourceType::Git,
            url,
            revision,
            extra: BTreeMap::new(),
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

pub fn load(path: &Path) -> Result<Config> {
    let contents = match fs::read_to_string(path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Config::default()),
        Err(error) => {
            return Err(error).with_context(|| format!("could not read {}", path.display()));
        }
    };

    let config: Config =
        toml::from_str(&contents).with_context(|| format!("could not parse {}", path.display()))?;
    if config.version != 1 {
        bail!(
            "unsupported configuration version {} in {}",
            config.version,
            path.display()
        );
    }
    Ok(config)
}

pub fn save(path: &Path, config: &Config) -> Result<()> {
    let parent = path
        .parent()
        .context("configuration path has no parent directory")?;
    fs::create_dir_all(parent).with_context(|| format!("could not create {}", parent.display()))?;

    let contents = toml::to_string_pretty(config).context("could not serialize configuration")?;
    let temporary = temporary_path(path);
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .with_context(|| format!("could not create {}", temporary.display()))?;

    let write_result = (|| -> Result<()> {
        file.write_all(contents.as_bytes())?;
        file.sync_all()?;
        drop(file);
        replace_file(&temporary, path)?;
        Ok(())
    })();

    if write_result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    write_result.with_context(|| format!("could not write {}", path.display()))
}

fn temporary_path(path: &Path) -> PathBuf {
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("config.toml");
    path.with_file_name(format!(".{file_name}.{}.tmp", std::process::id()))
}

#[cfg(not(windows))]
fn replace_file(source: &Path, destination: &Path) -> std::io::Result<()> {
    fs::rename(source, destination)
}

#[cfg(windows)]
fn replace_file(source: &Path, destination: &Path) -> std::io::Result<()> {
    if !destination.exists() {
        return fs::rename(source, destination);
    }

    let backup = destination.with_extension("toml.loadbot-backup");
    fs::rename(destination, &backup)?;
    match fs::rename(source, destination) {
        Ok(()) => {
            let _ = fs::remove_file(backup);
            Ok(())
        }
        Err(error) => {
            let _ = fs::rename(backup, destination);
            Err(error)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn configuration_round_trips_and_preserves_extra_fields() {
        let input = r#"
version = 1
owner = "local"

[tools.demo]
type = "git"
url = "https://example.test/demo.git"
revision = "main"
note = "keep me"
"#;
        let parsed: Config = toml::from_str(input).unwrap();
        let serialized = toml::to_string(&parsed).unwrap();
        let reparsed: Config = toml::from_str(&serialized).unwrap();

        assert_eq!(parsed, reparsed);
        assert_eq!(reparsed.extra["owner"].as_str(), Some("local"));
        assert_eq!(
            reparsed.tools["demo"].extra["note"].as_str(),
            Some("keep me")
        );
    }
}
