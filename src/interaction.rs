//! UI-neutral decisions and structured operation progress.
use std::path::PathBuf;

use anyhow::Result;

use crate::git::RotIdentity;

/// Decisions discovered during Git operations, with domain data rather than prompt strings.
pub trait Interaction {
    fn can_choose(&self) -> bool {
        false
    }
    fn choose_identity(&mut self, _identities: &[RotIdentity]) -> Result<Option<usize>> {
        Ok(None)
    }
    fn configure_push(&mut self) -> Result<bool> {
        Ok(false)
    }
    fn reconcile_checkout(&mut self, _fetch: &str, _canonical: &str) -> Result<bool> {
        Ok(false)
    }
    fn replace_push(&mut self, _existing: &str, _replacement: &str) -> Result<bool> {
        Ok(false)
    }
    /// Immediate domain progress notification, also retained by the context on failure.
    fn notice(&mut self, _notice: &Notice) {}
}

/// Safe unattended policy: never authorizes optional changes or chooses among identities.
pub struct Unattended;
impl Interaction for Unattended {}

/// Caller-owned structured progress and warnings, including partial success before an error.
pub struct OperationContext<'a> {
    pub notices: Vec<Notice>,
    pub interaction: &'a mut dyn Interaction,
}
impl<'a> OperationContext<'a> {
    pub fn new(interaction: &'a mut dyn Interaction) -> Self {
        Self {
            notices: Vec::new(),
            interaction,
        }
    }
    /// Run one operation and return its result together with all progress and diagnostics.
    /// The report is retained on failure; earlier successful steps are never hidden.
    pub fn run<T>(&mut self, operation: impl FnOnce(&mut Self) -> Result<T>) -> OperationReport<T> {
        let start = self.notices.len();
        let result = operation(self);
        OperationReport {
            result,
            notices: self.notices[start..].to_vec(),
        }
    }
    pub(crate) fn outcome_since(&self, start: usize) -> MutationOutcome {
        MutationOutcome {
            notices: self.notices[start..].to_vec(),
        }
    }
    pub(crate) fn record(&mut self, notice: Notice) {
        self.interaction.notice(&notice);
        self.notices.push(notice);
    }
}

#[derive(Debug, Clone)]
pub enum Notice {
    CatalogInspected(crate::operations::CatalogSummary),
    ToolInspected(crate::operations::ToolSummary),
    CatalogResolved {
        name: String,
        path: PathBuf,
        source: crate::config::CatalogSource,
    },
    ToolResolved {
        tool: crate::catalog::ResolvedTool,
        path: PathBuf,
    },
    ToolSourceInspected {
        installed: bool,
        url: String,
        revision: Option<String>,
    },
    RepositoryInspected(Option<crate::git::RepositoryStatus>),
    CatalogFileInspected(crate::operations::CatalogValidity),
    CatalogRegistered {
        name: String,
    },
    CatalogAlreadyRegistered {
        name: String,
    },
    CatalogInstalled {
        name: String,
        path: PathBuf,
    },
    CatalogAlreadyInstalled {
        name: String,
    },
    CatalogCreated {
        name: String,
    },
    CatalogAlreadyInitialized {
        name: String,
    },
    InitialCatalogCommitted {
        commit_hash: String,
    },
    InitialCatalogAlreadyCommitted,
    InitialCatalogUncommitted,
    InitialCatalogPushed {
        name: String,
    },
    InitialCatalogNotPushed,
    CatalogCurrent {
        name: String,
        new_commit: String,
    },
    CatalogSynced {
        name: String,
        old_commit: String,
        new_commit: String,
    },
    CatalogMigrated {
        name: String,
    },
    CatalogUncommitted,
    ToolAlreadyDefined {
        name: String,
        catalog_name: String,
    },
    ToolAdded {
        name: String,
        catalog_name: String,
    },
    CatalogCommitted {
        commit_hash: String,
    },
    CatalogAlreadyCommitted,
    CatalogPushed {
        catalog_name: String,
    },
    ToolAlreadyInstalled {
        name: String,
    },
    ToolInstalled {
        name: String,
        path: PathBuf,
    },
    PushUrlConfigured {
        push_url: String,
    },
    ToolReconciled {
        name: String,
    },
    ToolCurrent {
        name: String,
        new_commit: String,
    },
    ToolUpdated {
        name: String,
        old_commit: String,
        new_commit: String,
    },
    SkippedCatalog {
        name: String,
        diagnostic: String,
    },
}

#[derive(Debug)]
pub struct OperationReport<T> {
    pub result: Result<T>,
    pub notices: Vec<Notice>,
}

/// Completed mutation steps. On failure, the context/report retains partial progress.
#[derive(Debug, Clone)]
pub struct MutationOutcome {
    pub notices: Vec<Notice>,
}
