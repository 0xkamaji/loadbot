use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use crate::catalog::{self, CatalogFile, ResolvedTool, Runner, ToolConfig};
use crate::config::{self, CatalogSource, LocalConfig};
use crate::git;
use crate::interaction::{
    Interaction, MutationOutcome, Notice, OperationContext, ToolOperation, ToolOperationStage,
};
use crate::paths::{self, Paths};
use crate::shortcuts::{self, Shortcut};

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

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolPushInspection {
    pub changed_files: Vec<git::RepositoryChange>,
    pub commits_ahead: bool,
}

/// Lazily resolves verified Rot aliases for one managed-tool operation.
/// Canonical and missing checkouts never need to invoke Rot; alias candidates
/// share one verified identity snapshot across the whole operation.
struct ManagedToolRepositories {
    verified: Option<git::ManagedRepositoryMatcher>,
}

impl ManagedToolRepositories {
    fn new() -> Self {
        Self { verified: None }
    }

    #[cfg(test)]
    fn with_verified_identities(identities: &[git::RotIdentity]) -> Self {
        Self {
            verified: Some(git::ManagedRepositoryMatcher::from_verified_rot_identities(
                identities,
            )),
        }
    }

    fn checkout_match(
        &mut self,
        path: &Path,
        configured_url: &str,
    ) -> Result<git::ManagedCheckout> {
        let direct =
            git::ManagedRepositoryMatcher::canonical().checkout_match(path, configured_url)?;
        if direct != git::ManagedCheckout::RequiresVerifiedAlias {
            return Ok(direct);
        }
        if self.verified.is_none() {
            let identities = optional_identities(git::verified_rot_identities())?;
            self.verified = Some(git::ManagedRepositoryMatcher::from_verified_rot_identities(
                &identities,
            ));
        }
        let verified = self.verified.as_ref().expect("verified matcher was set");
        Ok(match verified.checkout_match(path, configured_url)? {
            git::ManagedCheckout::RequiresVerifiedAlias => git::ManagedCheckout::Mismatch,
            matched => matched,
        })
    }

    fn is_managed_checkout(&mut self, path: &Path, configured_url: &str) -> Result<bool> {
        Ok(matches!(
            self.checkout_match(path, configured_url)?,
            git::ManagedCheckout::ExpectedTransport | git::ManagedCheckout::EquivalentTransport
        ))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ShortcutIdentity {
    pub name: String,
    pub catalog: String,
    pub tool: String,
    pub path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShortcutHelpRequest {
    pub catalog: String,
    pub tool: String,
    pub target: String,
    pub runner: Runner,
    pub working_directory: crate::recipe::WorkingDirectory,
}

/// Resolve and probe help for one project-owned target. This is deliberately a
/// read-only discovery operation: it never persists a shortcut or parses output.
pub fn shortcut_help(
    paths: &Paths,
    request: &ShortcutHelpRequest,
    context: &mut OperationContext<'_>,
) -> Result<crate::launcher::HelpResult> {
    let _process_scope = crate::process::scope(&context.process);
    context.process.cancellation.check()?;
    let project_path = paths.tool(&request.catalog, &request.tool)?;
    let _repository_lease = context.lease(&project_path)?;
    let root = installed_tool_path(paths, &request.tool, &request.catalog, context)?;
    let arguments = if request.runner == Runner::Direct {
        Vec::new()
    } else {
        vec![crate::recipe::RecipeArgument::ProjectPath {
            path: request.target.clone(),
        }]
    };
    let recipe = crate::recipe::RecipeDefinition {
        version: crate::recipe::RECIPE_VERSION,
        behavior: crate::recipe::InvocationBehavior::Run,
        program: if request.runner == Runner::Direct {
            crate::recipe::RecipeProgram::ProjectFile {
                path: request.target.clone(),
            }
        } else {
            crate::recipe::RecipeProgram::Interpreter {
                runner: request.runner,
            }
        },
        working_directory: request.working_directory.clone(),
        arguments,
    };
    let resolved =
        crate::recipe::resolve_recipe(&root, &recipe, &crate::recipe::RuntimeInputs::new())?;
    let target = match resolved.program {
        crate::recipe::ResolvedProgram::ProjectFile(path) => path,
        crate::recipe::ResolvedProgram::Interpreter { .. } => resolved
            .argv
            .first()
            .map(PathBuf::from)
            .context("resolved help invocation has no project target")?,
        crate::recipe::ResolvedProgram::SearchPath { .. } => {
            bail!("help discovery requires a project-owned target")
        }
    };
    crate::launcher::view_help(&target, &resolved.cwd, request.runner, context)
}

/// Add one personal shortcut through the same qualified project and path checks used
/// by the launcher. The caller supplies semantic fields, never a native absolute path.
#[allow(clippy::too_many_arguments)]
pub fn shortcut_add(
    paths: &Paths,
    catalog_name: &str,
    tool_name: &str,
    name: &str,
    path: &str,
    description: Option<String>,
    runner: Option<Runner>,
    context: &mut OperationContext<'_>,
) -> Result<ShortcutIdentity> {
    let _process_scope = crate::process::scope(&context.process);
    context.process.cancellation.check()?;
    paths::validate_name(name).context("invalid shortcut name")?;
    let root = installed_tool_path(paths, tool_name, catalog_name, context)?;
    let relative = shortcuts::relative_path(path)?;
    crate::launcher::safe_target(&root, &relative)?;
    let portable = shortcuts::portable_path(&relative)?;
    let mut shortcut = Shortcut::new(
        catalog_name.to_owned(),
        tool_name.to_owned(),
        portable.clone(),
    )?;
    shortcut.description = description.filter(|value| !value.trim().is_empty());
    shortcut.invocation = crate::recipe::StoredInvocation::legacy(portable.clone(), runner);
    shortcuts::save(&paths.shortcuts()?, name, shortcut.clone())?;
    Ok(ShortcutIdentity {
        name: name.to_owned(),
        catalog: shortcut.catalog,
        tool: shortcut.tool,
        path: Some(portable),
    })
}

/// Create one personal structured Recipe. Project-owned paths are checked against
/// the installed project before the atomic shortcut transaction is attempted.
pub fn shortcut_add_recipe(
    paths: &Paths,
    catalog_name: &str,
    tool_name: &str,
    name: &str,
    description: Option<String>,
    recipe: crate::recipe::RecipeDefinition,
    context: &mut OperationContext<'_>,
) -> Result<ShortcutIdentity> {
    let _process_scope = crate::process::scope(&context.process);
    context.process.cancellation.check()?;
    paths::validate_name(name).context("invalid shortcut name")?;
    let root = installed_tool_path(paths, tool_name, catalog_name, context)?;
    crate::recipe::validate_recipe_for_project(&root, &recipe)?;
    let mut shortcut = Shortcut::with_invocation(
        catalog_name.to_owned(),
        tool_name.to_owned(),
        crate::recipe::StoredInvocation::Recipe(recipe),
    )?;
    shortcut.description = description.filter(|value| !value.trim().is_empty());
    shortcuts::save(&paths.shortcuts()?, name, shortcut)?;
    Ok(ShortcutIdentity {
        name: name.to_owned(),
        catalog: catalog_name.to_owned(),
        tool: tool_name.to_owned(),
        path: None,
    })
}

/// Update an existing personal Recipe in place. Legacy shortcuts deliberately fail
/// closed here and are never converted simply because an editor opened them.
pub fn shortcut_update_recipe(
    paths: &Paths,
    catalog_name: &str,
    tool_name: &str,
    name: &str,
    description: Option<String>,
    recipe: crate::recipe::RecipeDefinition,
    context: &mut OperationContext<'_>,
) -> Result<ShortcutIdentity> {
    let _process_scope = crate::process::scope(&context.process);
    context.process.cancellation.check()?;
    paths::validate_name(name).context("invalid shortcut name")?;
    let root = installed_tool_path(paths, tool_name, catalog_name, context)?;
    crate::recipe::validate_recipe_for_project(&root, &recipe)?;
    let path = paths.shortcuts()?;
    let current = shortcuts::load(&path)?;
    let existing = current
        .shortcuts
        .get(name)
        .with_context(|| format!("shortcut '{name}' does not exist"))?;
    if existing.catalog != catalog_name || existing.tool != tool_name {
        bail!("shortcut '{name}' does not belong to {catalog_name}/{tool_name}");
    }
    if existing.invocation.as_recipe().is_none() {
        bail!("legacy shortcut '{name}' cannot be edited as a Recipe");
    }
    let mut replacement = existing.clone();
    replacement.description = description.filter(|value| !value.trim().is_empty());
    replacement.invocation = crate::recipe::StoredInvocation::Recipe(recipe);
    shortcuts::update_if_matches(&path, name, existing, replacement)?;
    Ok(ShortcutIdentity {
        name: name.to_owned(),
        catalog: catalog_name.to_owned(),
        tool: tool_name.to_owned(),
        path: None,
    })
}

/// Delete personal shortcuts only after their complete qualified identities have
/// been resolved and rechecked under the shortcuts-file lease.
pub fn shortcut_delete_many(
    paths: &Paths,
    identities: &[ShortcutIdentity],
    context: &mut OperationContext<'_>,
) -> Result<usize> {
    let _process_scope = crate::process::scope(&context.process);
    context.process.cancellation.check()?;
    if identities.is_empty() {
        bail!("at least one shortcut is required");
    }
    let path = paths.shortcuts()?;
    let file = shortcuts::load(&path)?;
    let mut expected = Vec::with_capacity(identities.len());
    for identity in identities {
        context.process.cancellation.check()?;
        paths::validate_name(&identity.name).context("invalid shortcut name")?;
        let shortcut = file
            .shortcuts
            .get(&identity.name)
            .with_context(|| format!("personal shortcut '{}' does not exist", identity.name))?;
        if shortcut.catalog != identity.catalog || shortcut.tool != identity.tool {
            bail!(
                "personal shortcut '{}' does not belong to {}/{}",
                identity.name,
                identity.catalog,
                identity.tool
            );
        }
        match (&shortcut.invocation, &identity.path) {
            (crate::recipe::StoredInvocation::Legacy(legacy), Some(path))
                if legacy.path == *path => {}
            (crate::recipe::StoredInvocation::Recipe(_), None) => {}
            _ => bail!(
                "personal shortcut '{}' no longer matches the selected definition",
                identity.name
            ),
        }
        expected.push((identity.name.clone(), shortcut.clone()));
    }
    shortcuts::remove_many_if_matches(&path, &expected)?;
    Ok(expected.len())
}

pub fn shortcut_delete(
    paths: &Paths,
    identity: &ShortcutIdentity,
    context: &mut OperationContext<'_>,
) -> Result<()> {
    shortcut_delete_many(paths, std::slice::from_ref(identity), context).map(|_| ())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectPathKind {
    File,
    Directory,
}

/// Convert one native picker result into a portable project-relative path. Both
/// sides are canonicalized so symlinks cannot escape the installed project.
pub fn portable_project_path(
    paths: &Paths,
    catalog_name: &str,
    tool_name: &str,
    selected: &Path,
    kind: ProjectPathKind,
    context: &mut OperationContext<'_>,
) -> Result<String> {
    let root = installed_tool_path(paths, tool_name, catalog_name, context)?;
    let canonical_root = fs::canonicalize(&root)
        .with_context(|| format!("could not resolve installed project {}", root.display()))?;
    let canonical_selected = fs::canonicalize(selected)
        .with_context(|| format!("could not resolve selected path {}", selected.display()))?;
    let metadata = fs::metadata(&canonical_selected)?;
    match kind {
        ProjectPathKind::File if !metadata.is_file() => bail!("selected path is not a file"),
        ProjectPathKind::Directory if !metadata.is_dir() => {
            bail!("selected path is not a directory")
        }
        _ => {}
    }
    let relative = canonical_selected
        .strip_prefix(&canonical_root)
        .with_context(|| {
            format!("selected path is outside installed project {catalog_name}/{tool_name}")
        })?;
    if relative.as_os_str().is_empty() {
        bail!("select a path inside the tool; use Tool folder for the project root");
    }
    shortcuts::portable_path(relative)
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
        if git::ManagedRepositoryMatcher::canonical()
            .is_managed_checkout(&destination, &source.url)?
        {
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
        if !git::ManagedRepositoryMatcher::canonical()
            .is_managed_checkout(&destination, &source.url)?
        {
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
        if !git::ManagedRepositoryMatcher::canonical()
            .is_managed_checkout(&destination, &source.url)?
        {
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
        } else if git::ManagedRepositoryMatcher::canonical()
            .is_managed_checkout(&destination, &source.url)?
        {
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
    context.record(Notice::CatalogSyncStarted {
        name: name.to_owned(),
    });
    let destination = checked_catalog_repository(paths, name, source)?;
    context.record(Notice::CatalogSyncRepositoryChecked {
        name: name.to_owned(),
    });
    context.record(Notice::CatalogSyncUpdateStarted {
        name: name.to_owned(),
    });
    let (old_commit, new_commit) = git::update(&destination, &source.url, None, context)
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
    let mut repositories = ManagedToolRepositories::new();
    tool_list_with_repositories(paths, context, &mut repositories)
}

fn tool_list_with_repositories(
    paths: &Paths,
    context: &mut OperationContext<'_>,
    repositories: &mut ManagedToolRepositories,
) -> Result<Vec<ToolSummary>> {
    let _process_scope = crate::process::scope(&context.process);
    context.process.cancellation.check()?;
    let tools = all_tools(paths, context)?;
    let mut rows = Vec::new();
    for tool in tools {
        context.process.cancellation.check()?;
        let destination = paths.tool(&tool.catalog, &tool.name)?;
        let installed = repositories.is_managed_checkout(&destination, &tool.definition.url)?;
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
    mut load_identities: I,
    mut clone_repository: C,
) -> Result<()>
where
    I: FnMut() -> Result<Vec<git::RotIdentity>>,
    C: FnMut(&str, Option<&str>, &Path, &mut dyn Interaction) -> Result<()>,
{
    let mut cached_identities: Option<Vec<git::RotIdentity>> = None;
    let mut identities = || -> Result<Vec<git::RotIdentity>> {
        if let Some(identities) = cached_identities.as_ref() {
            return Ok(identities.clone());
        }
        let identities = optional_identities(load_identities())?;
        cached_identities = Some(identities.clone());
        Ok(identities)
    };
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
    context.record(Notice::ToolOperationStage {
        operation: ToolOperation::Pull,
        stage: ToolOperationStage::ValidatingCheckout,
        name: tool.name.clone(),
        catalog_name: tool.catalog.clone(),
    });
    if path_exists(&destination) {
        if !git::is_repository(&destination)? {
            bail!("destination exists but is not a Git repository");
        }
        let checkout_match =
            managed_checkout_with_identities(&destination, &tool.definition.url, &mut identities)?;
        if checkout_match == git::ManagedCheckout::ExpectedTransport {
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
        if checkout_match == git::ManagedCheckout::EquivalentTransport {
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
    context.record(Notice::ToolOperationStage {
        operation: ToolOperation::Pull,
        stage: ToolOperationStage::CloningProject,
        name: tool.name.clone(),
        catalog_name: tool.catalog.clone(),
    });
    if let Err(mut error) = clone_repository(
        &tool.definition.url,
        tool.definition.revision.as_deref(),
        &destination,
        context,
    ) {
        error = cleanup_failed_clone(&destination, error);
        return Err(error).context(format!("could not clone tool '{name}'"));
    }
    context.record(Notice::ToolOperationStage {
        operation: ToolOperation::Pull,
        stage: ToolOperationStage::ValidatingFreshCheckout,
        name: tool.name.clone(),
        catalog_name: tool.catalog.clone(),
    });
    let validation = (|| -> Result<()> {
        context.process.cancellation.check()?;
        if managed_checkout_with_identities(&destination, &tool.definition.url, &mut identities)?
            == git::ManagedCheckout::Mismatch
        {
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

fn managed_checkout_with_identities<I>(
    destination: &Path,
    configured_url: &str,
    identities: &mut I,
) -> Result<git::ManagedCheckout>
where
    I: FnMut() -> Result<Vec<git::RotIdentity>>,
{
    let direct =
        git::ManagedRepositoryMatcher::canonical().checkout_match(destination, configured_url)?;
    if direct != git::ManagedCheckout::RequiresVerifiedAlias {
        return Ok(direct);
    }
    let identities = optional_identities(identities())?;
    Ok(
        match git::ManagedRepositoryMatcher::from_verified_rot_identities(&identities)
            .checkout_match(destination, configured_url)?
        {
            git::ManagedCheckout::RequiresVerifiedAlias => git::ManagedCheckout::Mismatch,
            matched => matched,
        },
    )
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
    let identity = git::select_verified_rot_identity(available.clone(), context)?;
    let push_url = git::github_ssh_push_url(canonical_url, &identity.alias)
        .context("could not derive the GitHub SSH push URL")?;
    let repositories = git::ManagedRepositoryMatcher::from_verified_rot_identities(&available);
    if git::push_url(destination)?.is_some()
        || !repositories.is_managed_checkout(destination, canonical_url)?
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
    if !git::ManagedRepositoryMatcher::canonical()
        .is_managed_checkout(destination, canonical_url)?
    {
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
    let mut repositories = ManagedToolRepositories::new();
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
    context.record(Notice::ToolOperationStage {
        operation: ToolOperation::Update,
        stage: ToolOperationStage::ValidatingCheckout,
        name: tool.name.clone(),
        catalog_name: tool.catalog.clone(),
    });
    if !path_exists(&destination) {
        bail!("tool '{name}' is not installed; run 'loadbot pull {name}' first");
    }
    if !git::is_repository(&destination)? {
        bail!("destination exists but is not a Git repository");
    }
    if !repositories.is_managed_checkout(&destination, &tool.definition.url)? {
        bail!("destination is not the configured Git repository");
    }

    context.record(Notice::ToolOperationStage {
        operation: ToolOperation::Update,
        stage: ToolOperationStage::FetchingAndUpdating,
        name: tool.name.clone(),
        catalog_name: tool.catalog.clone(),
    });
    let (old_commit, new_commit) = git::update(
        &destination,
        &tool.definition.url,
        tool.definition.revision.as_deref(),
        context,
    )
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

/// Push local commits to the configured remote for an installed managed tool.
/// The repository must be a valid managed checkout with a clean working tree.
/// Uses the configured push URL if present, otherwise falls back to the origin URL.
pub fn tool_push(
    paths: &Paths,
    name: &str,
    catalog_name: Option<&str>,
    context: &mut OperationContext<'_>,
) -> Result<MutationOutcome> {
    let _process_scope = crate::process::scope(&context.process);
    let mut repositories = ManagedToolRepositories::new();
    context.process.cancellation.check()?;
    let notice_start = context.notices.len();
    let tool = resolve_tool(paths, name, catalog_name, context)?;
    let destination = paths.tool(&tool.catalog, &tool.name)?;
    let _repository_lease = context.lease(&destination)?;
    let _configuration_snapshot = context.watch(&paths.config())?;
    let _catalog_snapshot = context.watch(&paths.catalog_file(&tool.catalog))?;
    context.record(Notice::ToolOperationStage {
        operation: ToolOperation::Push,
        stage: ToolOperationStage::ValidatingCheckout,
        name: tool.name.clone(),
        catalog_name: tool.catalog.clone(),
    });
    validate_managed_push_checkout(paths, &tool, &destination, context, &mut repositories)?;
    if git::status(&destination)?.dirty {
        bail!("working tree has local changes; commit or discard them explicitly before retrying");
    }
    context.record(Notice::ToolOperationStage {
        operation: ToolOperation::Push,
        stage: ToolOperationStage::PushingCommits,
        name: tool.name.clone(),
        catalog_name: tool.catalog.clone(),
    });
    git::push_origin(&destination, context)
        .with_context(|| format!("could not push tool '{name}'"))?;
    context.record(Notice::ToolPushed {
        name: tool.name,
        catalog_name: tool.catalog,
    });
    Ok(context.outcome_since(notice_start))
}

/// Inspect the shared, managed checkout state used by GUI Push routing.
/// This is read-only; commit selection is revalidated by `tool_commit_and_push`.
pub fn tool_push_inspect(
    paths: &Paths,
    name: &str,
    catalog_name: Option<&str>,
    context: &mut OperationContext<'_>,
) -> Result<ToolPushInspection> {
    let _process_scope = crate::process::scope(&context.process);
    let mut repositories = ManagedToolRepositories::new();
    context.process.cancellation.check()?;
    let tool = resolve_tool(paths, name, catalog_name, context)?;
    let destination = paths.tool(&tool.catalog, &tool.name)?;
    let _repository_lease = context.lease(&destination)?;
    let _configuration_snapshot = context.watch(&paths.config())?;
    let _catalog_snapshot = context.watch(&paths.catalog_file(&tool.catalog))?;
    context.record(Notice::ToolOperationStage {
        operation: ToolOperation::Push,
        stage: ToolOperationStage::ValidatingCheckout,
        name: tool.name.clone(),
        catalog_name: tool.catalog.clone(),
    });
    validate_managed_push_checkout(paths, &tool, &destination, context, &mut repositories)?;
    context.record(Notice::ToolOperationStage {
        operation: ToolOperation::Push,
        stage: ToolOperationStage::InspectingRepository,
        name: tool.name.clone(),
        catalog_name: tool.catalog.clone(),
    });
    let changed_files = git::repository_changes(&destination)?;
    let commits_ahead = git::has_local_commits_not_on_origin(&destination)?;
    if !changed_files.is_empty() {
        context.record(Notice::ToolOperationStage {
            operation: ToolOperation::Push,
            stage: ToolOperationStage::AwaitingCommit,
            name: tool.name,
            catalog_name: tool.catalog,
        });
    }
    Ok(ToolPushInspection {
        changed_files,
        commits_ahead,
    })
}

/// Commit explicitly selected working-tree changes and continue through the
/// normal shared Push transport/authentication path. Unselected changes remain.
pub fn tool_commit_and_push(
    paths: &Paths,
    name: &str,
    catalog_name: Option<&str>,
    selected_paths: &[String],
    message: &str,
    context: &mut OperationContext<'_>,
) -> Result<MutationOutcome> {
    let _process_scope = crate::process::scope(&context.process);
    let mut repositories = ManagedToolRepositories::new();
    context.process.cancellation.check()?;
    if selected_paths.is_empty() {
        bail!("select at least one changed file to commit");
    }
    if message.trim().is_empty() {
        bail!("commit message must not be empty");
    }
    let notice_start = context.notices.len();
    let tool = resolve_tool(paths, name, catalog_name, context)?;
    let destination = paths.tool(&tool.catalog, &tool.name)?;
    let _repository_lease = context.lease(&destination)?;
    let _configuration_snapshot = context.watch(&paths.config())?;
    let _catalog_snapshot = context.watch(&paths.catalog_file(&tool.catalog))?;
    context.record(Notice::ToolOperationStage {
        operation: ToolOperation::Push,
        stage: ToolOperationStage::ValidatingCheckout,
        name: tool.name.clone(),
        catalog_name: tool.catalog.clone(),
    });
    validate_managed_push_checkout(paths, &tool, &destination, context, &mut repositories)?;

    let current_changes = git::repository_changes(&destination)?;
    let mut selected = BTreeSet::new();
    for path in selected_paths {
        if !selected.insert(path.as_str()) {
            bail!("changed path '{path}' was selected more than once");
        }
    }
    let mut stage_paths = Vec::new();
    let mut commit_paths = Vec::new();
    for path in selected {
        let change = current_changes
            .iter()
            .find(|change| change.path == path)
            .with_context(|| {
                format!("selected path '{path}' is no longer in the working-tree change set")
            })?;
        // A porcelain rename is already represented in Git's index. Re-adding
        // its missing source path fails, but `commit --only` needs both names
        // to include the deletion and destination in the selected commit.
        stage_paths.push(change.path.clone());
        if let Some(original) = &change.original_path {
            commit_paths.push(original.clone());
        }
        commit_paths.push(change.path.clone());
    }

    context.record(Notice::ToolOperationStage {
        operation: ToolOperation::Push,
        stage: ToolOperationStage::StagingChanges,
        name: tool.name.clone(),
        catalog_name: tool.catalog.clone(),
    });
    context.record(Notice::ToolOperationStage {
        operation: ToolOperation::Push,
        stage: ToolOperationStage::CreatingCommit,
        name: tool.name.clone(),
        catalog_name: tool.catalog.clone(),
    });
    let commit_hash = git::commit_paths_with_interaction(
        &destination,
        &stage_paths,
        &commit_paths,
        message,
        context,
    )
    .with_context(|| format!("could not commit selected changes for tool '{name}'"))?;
    context.record(Notice::ToolCommitted {
        name: tool.name.clone(),
        catalog_name: tool.catalog.clone(),
        commit_hash: commit_hash.clone(),
    });

    context.process.cancellation.check()?;
    validate_managed_push_checkout(paths, &tool, &destination, context, &mut repositories)?;
    context.record(Notice::ToolOperationStage {
        operation: ToolOperation::Push,
        stage: ToolOperationStage::PushingCommits,
        name: tool.name.clone(),
        catalog_name: tool.catalog.clone(),
    });
    git::push_origin(&destination, context).with_context(|| {
        format!("commit {commit_hash} remains local, but pushing tool '{name}' failed")
    })?;
    context.record(Notice::ToolPushed {
        name: tool.name,
        catalog_name: tool.catalog,
    });
    Ok(context.outcome_since(notice_start))
}

/// Remove an installed managed checkout while retaining its catalog entry.
/// Both uncommitted changes and commits not present on `origin` fail closed.
pub fn tool_remove(
    paths: &Paths,
    name: &str,
    catalog_name: Option<&str>,
    context: &mut OperationContext<'_>,
) -> Result<MutationOutcome> {
    let _process_scope = crate::process::scope(&context.process);
    let mut repositories = ManagedToolRepositories::new();
    context.process.cancellation.check()?;
    let notice_start = context.notices.len();
    let tool = resolve_tool(paths, name, catalog_name, context)?;
    let destination = paths.tool(&tool.catalog, &tool.name)?;
    let _repository_lease = context.lease(&destination)?;
    let _configuration_snapshot = context.watch(&paths.config())?;
    let _catalog_snapshot = context.watch(&paths.catalog_file(&tool.catalog))?;
    context.record(Notice::ToolOperationStage {
        operation: ToolOperation::Remove,
        stage: ToolOperationStage::ValidatingCheckout,
        name: tool.name.clone(),
        catalog_name: tool.catalog.clone(),
    });
    validate_destructive_checkout(paths, &tool, &destination, context, &mut repositories)?;
    context.record(Notice::ToolOperationStage {
        operation: ToolOperation::Remove,
        stage: ToolOperationStage::RemovingCheckout,
        name: tool.name.clone(),
        catalog_name: tool.catalog.clone(),
    });
    let _completed_step = crate::process::critical_scope();
    fs::remove_dir_all(&destination).with_context(|| {
        format!(
            "could not remove managed checkout {}",
            destination.display()
        )
    })?;
    context.record(Notice::ToolRemoved {
        name: tool.name,
        catalog_name: tool.catalog,
    });
    Ok(context.outcome_since(notice_start))
}

/// Replace an installed managed checkout with a freshly cloned checkout.
/// The fresh clone is validated before the existing clean checkout is moved,
/// and a failed swap restores the original checkout.
pub fn tool_reinstall(
    paths: &Paths,
    name: &str,
    catalog_name: Option<&str>,
    context: &mut OperationContext<'_>,
) -> Result<MutationOutcome> {
    let _process_scope = crate::process::scope(&context.process);
    let mut repositories = ManagedToolRepositories::new();
    context.process.cancellation.check()?;
    let notice_start = context.notices.len();
    let tool = resolve_tool(paths, name, catalog_name, context)?;
    let destination = paths.tool(&tool.catalog, &tool.name)?;
    let _repository_lease = context.lease(&destination)?;
    let _configuration_snapshot = context.watch(&paths.config())?;
    let _catalog_snapshot = context.watch(&paths.catalog_file(&tool.catalog))?;
    context.record(Notice::ToolOperationStage {
        operation: ToolOperation::Reinstall,
        stage: ToolOperationStage::ValidatingCheckout,
        name: tool.name.clone(),
        catalog_name: tool.catalog.clone(),
    });
    validate_destructive_checkout(paths, &tool, &destination, context, &mut repositories)?;

    let parent = destination
        .parent()
        .context("tool destination has no parent directory")?;
    let fresh = tempfile::Builder::new()
        .prefix(".loadbot-fresh-")
        .tempdir_in(parent)?;
    context.record(Notice::ToolOperationStage {
        operation: ToolOperation::Reinstall,
        stage: ToolOperationStage::CloningProject,
        name: tool.name.clone(),
        catalog_name: tool.catalog.clone(),
    });
    git::clone_repository(
        &tool.definition.url,
        tool.definition.revision.as_deref(),
        fresh.path(),
        context,
    )
    .with_context(|| format!("could not create a fresh checkout for '{name}'"))?;
    context.record(Notice::ToolOperationStage {
        operation: ToolOperation::Reinstall,
        stage: ToolOperationStage::ValidatingFreshCheckout,
        name: tool.name.clone(),
        catalog_name: tool.catalog.clone(),
    });
    if !repositories.is_managed_checkout(fresh.path(), &tool.definition.url)? {
        bail!("fresh checkout is not the configured Git repository");
    }

    // Network work may have taken time. Recheck every destructive precondition.
    validate_destructive_checkout(paths, &tool, &destination, context, &mut repositories)?;
    let backup = tempfile::Builder::new()
        .prefix(".loadbot-backup-")
        .tempdir_in(parent)?;
    let backup_path = backup.keep();
    fs::remove_dir(&backup_path)?;
    let fresh_path = fresh.keep();
    context.record(Notice::ToolOperationStage {
        operation: ToolOperation::Reinstall,
        stage: ToolOperationStage::ReplacingCheckout,
        name: tool.name.clone(),
        catalog_name: tool.catalog.clone(),
    });
    let _completed_step = crate::process::critical_scope();
    fs::rename(&destination, &backup_path).with_context(|| {
        format!(
            "could not prepare {} for replacement",
            destination.display()
        )
    })?;
    if let Err(error) = fs::rename(&fresh_path, &destination) {
        fs::rename(&backup_path, &destination).with_context(|| {
            format!("fresh checkout swap failed ({error}); could not restore the original checkout")
        })?;
        return Err(error)
            .context("could not install the fresh checkout; original checkout restored");
    }
    fs::remove_dir_all(&backup_path).with_context(|| {
        format!(
            "fresh checkout installed, but old checkout cleanup failed at {}",
            backup_path.display()
        )
    })?;
    context.record(Notice::ToolReinstalled {
        name: tool.name,
        catalog_name: tool.catalog,
    });
    Ok(context.outcome_since(notice_start))
}

fn validate_destructive_checkout(
    paths: &Paths,
    tool: &ResolvedTool,
    destination: &Path,
    context: &mut OperationContext<'_>,
    repositories: &mut ManagedToolRepositories,
) -> Result<()> {
    context.process.cancellation.check()?;
    let current = resolve_tool(paths, &tool.name, Some(&tool.catalog), context)?;
    if current.definition != tool.definition {
        return Err(crate::persistence::Busy {
            resource: destination.to_owned(),
        }
        .into());
    }
    let metadata = fs::symlink_metadata(destination).with_context(|| {
        format!(
            "tool '{}' from catalog '{}' is not installed",
            tool.name, tool.catalog
        )
    })?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() || !git::is_repository(destination)?
    {
        bail!("refusing to remove a destination that is not a managed Git checkout");
    }
    if !repositories.is_managed_checkout(destination, &tool.definition.url)? {
        bail!("refusing to remove a checkout that is not the configured Git repository");
    }
    if git::status(destination)?.dirty {
        bail!(
            "working tree has local changes; preserve or discard them explicitly before retrying"
        );
    }
    if git::has_local_commits_not_on_origin(destination)? {
        bail!(
            "repository has local commits not present on origin; push or preserve them before retrying"
        );
    }
    Ok(())
}

/// Validate repository identity and installation for all push variants.
/// Callers separately decide whether a dirty tree is valid for their workflow.
fn validate_managed_push_checkout(
    paths: &Paths,
    tool: &ResolvedTool,
    destination: &Path,
    context: &mut OperationContext<'_>,
    repositories: &mut ManagedToolRepositories,
) -> Result<()> {
    context.process.cancellation.check()?;
    let current = resolve_tool(paths, &tool.name, Some(&tool.catalog), context)?;
    if current.definition != tool.definition {
        return Err(crate::persistence::Busy {
            resource: destination.to_owned(),
        }
        .into());
    }
    let metadata = fs::symlink_metadata(destination).with_context(|| {
        format!(
            "tool '{}' from catalog '{}' is not installed",
            tool.name, tool.catalog
        )
    })?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() || !git::is_repository(destination)?
    {
        bail!("refusing to push a destination that is not a managed Git checkout");
    }
    if !repositories.is_managed_checkout(destination, &tool.definition.url)? {
        bail!("refusing to push a checkout that is not the configured Git repository");
    }
    Ok(())
}

pub fn tool_status(
    paths: &Paths,
    name: &str,
    catalog_name: Option<&str>,
    context: &mut OperationContext<'_>,
) -> Result<ToolStatus> {
    let _process_scope = crate::process::scope(&context.process);
    let mut repositories = ManagedToolRepositories::new();
    tool_status_with_repositories(paths, name, catalog_name, context, &mut repositories)
}

fn tool_status_with_repositories(
    paths: &Paths,
    name: &str,
    catalog_name: Option<&str>,
    context: &mut OperationContext<'_>,
    repositories: &mut ManagedToolRepositories,
) -> Result<ToolStatus> {
    let _process_scope = crate::process::scope(&context.process);
    context.process.cancellation.check()?;
    let tool = resolve_tool(paths, name, catalog_name, context)?;
    let destination = paths.tool(&tool.catalog, &tool.name)?;
    context.record(Notice::ToolResolved {
        tool: tool.clone(),
        path: destination.clone(),
    });
    let installed = repositories.is_managed_checkout(&destination, &tool.definition.url)?;
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
    let mut repositories = ManagedToolRepositories::new();
    installed_tool_path_with_repositories(paths, name, catalog_name, context, &mut repositories)
}

/// Construct an interactive shell for an authoritative managed checkout.
/// The caller receives a typed backend command, never a path or argv supplied
/// by a presentation adapter.
pub fn project_terminal_command(
    paths: &Paths,
    name: &str,
    catalog_name: &str,
    context: &mut OperationContext<'_>,
) -> Result<crate::process::InteractiveCommand> {
    let _process_scope = crate::process::scope(&context.process);
    context.process.cancellation.check()?;
    let destination = paths.tool(catalog_name, name)?;
    let metadata = fs::symlink_metadata(&destination)
        .with_context(|| format!("tool '{name}' from catalog '{catalog_name}' is not installed"))?;
    if metadata.file_type().is_symlink() {
        bail!("refusing to open a terminal for a symlinked tool destination");
    }
    if !metadata.is_dir() {
        bail!("installed tool destination is not a directory");
    }
    let directory = installed_tool_path(paths, name, catalog_name, context)?;
    let directory = fs::canonicalize(&directory).with_context(|| {
        format!(
            "could not resolve project directory {}",
            directory.display()
        )
    })?;
    crate::process::InteractiveCommand::user_shell_in(directory)
}

fn installed_tool_path_with_repositories(
    paths: &Paths,
    name: &str,
    catalog_name: &str,
    context: &mut OperationContext<'_>,
    repositories: &mut ManagedToolRepositories,
) -> Result<PathBuf> {
    let _process_scope = crate::process::scope(&context.process);
    context.process.cancellation.check()?;
    let tool = resolve_tool(paths, name, Some(catalog_name), context)?;
    let destination = paths.tool(&tool.catalog, &tool.name)?;
    if !path_exists(&destination) {
        bail!("tool '{name}' from catalog '{catalog_name}' is not installed");
    }
    if !repositories.is_managed_checkout(&destination, &tool.definition.url)? {
        bail!("installed tool destination is not the configured Git repository");
    }
    Ok(destination)
}

pub fn installed_tools(
    paths: &Paths,
    context: &mut OperationContext<'_>,
) -> Result<Vec<ResolvedTool>> {
    let _process_scope = crate::process::scope(&context.process);
    let mut repositories = ManagedToolRepositories::new();
    context.process.cancellation.check()?;
    let mut installed = Vec::new();
    for tool in all_tools(paths, context)? {
        context.process.cancellation.check()?;
        let destination = paths.tool(&tool.catalog, &tool.name)?;
        if repositories.is_managed_checkout(&destination, &tool.definition.url)? {
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
    if !git::ManagedRepositoryMatcher::canonical().is_managed_checkout(&destination, &source.url)? {
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
    use std::sync::{Arc, Mutex, mpsc};
    use std::time::Duration;

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

    fn query_git(directory: &Path, arguments: &[&str]) -> String {
        let output = Command::new("git")
            .args(arguments)
            .current_dir(directory)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "Git failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap()
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

    fn lifecycle_fixture() -> (TempDir, Paths, PathBuf) {
        let temporary = TempDir::new().unwrap();
        let paths = Paths::with_root(temporary.path().join("loadbot"));
        let catalog_remote = valid_catalog_remote(temporary.path(), "lifecycle-catalog");
        let tool_remote = populated_remote(temporary.path(), "lifecycle-tool");
        catalog_add(
            &paths,
            "personal",
            catalog_remote.display().to_string(),
            false,
            &mut OperationContext::new(&mut crate::interaction::Unattended),
        )
        .unwrap();
        let mut file = catalog::load(&paths.catalog_file("personal")).unwrap();
        file.tools.insert(
            "demo".to_owned(),
            ToolConfig::git(tool_remote.display().to_string(), None),
        );
        catalog::save(&paths.catalog_file("personal"), &file).unwrap();
        tool_pull(
            &paths,
            "demo",
            Some("personal"),
            &mut OperationContext::new(&mut crate::interaction::Unattended),
        )
        .unwrap();
        let destination = paths.tool("personal", "demo").unwrap();
        (temporary, paths, destination)
    }

    #[test]
    fn tool_push_allows_local_commits_ahead_of_origin() {
        let (_temporary, paths, destination) = lifecycle_fixture();
        git(["config", "user.name", "Loadbot Tests"], Some(&destination));
        git(
            ["config", "user.email", "loadbot@example.test"],
            Some(&destination),
        );
        fs::write(destination.join("local.txt"), "local commit\n").unwrap();
        git(["add", "local.txt"], Some(&destination));
        git(["commit", "-m", "local work"], Some(&destination));
        // Push should succeed even with local commits not on origin
        let mut policy = crate::interaction::Unattended;
        let mut context = OperationContext::background(&mut policy);
        let result = tool_push(&paths, "demo", Some("personal"), &mut context);
        assert!(
            result.is_ok(),
            "push should succeed with local commits: {:?}",
            result.err()
        );
        assert!(destination.join("local.txt").exists());
        assert!(!git::has_local_commits_not_on_origin(&destination).unwrap());
    }

    #[test]
    fn interactive_push_keeps_the_shared_operation_pending_until_success() {
        let (temporary, paths, destination) = lifecycle_fixture();
        git(["config", "user.name", "Loadbot Tests"], Some(&destination));
        git(
            ["config", "user.email", "loadbot@example.test"],
            Some(&destination),
        );
        fs::write(destination.join("local.txt"), "local commit\n").unwrap();
        git(["add", "local.txt"], Some(&destination));
        git(["commit", "-m", "local work"], Some(&destination));
        let missing = temporary.path().join("missing-remote.git");
        git(
            [
                OsStr::new("remote"),
                OsStr::new("set-url"),
                OsStr::new("--push"),
                OsStr::new("origin"),
                missing.as_os_str(),
            ],
            Some(&destination),
        );

        let (started_sender, started_receiver) = mpsc::channel();
        let (finish_sender, finish_receiver) = mpsc::channel();
        let finish_receiver = Arc::new(Mutex::new(finish_receiver));
        let worker = std::thread::spawn(move || {
            let mut policy = crate::interaction::Unattended;
            let mut context = OperationContext::background(&mut policy);
            context.process.interactive_executor = Some(Arc::new(move |command, _| {
                assert_eq!(command.program(), OsStr::new("git"));
                assert_eq!(
                    command.arguments().collect::<Vec<_>>(),
                    [
                        OsStr::new("-C"),
                        destination.as_os_str(),
                        OsStr::new("push"),
                        OsStr::new("origin"),
                        OsStr::new("HEAD")
                    ]
                );
                started_sender.send(()).unwrap();
                finish_receiver.lock().unwrap().recv().unwrap();
                Ok(crate::process::InteractiveExecutionOutput {
                    status: crate::process::InteractiveExitStatus {
                        code: 0,
                        signal: None,
                    },
                    output: b"push completed".to_vec(),
                    cancelled: false,
                })
            }));
            tool_push(&paths, "demo", Some("personal"), &mut context)
        });

        started_receiver
            .recv_timeout(Duration::from_secs(5))
            .unwrap();
        assert!(
            !worker.is_finished(),
            "push completed before the PTY exited"
        );
        finish_sender.send(()).unwrap();
        let outcome = worker.join().unwrap().unwrap();
        assert!(outcome.notices.iter().any(|notice| matches!(
            notice,
            Notice::ToolPushed { name, .. } if name == "demo"
        )));
    }

    #[test]
    fn interactive_push_nonzero_and_cancellation_fail_the_shared_operation() {
        for (cancelled, expected) in [(false, "interactive push rejected"), (true, "cancelled")] {
            let (temporary, paths, destination) = lifecycle_fixture();
            let missing = temporary.path().join("missing-remote.git");
            git(
                [
                    OsStr::new("remote"),
                    OsStr::new("set-url"),
                    OsStr::new("--push"),
                    OsStr::new("origin"),
                    missing.as_os_str(),
                ],
                Some(&destination),
            );
            let mut policy = crate::interaction::Unattended;
            let mut context = OperationContext::background(&mut policy);
            context.process.interactive_executor = Some(Arc::new(move |_, _| {
                Ok(crate::process::InteractiveExecutionOutput {
                    status: crate::process::InteractiveExitStatus {
                        code: 23,
                        signal: None,
                    },
                    output: b"interactive push rejected".to_vec(),
                    cancelled,
                })
            }));

            let error = tool_push(&paths, "demo", Some("personal"), &mut context).unwrap_err();
            assert!(format!("{error:#}").to_ascii_lowercase().contains(expected));
            if cancelled {
                assert!(error.downcast_ref::<crate::process::Cancelled>().is_some());
            }
        }
    }

    #[test]
    fn push_honors_a_rot_alias_push_url_without_requiring_rot_at_runtime() {
        let (temporary, paths, destination) = lifecycle_fixture();
        let alternate = empty_remote(temporary.path(), "rot-target");
        let alias_url = "git@github-kamaji:owner/rot-target.git";
        let rewrite_key = format!("url.{}/.insteadOf", temporary.path().display());
        git(
            ["config", rewrite_key.as_str(), "git@github-kamaji:owner/"],
            Some(&destination),
        );
        git(
            [
                OsStr::new("remote"),
                OsStr::new("set-url"),
                OsStr::new("--push"),
                OsStr::new("origin"),
                OsStr::new(alias_url),
            ],
            Some(&destination),
        );
        assert_eq!(
            git::push_url(&destination).unwrap().as_deref(),
            Some(alias_url)
        );
        git(["config", "user.name", "Loadbot Tests"], Some(&destination));
        git(
            ["config", "user.email", "loadbot@example.test"],
            Some(&destination),
        );
        fs::write(destination.join("push-url.txt"), "alternate\n").unwrap();
        git(["add", "push-url.txt"], Some(&destination));
        git(
            ["commit", "-m", "push to configured URL"],
            Some(&destination),
        );

        let mut policy = crate::interaction::Unattended;
        let mut context = OperationContext::background(&mut policy);
        tool_push(&paths, "demo", Some("personal"), &mut context).unwrap();

        let output = Command::new("git")
            .args(["--git-dir"])
            .arg(&alternate)
            .args(["show", "HEAD:push-url.txt"])
            .output()
            .unwrap();
        assert!(output.status.success());
        assert_eq!(output.stdout, b"alternate\n");
    }

    #[test]
    fn tool_push_rejects_dirty_worktree() {
        let (_temporary, paths, destination) = lifecycle_fixture();
        fs::write(destination.join("dirty.txt"), "dirty\n").unwrap();
        let mut policy = crate::interaction::Unattended;
        let mut context = OperationContext::background(&mut policy);
        let error = tool_push(&paths, "demo", Some("personal"), &mut context).unwrap_err();
        assert!(error.to_string().contains("working tree has local changes"));
    }

    #[test]
    fn tool_push_rejects_invalid_repository() {
        let temporary = TempDir::new().unwrap();
        let paths = Paths::with_root(temporary.path().join("loadbot"));
        let mut policy = crate::interaction::Unattended;
        let mut context = OperationContext::background(&mut policy);
        let error = tool_push(&paths, "nonexistent", Some("personal"), &mut context).unwrap_err();
        // When catalog doesn't exist, we get a catalog error
        assert!(
            error.to_string().contains("catalog") && error.to_string().contains("not configured")
        );
    }

    #[test]
    fn push_inspection_reports_structured_modified_untracked_and_deleted_files() {
        let (_temporary, paths, destination) = lifecycle_fixture();
        fs::write(destination.join("README.md"), "modified\n").unwrap();
        fs::write(destination.join("new file.txt"), "untracked\n").unwrap();
        fs::write(destination.join("deleted.txt"), "tracked\n").unwrap();
        fs::write(destination.join("rename-me.txt"), "tracked\n").unwrap();
        git(["add", "deleted.txt", "rename-me.txt"], Some(&destination));
        git(["config", "user.name", "Loadbot Tests"], Some(&destination));
        git(
            ["config", "user.email", "loadbot@example.test"],
            Some(&destination),
        );
        git(
            ["commit", "-m", "track deleted fixture"],
            Some(&destination),
        );
        git(["push", "origin", "HEAD"], Some(&destination));
        fs::remove_file(destination.join("deleted.txt")).unwrap();
        git(
            ["mv", "rename-me.txt", "renamed file.txt"],
            Some(&destination),
        );

        let mut policy = crate::interaction::Unattended;
        let mut context = OperationContext::background(&mut policy);
        let inspection = tool_push_inspect(&paths, "demo", Some("personal"), &mut context).unwrap();
        assert!(!inspection.commits_ahead);
        assert!(inspection.changed_files.contains(&git::RepositoryChange {
            path: "README.md".into(),
            original_path: None,
            status: git::RepositoryChangeKind::Modified,
        }));
        assert!(inspection.changed_files.contains(&git::RepositoryChange {
            path: "new file.txt".into(),
            original_path: None,
            status: git::RepositoryChangeKind::Added,
        }));
        assert!(inspection.changed_files.contains(&git::RepositoryChange {
            path: "deleted.txt".into(),
            original_path: None,
            status: git::RepositoryChangeKind::Deleted,
        }));
        assert!(inspection.changed_files.contains(&git::RepositoryChange {
            path: "renamed file.txt".into(),
            original_path: Some("rename-me.txt".into()),
            status: git::RepositoryChangeKind::Renamed,
        }));
    }

    #[test]
    fn commit_and_push_commits_only_selected_paths_with_literal_message() {
        let (temporary, paths, destination) = lifecycle_fixture();
        git(["config", "user.name", "Loadbot Tests"], Some(&destination));
        git(
            ["config", "user.email", "loadbot@example.test"],
            Some(&destination),
        );
        let selected = ":(glob)* chosen file.txt";
        fs::write(destination.join(selected), "selected\n").unwrap();
        fs::remove_file(destination.join("README.md")).unwrap();
        fs::write(destination.join("left staged.txt"), "leave me\n").unwrap();
        git(["add", "left staged.txt"], Some(&destination));
        let injected = temporary.path().join("message-was-shell");
        let message = format!("literal $(touch {}) ; commit", injected.display());

        let mut policy = crate::interaction::Unattended;
        let mut context = OperationContext::background(&mut policy);
        let events = Arc::new(Mutex::new(Vec::new()));
        let observed_events = events.clone();
        context.process.observer = Some(Arc::new(move |event| {
            observed_events.lock().unwrap().push(event);
        }));
        tool_commit_and_push(
            &paths,
            "demo",
            Some("personal"),
            &[selected.to_owned(), "README.md".into()],
            &message,
            &mut context,
        )
        .unwrap();

        assert_eq!(
            Command::new("git")
                .args([
                    "-C",
                    destination.to_str().unwrap(),
                    "show",
                    "-s",
                    "--format=%s",
                    "HEAD"
                ])
                .output()
                .map(|output| String::from_utf8(output.stdout).unwrap().trim().to_owned())
                .unwrap(),
            message
        );
        assert!(!injected.exists());
        assert!(!format!("{:?}", events.lock().unwrap()).contains(&message));
        assert!(!destination.join("README.md").exists());
        assert!(
            git::working_tree_changes(&destination)
                .unwrap()
                .contains("left staged.txt")
        );
        assert!(
            query_git(&destination, &["diff", "--cached", "--name-only"])
                .lines()
                .any(|path| path == "left staged.txt")
        );
        assert!(!git::has_local_commits_not_on_origin(&destination).unwrap());
    }

    #[test]
    fn commit_and_push_rejects_empty_or_stale_selection_and_empty_message() {
        let (_temporary, paths, destination) = lifecycle_fixture();
        fs::write(destination.join("dirty.txt"), "dirty\n").unwrap();
        for (selected, message, expected) in [
            (Vec::<String>::new(), "message", "select at least one"),
            (vec!["dirty.txt".into()], "   ", "message must not be empty"),
            (vec!["not-dirty.txt".into()], "message", "no longer"),
        ] {
            let mut policy = crate::interaction::Unattended;
            let mut context = OperationContext::background(&mut policy);
            let error = tool_commit_and_push(
                &paths,
                "demo",
                Some("personal"),
                &selected,
                message,
                &mut context,
            )
            .unwrap_err();
            assert!(format!("{error:#}").contains(expected), "{error:#}");
        }
        assert!(git::head_commit(&destination).unwrap().is_some());
        assert!(destination.join("dirty.txt").exists());
    }

    #[test]
    fn commit_and_push_expands_a_selected_rename_to_both_paths() {
        let (_temporary, paths, destination) = lifecycle_fixture();
        git(["config", "user.name", "Loadbot Tests"], Some(&destination));
        git(
            ["config", "user.email", "loadbot@example.test"],
            Some(&destination),
        );
        git(["mv", "README.md", "renamed readme.md"], Some(&destination));
        let mut policy = crate::interaction::Unattended;
        let mut context = OperationContext::background(&mut policy);
        tool_commit_and_push(
            &paths,
            "demo",
            Some("personal"),
            &["renamed readme.md".into()],
            "rename readme",
            &mut context,
        )
        .unwrap();
        assert!(!git::status(&destination).unwrap().dirty);
        assert_eq!(
            query_git(&destination, &["show", "HEAD:renamed readme.md"]),
            "existing data\n"
        );
        let old = Command::new("git")
            .args(["show", "HEAD:README.md"])
            .current_dir(&destination)
            .output()
            .unwrap();
        assert!(!old.status.success());
    }

    #[test]
    fn push_inspection_detects_nothing_to_push_and_clean_commits_ahead() {
        let (_temporary, paths, destination) = lifecycle_fixture();
        let mut policy = crate::interaction::Unattended;
        let mut context = OperationContext::background(&mut policy);
        let current = tool_push_inspect(&paths, "demo", Some("personal"), &mut context).unwrap();
        assert!(current.changed_files.is_empty());
        assert!(!current.commits_ahead);

        git(["config", "user.name", "Loadbot Tests"], Some(&destination));
        git(
            ["config", "user.email", "loadbot@example.test"],
            Some(&destination),
        );
        fs::write(destination.join("ahead.txt"), "ahead\n").unwrap();
        git(["add", "ahead.txt"], Some(&destination));
        git(["commit", "-m", "ahead"], Some(&destination));
        let mut policy = crate::interaction::Unattended;
        let mut context = OperationContext::background(&mut policy);
        let ahead = tool_push_inspect(&paths, "demo", Some("personal"), &mut context).unwrap();
        assert!(ahead.changed_files.is_empty());
        assert!(ahead.commits_ahead);
    }

    #[test]
    fn commit_remains_local_when_the_following_push_fails() {
        let (temporary, paths, destination) = lifecycle_fixture();
        git(["config", "user.name", "Loadbot Tests"], Some(&destination));
        git(
            ["config", "user.email", "loadbot@example.test"],
            Some(&destination),
        );
        fs::write(destination.join("committed.txt"), "keep commit\n").unwrap();
        let before = git::head_commit(&destination).unwrap();
        let missing = temporary.path().join("missing.git");
        git(
            [
                OsStr::new("remote"),
                OsStr::new("set-url"),
                OsStr::new("--push"),
                OsStr::new("origin"),
                missing.as_os_str(),
            ],
            Some(&destination),
        );
        let mut policy = crate::interaction::Unattended;
        let mut context = OperationContext::background(&mut policy);
        let error = tool_commit_and_push(
            &paths,
            "demo",
            Some("personal"),
            &["committed.txt".into()],
            "commit before failed push",
            &mut context,
        )
        .unwrap_err();
        assert!(format!("{error:#}").contains("remains local"));
        assert_ne!(git::head_commit(&destination).unwrap(), before);
        assert!(git::has_local_commits_not_on_origin(&destination).unwrap());
        assert!(!git::status(&destination).unwrap().dirty);
    }

    #[test]
    fn commit_and_push_revalidates_managed_repository_identity() {
        let (temporary, paths, destination) = lifecycle_fixture();
        let wrong = populated_remote(temporary.path(), "wrong-push-tool");
        git(
            [
                OsStr::new("remote"),
                OsStr::new("set-url"),
                OsStr::new("origin"),
                wrong.as_os_str(),
            ],
            Some(&destination),
        );
        fs::write(destination.join("dirty.txt"), "dirty\n").unwrap();
        let mut policy = crate::interaction::Unattended;
        let mut context = OperationContext::background(&mut policy);
        let error = tool_commit_and_push(
            &paths,
            "demo",
            Some("personal"),
            &["dirty.txt".into()],
            "must not commit",
            &mut context,
        )
        .unwrap_err();
        assert!(format!("{error:#}").contains("not the configured Git repository"));
        assert!(git::status(&destination).unwrap().dirty);
    }

    #[test]
    fn commit_and_push_uses_the_existing_interactive_push_executor() {
        let (temporary, paths, destination) = lifecycle_fixture();
        git(["config", "user.name", "Loadbot Tests"], Some(&destination));
        git(
            ["config", "user.email", "loadbot@example.test"],
            Some(&destination),
        );
        fs::write(destination.join("interactive.txt"), "commit first\n").unwrap();
        let missing = temporary.path().join("interactive-missing.git");
        git(
            [
                OsStr::new("remote"),
                OsStr::new("set-url"),
                OsStr::new("--push"),
                OsStr::new("origin"),
                missing.as_os_str(),
            ],
            Some(&destination),
        );
        let invoked = Arc::new(Mutex::new(false));
        let observed = invoked.clone();
        let mut policy = crate::interaction::Unattended;
        let mut context = OperationContext::background(&mut policy);
        context.process.interactive_executor = Some(Arc::new(move |command, _| {
            assert!(command.arguments().any(|argument| argument == "push"));
            *observed.lock().unwrap() = true;
            Ok(crate::process::InteractiveExecutionOutput {
                status: crate::process::InteractiveExitStatus {
                    code: 0,
                    signal: None,
                },
                output: Vec::new(),
                cancelled: false,
            })
        }));
        tool_commit_and_push(
            &paths,
            "demo",
            Some("personal"),
            &["interactive.txt".into()],
            "interactive push",
            &mut context,
        )
        .unwrap();
        assert!(*invoked.lock().unwrap());
    }

    #[test]
    fn project_terminal_command_uses_the_authoritative_managed_directory_and_environment() {
        let (_temporary, paths, destination) = lifecycle_fixture();
        let mut policy = crate::interaction::Unattended;
        let mut context = OperationContext::background(&mut policy);
        let command = project_terminal_command(&paths, "demo", "personal", &mut context).unwrap();
        assert_eq!(
            command.current_directory(),
            Some(fs::canonicalize(destination).unwrap().as_path())
        );
        assert!(!command.program().is_empty());
        #[cfg(unix)]
        assert_eq!(command.arguments().collect::<Vec<_>>(), [OsStr::new("-i")]);
        #[cfg(windows)]
        assert_eq!(command.arguments().count(), 0);
        assert_eq!(
            command.environment("PATH"),
            std::env::var_os("PATH").as_deref()
        );
        assert_eq!(
            command.environment("TERM"),
            Some(OsStr::new("xterm-256color"))
        );
    }

    #[test]
    fn project_terminal_command_rejects_uninstalled_and_foreign_checkouts() {
        let (temporary, paths, destination) = lifecycle_fixture();
        fs::remove_dir_all(&destination).unwrap();
        let mut policy = crate::interaction::Unattended;
        let mut context = OperationContext::background(&mut policy);
        let missing = project_terminal_command(&paths, "demo", "personal", &mut context)
            .err()
            .expect("missing checkout must be rejected");
        assert!(format!("{missing:#}").contains("not installed"));

        let wrong = populated_remote(temporary.path(), "terminal-foreign");
        git(
            [
                "clone",
                wrong.to_str().unwrap(),
                destination.to_str().unwrap(),
            ],
            None,
        );
        let mut context = OperationContext::background(&mut policy);
        let foreign = project_terminal_command(&paths, "demo", "personal", &mut context)
            .err()
            .expect("foreign checkout must be rejected");
        assert!(format!("{foreign:#}").contains("not the configured Git repository"));
    }

    #[cfg(unix)]
    #[test]
    fn project_terminal_command_rejects_a_symlinked_destination() {
        use std::os::unix::fs::symlink;

        let (_temporary, paths, destination) = lifecycle_fixture();
        let real = destination.with_extension("real");
        fs::rename(&destination, &real).unwrap();
        symlink(&real, &destination).unwrap();
        let mut policy = crate::interaction::Unattended;
        let mut context = OperationContext::background(&mut policy);
        let error = project_terminal_command(&paths, "demo", "personal", &mut context)
            .err()
            .expect("symlinked checkout must be rejected");
        assert!(format!("{error:#}").contains("symlinked tool destination"));
    }

    #[cfg(unix)]
    #[test]
    fn managed_project_terminal_streams_input_exit_and_termination_without_logging_stdin() {
        let (_temporary, paths, destination) = lifecycle_fixture();
        let events = Arc::new(Mutex::new(Vec::new()));
        let observed_events = events.clone();
        let control = crate::process::Control {
            observer: Some(Arc::new(move |event| {
                observed_events.lock().unwrap().push(format!("{event:?}"));
            })),
            ..crate::process::Control::default()
        };
        let mut policy = crate::interaction::Unattended;
        let mut context = OperationContext::background(&mut policy);
        let command = project_terminal_command(&paths, "demo", "personal", &mut context).unwrap();
        let (sender, receiver) = mpsc::channel();
        let session = crate::process::InteractiveSession::start(
            command,
            &control,
            crate::process::OperationId::random(),
            Arc::new(move |event| sender.send(event).unwrap()),
        )
        .unwrap();
        session
            .send_input(b"printf 'loadbot-terminal:%s\\n' \"$PWD\"\rexit\r")
            .unwrap();
        let mut output = Vec::new();
        loop {
            match receiver.recv_timeout(Duration::from_secs(5)).unwrap() {
                crate::process::InteractiveSessionEvent::Output { bytes, .. } => {
                    output.extend(bytes)
                }
                crate::process::InteractiveSessionEvent::Exited { status, .. } => {
                    assert!(status.success());
                    break;
                }
                crate::process::InteractiveSessionEvent::Failed { diagnostic, .. } => {
                    panic!("{diagnostic}")
                }
            }
        }
        let output = String::from_utf8_lossy(&output);
        assert!(output.contains("loadbot-terminal:"));
        assert!(output.contains(&fs::canonicalize(destination).unwrap().display().to_string()));
        assert!(
            !events
                .lock()
                .unwrap()
                .join("\n")
                .contains("loadbot-terminal")
        );

        let mut context = OperationContext::background(&mut policy);
        let command = project_terminal_command(&paths, "demo", "personal", &mut context).unwrap();
        let (sender, receiver) = mpsc::channel();
        let session = crate::process::InteractiveSession::start(
            command,
            &crate::process::Control::default(),
            crate::process::OperationId::random(),
            Arc::new(move |event| sender.send(event).unwrap()),
        )
        .unwrap();
        session.terminate().unwrap();
        loop {
            match receiver.recv_timeout(Duration::from_secs(5)).unwrap() {
                crate::process::InteractiveSessionEvent::Exited { cancelled, .. } => {
                    assert!(cancelled);
                    break;
                }
                crate::process::InteractiveSessionEvent::Failed { diagnostic, .. } => {
                    panic!("{diagnostic}")
                }
                crate::process::InteractiveSessionEvent::Output { .. } => {}
            }
        }
    }

    #[test]
    fn destructive_project_operations_protect_dirty_and_local_only_work() {
        let (_temporary, paths, destination) = lifecycle_fixture();
        fs::write(destination.join("dirty.txt"), "preserve\n").unwrap();
        let error = tool_remove(
            &paths,
            "demo",
            Some("personal"),
            &mut OperationContext::new(&mut crate::interaction::Unattended),
        )
        .unwrap_err();
        assert!(error.to_string().contains("working tree has local changes"));
        assert!(destination.join("dirty.txt").exists());

        fs::remove_file(destination.join("dirty.txt")).unwrap();
        git(["config", "user.name", "Loadbot Tests"], Some(&destination));
        git(
            ["config", "user.email", "loadbot@example.test"],
            Some(&destination),
        );
        fs::write(destination.join("local.txt"), "preserve commit\n").unwrap();
        git(["add", "local.txt"], Some(&destination));
        git(["commit", "-m", "local work"], Some(&destination));
        let error = tool_remove(
            &paths,
            "demo",
            Some("personal"),
            &mut OperationContext::new(&mut crate::interaction::Unattended),
        )
        .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("local commits not present on origin")
        );
        assert!(destination.join("local.txt").exists());

        let error = tool_reinstall(
            &paths,
            "demo",
            Some("personal"),
            &mut OperationContext::new(&mut crate::interaction::Unattended),
        )
        .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("local commits not present on origin")
        );
        assert!(destination.join("local.txt").exists());
    }

    #[test]
    fn remove_retains_catalog_entry_and_reinstall_creates_a_fresh_managed_checkout() {
        let (_temporary, paths, destination) = lifecycle_fixture();
        let mut policy = crate::interaction::Unattended;
        let mut context = OperationContext::background(&mut policy);
        let reinstalled = tool_reinstall(&paths, "demo", Some("personal"), &mut context).unwrap();
        assert!(reinstalled.notices.iter().any(|notice| matches!(
            notice,
            Notice::ToolOperationStage {
                operation: ToolOperation::Reinstall,
                stage: ToolOperationStage::CloningProject,
                ..
            }
        )));
        assert!(reinstalled.notices.iter().any(|notice| matches!(
            notice,
            Notice::ToolOperationStage {
                operation: ToolOperation::Reinstall,
                stage: ToolOperationStage::ReplacingCheckout,
                ..
            }
        )));
        assert!(git::is_repository(&destination).unwrap());
        let installed_inventory = crate::launcher::read_project_inventory(
            &paths,
            &mut OperationContext::background(&mut crate::interaction::Unattended),
        )
        .unwrap();
        let demo_project = installed_inventory
            .iter()
            .find(|p| p.tool == "demo" && p.catalog == "personal")
            .expect("demo project not found");
        assert!(demo_project.installed);
        let removed = tool_remove(
            &paths,
            "demo",
            Some("personal"),
            &mut OperationContext::background(&mut crate::interaction::Unattended),
        )
        .unwrap();
        assert!(removed.notices.iter().any(|notice| matches!(
            notice,
            Notice::ToolOperationStage {
                operation: ToolOperation::Remove,
                stage: ToolOperationStage::ValidatingCheckout,
                ..
            }
        )));
        assert!(removed.notices.iter().any(|notice| matches!(
            notice,
            Notice::ToolOperationStage {
                operation: ToolOperation::Remove,
                stage: ToolOperationStage::RemovingCheckout,
                ..
            }
        )));
        assert!(!destination.exists());
        assert!(
            catalog::load(&paths.catalog_file("personal"))
                .unwrap()
                .tools
                .contains_key("demo")
        );
        let listed = tool_list(
            &paths,
            &mut OperationContext::background(&mut crate::interaction::Unattended),
        )
        .unwrap();
        assert_eq!(listed.len(), 1);
        assert!(!listed[0].installed);
        let available_inventory = crate::launcher::read_project_inventory(
            &paths,
            &mut OperationContext::background(&mut crate::interaction::Unattended),
        )
        .unwrap();
        let demo_project = available_inventory
            .iter()
            .find(|p| p.tool == "demo" && p.catalog == "personal")
            .expect("demo project not found");
        assert!(!demo_project.installed);
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

    fn verified_alias_fixture() -> (TempDir, Paths, PathBuf, ManagedToolRepositories) {
        let temporary = TempDir::new().unwrap();
        let paths = Paths::with_root(temporary.path().join("loadbot"));
        let remote = valid_catalog_remote(temporary.path(), "verified-alias-catalog");
        catalog_add(
            &paths,
            "personal",
            remote.display().to_string(),
            false,
            &mut OperationContext::new(&mut crate::interaction::Unattended),
        )
        .unwrap();
        fs::write(
            paths.catalog_file("personal"),
            "version = 1\n[tools.demo]\ntype = \"git\"\nurl = \"https://github.com/owner/repo.git\"\n",
        )
        .unwrap();
        fs::create_dir_all(paths.tools().join("personal")).unwrap();
        let checkout = repository_with_origin(
            &paths.tools().join("personal"),
            "git@github-work:owner/repo.git",
        );
        let destination = paths.tool("personal", "demo").unwrap();
        fs::rename(checkout, &destination).unwrap();
        let repositories =
            ManagedToolRepositories::with_verified_identities(&[identity("github-work", "owner")]);
        (temporary, paths, destination, repositories)
    }

    #[test]
    fn list_status_and_installed_path_accept_a_verified_rot_alias_checkout() {
        let (_temporary, paths, destination, mut repositories) = verified_alias_fixture();

        let listed = tool_list_with_repositories(
            &paths,
            &mut OperationContext::background(&mut crate::interaction::Unattended),
            &mut repositories,
        )
        .unwrap();
        assert_eq!(listed.len(), 1);
        assert!(listed[0].installed);

        let status = tool_status_with_repositories(
            &paths,
            "demo",
            Some("personal"),
            &mut OperationContext::background(&mut crate::interaction::Unattended),
            &mut repositories,
        )
        .unwrap();
        assert!(status.installed);

        let installed = installed_tool_path_with_repositories(
            &paths,
            "demo",
            "personal",
            &mut OperationContext::background(&mut crate::interaction::Unattended),
            &mut repositories,
        )
        .unwrap();
        assert_eq!(installed, destination);
    }

    #[test]
    fn pull_recognizes_a_verified_rot_alias_checkout_before_transport_reconciliation() {
        let (_temporary, paths, destination, _repositories) = verified_alias_fixture();
        let mut prompt = TestPrompt::default();
        let error = tool_pull_with(
            &paths,
            "demo",
            Some("personal"),
            false,
            &mut OperationContext::background(&mut prompt),
            || Ok(vec![identity("github-work", "owner")]),
            |_, _, _, _| panic!("a recognized existing checkout must not be cloned"),
        )
        .unwrap_err();

        assert!(error.to_string().contains("different transport"));
        assert!(
            !error
                .to_string()
                .contains("not the configured Git repository")
        );
        assert_eq!(
            git::fetch_url(&destination).unwrap().as_deref(),
            Some("git@github-work:owner/repo.git")
        );
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
    fn launcher_validation_accepts_transport_equivalent_github_checkouts() {
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

        let installed = installed_tool_path(
            &paths,
            "demo",
            "personal",
            &mut OperationContext::new(&mut crate::interaction::Unattended),
        )
        .unwrap();

        assert_eq!(installed, paths.tool("personal", "demo").unwrap());
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
    fn catalog_sync_updates_only_the_requested_isolated_catalog() {
        let temporary = TempDir::new().unwrap();
        let paths = Paths::with_root(temporary.path().join("loadbot"));
        let remote = valid_catalog_remote(temporary.path(), "sync-catalog");
        let mut policy = crate::interaction::Unattended;
        let mut context = OperationContext::new(&mut policy);
        catalog_add(
            &paths,
            "personal",
            remote.display().to_string(),
            true,
            &mut context,
        )
        .unwrap();
        let source = temporary.path().join("sync-catalog-source");
        fs::write(source.join("new.txt"), "new catalog data\n").unwrap();
        git(["add", "new.txt"], Some(&source));
        git(["commit", "-m", "catalog update"], Some(&source));
        git(["push", "origin", "main"], Some(&source));

        let outcome = catalog_sync(&paths, "personal", &mut context).unwrap();
        assert!(paths.catalog("personal").join("new.txt").is_file());
        assert!(
            matches!(outcome.notices.first(), Some(Notice::CatalogSyncStarted { name }) if name == "personal")
        );
        assert!(outcome.notices.iter().any(
            |notice| matches!(notice, Notice::CatalogSyncRepositoryChecked { name } if name == "personal")
        ));
        assert!(outcome.notices.iter().any(
            |notice| matches!(notice, Notice::CatalogSyncUpdateStarted { name } if name == "personal")
        ));
        assert!(outcome.notices.iter().any(
            |notice| matches!(notice, Notice::CatalogSynced { name, .. } if name == "personal")
        ));
        let catalog = catalog_list(&paths, &mut context).unwrap().remove(0);
        assert_eq!(catalog.state, CatalogState::Installed);
        assert!(
            catalog.source.writable,
            "sync must not change management eligibility"
        );
    }

    #[test]
    fn catalog_sync_reads_a_configured_github_ssh_remote_over_https() {
        let temporary = TempDir::new().unwrap();
        let paths = Paths::with_root(temporary.path().join("loadbot"));
        let remote = valid_catalog_remote(temporary.path(), "github-transport-catalog");
        let configured = "git@github.com:0xkamaji/loadbot-catalog.git";
        let read_url = "https://github.com/0xkamaji/loadbot-catalog.git";
        let mut policy = crate::interaction::Unattended;
        let mut context = OperationContext::background(&mut policy);
        catalog_add(
            &paths,
            "personal",
            remote.display().to_string(),
            true,
            &mut context,
        )
        .unwrap();
        config::update(&paths.config(), |local| {
            local.catalogs.get_mut("personal").unwrap().url = configured.to_owned();
            Ok(())
        })
        .unwrap();
        let checkout = paths.catalog("personal");
        git(["remote", "set-url", "origin", configured], Some(&checkout));
        let rewrite = format!("url.{}.insteadOf", remote.display());
        git(
            vec![
                "config".to_owned(),
                "--local".to_owned(),
                rewrite,
                read_url.to_owned(),
            ],
            Some(&checkout),
        );

        let source = temporary.path().join("github-transport-catalog-source");
        fs::write(
            source.join("catalog.toml"),
            "version = 1\n\n[tools.radio-configs]\ntype = \"git\"\nurl = \"https://github.com/0xkamaji/radio-configs.git\"\n",
        )
        .unwrap();
        git(["add", "catalog.toml"], Some(&source));
        git(["commit", "-m", "add radio configs"], Some(&source));
        git(["push", "origin", "main"], Some(&source));

        catalog_sync(&paths, "personal", &mut context).unwrap();

        assert!(
            catalog::load(&paths.catalog_file("personal"))
                .unwrap()
                .tools
                .contains_key("radio-configs")
        );
        assert_eq!(
            git::fetch_url(&checkout).unwrap().as_deref(),
            Some(read_url)
        );
        assert_eq!(
            git::push_url(&checkout).unwrap().as_deref(),
            Some(configured)
        );
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
            &mut OperationContext::background(&mut crate::interaction::Unattended),
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
            &mut OperationContext::background(&mut crate::interaction::Unattended),
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
            &mut OperationContext::background(&mut crate::interaction::Unattended),
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
            &mut OperationContext::background(&mut crate::interaction::Unattended),
        )
        .unwrap();
        assert_eq!(
            git::head_commit(&paths.catalog("personal")).unwrap(),
            before
        );
    }

    #[test]
    fn catalog_initialization_creates_a_registered_valid_catalog() {
        let temporary = TempDir::new().unwrap();
        let paths = Paths::with_root(temporary.path().join("loadbot"));
        let remote = empty_remote(temporary.path(), "created-catalog");
        let url = remote.display().to_string();

        catalog_initialize(
            &paths,
            "created",
            url.clone(),
            true,
            false,
            false,
            &mut OperationContext::background(&mut crate::interaction::Unattended),
        )
        .unwrap();

        assert!(git::is_repository(&paths.catalog("created")).unwrap());
        assert_eq!(
            fs::read_to_string(paths.catalog_file("created")).unwrap(),
            "version = 1\n\n[tools]\n"
        );
        assert_eq!(
            catalog::load(&paths.catalog_file("created")).unwrap(),
            CatalogFile::default()
        );
        let local = config::load(&paths.config()).unwrap();
        assert_eq!(local.default_catalog.as_deref(), Some("created"));
        assert_eq!(
            local.catalogs.get("created"),
            Some(&CatalogSource::new(url, true))
        );
        assert_eq!(
            catalog_list(
                &paths,
                &mut OperationContext::background(&mut crate::interaction::Unattended)
            )
            .unwrap()[0]
                .state,
            CatalogState::Installed
        );
    }

    #[test]
    fn initialized_catalog_can_immediately_accept_a_tool_definition() {
        let temporary = TempDir::new().unwrap();
        let paths = Paths::with_root(temporary.path().join("loadbot"));
        let remote = empty_remote(temporary.path(), "ready-catalog");
        let url = remote.display().to_string();
        fs::create_dir_all(paths.catalogs()).unwrap();
        git::clone_repository(
            &url,
            None,
            &paths.catalog("ready"),
            &mut crate::interaction::Unattended,
        )
        .unwrap();
        git(
            ["config", "user.name", "Loadbot Tests"],
            Some(&paths.catalog("ready")),
        );
        git(
            ["config", "user.email", "loadbot@example.test"],
            Some(&paths.catalog("ready")),
        );

        catalog_initialize(
            &paths,
            "ready",
            url,
            true,
            true,
            false,
            &mut OperationContext::background(&mut crate::interaction::Unattended),
        )
        .unwrap();
        tool_add(
            &paths,
            "ready",
            "demo",
            "https://example.test/demo.git".to_owned(),
            None,
            false,
            false,
            &mut OperationContext::background(&mut crate::interaction::Unattended),
        )
        .unwrap();

        let created = catalog::load(&paths.catalog_file("ready")).unwrap();
        assert_eq!(
            created.tools.get("demo"),
            Some(&ToolConfig::git(
                "https://example.test/demo.git".to_owned(),
                None
            ))
        );
    }

    #[test]
    fn catalog_initialization_rejects_invalid_duplicate_and_destination_collisions_safely() {
        let temporary = TempDir::new().unwrap();
        let paths = Paths::with_root(temporary.path().join("loadbot"));
        let existing_remote = empty_remote(temporary.path(), "existing-catalog");
        catalog_initialize(
            &paths,
            "existing",
            existing_remote.display().to_string(),
            true,
            false,
            false,
            &mut OperationContext::background(&mut crate::interaction::Unattended),
        )
        .unwrap();
        let config_before = fs::read(paths.config()).unwrap();

        let invalid = catalog_initialize(
            &paths,
            "../invalid",
            existing_remote.display().to_string(),
            true,
            false,
            false,
            &mut OperationContext::background(&mut crate::interaction::Unattended),
        )
        .unwrap_err();
        assert!(format!("{invalid:#}").contains("invalid tool name"));

        let other_remote = empty_remote(temporary.path(), "other-catalog");
        let duplicate = catalog_initialize(
            &paths,
            "existing",
            other_remote.display().to_string(),
            true,
            false,
            false,
            &mut OperationContext::background(&mut crate::interaction::Unattended),
        )
        .unwrap_err();
        assert!(format!("{duplicate:#}").contains("different settings"));

        fs::create_dir_all(paths.catalog("collision")).unwrap();
        fs::write(paths.catalog("collision").join("keep.txt"), "keep\n").unwrap();
        let collision = catalog_initialize(
            &paths,
            "collision",
            other_remote.display().to_string(),
            true,
            false,
            false,
            &mut OperationContext::background(&mut crate::interaction::Unattended),
        )
        .unwrap_err();
        assert!(format!("{collision:#}").contains("not a Git repository"));
        assert_eq!(
            fs::read_to_string(paths.catalog("collision").join("keep.txt")).unwrap(),
            "keep\n"
        );
        assert_eq!(fs::read(paths.config()).unwrap(), config_before);
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
