use loadbot::interaction::Notice;

#[derive(Clone, Copy)]
pub enum Query {
    General,
    CatalogList,
}

struct Renderer {
    query: Query,
    first: bool,
}

impl Renderer {
    fn notice(&mut self, notice: &Notice) {
        match notice {
            Notice::CatalogInspected(row) => catalog_row(row),
            Notice::ToolInspected(row) => {
                if !self.first {
                    println!();
                }
                self.first = false;
                println!("{}", row.tool.name);
                println!("  catalog  {}", row.tool.catalog);
                println!("  type     {}", row.tool.definition.source_type.as_str());
                println!(
                    "  state    {}",
                    if row.installed {
                        "installed"
                    } else {
                        "missing"
                    }
                );
            }
            Notice::CatalogResolved { name, path, source } => {
                if matches!(self.query, Query::CatalogList) {
                    if self.first {
                        println!("NAME\tSTATE\tACCESS\tDEFAULT\tURL");
                        self.first = false;
                    }
                    return;
                }
                println!("Name: {name}");
                println!("Path: {}", path.display());
                println!("Catalog fetch URL: {}", source.url);
                println!("Writable: {}", if source.writable { "yes" } else { "no" });
            }
            Notice::ToolResolved { tool, path } => {
                println!("Name: {}", tool.name);
                println!("Catalog: {}", tool.catalog);
                println!("Path: {}", path.display());
            }
            Notice::ToolSourceInspected {
                installed,
                url,
                revision,
            } => {
                println!("Installed: {}", if *installed { "yes" } else { "no" });
                println!("Catalog fetch URL: {url}");
                println!(
                    "Configured revision: {}",
                    revision.as_deref().unwrap_or("(default)")
                );
            }
            Notice::RepositoryInspected(value) => repository(value.as_ref()),
            Notice::CatalogFileInspected(value) => match value {
                CatalogValidity::Unmanaged => {
                    println!("Catalog file: unavailable (destination is not a managed repository)")
                }
                CatalogValidity::Missing => println!("Catalog file: missing"),
                CatalogValidity::Valid => println!("Catalog file: valid"),
                CatalogValidity::Invalid(error) => println!("Catalog file: invalid ({error})"),
            },
            Notice::CatalogRegistered { name } => println!("registered catalog '{name}'"),
            Notice::CatalogAlreadyRegistered { name } => {
                println!("catalog '{name}' is already registered")
            }
            Notice::CatalogInstalled { name, path } => {
                println!("installed catalog '{name}' at {}", path.display())
            }
            Notice::CatalogAlreadyInstalled { name } => {
                println!("catalog '{name}' is already installed")
            }
            Notice::CatalogCreated { name } => {
                println!("created initial catalog.toml for catalog '{name}'")
            }
            Notice::CatalogAlreadyInitialized { name } => {
                println!("catalog '{name}' is already initialized")
            }
            Notice::InitialCatalogCommitted { commit_hash } => {
                println!("committed initial catalog at {commit_hash}")
            }
            Notice::InitialCatalogAlreadyCommitted => {
                println!("initial catalog is already committed")
            }
            Notice::InitialCatalogUncommitted => {
                println!("initial catalog.toml was not committed or pushed")
            }
            Notice::InitialCatalogPushed { name } => {
                println!("pushed initial catalog '{name}' to origin")
            }
            Notice::InitialCatalogNotPushed => println!("initial catalog commit was not pushed"),
            Notice::CatalogCurrent { name, new_commit } => {
                println!("catalog '{name}' is already current at {new_commit}")
            }
            Notice::CatalogSynced {
                name,
                old_commit,
                new_commit,
            } => println!("synchronized catalog '{name}' from {old_commit} to {new_commit}"),
            Notice::CatalogMigrated { name } => {
                println!("migrated legacy tools to writable catalog '{name}'")
            }
            Notice::CatalogUncommitted => println!("catalog changes were not committed or pushed"),
            Notice::ToolAlreadyDefined { name, catalog_name } => {
                println!("tool '{name}' is already defined in catalog '{catalog_name}'")
            }
            Notice::ToolAdded { name, catalog_name } => {
                println!("added tool '{name}' to catalog '{catalog_name}'")
            }
            Notice::CatalogCommitted { commit_hash } => {
                println!("committed catalog change at {commit_hash}")
            }
            Notice::CatalogAlreadyCommitted => println!("catalog change is already committed"),
            Notice::CatalogPushed { catalog_name } => {
                println!("pushed catalog '{catalog_name}' to origin")
            }
            Notice::ToolAlreadyInstalled { name } => println!("tool '{name}' is already installed"),
            Notice::ToolInstalled { name, path } => {
                println!("installed tool '{name}' at {}", path.display())
            }
            Notice::PushUrlConfigured { push_url } => {
                println!("configured SSH push URL: {push_url}")
            }
            Notice::ToolReconciled { name } => {
                println!("reconciled tool '{name}' fetch and push URLs")
            }
            Notice::ToolCurrent { name, new_commit } => {
                println!("tool '{name}' is already current at {new_commit}")
            }
            Notice::ToolUpdated {
                name,
                old_commit,
                new_commit,
            } => println!("updated tool '{name}' from {old_commit} to {new_commit}"),
            Notice::SkippedCatalog { name, diagnostic } => eprintln!(
                "warning: skipping catalog '{name}': {diagnostic}; run 'loadbot catalog status {name}' for details"
            ),
        }
    }
}

use super::menus::{Prompt, TerminalPrompt, terminal_is_interactive};
use anyhow::{Context, Result};
use loadbot::{
    git,
    interaction::{Interaction, OperationContext},
    operations::*,
};

pub struct TerminalInteraction<P> {
    pub prompt: P,
    pub interactive: bool,
    renderer: Renderer,
}
impl<P: Prompt> Interaction for TerminalInteraction<P> {
    fn can_choose(&self) -> bool {
        self.interactive
    }
    fn choose_identity(&mut self, identities: &[git::RotIdentity]) -> Result<Option<usize>> {
        let choices: Vec<_> = identities
            .iter()
            .map(|identity| {
                format!(
                    "{} -> {}",
                    identity.alias,
                    identity.username.as_deref().expect("verified username")
                )
            })
            .collect();
        let Some(selection) = self
            .prompt
            .select("Choose a Rot-managed GitHub SSH identity:", &choices)?
        else {
            return Ok(None);
        };
        Ok(Some(
            choices
                .iter()
                .position(|choice| choice == &selection)
                .context("an invalid SSH identity was selected")?,
        ))
    }
    fn configure_push(&mut self) -> Result<bool> {
        Ok(self
            .prompt
            .confirm("Configure SSH for pushes on this machine?", false)?
            == Some(true))
    }
    fn reconcile_checkout(&mut self, _: &str, _: &str) -> Result<bool> {
        Ok(self.prompt.confirm("This checkout is the configured GitHub repository but uses SSH for fetches.\nUse the catalog HTTPS URL for fetches and keep SSH for pushes?", false)? == Some(true))
    }
    fn replace_push(&mut self, existing: &str, replacement: &str) -> Result<bool> {
        Ok(self.prompt.confirm(
            &format!("Replace existing push URL '{existing}' with '{replacement}'?"),
            false,
        )? == Some(true))
    }
    fn notice(&mut self, value: &Notice) {
        self.renderer.notice(value);
    }
}

pub fn with_context<T>(
    operation: impl FnOnce(&mut OperationContext<'_>) -> Result<T>,
) -> Result<T> {
    with_query(Query::General, operation)
}

pub fn with_query<T>(
    query: Query,
    operation: impl FnOnce(&mut OperationContext<'_>) -> Result<T>,
) -> Result<T> {
    let mut interaction = TerminalInteraction {
        prompt: TerminalPrompt,
        interactive: terminal_is_interactive(),
        renderer: Renderer { query, first: true },
    };
    operation(&mut OperationContext::new(&mut interaction))
}

fn repository(status: Option<&git::RepositoryStatus>) {
    if let Some(repository) = status {
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
            "Fetch URL: {}",
            repository.origin.as_deref().unwrap_or("(none)")
        );
        if let Some(push_url) = repository.push_url.as_deref() {
            println!("Push URL: {push_url}");
        } else if let Some(fetch_url) = repository.origin.as_deref() {
            println!("Push URL: {fetch_url} (uses fetch URL)");
        } else {
            println!("Push URL: (none)");
        }
    } else {
        println!("Current branch: -");
        println!("Current commit: -");
        println!("Working tree: -");
        println!("Fetch URL: -");
        println!("Push URL: -");
    }
}

fn catalog_row(row: &CatalogSummary) {
    let state = match row.state {
        CatalogState::Missing => "missing",
        CatalogState::Installed => "installed",
        CatalogState::Mismatch => "mismatch",
    };
    println!(
        "{}\t{state}\t{}\t{}\t{}",
        row.name,
        if row.source.writable {
            "writable"
        } else {
            "read-only"
        },
        if row.default { "yes" } else { "no" },
        row.source.url
    );
}

pub fn error(error: &anyhow::Error) -> i32 {
    eprintln!("error: {error:#}");
    error
        .downcast_ref::<loadbot::launcher::ChildExit>()
        .map_or(1, |exit| exit.code())
}
