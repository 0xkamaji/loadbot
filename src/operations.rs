use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use anyhow::{Context, Result, bail};

use crate::catalog::{self, CatalogFile, ResolvedTool, ToolConfig};
use crate::config::{self, CatalogSource, LocalConfig};
use crate::git;
use crate::paths::{self, Paths};

pub fn catalog_add(paths: &Paths, name: &str, url: String, writable: bool) -> Result<()> {
    paths::validate_name(name)?;
    validate_url(&url)?;
    let mut local = config::load(&paths.config())?;
    let source = CatalogSource::new(url, writable);
    let destination = paths.catalog(name);
    let existing_differs = local
        .catalogs
        .get(name)
        .is_some_and(|existing| existing.url != source.url || existing.writable != source.writable);
    if existing_differs && path_exists(&destination) {
        bail!("catalog '{name}' is already configured with different settings");
    }
    for existing_name in local.catalogs.keys() {
        if existing_name != name && existing_name.eq_ignore_ascii_case(name) {
            bail!("catalog name '{name}' conflicts with configured catalog '{existing_name}'");
        }
    }
    let installed = if path_exists(&destination) {
        if git::is_expected_repository(&destination, &source.url)? {
            true
        } else if git::is_repository(&destination)? {
            bail!("catalog destination exists but is not the configured Git repository");
        } else {
            bail!("catalog destination exists but is not a Git repository");
        }
    } else {
        false
    };

    let registration_changed = !local.catalogs.contains_key(name) || existing_differs;
    if registration_changed {
        local.catalogs.insert(name.to_owned(), source.clone());
        if local.default_catalog.is_none() {
            local.default_catalog = Some(name.to_owned());
        }
        config::save(&paths.config(), &local)?;
        println!("registered catalog '{name}'");
    } else {
        println!("catalog '{name}' is already registered");
    }

    if installed {
        println!("catalog '{name}' is already installed");
        return Ok(());
    }

    fs::create_dir_all(paths.catalogs())
        .with_context(|| format!("could not create {}", paths.catalogs().display()))?;
    if let Err(error) = git::clone_repository(&source.url, None, &destination) {
        cleanup_failed_clone(&destination);
        let context = if registration_changed {
            format!("catalog '{name}' was registered, but cloning it failed")
        } else {
            format!("could not clone catalog '{name}'")
        };
        return Err(error).context(context);
    }
    println!("installed catalog '{name}' at {}", destination.display());
    Ok(())
}

pub fn catalog_list(paths: &Paths) -> Result<()> {
    let local = config::load(&paths.config())?;
    if local.catalogs.is_empty() {
        println!("no catalogs configured");
        return Ok(());
    }

    println!("NAME\tSTATE\tACCESS\tDEFAULT\tURL");
    for (name, source) in &local.catalogs {
        let destination = paths.catalog(name);
        let state = if !path_exists(&destination) {
            "missing"
        } else if git::is_expected_repository(&destination, &source.url)? {
            "installed"
        } else {
            "mismatch"
        };
        println!(
            "{name}\t{state}\t{}\t{}\t{}",
            if source.writable {
                "writable"
            } else {
                "read-only"
            },
            if local.default_catalog.as_deref() == Some(name) {
                "yes"
            } else {
                "no"
            },
            source.url
        );
    }
    Ok(())
}

pub fn catalog_sync(paths: &Paths, name: &str) -> Result<()> {
    let local = config::load(&paths.config())?;
    let source = configured_catalog(&local, name)?;
    let destination = checked_catalog_repository(paths, name, source)?;
    let (old_commit, new_commit) = git::update(&destination, None)
        .with_context(|| format!("refusing to sync catalog '{name}'"))?;
    if old_commit == new_commit {
        println!("catalog '{name}' is already current at {new_commit}");
    } else {
        println!("synchronized catalog '{name}' from {old_commit} to {new_commit}");
    }
    Ok(())
}

pub fn catalog_status(paths: &Paths, name: &str) -> Result<()> {
    let local = config::load(&paths.config())?;
    let source = configured_catalog(&local, name)?;
    let destination = paths.catalog(name);

    println!("Name: {name}");
    println!("Path: {}", destination.display());
    println!("Configured URL: {}", source.url);
    println!("Writable: {}", if source.writable { "yes" } else { "no" });

    let is_repository = path_exists(&destination) && git::is_repository(&destination)?;
    if is_repository {
        let repository = git::status(&destination)?;
        println!(
            "Current branch: {}",
            repository.branch.as_deref().unwrap_or("(detached)")
        );
        println!("Current commit: {}", repository.commit);
        println!(
            "Working tree: {}",
            if repository.dirty { "dirty" } else { "clean" }
        );
        println!(
            "Origin URL: {}",
            repository.origin.as_deref().unwrap_or("(none)")
        );
    } else {
        println!("Current branch: -");
        println!("Current commit: -");
        println!("Working tree: -");
        println!("Origin URL: -");
    }

    let catalog_path = paths.catalog_file(name);
    if path_exists(&destination) && !is_repository {
        println!("Catalog file: unavailable (destination is not a managed repository)");
    } else if !catalog_path.is_file() {
        println!("Catalog file: missing");
    } else {
        match catalog::load(&catalog_path) {
            Ok(_) => println!("Catalog file: valid"),
            Err(error) => println!("Catalog file: invalid ({error:#})"),
        }
    }
    Ok(())
}

pub fn catalog_path(paths: &Paths, name: &str) -> Result<()> {
    let local = config::load(&paths.config())?;
    configured_catalog(&local, name)?;
    println!("{}", paths.catalog(name).display());
    Ok(())
}

pub fn catalog_migrate(paths: &Paths, name: &str, url: String) -> Result<()> {
    paths::validate_name(name)?;
    validate_url(&url)?;
    let legacy = config::load_legacy(&paths.config())?;
    let destination = paths.catalog(name);
    if path_exists(&destination) {
        bail!("refusing migration: catalog destination already exists");
    }

    fs::create_dir_all(paths.catalogs())
        .with_context(|| format!("could not create {}", paths.catalogs().display()))?;
    if let Err(error) = git::clone_repository(&url, None, &destination) {
        cleanup_failed_clone(&destination);
        return Err(error).context("could not clone migration catalog");
    }

    let migration_result = (|| -> Result<()> {
        let catalog_path = paths.catalog_file(name);
        if path_exists(&catalog_path) {
            bail!(
                "refusing migration: {} already exists; legacy configuration was not changed",
                catalog_path.display()
            );
        }
        let catalog_file = CatalogFile {
            version: 1,
            tools: legacy.tools,
            extra: BTreeMap::new(),
        };
        catalog::save(&catalog_path, &catalog_file)
            .context("catalog was cloned, but writing catalog.toml failed")?;

        let mut catalogs = BTreeMap::new();
        catalogs.insert(name.to_owned(), CatalogSource::new(url, true));
        let local = LocalConfig {
            version: 1,
            default_catalog: Some(name.to_owned()),
            catalogs,
            extra: legacy.extra,
        };
        config::save(&paths.config(), &local)
            .context("catalog.toml was written, but replacing the legacy configuration failed")
    })();
    if let Err(error) = migration_result {
        cleanup_failed_clone(&destination);
        return Err(error);
    }
    println!("migrated legacy tools to writable catalog '{name}'");
    println!("catalog changes were not committed or pushed");
    Ok(())
}

pub fn tool_add(
    paths: &Paths,
    catalog_name: &str,
    name: &str,
    url: String,
    revision: Option<String>,
    commit: bool,
    push: bool,
) -> Result<()> {
    paths::validate_name(name)?;
    paths::validate_name(catalog_name)?;
    validate_url(&url)?;
    if revision.as_deref() == Some("") {
        bail!("revision must not be empty");
    }
    if push && !commit {
        bail!("pushing a catalog change requires --commit");
    }

    let local = config::load(&paths.config())?;
    let source = configured_catalog(&local, catalog_name)?;
    if !source.writable {
        bail!("catalog '{catalog_name}' is read-only");
    }
    let repository = checked_catalog_repository(paths, catalog_name, source)?;
    let catalog_path = paths.catalog_file(catalog_name);
    let mut catalog_file = catalog::load_or_default(&catalog_path)?;
    let definition = ToolConfig::git(url, revision);
    let changed = if let Some(existing) = catalog_file.tools.get(name) {
        if existing == &definition {
            println!("tool '{name}' is already defined in catalog '{catalog_name}'");
            false
        } else {
            bail!(
                "tool '{name}' already exists in catalog '{catalog_name}' with different settings"
            );
        }
    } else {
        for existing_name in catalog_file.tools.keys() {
            if existing_name.eq_ignore_ascii_case(name) {
                bail!("tool name '{name}' conflicts with existing tool '{existing_name}'");
            }
        }
        catalog_file.tools.insert(name.to_owned(), definition);
        catalog::save(&catalog_path, &catalog_file)?;
        println!("added tool '{name}' to catalog '{catalog_name}'");
        true
    };

    if commit {
        if git::path_has_changes(&repository, "catalog.toml")? {
            let message = format!("Add {name} to Loadbot catalog");
            let commit_hash = git::commit_file(&repository, "catalog.toml", &message)
                .context("tool definition was saved, but committing the catalog change failed")?;
            println!("committed catalog change at {commit_hash}");
        } else if changed {
            bail!("catalog changed but Git did not detect a catalog.toml modification");
        } else {
            println!("catalog change is already committed");
        }
    }
    if push {
        git::push_origin(&repository)
            .context("tool definition was saved and committed, but pushing the catalog failed")?;
        println!("pushed catalog '{catalog_name}' to origin");
    }
    Ok(())
}

pub fn tool_list(paths: &Paths) -> Result<()> {
    let tools = all_tools(paths)?;
    if tools.is_empty() {
        println!("no tools configured in registered catalogs");
        return Ok(());
    }
    let mut counts = BTreeMap::new();
    for tool in &tools {
        *counts.entry(tool.name.clone()).or_insert(0usize) += 1;
    }

    println!("NAME\tCATALOG\tTYPE\tSTATE\tREVISION\tAMBIGUOUS");
    for tool in tools {
        let destination = paths.tool(&tool.name);
        let installed = if path_exists(&destination) {
            git::is_expected_repository(&destination, &tool.definition.url)?
        } else {
            false
        };
        println!(
            "{}\t{}\t{}\t{}\t{}\t{}",
            tool.name,
            tool.catalog,
            tool.definition.source_type.as_str(),
            if installed { "installed" } else { "missing" },
            tool.definition.revision.as_deref().unwrap_or("-"),
            if counts[&tool.name] > 1 { "yes" } else { "no" }
        );
    }
    Ok(())
}

pub fn tool_pull(paths: &Paths, name: &str, catalog_name: Option<&str>) -> Result<()> {
    let tool = resolve_tool(paths, name, catalog_name)?;
    let destination = paths.tool(name);
    if path_exists(&destination) {
        if git::is_expected_repository(&destination, &tool.definition.url)? {
            println!("tool '{name}' is already installed");
            return Ok(());
        }
        if git::is_repository(&destination)? {
            bail!("destination exists but is not the configured Git repository");
        }
        bail!("destination exists but is not a Git repository");
    }

    fs::create_dir_all(paths.tools())
        .with_context(|| format!("could not create {}", paths.tools().display()))?;
    if let Err(error) = git::clone_repository(
        &tool.definition.url,
        tool.definition.revision.as_deref(),
        &destination,
    ) {
        cleanup_failed_clone(&destination);
        return Err(error).context(format!("could not clone tool '{name}'"));
    }
    println!("installed tool '{name}' at {}", destination.display());
    Ok(())
}

pub fn tool_update(paths: &Paths, name: &str, catalog_name: Option<&str>) -> Result<()> {
    let tool = resolve_tool(paths, name, catalog_name)?;
    let destination = paths.tool(name);
    if !path_exists(&destination) {
        bail!("tool '{name}' is not installed; run 'loadbot pull {name}' first");
    }
    if !git::is_repository(&destination)? {
        bail!("destination exists but is not a Git repository");
    }
    if !git::is_expected_repository(&destination, &tool.definition.url)? {
        bail!("destination is not the configured Git repository");
    }

    let (old_commit, new_commit) = git::update(&destination, tool.definition.revision.as_deref())
        .with_context(|| format!("refusing to update '{name}'"))?;
    if old_commit == new_commit {
        println!("tool '{name}' is already current at {new_commit}");
    } else {
        println!("updated tool '{name}' from {old_commit} to {new_commit}");
    }
    Ok(())
}

pub fn tool_status(paths: &Paths, name: &str, catalog_name: Option<&str>) -> Result<()> {
    let tool = resolve_tool(paths, name, catalog_name)?;
    let destination = paths.tool(name);
    println!("Name: {name}");
    println!("Catalog: {}", tool.catalog);
    println!("Path: {}", destination.display());
    println!(
        "Installed: {}",
        if path_exists(&destination)
            && git::is_expected_repository(&destination, &tool.definition.url)?
        {
            "yes"
        } else {
            "no"
        }
    );
    println!("Configured URL: {}", tool.definition.url);
    println!(
        "Configured revision: {}",
        tool.definition.revision.as_deref().unwrap_or("(default)")
    );

    if path_exists(&destination) && git::is_repository(&destination)? {
        let repository = git::status(&destination)?;
        println!(
            "Current branch: {}",
            repository.branch.as_deref().unwrap_or("(detached)")
        );
        println!("Current commit: {}", repository.commit);
        println!(
            "Working tree: {}",
            if repository.dirty { "dirty" } else { "clean" }
        );
        println!(
            "Origin URL: {}",
            repository.origin.as_deref().unwrap_or("(none)")
        );
    } else {
        println!("Current branch: -");
        println!("Current commit: -");
        println!("Working tree: -");
        println!("Origin URL: -");
    }
    Ok(())
}

pub fn tool_path(paths: &Paths, name: &str, catalog_name: Option<&str>) -> Result<()> {
    resolve_tool(paths, name, catalog_name)?;
    println!("{}", paths.tool(name).display());
    Ok(())
}

pub fn all_tools(paths: &Paths) -> Result<Vec<ResolvedTool>> {
    let local = config::load(&paths.config())?;
    let mut tools = Vec::new();
    let mut portable_names = BTreeMap::new();
    for (catalog_name, source) in &local.catalogs {
        if let Err(error) = checked_catalog_repository(paths, catalog_name, source) {
            eprintln!("warning: skipping catalog '{catalog_name}': {error:#}");
            continue;
        }
        let catalog_file = match catalog::load(&paths.catalog_file(catalog_name)) {
            Ok(catalog_file) => catalog_file,
            Err(error) => {
                eprintln!("warning: skipping catalog '{catalog_name}': {error:#}");
                continue;
            }
        };
        let mut normalized_names = BTreeMap::new();
        for (name, definition) in catalog_file.tools {
            paths::validate_name(&name).with_context(|| {
                format!("catalog '{catalog_name}' contains an unsafe tool name")
            })?;
            let normalized = name.to_ascii_lowercase();
            if let Some(existing) = normalized_names.insert(normalized, name.clone()) {
                bail!(
                    "catalog '{catalog_name}' contains case-insensitive tool-name collision '{existing}' and '{name}'"
                );
            }
            let portable_name = name.to_ascii_lowercase();
            if let Some(existing) = portable_names.insert(portable_name, name.clone())
                && existing != name
            {
                bail!(
                    "tool names '{existing}' and '{name}' conflict on case-insensitive filesystems"
                );
            }
            tools.push(ResolvedTool {
                name,
                catalog: catalog_name.clone(),
                definition,
            });
        }
    }
    tools.sort_by(|left, right| {
        left.name
            .cmp(&right.name)
            .then_with(|| left.catalog.cmp(&right.catalog))
    });
    Ok(tools)
}

pub fn resolve_tool(paths: &Paths, name: &str, catalog_name: Option<&str>) -> Result<ResolvedTool> {
    paths::validate_name(name)?;
    if let Some(catalog_name) = catalog_name {
        paths::validate_name(catalog_name)?;
        let local = config::load(&paths.config())?;
        let source = configured_catalog(&local, catalog_name)?;
        checked_catalog_repository(paths, catalog_name, source)?;
        let catalog_file = catalog::load(&paths.catalog_file(catalog_name))?;
        let definition =
            catalog_file.tools.get(name).cloned().with_context(|| {
                format!("tool '{name}' is not defined in catalog '{catalog_name}'")
            })?;
        return Ok(ResolvedTool {
            name: name.to_owned(),
            catalog: catalog_name.to_owned(),
            definition,
        });
    }

    let matches: Vec<_> = all_tools(paths)?
        .into_iter()
        .filter(|tool| tool.name == name)
        .collect();
    match matches.as_slice() {
        [] => bail!("tool '{name}' is not configured in any catalog"),
        [tool] => Ok(tool.clone()),
        _ => {
            let catalogs = matches
                .iter()
                .map(|tool| tool.catalog.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            bail!(
                "tool '{name}' is ambiguous across catalogs: {catalogs}; specify --catalog <name>"
            )
        }
    }
}

pub fn writable_catalogs(paths: &Paths) -> Result<Vec<String>> {
    let local = config::load(&paths.config())?;
    Ok(local
        .catalogs
        .into_iter()
        .filter_map(|(name, source)| source.writable.then_some(name))
        .collect())
}

pub fn default_writable_catalog(paths: &Paths) -> Result<Option<String>> {
    let local = config::load(&paths.config())?;
    let Some(name) = local.default_catalog else {
        return Ok(None);
    };
    Ok(local
        .catalogs
        .get(&name)
        .is_some_and(|source| source.writable)
        .then_some(name))
}

pub fn catalog_names(paths: &Paths) -> Result<Vec<String>> {
    Ok(config::load(&paths.config())?
        .catalogs
        .into_keys()
        .collect())
}

fn configured_catalog<'a>(local: &'a LocalConfig, name: &str) -> Result<&'a CatalogSource> {
    paths::validate_name(name)?;
    local
        .catalogs
        .get(name)
        .with_context(|| format!("catalog '{name}' is not configured"))
}

fn checked_catalog_repository(
    paths: &Paths,
    name: &str,
    source: &CatalogSource,
) -> Result<std::path::PathBuf> {
    let destination = paths.catalog(name);
    if !path_exists(&destination) {
        bail!("catalog '{name}' is not installed");
    }
    if !git::is_repository(&destination)? {
        bail!("catalog destination exists but is not a Git repository");
    }
    if !git::is_expected_repository(&destination, &source.url)? {
        bail!("catalog destination is not the configured Git repository");
    }
    Ok(destination)
}

fn validate_url(url: &str) -> Result<()> {
    if url.is_empty() {
        bail!("Git URL must not be empty");
    }
    Ok(())
}

fn cleanup_failed_clone(destination: &Path) {
    let Ok(metadata) = fs::symlink_metadata(destination) else {
        return;
    };
    if metadata.is_dir() && !metadata.file_type().is_symlink() {
        let _ = fs::remove_dir_all(destination);
    } else {
        let _ = fs::remove_file(destination);
    }
}

fn path_exists(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok()
}
