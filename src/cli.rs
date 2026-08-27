use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(name = "loadbot", version, about)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Debug, Subcommand)]
pub enum Commands {
    /// Add a tool definition to a writable catalog.
    Add {
        name: Option<String>,
        git_url: Option<String>,
        #[arg(long)]
        revision: Option<String>,
        #[arg(long)]
        catalog: Option<String>,
        #[arg(long)]
        commit: bool,
        #[arg(long, requires = "commit")]
        push: bool,
    },
    /// Clone a registered repository.
    Pull {
        name: Option<String>,
        #[arg(long)]
        catalog: Option<String>,
    },
    /// Fast-forward an installed repository.
    Update {
        name: Option<String>,
        #[arg(long)]
        catalog: Option<String>,
    },
    /// List configured tools.
    List,
    /// Print the absolute path assigned to a tool.
    Path {
        name: Option<String>,
        #[arg(long)]
        catalog: Option<String>,
    },
    /// Show local repository status.
    Status {
        name: Option<String>,
        #[arg(long)]
        catalog: Option<String>,
    },
    /// Manage Git-backed tool catalogs.
    Catalog {
        #[command(subcommand)]
        command: CatalogCommands,
    },
}

#[derive(Debug, Subcommand)]
pub enum CatalogCommands {
    /// Register and clone a catalog repository.
    Add {
        name: Option<String>,
        git_url: Option<String>,
        #[arg(long)]
        writable: bool,
    },
    /// List registered catalogs.
    List,
    /// Fast-forward a registered catalog.
    Sync { name: Option<String> },
    /// Show local catalog status.
    Status { name: Option<String> },
    /// Print the absolute path assigned to a catalog.
    Path { name: Option<String> },
    /// Move legacy local tool definitions into a new catalog clone.
    Migrate { name: String, git_url: String },
}
