use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::catalog::ToolConfig;
use crate::paths;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LocalConfig {
    pub version: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_catalog: Option<String>,
    #[serde(default)]
    pub catalogs: BTreeMap<String, CatalogSource>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, toml::Value>,
}

impl Default for LocalConfig {
    fn default() -> Self {
        Self {
            version: 1,
            default_catalog: None,
            catalogs: BTreeMap::new(),
            extra: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CatalogSource {
    pub url: String,
    #[serde(default)]
    pub writable: bool,
    #[serde(flatten)]
    pub extra: BTreeMap<String, toml::Value>,
}

impl CatalogSource {
    pub fn new(url: String, writable: bool) -> Self {
        Self {
            url,
            writable,
            extra: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LegacyConfig {
    pub version: u32,
    pub tools: BTreeMap<String, ToolConfig>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, toml::Value>,
}

pub fn load(path: &Path) -> Result<LocalConfig> {
    let Some(contents) = read_optional(path)? else {
        return Ok(LocalConfig::default());
    };
    let value: toml::Value =
        toml::from_str(&contents).with_context(|| format!("could not parse {}", path.display()))?;
    if value.get("tools").is_some() {
        bail!(
            "legacy [tools] configuration detected in {}; migrate it with 'loadbot catalog migrate NAME GIT_URL'",
            path.display()
        );
    }
    let config: LocalConfig = value
        .try_into()
        .with_context(|| format!("could not parse {}", path.display()))?;
    validate_version(config.version, path)?;
    for name in config.catalogs.keys() {
        paths::validate_name(name)
            .with_context(|| format!("configuration contains an unsafe catalog name '{name}'"))?;
    }
    Ok(config)
}

pub fn load_legacy(path: &Path) -> Result<LegacyConfig> {
    let contents = read_optional(path)?
        .with_context(|| format!("no legacy configuration exists at {}", path.display()))?;
    let config: LegacyConfig = toml::from_str(&contents)
        .with_context(|| format!("could not parse legacy configuration {}", path.display()))?;
    validate_version(config.version, path)?;
    if config.tools.is_empty() {
        bail!("legacy configuration contains no tool definitions");
    }
    Ok(config)
}

pub fn save(path: &Path, config: &LocalConfig) -> Result<()> {
    save_toml(path, config)
}

pub fn save_toml<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let parent = path
        .parent()
        .context("configuration path has no parent directory")?;
    fs::create_dir_all(parent).with_context(|| format!("could not create {}", parent.display()))?;

    let contents = toml::to_string_pretty(value).context("could not serialize configuration")?;
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

fn read_optional(path: &Path) -> Result<Option<String>> {
    match fs::read_to_string(path) {
        Ok(contents) => Ok(Some(contents)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error).with_context(|| format!("could not read {}", path.display())),
    }
}

fn validate_version(version: u32, path: &Path) -> Result<()> {
    if version != 1 {
        bail!(
            "unsupported configuration version {version} in {}",
            path.display()
        );
    }
    Ok(())
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
    fn local_configuration_round_trips_and_preserves_unknown_fields() {
        let input = r#"
version = 1
default_catalog = "personal"
theme = "plain"

[catalogs.personal]
url = "https://example.test/catalog.git"
writable = true
note = "keep me"
"#;
        let parsed: LocalConfig = toml::from_str(input).unwrap();
        let serialized = toml::to_string(&parsed).unwrap();
        let reparsed: LocalConfig = toml::from_str(&serialized).unwrap();

        assert_eq!(parsed, reparsed);
        assert_eq!(reparsed.extra["theme"].as_str(), Some("plain"));
        assert_eq!(
            reparsed.catalogs["personal"].extra["note"].as_str(),
            Some("keep me")
        );
    }
}
