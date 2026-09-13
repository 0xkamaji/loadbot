use clap::{Parser, Subcommand};
use clap_complete::engine::ArgValueCompleter;

#[derive(Debug, Parser)]
#[command(name = "loadbot", version, about)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,
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
    /// Launch an installed file or saved shortcut.
    Run {
        #[arg(add = ArgValueCompleter::new(crate::completion::shortcut_candidates))]
        shortcut: Option<String>,
    },
    /// Manage saved shortcuts.
    Shortcut {
        #[command(subcommand)]
        command: ShortcutCommands,
    },
    /// Manage Git-backed tool catalogs.
    Catalog {
        #[command(subcommand)]
        command: Option<CatalogCommands>,
    },
    #[command(hide = true)]
    Rot {
        #[command(subcommand)]
        command: RotCommands,
    },
}

#[derive(Debug, Subcommand)]
pub enum RotCommands {
    Complete {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        words: Vec<String>,
    },
}

#[derive(Debug, Subcommand)]
pub enum ShortcutCommands {
    /// Save an installed file as a shortcut without running it.
    Add,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bare_command_is_optional() {
        assert!(
            Cli::try_parse_from(["loadbot"])
                .unwrap()
                .command
                .is_none()
        );
        assert!(Cli::try_parse_from(["loadbot", "shortcut"]).is_err());
    }

    #[test]
    fn direct_tool_arguments_are_preserved() {
        for command in ["pull", "update", "path", "status"] {
            let parsed =
                Cli::try_parse_from(["loadbot", command, "demo", "--catalog", "personal"])
                    .unwrap();
            let (name, catalog) = match parsed.command.unwrap() {
                Commands::Pull { name, catalog }
                | Commands::Update { name, catalog }
                | Commands::Path { name, catalog }
                | Commands::Status { name, catalog } => (name, catalog),
                _ => panic!("wrong command"),
            };
            assert_eq!(name.as_deref(), Some("demo"));
            assert_eq!(catalog.as_deref(), Some("personal"));
        }
        let parsed = Cli::try_parse_from([
            "loadbot", "add", "demo", "local.git", "--revision", "main", "--catalog",
            "personal", "--commit", "--push",
        ])
        .unwrap();
        assert!(matches!(parsed.command, Some(Commands::Add { name: Some(name), git_url: Some(url), revision: Some(revision), catalog: Some(catalog), commit: true, push: true }) if name == "demo" && url == "local.git" && revision == "main" && catalog == "personal"));
        assert!(Cli::try_parse_from(["loadbot", "add", "--push"]).is_err());
    }

    #[test]
    fn parses_interactive_and_shortcut_run_forms() {
        let interactive = Cli::try_parse_from(["loadbot", "run"]).unwrap();
        assert!(matches!(
            interactive.command.unwrap(),
            Commands::Run { shortcut: None }
        ));

        let catalog = Cli::try_parse_from(["loadbot", "catalog"]).unwrap();
        assert!(matches!(
            catalog.command.unwrap(),
            Commands::Catalog { command: None }
        ));

        let direct = Cli::try_parse_from(["loadbot", "run", "print-strings"]).unwrap();
        assert!(matches!(
            direct.command.unwrap(),
            Commands::Run {
                shortcut: Some(name)
            } if name == "print-strings"
        ));

        let add = Cli::try_parse_from(["loadbot", "shortcut", "add"]).unwrap();
        assert!(matches!(
            add.command.unwrap(),
            Commands::Shortcut {
                command: ShortcutCommands::Add
            }
        ));
    }
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
