use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use crate::catalog::{self, CatalogFile, ResolvedTool, ToolConfig};
use crate::config::{self, CatalogSource, LocalConfig};
use crate::git;
use crate::paths::{self, Paths};

pub fn catalog_add(paths: &Paths, name: &str, url: String, writable: bool) -> Result<()> {
    catalog_add_with_save(paths, name, url, writable, config::save)
}

fn catalog_add_with_save<F>(
    paths: &Paths,
    name: &str,
    url: String,
    writable: bool,
    save_config: F,
) -> Result<()>
where
    F: FnOnce(&Path, &LocalConfig) -> Result<()>,
{
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
    let mut created_clone = false;
    if path_exists(&destination) {
        if git::is_expected_repository(&destination, &source.url)? {
        } else if git::is_repository(&destination)? {
            bail!("catalog destination exists but is not the configured Git repository");
        } else {
            bail!("catalog destination exists but is not a Git repository");
        }
    } else {
        fs::create_dir_all(paths.catalogs())
            .with_context(|| format!("could not create {}", paths.catalogs().display()))?;
        fs::create_dir(&destination).with_context(|| {
            format!(
                "catalog destination {} appeared while preparing installation; nothing was removed",
                destination.display()
            )
        })?;
        created_clone = true;
        if let Err(error) = git::clone_repository(&source.url, None, &destination) {
            return Err(cleanup_catalog_add_failure(
                &destination,
                error,
                &format!("could not install catalog '{name}'"),
            ));
        }
    }

    let validation = (|| -> Result<()> {
        if !git::is_expected_repository(&destination, &source.url)? {
            bail!("cloned catalog is not the configured Git repository");
        }
        catalog::load(&paths.catalog_file(name))
            .context("cloned repository is not a valid catalog")?;
        Ok(())
    })();
    if let Err(error) = validation {
        if created_clone {
            return Err(cleanup_catalog_add_failure(
                &destination,
                error,
                "catalog validation failed",
            ));
        }
        return Err(error);
    }

    let registration_changed = !local.catalogs.contains_key(name) || existing_differs;
    if registration_changed {
        local.catalogs.insert(name.to_owned(), source.clone());
        if local.default_catalog.is_none() {
            local.default_catalog = Some(name.to_owned());
        }
        if let Err(error) = save_config(&paths.config(), &local) {
            if created_clone {
                return Err(cleanup_catalog_add_failure(
                    &destination,
                    error,
                    "catalog registration failed after validation",
                ));
            }
            return Err(error).context("catalog registration failed after validation");
        }
        println!("registered catalog '{name}'");
    } else {
        println!("catalog '{name}' is already registered");
    }

    if created_clone {
        println!("installed catalog '{name}' at {}", destination.display());
    } else {
        println!("catalog '{name}' is already installed");
    }
    Ok(())
}

pub fn catalog_initialize(
    paths: &Paths,
    name: &str,
    url: String,
    writable: bool,
    commit: bool,
    push: bool,
) -> Result<()> {
    paths::validate_name(name)?;
    validate_url(&url)?;
    if !writable {
        bail!("a catalog initialized by Loadbot must be writable");
    }
    if push && !commit {
        bail!("pushing the initial catalog requires committing it first");
    }

    let mut local = config::load(&paths.config())?;
    let source = CatalogSource::new(url, true);
    let destination = paths.catalog(name);
    let existing_differs = local
        .catalogs
        .get(name)
        .is_some_and(|existing| existing.url != source.url || !existing.writable);
    if existing_differs && path_exists(&destination) {
        bail!("catalog '{name}' is already configured with different settings");
    }
    for existing_name in local.catalogs.keys() {
        if existing_name != name && existing_name.eq_ignore_ascii_case(name) {
            bail!("catalog name '{name}' conflicts with configured catalog '{existing_name}'");
        }
    }

    let mut created_clone = false;
    if path_exists(&destination) {
        if !git::is_repository(&destination)? {
            bail!("catalog destination exists but is not a Git repository");
        }
        if !git::is_expected_repository(&destination, &source.url)? {
            bail!("catalog destination exists but is not the configured Git repository");
        }
    } else {
        fs::create_dir_all(paths.catalogs())
            .with_context(|| format!("could not create {}", paths.catalogs().display()))?;
        if let Err(error) = git::clone_repository(&source.url, None, &destination) {
            cleanup_failed_clone(&destination);
            return Err(error).context("could not clone the catalog to initialize");
        }
        created_clone = true;
    }

    let catalog_path = paths.catalog_file(name);
    let catalog_exists = path_exists(&catalog_path);
    let preparation = (|| -> Result<(bool, bool)> {
        let changes = git::working_tree_changes(&destination)?;
        let only_catalog_changes = changes
            .lines()
            .all(|line| line.get(3..).is_some_and(|path| path == "catalog.toml"));
        if !changes.is_empty() && !only_catalog_changes {
            bail!("refusing to initialize catalog '{name}': working tree has unrelated changes");
        }

        let refs = git::origin_refs(&destination)?;
        let head = git::head_commit(&destination)?;
        let branch = git::current_branch(&destination)?;
        let tracked = git::tracked_files(&destination)?;
        if catalog_exists {
            let existing = catalog::load(&catalog_path).context(
                "refusing initialization because catalog.toml contains conflicting data",
            )?;
            if existing != CatalogFile::default() {
                bail!("refusing initialization because catalog.toml contains conflicting data");
            }
        }

        if !refs.is_empty() {
            let branch = branch
                .as_deref()
                .context("refusing initialization because the repository is detached")?;
            let head = head
                .as_deref()
                .context("refusing initialization because the repository has no local commit")?;
            let expected_ref = format!("refs/heads/{branch}");
            let exact_initial_remote = catalog_exists
                && changes.is_empty()
                && tracked == ["catalog.toml"]
                && refs.len() == 1
                && refs[0].0 == head
                && refs[0].1 == expected_ref;
            if !exact_initial_remote {
                bail!(
                    "refusing to initialize catalog '{name}': the repository is not an empty or already initialized Loadbot catalog"
                );
            }
            return Ok((true, false));
        }

        branch.context("refusing to initialize catalog: repository has no checked-out branch")?;
        if head.is_some() && (!catalog_exists || tracked != ["catalog.toml"] || !changes.is_empty())
        {
            bail!("refusing initialization because the local repository contains other data");
        }
        if head.is_none() && !catalog_exists && (!changes.is_empty() || !tracked.is_empty()) {
            bail!("refusing to initialize catalog '{name}': repository contains existing data");
        }
        if head.is_none() && catalog_exists && changes.is_empty() {
            bail!("refusing initialization because Git does not detect catalog.toml as a change");
        }
        Ok((false, !catalog_exists))
    })();
    let (already_initialized, create_catalog) = match preparation {
        Ok(state) => state,
        Err(error) => {
            if created_clone {
                cleanup_failed_clone(&destination);
            }
            return Err(error);
        }
    };

    if create_catalog && let Err(error) = catalog::save(&catalog_path, &CatalogFile::default()) {
        if created_clone {
            cleanup_failed_clone(&destination);
        }
        return Err(error).context("could not create initial catalog.toml");
    }

    let registration_changed = !local.catalogs.contains_key(name) || existing_differs;
    if registration_changed {
        local.catalogs.insert(name.to_owned(), source);
        if local.default_catalog.is_none() {
            local.default_catalog = Some(name.to_owned());
        }
        if let Err(error) = config::save(&paths.config(), &local) {
            if create_catalog {
                let _ = fs::remove_file(&catalog_path);
            }
            if created_clone {
                cleanup_failed_clone(&destination);
            }
            return Err(error).context("catalog was validated, but registration failed");
        }
        println!("registered catalog '{name}'");
    } else {
        println!("catalog '{name}' is already registered");
    }
    if created_clone {
        println!("installed catalog '{name}' at {}", destination.display());
    } else {
        println!("catalog '{name}' is already installed");
    }
    if create_catalog {
        println!("created initial catalog.toml for catalog '{name}'");
    }
    if already_initialized {
        println!("catalog '{name}' is already initialized");
        return Ok(());
    }

    if commit {
        if git::path_has_changes(&destination, "catalog.toml")? {
            let commit_hash =
                git::commit_file(&destination, "catalog.toml", "Initialize Loadbot catalog")
                    .context("catalog.toml was created, but committing it failed")?;
            println!("committed initial catalog at {commit_hash}");
        } else if git::head_commit(&destination)?.is_some() {
            println!("initial catalog is already committed");
        } else {
            bail!("catalog.toml exists but Git did not detect it as an initial change");
        }
    } else {
        println!("initial catalog.toml was not committed or pushed");
    }

    if push {
        if git::origin_has_refs(&destination)? {
            bail!("refusing to push because the remote is no longer empty");
        }
        git::push_origin(&destination)
            .context("initial catalog was committed locally, but pushing it failed")?;
        println!("pushed initial catalog '{name}' to origin");
    } else if commit {
        println!("initial catalog commit was not pushed");
    }
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
        println!(
            "Current commit: {}",
            repository.commit.as_deref().unwrap_or("(none)")
        );
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
    let catalog_has_changes = git::path_has_changes(&repository, "catalog.toml")?;
    let exact_definition_exists = catalog_file
        .tools
        .get(name)
        .is_some_and(|existing| existing.has_source(&definition));
    if catalog_has_changes && !exact_definition_exists {
        bail!(
            "catalog.toml already has uncommitted changes; refusing to combine them with a new tool addition; commit or otherwise handle those changes manually before retrying"
        );
    }
    let changed = if let Some(existing) = catalog_file.tools.get(name) {
        if existing.has_source(&definition) {
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
    for (index, tool) in tools.into_iter().enumerate() {
        let destination = paths.tool(&tool.catalog, &tool.name)?;
        let installed = if path_exists(&destination) {
            git::is_expected_repository(&destination, &tool.definition.url)?
        } else {
            false
        };
        if index > 0 {
            println!();
        }
        println!("{}", tool.name);
        println!("  catalog  {}", tool.catalog);
        println!("  type     {}", tool.definition.source_type.as_str());
        println!(
            "  state    {}",
            if installed { "installed" } else { "missing" }
        );
    }
    Ok(())
}

pub fn tool_pull(paths: &Paths, name: &str, catalog_name: Option<&str>) -> Result<()> {
    let tool = resolve_tool(paths, name, catalog_name)?;
    let destination = paths.tool(&tool.catalog, &tool.name)?;
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

    let parent = destination
        .parent()
        .context("tool destination has no parent directory")?;
    fs::create_dir_all(parent).with_context(|| format!("could not create {}", parent.display()))?;
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
    let destination = paths.tool(&tool.catalog, &tool.name)?;
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
    let destination = paths.tool(&tool.catalog, &tool.name)?;
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
        println!(
            "Current commit: {}",
            repository.commit.as_deref().unwrap_or("(none)")
        );
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
    let tool = resolve_tool(paths, name, catalog_name)?;
    println!("{}", paths.tool(&tool.catalog, &tool.name)?.display());
    Ok(())
}

pub fn installed_tool_path(paths: &Paths, name: &str, catalog_name: &str) -> Result<PathBuf> {
    let tool = resolve_tool(paths, name, Some(catalog_name))?;
    let destination = paths.tool(&tool.catalog, &tool.name)?;
    if !path_exists(&destination) {
        bail!("tool '{name}' from catalog '{catalog_name}' is not installed");
    }
    if !git::is_repository(&destination)? {
        bail!("installed tool destination is not a Git repository");
    }
    if !git::is_expected_repository(&destination, &tool.definition.url)? {
        bail!("installed tool destination is not the configured Git repository");
    }
    Ok(destination)
}

pub fn installed_tools(paths: &Paths) -> Result<Vec<ResolvedTool>> {
    let mut installed = Vec::new();
    for tool in all_tools(paths)? {
        let destination = paths.tool(&tool.catalog, &tool.name)?;
        if path_exists(&destination)
            && git::is_expected_repository(&destination, &tool.definition.url)?
        {
            installed.push(tool);
        }
    }
    Ok(installed)
}

pub fn all_tools(paths: &Paths) -> Result<Vec<ResolvedTool>> {
    let local = config::load(&paths.config())?;
    let mut tools = Vec::new();
    let mut portable_names = BTreeMap::new();
    for (catalog_name, source) in &local.catalogs {
        if let Err(error) = checked_catalog_repository(paths, catalog_name, source) {
            warn_skipped_catalog(catalog_name, &error);
            continue;
        }
        let catalog_file = match catalog::load(&paths.catalog_file(catalog_name)) {
            Ok(catalog_file) => catalog_file,
            Err(error) => {
                warn_skipped_catalog(catalog_name, &error);
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
    available_catalogs(paths, &local, true)
}

pub fn default_writable_catalog(paths: &Paths) -> Result<Option<String>> {
    let local = config::load(&paths.config())?;
    let Some(name) = local.default_catalog else {
        return Ok(None);
    };
    let Some(source) = local.catalogs.get(&name) else {
        return Ok(None);
    };
    if !source.writable || !catalog_is_available(paths, &name, source) {
        return Ok(None);
    }
    Ok(Some(name))
}

pub fn catalog_names(paths: &Paths) -> Result<Vec<String>> {
    Ok(config::load(&paths.config())?
        .catalogs
        .into_keys()
        .collect())
}

pub fn available_catalog_names(paths: &Paths) -> Result<Vec<String>> {
    let local = config::load(&paths.config())?;
    available_catalogs(paths, &local, false)
}

fn available_catalogs(
    paths: &Paths,
    local: &LocalConfig,
    writable_only: bool,
) -> Result<Vec<String>> {
    let mut names = Vec::new();
    for (name, source) in &local.catalogs {
        if writable_only && !source.writable {
            continue;
        }
        if catalog_is_available(paths, name, source) {
            names.push(name.clone());
        }
    }
    Ok(names)
}

fn catalog_is_available(paths: &Paths, name: &str, source: &CatalogSource) -> bool {
    let result = checked_catalog_repository(paths, name, source)
        .and_then(|_| catalog::load(&paths.catalog_file(name)).map(|_| ()));
    if let Err(error) = result {
        warn_skipped_catalog(name, &error);
        return false;
    }
    true
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

fn warn_skipped_catalog(name: &str, error: &anyhow::Error) {
    eprintln!(
        "warning: skipping catalog '{name}': {error:#}; run 'loadbot catalog status {name}' for details"
    );
}

fn cleanup_failed_clone(destination: &Path) {
    let _ = remove_failed_clone(destination);
}

fn cleanup_catalog_add_failure(
    destination: &Path,
    error: anyhow::Error,
    context: &str,
) -> anyhow::Error {
    match remove_failed_clone(destination) {
        Ok(()) => error.context(context.to_owned()),
        Err(cleanup_error) => error.context(format!(
            "{context}; additionally, cleanup of {} failed: {cleanup_error:#}",
            destination.display()
        )),
    }
}

fn remove_failed_clone(destination: &Path) -> Result<()> {
    let Ok(metadata) = fs::symlink_metadata(destination) else {
        return Ok(());
    };
    if metadata.is_dir() && !metadata.file_type().is_symlink() {
        fs::remove_dir_all(destination)
            .with_context(|| format!("could not remove {}", destination.display()))?;
    } else {
        fs::remove_file(destination)
            .with_context(|| format!("could not remove {}", destination.display()))?;
    }
    Ok(())
}

fn path_exists(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok()
}

#[cfg(test)]
mod tests {
    use std::ffi::OsStr;
    use std::process::Command;

    use tempfile::TempDir;

    use super::*;

    fn git<I, S>(arguments: I, directory: Option<&Path>)
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let mut command = Command::new("git");
        command.args(arguments);
        if let Some(directory) = directory {
            command.current_dir(directory);
        }
        let output = command.output().unwrap();
        assert!(
            output.status.success(),
            "Git failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn empty_remote(base: &Path, name: &str) -> PathBuf {
        let remote = base.join(format!("{name}.git"));
        git(
            [
                OsStr::new("init"),
                OsStr::new("--bare"),
                OsStr::new("--initial-branch"),
                OsStr::new("main"),
                remote.as_os_str(),
            ],
            None,
        );
        remote
    }

    fn populated_remote(base: &Path, name: &str) -> PathBuf {
        let remote = empty_remote(base, name);
        let source = base.join(format!("{name}-source"));
        git(
            [
                OsStr::new("init"),
                OsStr::new("--initial-branch"),
                OsStr::new("main"),
                source.as_os_str(),
            ],
            None,
        );
        git(["config", "user.name", "Loadbot Tests"], Some(&source));
        git(
            ["config", "user.email", "loadbot@example.test"],
            Some(&source),
        );
        fs::write(source.join("README.md"), "existing data\n").unwrap();
        git(["add", "README.md"], Some(&source));
        git(["commit", "-m", "initial"], Some(&source));
        git(
            [
                OsStr::new("remote"),
                OsStr::new("add"),
                OsStr::new("origin"),
                remote.as_os_str(),
            ],
            Some(&source),
        );
        git(["push", "origin", "main"], Some(&source));
        remote
    }

    fn valid_catalog_remote(base: &Path, name: &str) -> PathBuf {
        let remote = populated_remote(base, name);
        let source = base.join(format!("{name}-source"));
        fs::write(source.join("catalog.toml"), "version = 1\n\n[tools]\n").unwrap();
        git(["add", "catalog.toml"], Some(&source));
        git(["commit", "-m", "add catalog"], Some(&source));
        git(["push", "origin", "main"], Some(&source));
        remote
    }

    #[test]
    fn catalog_add_cleans_fresh_clone_when_configuration_save_fails() {
        let temporary = TempDir::new().unwrap();
        let paths = Paths::with_root(temporary.path().join("loadbot"));
        let remote = valid_catalog_remote(temporary.path(), "catalog");

        let error = catalog_add_with_save(
            &paths,
            "personal",
            remote.display().to_string(),
            true,
            |_, _| bail!("injected configuration failure"),
        )
        .unwrap_err();

        assert!(format!("{error:#}").contains("registration failed"));
        assert!(format!("{error:#}").contains("injected configuration failure"));
        assert!(!paths.catalog("personal").exists());
        assert!(!paths.config().exists());
    }

    #[test]
    fn catalog_add_rejects_invalid_catalog_and_cleans_its_clone() {
        let temporary = TempDir::new().unwrap();
        let paths = Paths::with_root(temporary.path().join("loadbot"));
        let remote = populated_remote(temporary.path(), "not-a-catalog");

        let error =
            catalog_add(&paths, "invalid", remote.display().to_string(), false).unwrap_err();

        assert!(format!("{error:#}").contains("not a valid catalog"));
        assert!(!paths.catalog("invalid").exists());
        assert!(!paths.config().exists());
    }

    #[test]
    fn catalog_add_never_cleans_preexisting_repository_after_save_failure() {
        let temporary = TempDir::new().unwrap();
        let paths = Paths::with_root(temporary.path().join("loadbot"));
        let remote = valid_catalog_remote(temporary.path(), "catalog");
        fs::create_dir_all(paths.catalogs()).unwrap();
        git::clone_repository(
            &remote.display().to_string(),
            None,
            &paths.catalog("personal"),
        )
        .unwrap();
        fs::write(paths.catalog("personal").join("keep.txt"), "keep\n").unwrap();

        assert!(
            catalog_add_with_save(
                &paths,
                "personal",
                remote.display().to_string(),
                true,
                |_, _| bail!("injected configuration failure"),
            )
            .is_err()
        );

        assert_eq!(
            fs::read_to_string(paths.catalog("personal").join("keep.txt")).unwrap(),
            "keep\n"
        );
        assert!(paths.catalog("personal/.git").is_dir());
        assert!(!paths.config().exists());
    }

    #[test]
    fn operational_catalog_choices_skip_missing_and_wrong_repositories_without_mutating_config() {
        let temporary = TempDir::new().unwrap();
        let paths = Paths::with_root(temporary.path().join("loadbot"));
        let expected = valid_catalog_remote(temporary.path(), "expected");
        let wrong = valid_catalog_remote(temporary.path(), "wrong");
        catalog_add(&paths, "personal", expected.display().to_string(), true).unwrap();
        let config_before = fs::read(paths.config()).unwrap();

        fs::remove_dir_all(paths.catalog("personal")).unwrap();
        assert!(available_catalog_names(&paths).unwrap().is_empty());
        assert!(writable_catalogs(&paths).unwrap().is_empty());
        assert_eq!(default_writable_catalog(&paths).unwrap(), None);
        assert_eq!(catalog_names(&paths).unwrap(), ["personal"]);
        assert_eq!(fs::read(paths.config()).unwrap(), config_before);

        git::clone_repository(
            &wrong.display().to_string(),
            None,
            &paths.catalog("personal"),
        )
        .unwrap();
        assert!(available_catalog_names(&paths).unwrap().is_empty());
        assert!(writable_catalogs(&paths).unwrap().is_empty());
        assert_eq!(fs::read(paths.config()).unwrap(), config_before);
    }

    #[test]
    fn catalog_initialization_requires_explicit_commit_and_push() {
        let temporary = TempDir::new().unwrap();
        let paths = Paths::with_root(temporary.path().join("loadbot"));
        let remote = empty_remote(temporary.path(), "new-catalog");
        let url = remote.display().to_string();

        catalog_initialize(&paths, "personal", url.clone(), true, false, false).unwrap();
        assert_eq!(
            catalog::load(&paths.catalog_file("personal")).unwrap(),
            CatalogFile::default()
        );
        assert_eq!(git::head_commit(&paths.catalog("personal")).unwrap(), None);
        assert!(!git::origin_has_refs(&paths.catalog("personal")).unwrap());
        catalog_status(&paths, "personal").unwrap();
        assert_eq!(
            paths.tool("personal", "demo").unwrap(),
            paths.tools().join("personal/demo")
        );

        git(
            ["config", "user.name", "Loadbot Tests"],
            Some(&paths.catalog("personal")),
        );
        git(
            ["config", "user.email", "loadbot@example.test"],
            Some(&paths.catalog("personal")),
        );
        catalog_initialize(&paths, "personal", url.clone(), true, true, true).unwrap();
        assert!(
            git::head_commit(&paths.catalog("personal"))
                .unwrap()
                .is_some()
        );
        assert!(git::origin_has_refs(&paths.catalog("personal")).unwrap());

        let before = git::head_commit(&paths.catalog("personal")).unwrap();
        catalog_initialize(&paths, "personal", url, true, true, true).unwrap();
        assert_eq!(
            git::head_commit(&paths.catalog("personal")).unwrap(),
            before
        );
    }

    #[test]
    fn catalog_initialization_refuses_nonempty_remote_and_conflicting_data() {
        let temporary = TempDir::new().unwrap();
        let paths = Paths::with_root(temporary.path().join("loadbot"));
        let populated = populated_remote(temporary.path(), "populated");
        let error = catalog_initialize(
            &paths,
            "populated",
            populated.display().to_string(),
            true,
            false,
            false,
        )
        .unwrap_err();
        assert!(format!("{error:#}").contains("not an empty"));
        assert!(!paths.catalog_file("populated").exists());
        assert!(!paths.config().exists());
        assert!(!paths.catalog("populated").exists());

        let populated_source = temporary.path().join("populated-source");
        fs::write(
            populated_source.join("catalog.toml"),
            "version = 1\n\n[tools]\n",
        )
        .unwrap();
        git(["add", "catalog.toml"], Some(&populated_source));
        git(
            ["commit", "-m", "add empty catalog"],
            Some(&populated_source),
        );
        git(["push", "origin", "main"], Some(&populated_source));
        let error = catalog_initialize(
            &paths,
            "populated",
            populated.display().to_string(),
            true,
            false,
            false,
        )
        .unwrap_err();
        assert!(format!("{error:#}").contains("not an empty or already initialized"));
        assert!(!paths.config().exists());
        assert!(!paths.catalog("populated").exists());

        let empty = empty_remote(temporary.path(), "conflicting");
        catalog_initialize(
            &paths,
            "conflicting",
            empty.display().to_string(),
            true,
            false,
            false,
        )
        .unwrap();
        fs::write(
            paths.catalog_file("conflicting"),
            "version = 1\nnote = 'do not overwrite'\n",
        )
        .unwrap();
        let before = fs::read(paths.catalog_file("conflicting")).unwrap();
        let error = catalog_initialize(
            &paths,
            "conflicting",
            empty.display().to_string(),
            true,
            false,
            false,
        )
        .unwrap_err();
        assert!(format!("{error:#}").contains("conflicting data"));
        assert_eq!(fs::read(paths.catalog_file("conflicting")).unwrap(), before);
    }

    #[test]
    fn catalog_initialization_refuses_unrelated_worktree_changes() {
        let temporary = TempDir::new().unwrap();
        let paths = Paths::with_root(temporary.path().join("loadbot"));
        let remote = empty_remote(temporary.path(), "dirty");
        let url = remote.display().to_string();
        catalog_initialize(&paths, "dirty", url.clone(), true, false, false).unwrap();
        fs::write(paths.catalog("dirty").join("unrelated.txt"), "keep\n").unwrap();

        let error = catalog_initialize(&paths, "dirty", url, true, true, false).unwrap_err();
        assert!(format!("{error:#}").contains("unrelated changes"));
        assert_eq!(
            fs::read_to_string(paths.catalog("dirty").join("unrelated.txt")).unwrap(),
            "keep\n"
        );
        assert_eq!(git::head_commit(&paths.catalog("dirty")).unwrap(), None);
    }
}
