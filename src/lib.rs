//! Loadbot's reusable backend.
//!
//! Read operations return domain data. Mutations validate configuration, paths,
//! and repositories here, and record typed progress in an operation context.
//! An embedding can use `Unattended` or implement the small `Interaction` trait
//! for SSH identity and checkout decisions. Neither policy needs a terminal.
//!
//! ```no_run
//! use loadbot::{interaction::{OperationContext, Unattended}, operations, paths::Paths};
//! let paths = Paths::discover()?;
//! let mut policy = Unattended;
//! let mut context = OperationContext::new(&mut policy);
//! let report = context.run(|context| operations::tool_list(&paths, context));
//! // Inspect report.notices even if report.result is an error.
//! let tools = report.result?;
//! # Ok::<(), anyhow::Error>(())
//! ```

/// Catalog definitions, shared commands, runners, and format-preserving persistence.
pub mod catalog;
/// Local catalog registration and atomic TOML persistence.
pub mod config;
/// Git inspection and guarded mutations, including Rot-assisted SSH retries.
pub mod git;
/// Typed decisions, progress, warnings, and partial-success reports.
pub mod interaction;
/// Project inventory, safe file resolution, and terminal or streamed child execution.
pub mod launcher;
/// Catalog and tool operations shared by every adapter.
pub mod operations;
/// Explicit or platform-default locations and portable-name validation.
pub mod paths;
/// Saved shortcut definitions, validation, and format-preserving persistence.
pub mod shortcuts;

/// Resource leases and shared durable replacement.
pub mod persistence;

/// Observable process execution and caller-owned cancellation.
pub mod process;
