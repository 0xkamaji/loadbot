use std::collections::BTreeMap;
#[cfg(test)]
use std::fs;
use std::path::Path;
use crate::persistence::read_optional;

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

/// Low-level whole-document replacement. Use `update` for read-modify-write.
pub fn save_toml<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    crate::persistence::write_toml(path, value)
}

/// The callback must not prompt or perform long-running work.
pub fn update<T>(path: &Path, change: impl FnOnce(&mut LocalConfig) -> Result<T>) -> Result<T> {
    let _lease = crate::persistence::Lease::acquire(path)?;
    let mut value = load(path)?;
    let result = change(&mut value)?;
    save(path, &value)?;
    Ok(result)
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

    #[test]
    fn loading_missing_catalog_registrations_does_not_mutate_configuration() {
        let root =
            std::env::temp_dir().join(format!("loadbot-config-reconcile-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("catalogs/real")).unwrap();
        let path = root.join("config.toml");
        fs::write(
            &path,
            r#"
version = 1
default_catalog = "stale"

[catalogs.real]
url = "https://example.test/real.git"
writable = true

[catalogs.stale]
url = "https://example.test/stale.git"
writable = true
"#,
        )
        .unwrap();

        let loaded = load(&path).unwrap();
        assert!(loaded.catalogs.contains_key("real"));
        assert!(loaded.catalogs.contains_key("stale"));
        assert_eq!(loaded.default_catalog.as_deref(), Some("stale"));

        let persisted = fs::read_to_string(&path).unwrap();
        assert!(persisted.contains("[catalogs.real]"));
        assert!(persisted.contains("[catalogs.stale]"));
        assert!(persisted.contains("default_catalog = \"stale\""));

        let _ = fs::remove_dir_all(&root);
    }
}
