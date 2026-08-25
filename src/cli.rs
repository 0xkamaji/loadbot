use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(name = "loadbot", version, about)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Debug, Subcommand)]
pub enum Commands {
    /// Register a Git repository without cloning it.
    Add {
        name: Option<String>,
        git_url: Option<String>,
        #[arg(long)]
        revision: Option<String>,
    },
    /// Clone a registered repository.
    Pull { name: Option<String> },
    /// Fast-forward an installed repository.
    Update { name: Option<String> },
    /// List configured tools.
    List,
    /// Print the absolute path assigned to a tool.
    Path { name: Option<String> },
    /// Show local repository status.
    Status { name: Option<String> },
}
