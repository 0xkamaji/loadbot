//! UI-neutral decisions and structured operation progress.
use std::path::PathBuf;

use crate::{
    persistence::Lease,
    process::{Control, Mode},
};
use anyhow::Result;
use std::cell::RefCell;
use std::rc::{Rc, Weak};

use crate::git::RotIdentity;

/// Decisions discovered during Git operations, with domain data rather than prompt strings.
pub trait Interaction {
    fn process_control(&self) -> Control {
        Control::default()
    }
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
    pub process: Control,
    pub tool_mode: Mode,
    leases: Vec<Weak<RefCell<Lease>>>,
    watches: Vec<Weak<Snapshot>>,
}
impl<'a> OperationContext<'a> {
    pub fn new(interaction: &'a mut dyn Interaction) -> Self {
        Self {
            notices: Vec::new(),
            interaction,
            process: Control::default(),
            tool_mode: Mode::Inherit,
            leases: Vec::new(),
            watches: Vec::new(),
        }
    }
    /// Run one operation and return its result together with all progress and diagnostics.
    /// The report is retained on failure; earlier successful steps are never hidden.
    pub fn run<T>(&mut self, operation: impl FnOnce(&mut Self) -> Result<T>) -> OperationReport<T> {
        let _process_scope = crate::process::scope(&self.process);
        let start = self.notices.len();
        self.process.emit(crate::process::Event::OperationStarted);
        let result = self
            .process
            .cancellation
            .check()
            .and_then(|()| operation(self));
        let report = OperationReport {
            result,
            notices: self.notices[start..].to_vec(),
        };
        self.process.emit(crate::process::Event::OperationFinished {
            outcome: report.status(),
            partial: report.is_partial(),
        });
        report
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
    pub(crate) fn lease(&mut self, path: &std::path::Path) -> Result<Rc<RefCell<Lease>>> {
        self.process.cancellation.check()?;
        let lease = Rc::new(RefCell::new(Lease::acquire(path)?));
        self.leases.retain(|lease| lease.strong_count() > 0);
        self.leases.push(Rc::downgrade(&lease));
        Ok(lease)
    }
    pub(crate) fn watch(&mut self, path: &std::path::Path) -> Result<Rc<Snapshot>> {
        let snapshot = Rc::new(Snapshot {
            path: path.to_owned(),
            contents: crate::persistence::read_optional(path)?,
        });
        self.watches.retain(|watch| watch.strong_count() > 0);
        self.watches.push(Rc::downgrade(&snapshot));
        Ok(snapshot)
    }
    fn decision<T>(&mut self, choose: impl FnOnce(&mut dyn Interaction) -> Result<T>) -> Result<T> {
        self.process.cancellation.check()?;
        let leases: Vec<_> = self.leases.iter().filter_map(Weak::upgrade).collect();
        for lease in leases.iter().rev() {
            lease.borrow().suspend()?;
        }
        let result = choose(self.interaction);
        // Always reacquire/revalidate, including cancelled or failed decisions.
        for lease in &leases {
            lease.borrow_mut().resume()?;
        }
        for snapshot in self.watches.iter().filter_map(Weak::upgrade) {
            if crate::persistence::read_optional(&snapshot.path)? != snapshot.contents {
                return Err(crate::persistence::Busy {
                    resource: snapshot.path.clone(),
                }
                .into());
            }
        }
        self.process.cancellation.check()?;
        result
    }
}

pub(crate) struct Snapshot {
    path: PathBuf,
    contents: Option<String>,
}

impl Interaction for OperationContext<'_> {
    fn process_control(&self) -> Control {
        self.process.clone()
    }
    fn can_choose(&self) -> bool {
        self.interaction.can_choose()
    }
    fn choose_identity(&mut self, identities: &[RotIdentity]) -> Result<Option<usize>> {
        self.decision(|interaction| interaction.choose_identity(identities))
    }
    fn configure_push(&mut self) -> Result<bool> {
        self.decision(|i| i.configure_push())
    }
    fn reconcile_checkout(&mut self, fetch: &str, canonical: &str) -> Result<bool> {
        self.decision(|i| i.reconcile_checkout(fetch, canonical))
    }
    fn replace_push(&mut self, existing: &str, replacement: &str) -> Result<bool> {
        self.decision(|i| i.replace_push(existing, replacement))
    }
    fn notice(&mut self, notice: &Notice) {
        self.record(notice.clone());
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperationStatus {
    Succeeded,
    Busy,
    Cancelled,
    Failed,
}
impl<T> OperationReport<T> {
    pub fn status(&self) -> OperationStatus {
        match &self.result {
            Ok(_) => OperationStatus::Succeeded,
            Err(error)
                if error
                    .downcast_ref::<crate::git::RemoteRecoveryIncomplete>()
                    .is_some() =>
            {
                OperationStatus::Failed
            }
            Err(error) if error.downcast_ref::<crate::persistence::Busy>().is_some() => {
                OperationStatus::Busy
            }
            Err(error) if error.downcast_ref::<crate::process::Cancelled>().is_some() => {
                OperationStatus::Cancelled
            }
            Err(_) => OperationStatus::Failed,
        }
    }
    pub fn is_partial(&self) -> bool {
        self.result.as_ref().err().is_some_and(|error| {
            self.notices.iter().any(Notice::completed_mutation)
                || error
                    .downcast_ref::<crate::persistence::DurabilityUncertain>()
                    .is_some()
                || error
                    .downcast_ref::<crate::git::RemoteRecoveryIncomplete>()
                    .is_some()
        })
    }
}
impl Notice {
    pub fn completed_mutation(&self) -> bool {
        matches!(
            self,
            Self::CatalogRegistered { .. }
                | Self::CatalogInstalled { .. }
                | Self::CatalogCreated { .. }
                | Self::InitialCatalogCommitted { .. }
                | Self::InitialCatalogPushed { .. }
                | Self::CatalogSynced { .. }
                | Self::CatalogMigrated { .. }
                | Self::ToolAdded { .. }
                | Self::CatalogCommitted { .. }
                | Self::CatalogPushed { .. }
                | Self::ToolInstalled { .. }
                | Self::PushUrlConfigured { .. }
                | Self::ToolReconciled { .. }
                | Self::ToolUpdated { .. }
        )
    }
}

/// Completed mutation steps. On failure, the context/report retains partial progress.
#[derive(Debug, Clone)]
pub struct MutationOutcome {
    pub notices: Vec<Notice>,
}

#[cfg(test)]
mod tests {
    use super::*;
    struct ConcurrentDecision {
        path: PathBuf,
    }
    impl Interaction for ConcurrentDecision {
        fn configure_push(&mut self) -> Result<bool> {
            let _other_operation = Lease::acquire(&self.path)?;
            Ok(true)
        }
    }
    #[test]
    fn decision_releases_lease_and_rejects_intervening_operation() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("repository");
        let mut interaction = ConcurrentDecision { path: path.clone() };
        let mut context = OperationContext::new(&mut interaction);
        let lease = context.lease(&path).unwrap();
        let error = context.configure_push().unwrap_err();
        assert!(error.downcast_ref::<crate::persistence::Busy>().is_some());
        drop(lease);
        assert!(Lease::acquire(&path).is_ok());
    }
}
