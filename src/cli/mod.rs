mod args;
pub(crate) mod completion;
mod gui;
mod launcher;
mod menus;
mod operations;
mod output;
mod setup;
mod shortcuts;

use loadbot::paths;
use menus as interactive;

use anyhow::{Context, Result, bail};
use clap::Parser;

use args::{CatalogCommands, Cli, Commands, RotCommands, ShortcutCommands};
use interactive::Prompt;
use paths::Paths;

pub fn main() -> i32 {
    match run() {
        Ok(()) => 0,
        Err(error) => output::error(&error),
    }
}

pub fn run() -> Result<()> {
    completion::complete();
    let cli = Cli::parse();
    if let Some(Commands::Rot {
        command: RotCommands::Complete { words },
    }) = &cli.command
    {
        return completion::rot_complete(words);
    }
    let command = match cli.command {
        Some(command) => command,
        None => {
            require_interactive("loadbot", "")?;
            let mut prompt = interactive::TerminalPrompt;
            let Some(command) = collect_main_command(&mut prompt)? else {
                return Ok(());
            };
            command
        }
    };
    match command {
        Commands::Gui { dev } => gui::run(dev),
        Commands::Setup(arguments) => setup::run(arguments),
        command => {
            let paths = Paths::discover()?;
            dispatch_command(&paths, command)
        }
    }
}

fn collect_main_command<P: Prompt>(prompt: &mut P) -> Result<Option<Commands>> {
    use interactive::MainMenuAction;
    Ok(match interactive::collect_main_menu(prompt)? {
        Some(MainMenuAction::Run) => Some(Commands::Run { shortcut: None }),
        Some(MainMenuAction::Add) => Some(Commands::Add {
            name: None,
            git_url: None,
            revision: None,
            catalog: None,
            commit: false,
            push: false,
        }),
        Some(MainMenuAction::Pull) => Some(Commands::Pull {
            name: None,
            catalog: None,
        }),
        Some(MainMenuAction::Update) => Some(Commands::Update {
            name: None,
            catalog: None,
        }),
        Some(MainMenuAction::List) => Some(Commands::List),
        Some(MainMenuAction::Path) => Some(Commands::Path {
            name: None,
            catalog: None,
        }),
        Some(MainMenuAction::Status) => Some(Commands::Status {
            name: None,
            catalog: None,
        }),
        Some(MainMenuAction::ManageShortcuts) => Some(Commands::Shortcut { command: None }),
        Some(MainMenuAction::ManageCatalogs) => Some(Commands::Catalog { command: None }),
        Some(MainMenuAction::Exit) | None => None,
    })
}

fn dispatch_command(paths: &Paths, command: Commands) -> Result<()> {
    match command {
        Commands::Add {
            name,
            git_url,
            revision,
            catalog,
            commit,
            push,
        } => run_tool_add(paths, name, git_url, revision, catalog, commit, push),
        Commands::Pull { name, catalog } => {
            run_tool_named(paths, name, catalog, "pull", operations::tool_pull)
        }
        Commands::Update { name, catalog } => {
            run_tool_named(paths, name, catalog, "update", operations::tool_update)
        }
        Commands::Remove { name, catalog } => {
            run_tool_named(paths, name, catalog, "remove", operations::tool_remove)
        }
        Commands::Reinstall { name, catalog } => run_tool_named(
            paths,
            name,
            catalog,
            "reinstall",
            operations::tool_reinstall,
        ),
        Commands::List => operations::tool_list(paths),
        Commands::Path { name, catalog } => {
            run_tool_named(paths, name, catalog, "path", operations::tool_path)
        }
        Commands::Status { name, catalog } => {
            run_tool_named(paths, name, catalog, "status", operations::tool_status)
        }
        Commands::Run { shortcut } => match shortcut {
            Some(name) => launcher::run_shortcut(paths, &name),
            None => {
                require_interactive("run", "SHORTCUT")?;
                let mut prompt = interactive::TerminalPrompt;
                launcher::run_interactive(paths, &mut prompt)
            }
        },
        Commands::Shortcut { command } => run_shortcut(paths, command),
        Commands::Catalog { command } => run_catalog(paths, command),
        Commands::Gui { .. } | Commands::Setup(_) => unreachable!(),
        Commands::Rot { .. } => unreachable!(),
    }
}

fn run_shortcut(paths: &Paths, command: Option<ShortcutCommands>) -> Result<()> {
    match &command {
        None => require_interactive("shortcut", "")?,
        Some(ShortcutCommands::Add) => require_interactive("shortcut add", "")?,
        Some(ShortcutCommands::List) => {}
        Some(ShortcutCommands::Remove { name, yes }) => {
            if name.is_none() || !yes {
                require_interactive("shortcut remove", "NAME --yes")?;
            }
        }
    }
    let mut prompt = interactive::TerminalPrompt;
    run_shortcut_with_prompt(paths, command, &mut prompt, launcher::add_shortcut)
}

fn run_shortcut_with_prompt<P, F>(
    paths: &Paths,
    command: Option<ShortcutCommands>,
    prompt: &mut P,
    add_shortcut: F,
) -> Result<()>
where
    P: Prompt,
    F: FnOnce(&Paths, &mut P) -> Result<()>,
{
    let command = match command {
        Some(command) => command,
        None => match interactive::collect_shortcut_menu(prompt)? {
            Some(interactive::ShortcutMenuAction::Add) => ShortcutCommands::Add,
            Some(interactive::ShortcutMenuAction::List) => ShortcutCommands::List,
            Some(interactive::ShortcutMenuAction::Remove) => ShortcutCommands::Remove {
                name: None,
                yes: false,
            },
            None => return Ok(()),
        },
    };
    match command {
        ShortcutCommands::Add => add_shortcut(paths, prompt),
        ShortcutCommands::List => {
            println!("{}", shortcuts::list(&paths.shortcuts()?)?);
            Ok(())
        }
        ShortcutCommands::Remove { name, yes } => {
            if let Some(name) =
                shortcuts::remove_with_prompt(&paths.shortcuts()?, name, yes, prompt)?
            {
                println!("removed shortcut '{name}'");
            }
            Ok(())
        }
    }
}

fn run_catalog(paths: &Paths, command: Option<CatalogCommands>) -> Result<()> {
    let Some(command) = command else {
        require_interactive("catalog", "")?;
        let mut prompt = interactive::TerminalPrompt;
        return run_catalog_menu(paths, &mut prompt);
    };
    match command {
        CatalogCommands::Add {
            name,
            git_url,
            writable,
        } => run_catalog_add(paths, name, git_url, writable),
        CatalogCommands::List => operations::catalog_list(paths),
        CatalogCommands::Sync { name } => {
            run_catalog_named(paths, name, "catalog sync", true, operations::catalog_sync)
        }
        CatalogCommands::Status { name } => run_catalog_named(
            paths,
            name,
            "catalog status",
            false,
            operations::catalog_status,
        ),
        CatalogCommands::Path { name } => {
            run_catalog_named(paths, name, "catalog path", false, operations::catalog_path)
        }
        CatalogCommands::Migrate { name, git_url } => {
            operations::catalog_migrate(paths, &name, git_url)
        }
    }
}

fn run_catalog_menu<P: Prompt>(paths: &Paths, prompt: &mut P) -> Result<()> {
    let Some(action) = interactive::collect_catalog_menu(prompt)? else {
        return cancelled();
    };
    match action {
        interactive::CatalogMenuAction::UseKamajiCatalog => {
            run_kamaji_catalog(paths, prompt, operations::catalog_add)
        }
        interactive::CatalogMenuAction::AddExisting => {
            let Some(input) = interactive::collect_catalog_add(prompt, None, None, false)? else {
                return cancelled();
            };
            operations::catalog_add(paths, &input.name, input.url, input.writable)
        }
        interactive::CatalogMenuAction::Initialize => {
            let Some(input) = interactive::collect_catalog_initialize(prompt)? else {
                return cancelled();
            };
            operations::catalog_initialize(
                paths,
                &input.catalog.name,
                input.catalog.url,
                input.catalog.writable,
                input.commit,
                input.push,
            )
        }
        interactive::CatalogMenuAction::List => operations::catalog_list(paths),
        interactive::CatalogMenuAction::Sync => run_catalog_named_with_prompt(
            paths,
            None,
            "catalog sync",
            true,
            operations::catalog_sync,
            prompt,
        ),
        interactive::CatalogMenuAction::Status => run_catalog_named_with_prompt(
            paths,
            None,
            "catalog status",
            false,
            operations::catalog_status,
            prompt,
        ),
        interactive::CatalogMenuAction::Path => run_catalog_named_with_prompt(
            paths,
            None,
            "catalog path",
            false,
            operations::catalog_path,
            prompt,
        ),
    }
}

fn run_kamaji_catalog<P, F>(paths: &Paths, prompt: &mut P, add_catalog: F) -> Result<()>
where
    P: Prompt,
    F: FnOnce(&Paths, &str, String, bool) -> Result<()>,
{
    let Some(input) = interactive::confirm_kamaji_catalog(prompt)? else {
        return cancelled();
    };
    add_catalog(paths, &input.name, input.url, input.writable)
}

fn run_catalog_add(
    paths: &Paths,
    name: Option<String>,
    url: Option<String>,
    writable_flag: bool,
) -> Result<()> {
    if let (Some(name), Some(url)) = (name.as_deref(), url.as_ref()) {
        return operations::catalog_add(paths, name, url.clone(), writable_flag);
    }
    require_interactive("catalog add", "NAME GIT_URL")?;
    let mut prompt = interactive::TerminalPrompt;
    let Some(input) = interactive::collect_catalog_add(&mut prompt, name, url, writable_flag)?
    else {
        return cancelled();
    };
    operations::catalog_add(paths, &input.name, input.url, input.writable)
}

#[allow(clippy::too_many_arguments)]
fn run_tool_add(
    paths: &Paths,
    name: Option<String>,
    url: Option<String>,
    revision: Option<String>,
    catalog_name: Option<String>,
    commit_flag: bool,
    push_flag: bool,
) -> Result<()> {
    if let (Some(name), Some(url)) = (name.as_deref(), url.as_ref()) {
        let selected_catalog = match catalog_name.as_deref() {
            Some(catalog_name) => Some(catalog_name.to_owned()),
            None => operations::default_writable_catalog(paths)?,
        };
        if let Some(selected_catalog) = selected_catalog {
            return operations::tool_add(
                paths,
                &selected_catalog,
                name,
                url.clone(),
                revision,
                commit_flag,
                push_flag,
            );
        }
    }
    require_interactive("add", "NAME GIT_URL --catalog CATALOG")?;
    let mut prompt = interactive::TerminalPrompt;
    let writable = operations::writable_catalogs(paths)?;
    let Some(input) = interactive::collect_tool_add(
        &mut prompt,
        name,
        url,
        revision,
        catalog_name,
        &writable,
        commit_flag,
        push_flag,
    )?
    else {
        return cancelled();
    };

    operations::tool_add(
        paths,
        &input.catalog,
        &input.name,
        input.url,
        input.revision,
        input.commit,
        input.push,
    )?;
    match prompt.confirm("Pull it now?", true)? {
        Some(true) => {
            operations::tool_pull(paths, &input.name, Some(&input.catalog))
                .context("tool definition was added, but pulling it now failed")?;
            prompt.message(&format!("Tool installed: {}", input.name))?;
            if prompt.confirm("Add shortcuts for this tool now?", false)? == Some(true) {
                launcher::add_shortcuts_for_tool(paths, &mut prompt, &input.catalog, &input.name)?;
            }
            Ok(())
        }
        Some(false) => Ok(()),
        None => {
            eprintln!("pull cancelled; tool definition remains in the catalog");
            Ok(())
        }
    }
}

fn run_catalog_named(
    paths: &Paths,
    name: Option<String>,
    command: &str,
    available_only: bool,
    operation: fn(&Paths, &str) -> Result<()>,
) -> Result<()> {
    if let Some(name) = name {
        return operation(paths, &name);
    }
    require_interactive(command, "NAME")?;
    let mut prompt = interactive::TerminalPrompt;
    run_catalog_named_with_prompt(paths, None, command, available_only, operation, &mut prompt)
}

fn run_catalog_named_with_prompt<P: Prompt>(
    paths: &Paths,
    name: Option<String>,
    _command: &str,
    available_only: bool,
    operation: fn(&Paths, &str) -> Result<()>,
    prompt: &mut P,
) -> Result<()> {
    if let Some(name) = name {
        return operation(paths, &name);
    }
    let choices = if available_only {
        operations::available_catalog_names(paths)?
    } else {
        operations::catalog_names(paths)?
    };
    if choices.is_empty() {
        if available_only {
            bail!(
                "No installed, valid catalogs are available.\nRun 'loadbot catalog list' or 'loadbot catalog status NAME' for details."
            );
        }
        bail!("No catalogs are configured.\nRun 'loadbot catalog add' to add one.");
    }
    match prompt.select("Select a catalog:\n", &choices)? {
        Some(name) => operation(paths, &name),
        None => cancelled(),
    }
}

fn run_tool_named(
    paths: &Paths,
    name: Option<String>,
    catalog_name: Option<String>,
    command: &str,
    operation: fn(&Paths, &str, Option<&str>) -> Result<()>,
) -> Result<()> {
    if let Some(name) = name {
        return operation(paths, &name, catalog_name.as_deref());
    }
    require_interactive(command, "NAME [--catalog CATALOG]")?;
    let tools = operations::all_tools(paths)?;
    if tools.is_empty() {
        bail!("No tools are configured.\nRun 'loadbot add' to add one.");
    }
    let choices: Vec<_> = tools
        .iter()
        .map(|tool| format!("{}/{}", tool.catalog, tool.name))
        .collect();
    let mut prompt = interactive::TerminalPrompt;
    let Some(selection) = prompt.select("Select a tool:\n", &choices)? else {
        return cancelled();
    };
    let (catalog_name, name) = selection
        .split_once('/')
        .context("invalid interactive tool selection")?;
    operation(paths, name, Some(catalog_name))
}

fn require_interactive(command: &str, arguments: &str) -> Result<()> {
    if !interactive::terminal_is_interactive() {
        if arguments.is_empty() {
            bail!("'{command}' requires an interactive terminal");
        } else {
            bail!(
                "missing required argument for '{command}'; run it in an interactive terminal or supply: loadbot {command} {arguments}"
            );
        }
    }
    Ok(())
}

fn cancelled() -> Result<()> {
    eprintln!("cancelled");
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::path::PathBuf;

    use super::*;

    struct MenuPrompt(Option<usize>);

    impl Prompt for MenuPrompt {
        fn input(&mut self, _: &str, _: Option<&str>) -> Result<Option<String>> {
            unreachable!()
        }
        fn confirm(&mut self, _: &str, _: bool) -> Result<Option<bool>> {
            unreachable!()
        }
        fn message(&mut self, _: &str) -> Result<()> {
            unreachable!()
        }
        fn select(&mut self, label: &str, choices: &[String]) -> Result<Option<String>> {
            assert_eq!(label, "Loadbot:");
            assert_eq!(
                choices,
                &[
                    "Run a tool",
                    "Add a tool",
                    "Pull/install a tool",
                    "Update a tool",
                    "List tools",
                    "Show tool path",
                    "Show tool status",
                    "Manage shortcuts",
                    "Manage catalogs",
                    "Exit",
                ]
            );
            Ok(self.0.map(|index| choices[index].clone()))
        }
    }

    #[test]
    fn every_main_menu_selection_delegates_to_its_direct_command() {
        let commands = [
            vec!["run"],
            vec!["add"],
            vec!["pull"],
            vec!["update"],
            vec!["list"],
            vec!["path"],
            vec!["status"],
            vec!["shortcut"],
            vec!["catalog"],
        ];
        for (index, arguments) in commands.into_iter().enumerate() {
            let direct = Cli::try_parse_from(std::iter::once("loadbot").chain(arguments)).unwrap();
            let selected = collect_main_command(&mut MenuPrompt(Some(index))).unwrap();
            assert_eq!(selected, direct.command);
        }
    }

    #[test]
    fn main_menu_exit_and_cancellation_do_not_dispatch() {
        for selection in [Some(9), None] {
            assert!(
                collect_main_command(&mut MenuPrompt(selection))
                    .unwrap()
                    .is_none()
            );
        }
    }

    struct ShortcutPrompt(Option<&'static str>);

    impl Prompt for ShortcutPrompt {
        fn input(&mut self, _: &str, _: Option<&str>) -> Result<Option<String>> {
            unreachable!()
        }
        fn confirm(&mut self, _: &str, _: bool) -> Result<Option<bool>> {
            unreachable!()
        }
        fn message(&mut self, _: &str) -> Result<()> {
            unreachable!()
        }
        fn select(&mut self, label: &str, choices: &[String]) -> Result<Option<String>> {
            assert_eq!(label, "Shortcuts:");
            assert_eq!(
                choices,
                &[
                    "Add a shortcut",
                    "List shortcuts",
                    "Remove a shortcut",
                    "Cancel"
                ]
            );
            Ok(self.0.map(str::to_owned))
        }
    }

    #[test]
    fn shortcut_menu_and_direct_add_delegate_to_the_same_handler() {
        let paths = Paths::with_root(PathBuf::from("/tmp/loadbot-shortcut-menu-test"));
        for command in [None, Some(ShortcutCommands::Add)] {
            let selection = command.is_none().then_some("Add a shortcut");
            let mut called = false;
            run_shortcut_with_prompt(
                &paths,
                command,
                &mut ShortcutPrompt(selection),
                |received, _| {
                    assert!(std::ptr::eq(received, &paths));
                    called = true;
                    Ok(())
                },
            )
            .unwrap();
            assert!(called);
        }
    }

    #[test]
    fn shortcut_menu_cancel_and_eof_never_call_add() {
        let paths = Paths::with_root(PathBuf::from("/tmp/loadbot-shortcut-menu-test"));
        for selection in [Some("Cancel"), None] {
            run_shortcut_with_prompt(&paths, None, &mut ShortcutPrompt(selection), |_, _| {
                panic!("shortcut add must not run after cancellation")
            })
            .unwrap();
        }
    }

    struct ConfirmPrompt(bool);

    impl Prompt for ConfirmPrompt {
        fn input(&mut self, _: &str, _: Option<&str>) -> Result<Option<String>> {
            unreachable!()
        }

        fn confirm(&mut self, _: &str, _: bool) -> Result<Option<bool>> {
            Ok(Some(self.0))
        }

        fn select(&mut self, _: &str, _: &[String]) -> Result<Option<String>> {
            unreachable!()
        }

        fn message(&mut self, _: &str) -> Result<()> {
            Ok(())
        }
    }

    #[test]
    fn kamaji_preset_delegates_exact_catalog_add_values() {
        let paths = Paths::with_root(PathBuf::from("/tmp/loadbot-menu-test"));
        let received = RefCell::new(None);
        run_kamaji_catalog(
            &paths,
            &mut ConfirmPrompt(true),
            |_, name, url, writable| {
                received.replace(Some((name.to_owned(), url, writable)));
                Ok(())
            },
        )
        .unwrap();

        assert_eq!(
            received.into_inner(),
            Some((
                "personal".to_owned(),
                "https://github.com/0xkamaji/loadbot-catalog.git".to_owned(),
                true,
            ))
        );
    }

    #[test]
    fn refusing_kamaji_preset_never_calls_catalog_add() {
        let paths = Paths::with_root(PathBuf::from("/tmp/loadbot-menu-test"));
        run_kamaji_catalog(&paths, &mut ConfirmPrompt(false), |_, _, _, _| {
            panic!("catalog add must not run after confirmation refusal")
        })
        .unwrap();
    }
}
