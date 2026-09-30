use clap::{Args, Parser, Subcommand};
use clap_complete::engine::ArgValueCompleter;

#[derive(Debug, Parser)]
#[command(name = "loadbot", version, about)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Debug, PartialEq, Eq, Subcommand)]
pub enum Commands {
    /// Launch the installed desktop GUI.
    Gui {
        /// Launch the source development environment with hot reload.
        #[arg(long)]
        dev: bool,
    },
    /// Install, configure, or repair Loadbot from a source checkout.
    Setup(SetupArgs),
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
    /// Push local commits to the configured remote for an installed tool.
    Push {
        name: Option<String>,
        #[arg(long)]
        catalog: Option<String>,
    },
    /// Remove a managed local checkout while retaining its catalog entry.
    Remove {
        name: Option<String>,
        #[arg(long)]
        catalog: Option<String>,
    },
    /// Replace a managed local checkout with a fresh clone.
    Reinstall {
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
        #[arg(add = ArgValueCompleter::new(super::completion::shortcut_candidates))]
        shortcut: Option<String>,
    },
    /// Manage saved shortcuts.
    Shortcut {
        #[command(subcommand)]
        command: Option<ShortcutCommands>,
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

#[derive(Debug, PartialEq, Eq, Args)]
#[group(multiple = false)]
pub struct SetupArgs {
    /// Install only the CLI and shell integration.
    #[arg(long)]
    pub cli: bool,
    /// Install the GUI launcher and desktop application without shell completion.
    #[arg(long)]
    pub gui: bool,
    /// Install the CLI, shell integration, and GUI.
    #[arg(long)]
    pub all: bool,
    /// Verify and conservatively restore the previously selected components.
    #[arg(long)]
    pub repair: bool,
}

#[derive(Debug, PartialEq, Eq, Subcommand)]
pub enum RotCommands {
    Complete {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        words: Vec<String>,
    },
}

#[derive(Debug, PartialEq, Eq, Subcommand)]
pub enum ShortcutCommands {
    /// Save an installed file as a shortcut without running it.
    Add,
    /// List saved shortcut definitions.
    List,
    /// Remove a saved shortcut definition.
    Remove {
        #[arg(add = ArgValueCompleter::new(super::completion::shortcut_candidates))]
        name: Option<String>,
        #[arg(long, requires = "name")]
        yes: bool,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shortcut_commands_compare_structurally() {
        for (args, expected) in [
            (vec!["add"], ShortcutCommands::Add),
            (vec!["list"], ShortcutCommands::List),
            (
                vec!["remove"],
                ShortcutCommands::Remove {
                    name: None,
                    yes: false,
                },
            ),
            (
                vec!["remove", "demo"],
                ShortcutCommands::Remove {
                    name: Some("demo".to_owned()),
                    yes: false,
                },
            ),
            (
                vec!["remove", "demo", "--yes"],
                ShortcutCommands::Remove {
                    name: Some("demo".to_owned()),
                    yes: true,
                },
            ),
        ] {
            let parsed =
                Cli::try_parse_from(["loadbot", "shortcut"].into_iter().chain(args)).unwrap();
            assert_eq!(
                parsed.command,
                Some(Commands::Shortcut {
                    command: Some(expected)
                })
            );
        }
        assert!(Cli::try_parse_from(["loadbot", "shortcut", "remove", "--yes"]).is_err());
    }

    #[test]
    fn bare_command_is_optional() {
        assert!(Cli::try_parse_from(["loadbot"]).unwrap().command.is_none());
        assert_eq!(
            Cli::try_parse_from(["loadbot", "shortcut"])
                .unwrap()
                .command,
            Some(Commands::Shortcut { command: None })
        );
    }

    #[test]
    fn direct_tool_arguments_are_preserved() {
        for command in [
            "pull",
            "update",
            "push",
            "remove",
            "reinstall",
            "path",
            "status",
        ] {
            let parsed =
                Cli::try_parse_from(["loadbot", command, "demo", "--catalog", "personal"]).unwrap();
            let (name, catalog) = match parsed.command.unwrap() {
                Commands::Pull { name, catalog }
                | Commands::Update { name, catalog }
                | Commands::Push { name, catalog }
                | Commands::Remove { name, catalog }
                | Commands::Reinstall { name, catalog }
                | Commands::Path { name, catalog }
                | Commands::Status { name, catalog } => (name, catalog),
                _ => panic!("wrong command"),
            };
            assert_eq!(name.as_deref(), Some("demo"));
            assert_eq!(catalog.as_deref(), Some("personal"));
        }
        let parsed = Cli::try_parse_from([
            "loadbot",
            "add",
            "demo",
            "local.git",
            "--revision",
            "main",
            "--catalog",
            "personal",
            "--commit",
            "--push",
        ])
        .unwrap();
        assert!(
            matches!(parsed.command, Some(Commands::Add { name: Some(name), git_url: Some(url), revision: Some(revision), catalog: Some(catalog), commit: true, push: true }) if name == "demo" && url == "local.git" && revision == "main" && catalog == "personal")
        );
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
        assert_eq!(
            add.command,
            Some(Commands::Shortcut {
                command: Some(ShortcutCommands::Add)
            })
        );
    }

    #[test]
    fn parses_gui_and_setup_modes_without_ambiguity() {
        assert_eq!(
            Cli::try_parse_from(["loadbot", "gui"]).unwrap().command,
            Some(Commands::Gui { dev: false })
        );
        assert_eq!(
            Cli::try_parse_from(["loadbot", "gui", "--dev"])
                .unwrap()
                .command,
            Some(Commands::Gui { dev: true })
        );
        assert!(matches!(
            Cli::try_parse_from(["loadbot", "setup"]).unwrap().command,
            Some(Commands::Setup(SetupArgs {
                cli: false,
                gui: false,
                all: false,
                repair: false
            }))
        ));
        assert!(matches!(
            Cli::try_parse_from(["loadbot", "setup", "--gui"])
                .unwrap()
                .command,
            Some(Commands::Setup(SetupArgs { gui: true, .. }))
        ));
        assert!(Cli::try_parse_from(["loadbot", "setup", "--cli", "--gui"]).is_err());
        assert!(Cli::try_parse_from(["loadbot", "gui", "--fixture"]).is_err());
    }
}

#[derive(Debug, PartialEq, Eq, Subcommand)]
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
