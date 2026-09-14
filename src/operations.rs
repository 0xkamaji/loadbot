use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use crate::catalog::{self, CatalogFile, ResolvedTool, ToolConfig};
use crate::config::{self, CatalogSource, LocalConfig};
use crate::git;
use crate::interaction::{Interaction, MutationOutcome, Notice, OperationContext};
use crate::paths::{self, Paths};

#[derive(Debug, Clone)]
pub struct CatalogSummary {
    pub name: String,
    pub source: CatalogSource,
    pub state: CatalogState,
    pub default: bool,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CatalogState {
    Missing,
    Installed,
    Mismatch,
}
#[derive(Debug, Clone)]
pub enum CatalogValidity {
    Unmanaged,
    Missing,
    Valid,
    Invalid(String),
}
#[derive(Debug, Clone)]
pub struct CatalogStatus {
    pub name: String,
    pub path: PathBuf,
    pub source: CatalogSource,
    pub repository: Option<git::RepositoryStatus>,
    pub file: CatalogValidity,
}
#[derive(Debug, Clone)]
pub struct ToolSummary {
    pub tool: ResolvedTool,
    pub installed: bool,
}
#[derive(Debug, Clone)]
pub struct ToolStatus {
    pub tool: ResolvedTool,
    pub path: PathBuf,
    pub installed: bool,
    pub repository: Option<git::RepositoryStatus>,
}

pub fn catalog_add(
    paths: &Paths,
    name: &str,
    url: String,
    writable: bool,
    context: &mut OperationContext<'_>,
) -> Result<MutationOutcome> {
    let _process_scope = crate::process::scope(&context.process);
    context.process.cancellation.check()?;
    let notice_start = context.notices.len();
    catalog_add_with_save(paths, name, url, writable, config::save, context)?;
    Ok(context.outcome_since(notice_start))
}

fn catalog_add_with_save<F>(
    paths: &Paths,
    name: &str,
    url: String,
    writable: bool,
    save_config: F,
    context: &mut OperationContext<'_>,
) -> Result<()>
where
    F: FnOnce(&Path, &LocalConfig) -> Result<()>,
{
    paths::validate_name(name)?;
    validate_url(&url)?;
    let _repository_lease = context.lease(&paths.catalog(name))?;
    let mut local = config::load(&paths.config())?;
    let original_local = local.clone();
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
        if let Err(error) = git::clone_repository(&source.url, None, &destination, context) {
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
        if let Err(error) =
            merge_registration(&paths.config(), &original_local, &local, save_config)
        {
            if created_clone {
                return Err(cleanup_catalog_add_failure(
                    &destination,
                    error,
                    "catalog registration failed after validation",
                ));
            }
            return Err(error).context("catalog registration failed after validation");
        }
        context.record(Notice::CatalogRegistered {
            name: name.to_owned(),
        });
    } else {
        context.record(Notice::CatalogAlreadyRegistered {
            name: name.to_owned(),
        });
    }

    if created_clone {
        context.record(Notice::CatalogInstalled {
            name: name.to_owned(),
            path: destination.clone(),
        });
    } else {
        context.record(Notice::CatalogAlreadyInstalled {
            name: name.to_owned(),
        });
    }
    Ok(())
}

fn merge_registration(
    path: &Path,
    original: &LocalConfig,
    desired: &LocalConfig,
    save: impl FnOnce(&Path, &LocalConfig) -> Result<()>,
) -> Result<()> {
    let _lease = crate::persistence::Lease::acquire(path)?;
    let mut current = config::load(path)?;
    for (name, source) in &desired.catalogs {
        if original.catalogs.get(name) == Some(source) {
            continue;
        }
        if current.catalogs.get(name) != original.catalogs.get(name) {
            return Err(crate::persistence::Busy {
                resource: path.to_owned(),
            }
            .into());
        }
        if current
            .catalogs
            .keys()
            .any(|key| key != name && key.eq_ignore_ascii_case(name))
        {
            bail!("catalog name '{name}' conflicts with a concurrently registered catalog");
        }
        current.catalogs.insert(name.clone(), source.clone());
    }
    if current.default_catalog.is_none() {
        current.default_catalog = desired.default_catalog.clone();
    }
    save(path, &current)
}

pub fn catalog_initialize(
    paths: &Paths,
    name: &str,
    url: String,
    writable: bool,
    commit: bool,
    push: bool,
    context: &mut OperationContext<'_>,
) -> Result<MutationOutcome> {
    let _process_scope = crate::process::scope(&context.process);
    context.process.cancellation.check()?;
    let notice_start = context.notices.len();
    paths::validate_name(name)?;
    validate_url(&url)?;
    if !writable {
        bail!("a catalog initialized by Loadbot must be writable");
    }
    if push && !commit {
        bail!("pushing the initial catalog requires committing it first");
    }

    let _repository_lease = context.lease(&paths.catalog(name))?;
    let mut local = config::load(&paths.config())?;
    let original_local = local.clone();
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
        fs::create_dir(&destination)
            .context("catalog destination appeared before clone; nothing removed")?;
        if let Err(mut error) = git::clone_repository(&source.url, None, &destination, context) {
            error = cleanup_failed_clone(&destination, error);
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

        let refs = git::origin_refs(&destination, context)?;
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
        Err(mut error) => {
            if created_clone {
                error = cleanup_failed_clone(&destination, error);
            }
            return Err(error);
        }
    };

    if create_catalog && let Err(mut error) = catalog::save(&catalog_path, &CatalogFile::default())
    {
        if created_clone {
            error = cleanup_failed_clone(&destination, error);
        }
        return Err(error).context("could not create initial catalog.toml");
    }

    let registration_changed = !local.catalogs.contains_key(name) || existing_differs;
    if registration_changed {
        local.catalogs.insert(name.to_owned(), source);
        if local.default_catalog.is_none() {
            local.default_catalog = Some(name.to_owned());
        }
        if let Err(mut error) =
            merge_registration(&paths.config(), &original_local, &local, config::save)
        {
            if create_catalog && cleanup_is_safe(&error) {
                let _ = fs::remove_file(&catalog_path);
            }
            if created_clone {
                error = cleanup_failed_clone(&destination, error);
            }
            return Err(error).context("catalog was validated, but registration failed");
        }
        context.record(Notice::CatalogRegistered {
            name: name.to_owned(),
        });
    } else {
        context.record(Notice::CatalogAlreadyRegistered {
            name: name.to_owned(),
        });
    }
    if created_clone {
        context.record(Notice::CatalogInstalled {
            name: name.to_owned(),
            path: destination.clone(),
        });
    } else {
        context.record(Notice::CatalogAlreadyInstalled {
            name: name.to_owned(),
        });
    }
    if create_catalog {
        context.record(Notice::CatalogCreated {
            name: name.to_owned(),
        });
    }
    if already_initialized {
        context.record(Notice::CatalogAlreadyInitialized {
            name: name.to_owned(),
        });
        return Ok(context.outcome_since(notice_start));
    }

    context.process.cancellation.check()?;
    if commit {
        if git::path_has_changes(&destination, "catalog.toml")? {
            let commit_hash = git::commit_file_with_interaction(
                &destination,
                "catalog.toml",
                "Initialize Loadbot catalog",
                context,
            )
            .context("catalog.toml was created, but committing it failed")?;
            context.record(Notice::InitialCatalogCommitted { commit_hash });
        } else if git::head_commit(&destination)?.is_some() {
            context.record(Notice::InitialCatalogAlreadyCommitted);
        } else {
            bail!("catalog.toml exists but Git did not detect it as an initial change");
        }
    } else {
        context.record(Notice::InitialCatalogUncommitted);
    }

    context.process.cancellation.check()?;
    if push {
        if git::origin_has_refs(&destination, context)? {
            bail!("refusing to push because the remote is no longer empty");
        }
        git::push_origin(&destination, context)
            .context("initial catalog was committed locally, but pushing it failed")?;
        context.record(Notice::InitialCatalogPushed {
            name: name.to_owned(),
        });
    } else if commit {
        context.record(Notice::InitialCatalogNotPushed);
    }
    Ok(context.outcome_since(notice_start))
}

pub fn catalog_list(
    paths: &Paths,
    context: &mut OperationContext<'_>,
) -> Result<Vec<CatalogSummary>> {
    let _process_scope = crate::process::scope(&context.process);
    context.process.cancellation.check()?;
    let local = config::load(&paths.config())?;
    let mut rows = Vec::new();
    for (name, source) in &local.catalogs {
        context.process.cancellation.check()?;
        let destination = paths.catalog(name);
        context.record(Notice::CatalogResolved {
            name: name.clone(),
            path: destination.clone(),
            source: source.clone(),
        });
        let state = if !path_exists(&destination) {
            CatalogState::Missing
        } else if git::is_expected_repository(&destination, &source.url)? {
            CatalogState::Installed
        } else {
            CatalogState::Mismatch
        };
        let row = CatalogSummary {
            name: name.clone(),
            source: source.clone(),
            state,
            default: local.default_catalog.as_deref() == Some(name),
        };
        context.record(Notice::CatalogInspected(row.clone()));
        rows.push(row);
    }
    Ok(rows)
}

pub fn catalog_sync(
    paths: &Paths,
    name: &str,
    context: &mut OperationContext<'_>,
) -> Result<MutationOutcome> {
    let _process_scope = crate::process::scope(&context.process);
    context.process.cancellation.check()?;
    let notice_start = context.notices.len();
    paths::validate_name(name)?;
    let _repository_lease = context.lease(&paths.catalog(name))?;
    let local = config::load(&paths.config())?;
    let source = configured_catalog(&local, name)?;
    let destination = checked_catalog_repository(paths, name, source)?;
    let (old_commit, new_commit) = git::update(&destination, None, context)
        .with_context(|| format!("refusing to sync catalog '{name}'"))?;
    if old_commit == new_commit {
        context.record(Notice::CatalogCurrent {
            name: name.to_owned(),
            new_commit,
        });
    } else {
        context.record(Notice::CatalogSynced {
            name: name.to_owned(),
            old_commit,
            new_commit,
        });
    }
    Ok(context.outcome_since(notice_start))
}

pub fn catalog_status(
    paths: &Paths,
    name: &str,
    context: &mut OperationContext<'_>,
) -> Result<CatalogStatus> {
    let _process_scope = crate::process::scope(&context.process);
    context.process.cancellation.check()?;
    let local = config::load(&paths.config())?;
    let source = configured_catalog(&local, name)?.clone();
    let destination = paths.catalog(name);
    context.record(Notice::CatalogResolved {
        name: name.to_owned(),
        path: destination.clone(),
        source: source.clone(),
    });
    let is_repository = path_exists(&destination) && git::is_repository(&destination)?;
    let repository = if is_repository {
        Some(git::status(&destination)?)
    } else {
        None
    };
    context.record(Notice::RepositoryInspected(repository.clone()));
    let catalog_path = paths.catalog_file(name);
    let file = if path_exists(&destination) && !is_repository {
        CatalogValidity::Unmanaged
    } else if !catalog_path.is_file() {
        match crate::persistence::read_optional(&catalog_path) {
            Err(error) => CatalogValidity::Invalid(format!("{error:#}")),
            Ok(_) => CatalogValidity::Missing,
        }
    } else {
        match catalog::load(&catalog_path) {
            Ok(_) => CatalogValidity::Valid,
            Err(error) => CatalogValidity::Invalid(format!("{error:#}")),
        }
    };
    context.record(Notice::CatalogFileInspected(file.clone()));
    Ok(CatalogStatus {
        name: name.to_owned(),
        path: destination,
        source,
        repository,
        file,
    })
}

pub fn catalog_path(paths: &Paths, name: &str) -> Result<PathBuf> {
    let local = config::load(&paths.config())?;
    configured_catalog(&local, name)?;
    Ok(paths.catalog(name))
}

pub fn catalog_migrate(
    paths: &Paths,
    name: &str,
    url: String,
    context: &mut OperationContext<'_>,
) -> Result<MutationOutcome> {
    let _process_scope = crate::process::scope(&context.process);
    context.process.cancellation.check()?;
    let notice_start = context.notices.len();
    paths::validate_name(name)?;
    validate_url(&url)?;
    let _repository_lease = context.lease(&paths.catalog(name))?;
    let original_bytes = crate::persistence::read_optional(&paths.config())?;
    let legacy = config::load_legacy(&paths.config())?;
    let destination = paths.catalog(name);
    if path_exists(&destination) {
        bail!("refusing migration: catalog destination already exists");
    }

    fs::create_dir_all(paths.catalogs())
        .with_context(|| format!("could not create {}", paths.catalogs().display()))?;
    fs::create_dir(&destination)
        .context("migration destination appeared before clone; nothing removed")?;
    if let Err(mut error) = git::clone_repository(&url, None, &destination, context) {
        error = cleanup_failed_clone(&destination, error);
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
        let _configuration_lease = crate::persistence::Lease::acquire(&paths.config())?;
        if crate::persistence::read_optional(&paths.config())? != original_bytes {
            return Err(crate::persistence::Busy {
                resource: paths.config(),
            }
            .into());
        }
        config::save(&paths.config(), &local)
            .context("catalog.toml was written, but replacing the legacy configuration failed")
    })();
    if let Err(mut error) = migration_result {
        error = cleanup_failed_clone(&destination, error);
        return Err(error);
    }
    context.record(Notice::CatalogMigrated {
        name: name.to_owned(),
    });
    context.record(Notice::CatalogUncommitted);
    Ok(context.outcome_since(notice_start))
}

#[allow(clippy::too_many_arguments)]
pub fn tool_add(
    paths: &Paths,
    catalog_name: &str,
    name: &str,
    url: String,
    revision: Option<String>,
    commit: bool,
    push: bool,
    context: &mut OperationContext<'_>,
) -> Result<MutationOutcome> {
    let _process_scope = crate::process::scope(&context.process);
    context.process.cancellation.check()?;
    let notice_start = context.notices.len();
    paths::validate_name(name)?;
    paths::validate_name(catalog_name)?;
    validate_url(&url)?;
    if revision.as_deref() == Some("") {
        bail!("revision must not be empty");
    }
    if push && !commit {
        bail!("pushing a catalog change requires --commit");
    }

    let _repository_lease = context.lease(&paths.catalog(catalog_name))?;
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
            context.record(Notice::ToolAlreadyDefined {
                name: name.to_owned(),
                catalog_name: catalog_name.to_owned(),
            });
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
        context.record(Notice::ToolAdded {
            name: name.to_owned(),
            catalog_name: catalog_name.to_owned(),
        });
        true
    };

    context.process.cancellation.check()?;
    if commit {
        if git::path_has_changes(&repository, "catalog.toml")? {
            let message = format!("Add {name} to Loadbot catalog");
            let commit_hash = git::commit_file_with_interaction(
                &repository,
                "catalog.toml",
                &message,
                context,
            )
            .context("tool definition was saved, but committing the catalog change failed")?;
            context.record(Notice::CatalogCommitted { commit_hash });
        } else if changed {
            bail!("catalog changed but Git did not detect a catalog.toml modification");
        } else {
            context.record(Notice::CatalogAlreadyCommitted);
        }
    }
    context.process.cancellation.check()?;
    if push {
        git::push_origin(&repository, context)
            .context("tool definition was saved and committed, but pushing the catalog failed")?;
        context.record(Notice::CatalogPushed {
            catalog_name: catalog_name.to_owned(),
        });
    }
    Ok(context.outcome_since(notice_start))
}

pub fn tool_list(paths: &Paths, context: &mut OperationContext<'_>) -> Result<Vec<ToolSummary>> {
    let _process_scope = crate::process::scope(&context.process);
    context.process.cancellation.check()?;
    let tools = all_tools(paths, context)?;
    let mut rows = Vec::new();
    for tool in tools {
        context.process.cancellation.check()?;
        let destination = paths.tool(&tool.catalog, &tool.name)?;
        let installed = path_exists(&destination)
            && git::is_expected_repository(&destination, &tool.definition.url)?;
        let row = ToolSummary { tool, installed };
        context.record(Notice::ToolInspected(row.clone()));
        rows.push(row);
    }
    Ok(rows)
}

pub fn tool_pull(
    paths: &Paths,
    name: &str,
    catalog_name: Option<&str>,
    context: &mut OperationContext<'_>,
) -> Result<MutationOutcome> {
    let _process_scope = crate::process::scope(&context.process);
    context.process.cancellation.check()?;
    let notice_start = context.notices.len();
    tool_pull_with(
        paths,
        name,
        catalog_name,
        context.can_choose(),
        context,
        git::verified_rot_identities,
        git::clone_repository,
    )?;
    Ok(context.outcome_since(notice_start))
}

fn tool_pull_with<I, C>(
    paths: &Paths,
    name: &str,
    catalog_name: Option<&str>,
    interactive: bool,
    context: &mut OperationContext<'_>,
    mut identities: I,
    mut clone_repository: C,
) -> Result<()>
where
    I: FnMut() -> Result<Vec<git::RotIdentity>>,
    C: FnMut(&str, Option<&str>, &Path, &mut dyn Interaction) -> Result<()>,
{
    let tool = resolve_tool(paths, name, catalog_name, context)?;
    let destination = paths.tool(&tool.catalog, &tool.name)?;
    let _repository_lease = context.lease(&destination)?;
    let _configuration_snapshot = context.watch(&paths.config())?;
    let _catalog_snapshot = context.watch(&paths.catalog_file(&tool.catalog))?;
    let current_tool = resolve_tool(paths, name, catalog_name, context)?;
    if current_tool.definition != tool.definition {
        return Err(crate::persistence::Busy {
            resource: destination,
        }
        .into());
    }
    if path_exists(&destination) {
        if !git::is_repository(&destination)? {
            bail!("destination exists but is not a Git repository");
        }
        let repository_match =
            repository_match_with_identities(&destination, &tool.definition.url, &mut identities)?;
        if repository_match == git::RepositoryMatch::Exact {
            context.record(Notice::ToolAlreadyInstalled {
                name: name.to_owned(),
            });
            offer_ssh_push(
                &destination,
                &tool.definition.url,
                interactive,
                context,
                &mut identities,
            )?;
            return Ok(());
        }
        if repository_match == git::RepositoryMatch::EquivalentGithub {
            return reconcile_existing_checkout(
                &destination,
                name,
                &tool.definition.url,
                interactive,
                context,
            );
        }
        let actual = git::fetch_url(&destination)?.unwrap_or_else(|| "(none)".to_owned());
        bail!(
            "destination exists but is not the configured Git repository\nFetch URL: {actual}\nCatalog URL: {}",
            tool.definition.url
        );
    }

    let parent = destination
        .parent()
        .context("tool destination has no parent directory")?;
    fs::create_dir_all(parent).with_context(|| format!("could not create {}", parent.display()))?;
    fs::create_dir(&destination)
        .context("tool destination appeared before clone; nothing removed")?;
    if let Err(mut error) = clone_repository(
        &tool.definition.url,
        tool.definition.revision.as_deref(),
        &destination,
        context,
    ) {
        error = cleanup_failed_clone(&destination, error);
        return Err(error).context(format!("could not clone tool '{name}'"));
    }
    let validation = (|| -> Result<()> {
        context.process.cancellation.check()?;
        if !git::is_expected_repository(&destination, &tool.definition.url)? {
            bail!("cloned tool is not the configured Git repository");
        }
        Ok(())
    })();
    if let Err(error) = validation {
        return Err(cleanup_failed_clone(&destination, error));
    }
    context.record(Notice::ToolInstalled {
        name: name.to_owned(),
        path: destination.clone(),
    });
    context.process.cancellation.check()?;
    offer_ssh_push(
        &destination,
        &tool.definition.url,
        interactive,
        context,
        &mut identities,
    )?;
    Ok(())
}

fn repository_match_with_identities<I>(
    destination: &Path,
    configured_url: &str,
    identities: &mut I,
) -> Result<git::RepositoryMatch>
where
    I: FnMut() -> Result<Vec<git::RotIdentity>>,
{
    let direct = git::repository_match(destination, configured_url, &[])?;
    if direct != git::RepositoryMatch::Mismatch {
        return Ok(direct);
    }
    let aliases = optional_identities(identities())?
        .into_iter()
        .map(|identity| identity.alias)
        .collect::<Vec<_>>();
    git::repository_match(destination, configured_url, &aliases)
}

fn offer_ssh_push<I>(
    destination: &Path,
    canonical_url: &str,
    interactive: bool,
    context: &mut OperationContext<'_>,
    identities: &mut I,
) -> Result<()>
where
    I: FnMut() -> Result<Vec<git::RotIdentity>>,
{
    if !interactive
        || git::push_url(destination)?.is_some()
        || git::github_ssh_push_url(canonical_url, "placeholder").is_none()
    {
        return Ok(());
    }
    let available = optional_identities(identities())?;
    if available.is_empty() || !context.configure_push()? {
        return Ok(());
    }
    let identity = git::select_verified_rot_identity(available, context)?;
    let push_url = git::github_ssh_push_url(canonical_url, &identity.alias)
        .context("could not derive the GitHub SSH push URL")?;
    if git::push_url(destination)?.is_some()
        || !git::is_expected_repository(destination, canonical_url)?
    {
        bail!("repository URLs changed while awaiting a decision; retry the operation");
    }
    git::set_push_url(destination, &push_url)?;
    context.record(Notice::PushUrlConfigured { push_url });
    Ok(())
}

fn reconcile_existing_checkout(
    destination: &Path,
    name: &str,
    canonical_url: &str,
    interactive: bool,
    context: &mut OperationContext<'_>,
) -> Result<()> {
    let existing_fetch = git::fetch_url(destination)?.context("repository has no origin URL")?;
    if !interactive {
        bail!(
            "destination is the configured GitHub repository but uses a different transport\nFetch URL: {existing_fetch}\nCatalog URL: {canonical_url}\nRun 'loadbot pull {name}' interactively to reconcile its fetch and push URLs."
        );
    }
    if !context.reconcile_checkout(&existing_fetch, canonical_url)? {
        bail!(
            "repository URL mismatch was not changed\nFetch URL: {existing_fetch}\nCatalog URL: {canonical_url}"
        );
    }
    let original_push = git::push_url(destination)?;
    if let Some(existing_push) = original_push.as_ref()
        && existing_push != &existing_fetch
        && !context.replace_push(existing_push, &existing_fetch)?
    {
        bail!("existing push URL was preserved; repository URLs were not changed");
    }
    if git::fetch_url(destination)?.as_deref() != Some(&existing_fetch)
        || git::push_url(destination)? != original_push
    {
        bail!("repository URLs changed while awaiting a decision; retry the operation");
    }
    git::reconcile_remote(destination, canonical_url, &existing_fetch)?;
    // The transaction has verified both URLs. Retain that completed step before
    // cancellation can interrupt further inspection or subsequent work.
    context.record(Notice::ToolReconciled {
        name: name.to_owned(),
    });
    context.process.cancellation.check()?;
    if !git::is_expected_repository(destination, canonical_url)? {
        bail!("reconciled repository did not match the catalog URL");
    }
    context.process.cancellation.check()
}

pub fn tool_update(
    paths: &Paths,
    name: &str,
    catalog_name: Option<&str>,
    context: &mut OperationContext<'_>,
) -> Result<MutationOutcome> {
    let _process_scope = crate::process::scope(&context.process);
    context.process.cancellation.check()?;
    let notice_start = context.notices.len();
    let tool = resolve_tool(paths, name, catalog_name, context)?;
    let destination = paths.tool(&tool.catalog, &tool.name)?;
    let _repository_lease = context.lease(&destination)?;
    let _configuration_snapshot = context.watch(&paths.config())?;
    let _catalog_snapshot = context.watch(&paths.catalog_file(&tool.catalog))?;
    let current_tool = resolve_tool(paths, name, catalog_name, context)?;
    if current_tool.definition != tool.definition {
        return Err(crate::persistence::Busy {
            resource: destination,
        }
        .into());
    }
    if !path_exists(&destination) {
        bail!("tool '{name}' is not installed; run 'loadbot pull {name}' first");
    }
    if !git::is_repository(&destination)? {
        bail!("destination exists but is not a Git repository");
    }
    if !git::is_expected_repository(&destination, &tool.definition.url)? {
        if equivalent_github_checkout(&destination, &tool.definition.url)? {
            bail!(
                "destination uses a different transport for the configured GitHub repository; run 'loadbot pull {name}' interactively to reconcile its fetch and push URLs"
            );
        }
        bail!("destination is not the configured Git repository");
    }

    let (old_commit, new_commit) =
        git::update(&destination, tool.definition.revision.as_deref(), context)
            .with_context(|| format!("refusing to update '{name}'"))?;
    if old_commit == new_commit {
        context.record(Notice::ToolCurrent {
            name: name.to_owned(),
            new_commit,
        });
    } else {
        context.record(Notice::ToolUpdated {
            name: name.to_owned(),
            old_commit,
            new_commit,
        });
    }
    Ok(context.outcome_since(notice_start))
}

pub fn tool_status(
    paths: &Paths,
    name: &str,
    catalog_name: Option<&str>,
    context: &mut OperationContext<'_>,
) -> Result<ToolStatus> {
    let _process_scope = crate::process::scope(&context.process);
    context.process.cancellation.check()?;
    let tool = resolve_tool(paths, name, catalog_name, context)?;
    let destination = paths.tool(&tool.catalog, &tool.name)?;
    context.record(Notice::ToolResolved {
        tool: tool.clone(),
        path: destination.clone(),
    });
    let installed = path_exists(&destination)
        && git::is_expected_repository(&destination, &tool.definition.url)?;
    context.record(Notice::ToolSourceInspected {
        installed,
        url: tool.definition.url.clone(),
        revision: tool.definition.revision.clone(),
    });
    let repository = if path_exists(&destination) && git::is_repository(&destination)? {
        Some(git::status(&destination)?)
    } else {
        None
    };
    context.record(Notice::RepositoryInspected(repository.clone()));
    Ok(ToolStatus {
        tool,
        path: destination,
        installed,
        repository,
    })
}

pub fn tool_path(
    paths: &Paths,
    name: &str,
    catalog_name: Option<&str>,
    context: &mut OperationContext<'_>,
) -> Result<PathBuf> {
    let _process_scope = crate::process::scope(&context.process);
    context.process.cancellation.check()?;
    let tool = resolve_tool(paths, name, catalog_name, context)?;
    paths.tool(&tool.catalog, &tool.name)
}

pub fn installed_tool_path(
    paths: &Paths,
    name: &str,
    catalog_name: &str,
    context: &mut OperationContext<'_>,
) -> Result<PathBuf> {
    let _process_scope = crate::process::scope(&context.process);
    context.process.cancellation.check()?;
    let tool = resolve_tool(paths, name, Some(catalog_name), context)?;
    let destination = paths.tool(&tool.catalog, &tool.name)?;
    if !path_exists(&destination) {
        bail!("tool '{name}' from catalog '{catalog_name}' is not installed");
    }
    if !git::is_repository(&destination)? {
        bail!("installed tool destination is not a Git repository");
    }
    if !git::is_expected_repository(&destination, &tool.definition.url)? {
        if equivalent_github_checkout(&destination, &tool.definition.url)? {
            bail!(
                "installed tool destination uses a different transport for the configured GitHub repository\nRun 'loadbot pull {name}' interactively to reconcile its fetch and push URLs."
            );
        }
        bail!("installed tool destination is not the configured Git repository");
    }
    Ok(destination)
}

fn equivalent_github_checkout(destination: &Path, configured_url: &str) -> Result<bool> {
    let direct = git::repository_match(destination, configured_url, &[])?;
    if direct == git::RepositoryMatch::EquivalentGithub {
        return Ok(true);
    }
    if direct != git::RepositoryMatch::Mismatch {
        return Ok(false);
    }
    let aliases = optional_identities(git::verified_rot_identities())?
        .into_iter()
        .map(|identity| identity.alias)
        .collect::<Vec<_>>();
    Ok(
        git::repository_match(destination, configured_url, &aliases)?
            == git::RepositoryMatch::EquivalentGithub,
    )
}

pub fn installed_tools(
    paths: &Paths,
    context: &mut OperationContext<'_>,
) -> Result<Vec<ResolvedTool>> {
    let _process_scope = crate::process::scope(&context.process);
    context.process.cancellation.check()?;
    let mut installed = Vec::new();
    for tool in all_tools(paths, context)? {
        context.process.cancellation.check()?;
        let destination = paths.tool(&tool.catalog, &tool.name)?;
        if path_exists(&destination)
            && git::is_expected_repository(&destination, &tool.definition.url)?
        {
            installed.push(tool);
        }
    }
    Ok(installed)
}

pub fn all_tools(paths: &Paths, context: &mut OperationContext<'_>) -> Result<Vec<ResolvedTool>> {
    let _process_scope = crate::process::scope(&context.process);
    context.process.cancellation.check()?;
    let local = config::load(&paths.config())?;
    let mut tools = Vec::new();
    let mut portable_names = BTreeMap::new();
    for (catalog_name, source) in &local.catalogs {
        context.process.cancellation.check()?;
        if let Err(error) = checked_catalog_repository(paths, catalog_name, source) {
            warn_skipped_catalog(catalog_name, error, context)?;
            continue;
        }
        let catalog_file = match catalog::load(&paths.catalog_file(catalog_name)) {
            Ok(catalog_file) => catalog_file,
            Err(error) => {
                warn_skipped_catalog(catalog_name, error, context)?;
                continue;
            }
        };
        let mut normalized_names = BTreeMap::new();
        for (name, definition) in catalog_file.tools {
            context.process.cancellation.check()?;
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

pub fn resolve_tool(
    paths: &Paths,
    name: &str,
    catalog_name: Option<&str>,
    context: &mut OperationContext<'_>,
) -> Result<ResolvedTool> {
    let _process_scope = crate::process::scope(&context.process);
    context.process.cancellation.check()?;
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

    let matches: Vec<_> = all_tools(paths, context)?
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

pub fn writable_catalogs(paths: &Paths, context: &mut OperationContext<'_>) -> Result<Vec<String>> {
    let _process_scope = crate::process::scope(&context.process);
    context.process.cancellation.check()?;
    let local = config::load(&paths.config())?;
    available_catalogs(paths, &local, true, context)
}

pub fn default_writable_catalog(
    paths: &Paths,
    context: &mut OperationContext<'_>,
) -> Result<Option<String>> {
    let _process_scope = crate::process::scope(&context.process);
    context.process.cancellation.check()?;
    let local = config::load(&paths.config())?;
    let Some(name) = local.default_catalog else {
        return Ok(None);
    };
    let Some(source) = local.catalogs.get(&name) else {
        return Ok(None);
    };
    if !source.writable || !catalog_is_available(paths, &name, source, context)? {
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

pub fn available_catalog_names(
    paths: &Paths,
    context: &mut OperationContext<'_>,
) -> Result<Vec<String>> {
    let _process_scope = crate::process::scope(&context.process);
    context.process.cancellation.check()?;
    let local = config::load(&paths.config())?;
    available_catalogs(paths, &local, false, context)
}

fn available_catalogs(
    paths: &Paths,
    local: &LocalConfig,
    writable_only: bool,
    context: &mut OperationContext<'_>,
) -> Result<Vec<String>> {
    let mut names = Vec::new();
    for (name, source) in &local.catalogs {
        context.process.cancellation.check()?;
        if writable_only && !source.writable {
            continue;
        }
        if catalog_is_available(paths, name, source, context)? {
            names.push(name.clone());
        }
    }
    Ok(names)
}

fn catalog_is_available(
    paths: &Paths,
    name: &str,
    source: &CatalogSource,
    context: &mut OperationContext<'_>,
) -> Result<bool> {
    let result = checked_catalog_repository(paths, name, source)
        .and_then(|_| catalog::load(&paths.catalog_file(name)).map(|_| ()));
    if let Err(error) = result {
        warn_skipped_catalog(name, error, context)?;
        return Ok(false);
    }
    Ok(true)
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

fn aborts_operation(error: &anyhow::Error) -> bool {
    error.downcast_ref::<crate::process::Cancelled>().is_some()
        || error
            .downcast_ref::<crate::process::CleanupIncomplete>()
            .is_some()
        || error.downcast_ref::<crate::persistence::Busy>().is_some()
}

fn optional_identities(result: Result<Vec<git::RotIdentity>>) -> Result<Vec<git::RotIdentity>> {
    match result {
        Ok(identities) => Ok(identities),
        Err(error) if aborts_operation(&error) => Err(error),
        Err(_) => Ok(Vec::new()),
    }
}

fn warn_skipped_catalog(
    name: &str,
    error: anyhow::Error,
    context: &mut OperationContext<'_>,
) -> Result<()> {
    if aborts_operation(&error) {
        return Err(error);
    }
    context.record(Notice::SkippedCatalog {
        name: name.to_owned(),
        diagnostic: format!("{error:#}"),
    });
    context.process.cancellation.check()
}

fn cleanup_is_safe(error: &anyhow::Error) -> bool {
    error.downcast_ref::<crate::persistence::Busy>().is_none()
        && error
            .downcast_ref::<crate::process::CleanupIncomplete>()
            .is_none()
        && error
            .downcast_ref::<crate::persistence::DurabilityUncertain>()
            .is_none()
}

fn cleanup_failed_clone(destination: &Path, error: anyhow::Error) -> anyhow::Error {
    cleanup_catalog_add_failure(destination, error, "clone operation failed")
}

fn cleanup_catalog_add_failure(
    destination: &Path,
    error: anyhow::Error,
    context: &str,
) -> anyhow::Error {
    if !cleanup_is_safe(&error) {
        return error.context("checkout retained because cleanup is not safe");
    }
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
    use std::collections::VecDeque;
    use std::ffi::OsStr;
    use std::process::Command;

    use tempfile::TempDir;

    use super::*;

    #[derive(Default)]
    struct TestPrompt {
        confirmations: VecDeque<bool>,
        selection: Option<String>,
        confirm_calls: usize,
        select_calls: usize,
    }

    impl TestPrompt {
        fn with_confirmations(confirmations: impl IntoIterator<Item = bool>) -> Self {
            Self {
                confirmations: confirmations.into_iter().collect(),
                ..Self::default()
            }
        }
    }

    impl Interaction for TestPrompt {
        fn configure_push(&mut self) -> Result<bool> {
            self.confirm_calls += 1;
            Ok(self.confirmations.pop_front().unwrap_or(false))
        }
        fn reconcile_checkout(&mut self, _: &str, _: &str) -> Result<bool> {
            self.configure_push()
        }
        fn replace_push(&mut self, _: &str, _: &str) -> Result<bool> {
            self.configure_push()
        }
        fn choose_identity(&mut self, identities: &[git::RotIdentity]) -> Result<Option<usize>> {
            self.select_calls += 1;
            Ok(self.selection.as_ref().and_then(|selected| {
                identities.iter().position(|identity| {
                    format!(
                        "{} -> {}",
                        identity.alias,
                        identity.username.as_deref().unwrap()
                    ) == *selected
                })
            }))
        }
    }

    fn identity(alias: &str, username: &str) -> git::RotIdentity {
        git::RotIdentity {
            alias: alias.to_owned(),
            username: Some(username.to_owned()),
            verification: "verified".to_owned(),
        }
    }

    fn repository_with_origin(base: &Path, origin: &str) -> PathBuf {
        let repository = base.join("checkout");
        git(["init", "--quiet", repository.to_str().unwrap()], None);
        git(["remote", "add", "origin", origin], Some(&repository));
        repository
    }

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
    fn public_https_push_configuration_is_optional_and_rot_is_not_required() {
        let temporary = TempDir::new().unwrap();
        let repository =
            repository_with_origin(temporary.path(), "https://github.com/owner/repo.git");
        let mut prompt = TestPrompt::default();

        offer_ssh_push(
            &repository,
            "https://github.com/owner/repo.git",
            true,
            &mut OperationContext::new(&mut prompt),
            &mut || bail!("Rot unavailable"),
        )
        .unwrap();

        assert_eq!(
            git::fetch_url(&repository).unwrap().as_deref(),
            Some("https://github.com/owner/repo.git")
        );
        assert_eq!(git::push_url(&repository).unwrap(), None);
        assert_eq!(prompt.confirm_calls, 0);
    }

    #[test]
    fn public_https_push_configuration_can_be_declined() {
        let temporary = TempDir::new().unwrap();
        let repository =
            repository_with_origin(temporary.path(), "https://github.com/owner/repo.git");
        let mut prompt = TestPrompt::with_confirmations([false]);

        offer_ssh_push(
            &repository,
            "https://github.com/owner/repo.git",
            true,
            &mut OperationContext::new(&mut prompt),
            &mut || Ok(vec![identity("github-work", "owner")]),
        )
        .unwrap();

        assert_eq!(git::push_url(&repository).unwrap(), None);
        assert_eq!(prompt.confirm_calls, 1);
    }

    #[test]
    fn public_https_push_configuration_uses_one_or_selected_rot_identity() {
        for (identities, selection, expected, expected_selects) in [
            (
                vec![identity("github-only", "owner")],
                None,
                "git@github-only:Owner/Repo.git",
                0,
            ),
            (
                vec![
                    identity("github-personal", "owner"),
                    identity("github-work", "work"),
                ],
                Some("github-work -> work".to_owned()),
                "git@github-work:Owner/Repo.git",
                1,
            ),
        ] {
            let temporary = TempDir::new().unwrap();
            let repository =
                repository_with_origin(temporary.path(), "https://github.com/Owner/Repo.git");
            let mut prompt = TestPrompt::with_confirmations([true]);
            prompt.selection = selection;

            offer_ssh_push(
                &repository,
                "https://github.com/Owner/Repo.git",
                true,
                &mut OperationContext::new(&mut prompt),
                &mut || Ok(identities.clone()),
            )
            .unwrap();

            assert_eq!(
                git::push_url(&repository).unwrap().as_deref(),
                Some(expected)
            );
            assert_eq!(prompt.select_calls, expected_selects);
        }
    }

    #[test]
    fn existing_push_url_is_preserved_without_prompting_or_querying_rot() {
        let temporary = TempDir::new().unwrap();
        let repository =
            repository_with_origin(temporary.path(), "https://github.com/owner/repo.git");
        git::set_push_url(&repository, "git@example.test:owner/repo.git").unwrap();
        let mut prompt = TestPrompt::default();
        let mut queries = 0;

        offer_ssh_push(
            &repository,
            "https://github.com/owner/repo.git",
            true,
            &mut OperationContext::new(&mut prompt),
            &mut || {
                queries += 1;
                Ok(vec![identity("github-work", "owner")])
            },
        )
        .unwrap();

        assert_eq!(
            git::push_url(&repository).unwrap().as_deref(),
            Some("git@example.test:owner/repo.git")
        );
        assert_eq!(queries, 0);
        assert_eq!(prompt.confirm_calls, 0);
    }

    #[test]
    fn approved_github_ssh_transports_reconcile_without_touching_dirty_files() {
        for (index, origin) in [
            "git@github.com:owner/repo.git",
            "ssh://git@github.com/owner/repo.git",
            "git@github-work:owner/repo.git",
        ]
        .into_iter()
        .enumerate()
        {
            let temporary = TempDir::new().unwrap();
            let repository = repository_with_origin(temporary.path(), origin);
            fs::write(repository.join(format!("dirty-{index}.txt")), "keep\n").unwrap();
            let before = git::working_tree_changes(&repository).unwrap();
            let mut prompt = TestPrompt::with_confirmations([true]);

            reconcile_existing_checkout(
                &repository,
                "demo",
                "https://github.com/owner/repo.git",
                true,
                &mut OperationContext::new(&mut prompt),
            )
            .unwrap();

            assert_eq!(
                git::fetch_url(&repository).unwrap().as_deref(),
                Some("https://github.com/owner/repo.git")
            );
            assert_eq!(git::push_url(&repository).unwrap().as_deref(), Some(origin));
            assert_eq!(git::working_tree_changes(&repository).unwrap(), before);
        }
    }

    #[test]
    fn declined_and_noninteractive_reconciliation_change_nothing() {
        for (interactive, confirmations, expected) in [
            (true, vec![false], "mismatch was not changed"),
            (false, Vec::new(), "interactively to reconcile"),
        ] {
            let temporary = TempDir::new().unwrap();
            let origin = "git@github.com:owner/repo.git";
            let repository = repository_with_origin(temporary.path(), origin);
            let mut prompt = TestPrompt::with_confirmations(confirmations);

            let error = reconcile_existing_checkout(
                &repository,
                "demo",
                "https://github.com/owner/repo.git",
                interactive,
                &mut OperationContext::new(&mut prompt),
            )
            .unwrap_err();

            assert!(error.to_string().contains(expected));
            assert_eq!(
                git::fetch_url(&repository).unwrap().as_deref(),
                Some(origin)
            );
            assert_eq!(git::push_url(&repository).unwrap(), None);
        }
    }

    fn reconciliation_fixture(push: Option<&str>) -> (TempDir, Paths, PathBuf) {
        let temporary = TempDir::new().unwrap();
        let paths = Paths::with_root(temporary.path().join("loadbot"));
        let remote = valid_catalog_remote(temporary.path(), "reconciliation-catalog");
        catalog_add(
            &paths,
            "personal",
            remote.display().to_string(),
            false,
            &mut OperationContext::new(&mut crate::interaction::Unattended),
        )
        .unwrap();
        fs::write(paths.catalog_file("personal"),
            "version = 1\n[tools.demo]\ntype = \"git\"\nurl = \"https://github.com/owner/repo.git\"\n",
        ).unwrap();
        fs::create_dir_all(paths.tools().join("personal")).unwrap();
        let checkout = repository_with_origin(
            &paths.tools().join("personal"),
            "git@github.com:owner/repo.git",
        );
        let destination = paths.tool("personal", "demo").unwrap();
        fs::rename(checkout, &destination).unwrap();
        if let Some(push) = push {
            git::set_push_url(&destination, push).unwrap();
        }
        fs::write(destination.join("dirty.txt"), "preserve\n").unwrap();
        (temporary, paths, destination)
    }

    fn reconciliation_report(
        paths: &Paths,
        context: &mut OperationContext<'_>,
    ) -> crate::interaction::OperationReport<()> {
        context.run(|context| {
            tool_pull_with(
                paths,
                "demo",
                Some("personal"),
                true,
                context,
                || Ok(Vec::new()),
                |_, _, _, _| panic!("an existing checkout must not be cloned"),
            )
        })
    }

    #[test]
    fn reconciliation_records_completion_when_cancelled_between_or_after_url_writes() {
        use crate::{interaction::OperationStatus, process::Event};
        use std::sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        };

        for cancel_between_writes in [true, false] {
            let (_temporary, paths, destination) = reconciliation_fixture(None);
            let mut prompt = TestPrompt::with_confirmations([true]);
            let mut context = OperationContext::new(&mut prompt);
            let token = context.process.cancellation.clone();
            let fetch_write = AtomicBool::new(false);
            let inspected_destination = destination.clone();
            context.process.observer = Some(Arc::new(move |event| match event {
                Event::Starting { arguments, .. } => {
                    let writing_fetch = arguments.iter().any(|arg| arg == "--replace-all")
                        && arguments.iter().any(|arg| arg == "remote.origin.url");
                    fetch_write.store(writing_fetch, Ordering::SeqCst);
                    if writing_fetch {
                        assert!(
                            crate::persistence::Lease::acquire(&inspected_destination)
                                .err()
                                .unwrap()
                                .downcast_ref::<crate::persistence::Busy>()
                                .is_some()
                        );
                        if cancel_between_writes {
                            token.cancel();
                        }
                    }
                }
                Event::Exited { status, .. } if fetch_write.load(Ordering::SeqCst) => {
                    assert!(status.success());
                    if !cancel_between_writes {
                        token.cancel();
                    }
                }
                _ => {}
            }));
            let report = reconciliation_report(&paths, &mut context);
            assert!(context.process.cancellation.is_cancelled());
            assert_eq!(report.status(), OperationStatus::Cancelled, "{report:?}");
            assert!(report.is_partial());
            assert_eq!(
                report
                    .notices
                    .iter()
                    .filter(
                        |notice| matches!(notice, Notice::ToolReconciled { name } if name == "demo")
                    )
                    .count(),
                1
            );
            assert_eq!(
                git::fetch_url(&destination).unwrap().as_deref(),
                Some("https://github.com/owner/repo.git")
            );
            assert_eq!(
                git::push_url(&destination).unwrap().as_deref(),
                Some("git@github.com:owner/repo.git")
            );
            assert_eq!(
                fs::read_to_string(destination.join("dirty.txt")).unwrap(),
                "preserve\n"
            );
            assert!(crate::persistence::Lease::acquire(&destination).is_ok());
        }
    }

    #[test]
    fn reconciliation_recovers_original_urls_after_write_failure_despite_cancellation() {
        use crate::{interaction::OperationStatus, process::Event};
        use std::sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        };

        for original_push in [None, Some("git@other:owner/repo.git")] {
            let (_temporary, paths, destination) = reconciliation_fixture(original_push);
            let before = fs::read(destination.join(".git/config")).unwrap();
            let mut prompt = TestPrompt::with_confirmations([true, true]);
            let mut context = OperationContext::new(&mut prompt);
            let token = context.process.cancellation.clone();
            let inject = AtomicBool::new(true);
            let failing = AtomicBool::new(false);
            let lock = destination.join(".git/config.lock");
            context.process.observer = Some(Arc::new(move |event| match event {
                Event::Starting { arguments, .. }
                    if arguments.iter().any(|arg| arg == "--replace-all")
                        && arguments.iter().any(|arg| arg == "remote.origin.url")
                        && inject.swap(false, Ordering::SeqCst) =>
                {
                    fs::write(&lock, "owned by this test").unwrap();
                    failing.store(true, Ordering::SeqCst);
                    token.cancel();
                }
                Event::Exited { status, .. } if failing.swap(false, Ordering::SeqCst) => {
                    assert!(!status.success());
                    fs::remove_file(&lock).unwrap();
                }
                _ => {}
            }));
            let report = reconciliation_report(&paths, &mut context);
            assert!(context.process.cancellation.is_cancelled());
            assert_eq!(report.status(), OperationStatus::Failed, "{report:?}");
            assert!(!report.is_partial());
            assert!(
                !report
                    .notices
                    .iter()
                    .any(|notice| matches!(notice, Notice::ToolReconciled { .. }))
            );
            assert_eq!(
                git::fetch_url(&destination).unwrap().as_deref(),
                Some("git@github.com:owner/repo.git")
            );
            assert_eq!(
                git::push_url(&destination).unwrap().as_deref(),
                original_push
            );
            assert_eq!(fs::read(destination.join(".git/config")).unwrap(), before);
            assert_eq!(
                fs::read_to_string(destination.join("dirty.txt")).unwrap(),
                "preserve\n"
            );
            assert!(crate::persistence::Lease::acquire(&destination).is_ok());
        }
    }

    #[test]
    fn reconciliation_reports_remaining_changes_when_recovery_fails() {
        use crate::{interaction::OperationStatus, process::Event};
        use std::sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        };

        let (_temporary, paths, destination) = reconciliation_fixture(None);
        let mut prompt = TestPrompt::with_confirmations([true]);
        let mut context = OperationContext::new(&mut prompt);
        let token = context.process.cancellation.clone();
        let inject = AtomicBool::new(true);
        let lock = destination.join(".git/config.lock");
        let retained_lock = lock.clone();
        context.process.observer = Some(Arc::new(move |event| {
            if let Event::Starting { arguments, .. } = event
                && arguments.iter().any(|arg| arg == "--replace-all")
                && arguments.iter().any(|arg| arg == "remote.origin.url")
                && inject.swap(false, Ordering::SeqCst)
            {
                fs::write(&lock, "owned by this test").unwrap();
                token.cancel();
            }
        }));
        let report = reconciliation_report(&paths, &mut context);
        assert!(context.process.cancellation.is_cancelled());
        assert_eq!(report.status(), OperationStatus::Failed, "{report:?}");
        assert!(report.is_partial());
        assert!(
            report
                .result
                .as_ref()
                .unwrap_err()
                .downcast_ref::<git::RemoteRecoveryIncomplete>()
                .is_some()
        );
        assert!(
            !report
                .notices
                .iter()
                .any(|notice| matches!(notice, Notice::ToolReconciled { .. }))
        );
        assert_eq!(
            git::fetch_url(&destination).unwrap().as_deref(),
            Some("git@github.com:owner/repo.git")
        );
        assert_eq!(
            git::push_url(&destination).unwrap().as_deref(),
            Some("git@github.com:owner/repo.git")
        );
        assert_eq!(
            fs::read_to_string(&retained_lock).unwrap(),
            "owned by this test"
        );
        fs::remove_file(retained_lock).unwrap();
        assert!(crate::persistence::Lease::acquire(&destination).is_ok());
    }

    #[test]
    fn reconciliation_honors_cancellation_before_the_transaction() {
        let (_temporary, _paths, destination) = reconciliation_fixture(None);
        let before = fs::read(destination.join(".git/config")).unwrap();
        let mut prompt = TestPrompt::default();
        let mut context = OperationContext::new(&mut prompt);
        let report = context.run(|context| {
            let _lease = context.lease(&destination)?;
            context.process.cancellation.cancel();
            git::reconcile_remote(
                &destination,
                "https://github.com/owner/repo.git",
                "git@github.com:owner/repo.git",
            )
        });
        assert_eq!(
            report.status(),
            crate::interaction::OperationStatus::Cancelled
        );
        assert!(!report.is_partial());
        assert!(report.notices.is_empty());
        assert_eq!(fs::read(destination.join(".git/config")).unwrap(), before);
    }

    #[test]
    fn reconciliation_never_silently_overwrites_an_existing_push_url() {
        let temporary = TempDir::new().unwrap();
        let origin = "git@github.com:owner/repo.git";
        let repository = repository_with_origin(temporary.path(), origin);
        git::set_push_url(&repository, "git@other:owner/repo.git").unwrap();
        let mut prompt = TestPrompt::with_confirmations([true, false]);

        let error = reconcile_existing_checkout(
            &repository,
            "demo",
            "https://github.com/owner/repo.git",
            true,
            &mut OperationContext::new(&mut prompt),
        )
        .unwrap_err();

        assert!(
            error
                .to_string()
                .contains("existing push URL was preserved")
        );
        assert_eq!(
            git::fetch_url(&repository).unwrap().as_deref(),
            Some(origin)
        );
        assert_eq!(
            git::push_url(&repository).unwrap().as_deref(),
            Some("git@other:owner/repo.git")
        );
    }

    #[test]
    fn private_ssh_catalog_urls_do_not_offer_a_separate_push_url() {
        let temporary = TempDir::new().unwrap();
        let origin = "git@github.com:owner/private.git";
        let repository = repository_with_origin(temporary.path(), origin);
        let mut prompt = TestPrompt::default();
        let mut queries = 0;

        offer_ssh_push(
            &repository,
            origin,
            true,
            &mut OperationContext::new(&mut prompt),
            &mut || {
                queries += 1;
                Ok(vec![identity("github-work", "owner")])
            },
        )
        .unwrap();

        assert_eq!(
            git::fetch_url(&repository).unwrap().as_deref(),
            Some(origin)
        );
        assert_eq!(git::push_url(&repository).unwrap(), None);
        assert_eq!(queries, 0);
        assert_eq!(prompt.confirm_calls, 0);
    }

    #[test]
    fn launcher_validation_points_equivalent_checkouts_to_interactive_pull() {
        let temporary = TempDir::new().unwrap();
        let paths = Paths::with_root(temporary.path().join("loadbot"));
        let catalog_remote = valid_catalog_remote(temporary.path(), "launcher-catalog");
        catalog_add(
            &paths,
            "personal",
            catalog_remote.display().to_string(),
            false,
            &mut OperationContext::new(&mut crate::interaction::Unattended),
        )
        .unwrap();
        fs::write(
            paths.catalog_file("personal"),
            "version = 1\n\n[tools.demo]\ntype = \"git\"\nurl = \"https://github.com/owner/repo.git\"\n",
        )
        .unwrap();
        fs::create_dir_all(paths.tools().join("personal")).unwrap();
        let checkout = repository_with_origin(
            &paths.tools().join("personal"),
            "git@github.com:owner/repo.git",
        );
        fs::rename(checkout, paths.tool("personal", "demo").unwrap()).unwrap();

        let error = installed_tool_path(
            &paths,
            "demo",
            "personal",
            &mut OperationContext::new(&mut crate::interaction::Unattended),
        )
        .unwrap_err();

        assert!(
            error
                .to_string()
                .contains("Run 'loadbot pull demo' interactively")
        );
        assert_eq!(
            git::fetch_url(&paths.tool("personal", "demo").unwrap())
                .unwrap()
                .as_deref(),
            Some("git@github.com:owner/repo.git")
        );
    }

    #[test]
    fn new_public_https_clone_succeeds_without_rot_and_keeps_canonical_fetch() {
        let temporary = TempDir::new().unwrap();
        let paths = Paths::with_root(temporary.path().join("loadbot"));
        let catalog_remote = valid_catalog_remote(temporary.path(), "public-tool-catalog");
        catalog_add(
            &paths,
            "personal",
            catalog_remote.display().to_string(),
            false,
            &mut OperationContext::new(&mut crate::interaction::Unattended),
        )
        .unwrap();
        let canonical = "https://github.com/owner/repo.git";
        fs::write(
            paths.catalog_file("personal"),
            format!("version = 1\n\n[tools.demo]\ntype = \"git\"\nurl = {canonical:?}\n"),
        )
        .unwrap();
        let mut prompt = TestPrompt::default();

        tool_pull_with(
            &paths,
            "demo",
            Some("personal"),
            true,
            &mut OperationContext::new(&mut prompt),
            || bail!("Rot unavailable"),
            |url, _, destination, _interaction| {
                assert!(destination.is_dir());
                assert_eq!(fs::read_dir(destination).unwrap().count(), 0);
                git(["init", "--quiet"], Some(destination));
                git(["remote", "add", "origin", url], Some(destination));
                Ok(())
            },
        )
        .unwrap();

        let destination = paths.tool("personal", "demo").unwrap();
        assert_eq!(
            git::fetch_url(&destination).unwrap().as_deref(),
            Some(canonical)
        );
        assert_eq!(git::push_url(&destination).unwrap(), None);
        assert_eq!(prompt.confirm_calls, 0);
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
            &mut OperationContext::new(&mut crate::interaction::Unattended),
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

        let error = catalog_add(
            &paths,
            "invalid",
            remote.display().to_string(),
            false,
            &mut OperationContext::new(&mut crate::interaction::Unattended),
        )
        .unwrap_err();

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
            &mut crate::interaction::Unattended,
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
                &mut OperationContext::new(&mut crate::interaction::Unattended)
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
        catalog_add(
            &paths,
            "personal",
            expected.display().to_string(),
            true,
            &mut OperationContext::new(&mut crate::interaction::Unattended),
        )
        .unwrap();
        let config_before = fs::read(paths.config()).unwrap();

        fs::remove_dir_all(paths.catalog("personal")).unwrap();
        assert!(
            available_catalog_names(
                &paths,
                &mut OperationContext::new(&mut crate::interaction::Unattended)
            )
            .unwrap()
            .is_empty()
        );
        assert!(
            writable_catalogs(
                &paths,
                &mut OperationContext::new(&mut crate::interaction::Unattended)
            )
            .unwrap()
            .is_empty()
        );
        assert_eq!(
            default_writable_catalog(
                &paths,
                &mut OperationContext::new(&mut crate::interaction::Unattended)
            )
            .unwrap(),
            None
        );
        assert_eq!(catalog_names(&paths).unwrap(), ["personal"]);
        assert_eq!(fs::read(paths.config()).unwrap(), config_before);

        git::clone_repository(
            &wrong.display().to_string(),
            None,
            &paths.catalog("personal"),
            &mut crate::interaction::Unattended,
        )
        .unwrap();
        assert!(
            available_catalog_names(
                &paths,
                &mut OperationContext::new(&mut crate::interaction::Unattended)
            )
            .unwrap()
            .is_empty()
        );
        assert!(
            writable_catalogs(
                &paths,
                &mut OperationContext::new(&mut crate::interaction::Unattended)
            )
            .unwrap()
            .is_empty()
        );
        assert_eq!(fs::read(paths.config()).unwrap(), config_before);
    }

    #[test]
    fn catalog_initialization_requires_explicit_commit_and_push() {
        let temporary = TempDir::new().unwrap();
        let paths = Paths::with_root(temporary.path().join("loadbot"));
        let remote = empty_remote(temporary.path(), "new-catalog");
        let url = remote.display().to_string();

        catalog_initialize(
            &paths,
            "personal",
            url.clone(),
            true,
            false,
            false,
            &mut OperationContext::new(&mut crate::interaction::Unattended),
        )
        .unwrap();
        assert_eq!(
            catalog::load(&paths.catalog_file("personal")).unwrap(),
            CatalogFile::default()
        );
        assert_eq!(git::head_commit(&paths.catalog("personal")).unwrap(), None);
        assert!(
            !git::origin_has_refs(
                &paths.catalog("personal"),
                &mut crate::interaction::Unattended
            )
            .unwrap()
        );
        catalog_status(
            &paths,
            "personal",
            &mut OperationContext::new(&mut crate::interaction::Unattended),
        )
        .unwrap();
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
        catalog_initialize(
            &paths,
            "personal",
            url.clone(),
            true,
            true,
            true,
            &mut OperationContext::new(&mut crate::interaction::Unattended),
        )
        .unwrap();
        assert!(
            git::head_commit(&paths.catalog("personal"))
                .unwrap()
                .is_some()
        );
        assert!(
            git::origin_has_refs(
                &paths.catalog("personal"),
                &mut crate::interaction::Unattended
            )
            .unwrap()
        );

        let before = git::head_commit(&paths.catalog("personal")).unwrap();
        catalog_initialize(
            &paths,
            "personal",
            url,
            true,
            true,
            true,
            &mut OperationContext::new(&mut crate::interaction::Unattended),
        )
        .unwrap();
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
            &mut OperationContext::new(&mut crate::interaction::Unattended),
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
            &mut OperationContext::new(&mut crate::interaction::Unattended),
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
            &mut OperationContext::new(&mut crate::interaction::Unattended),
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
            &mut OperationContext::new(&mut crate::interaction::Unattended),
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
        catalog_initialize(
            &paths,
            "dirty",
            url.clone(),
            true,
            false,
            false,
            &mut OperationContext::new(&mut crate::interaction::Unattended),
        )
        .unwrap();
        fs::write(paths.catalog("dirty").join("unrelated.txt"), "keep\n").unwrap();

        let error = catalog_initialize(
            &paths,
            "dirty",
            url,
            true,
            true,
            false,
            &mut OperationContext::new(&mut crate::interaction::Unattended),
        )
        .unwrap_err();
        assert!(
            format!("{error:#}").contains("unrelated changes"),
            "{error:#}"
        );
        assert_eq!(
            fs::read_to_string(paths.catalog("dirty").join("unrelated.txt")).unwrap(),
            "keep\n"
        );
        assert_eq!(git::head_commit(&paths.catalog("dirty")).unwrap(), None);
    }
}
