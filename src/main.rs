mod catalog;
mod cli;
mod completion;
mod config;
mod git;
mod interactive;
mod launcher;
mod operations;
mod paths;
mod shortcuts;

use anyhow::{Context, Result, bail};
use clap::Parser;

use cli::{CatalogCommands, Cli, Commands, RotCommands, ShortcutCommands};
use interactive::Prompt;
use paths::Paths;

fn main() {
    completion::complete();
    if let Err(error) = run() {
        eprintln!("error: {error:#}");
        if let Some(exit) = error.downcast_ref::<launcher::ChildExit>() {
            std::process::exit(exit.code());
        }
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();
    if let Commands::Rot {
        command: RotCommands::Complete { words },
    } = &cli.command
    {
        return completion::rot_complete(words);
    }
    let paths = Paths::discover()?;
    match cli.command {
        Commands::Add {
            name,
            git_url,
            revision,
            catalog,
            commit,
            push,
        } => run_tool_add(&paths, name, git_url, revision, catalog, commit, push),
        Commands::Pull { name, catalog } => {
            run_tool_named(&paths, name, catalog, "pull", operations::tool_pull)
        }
        Commands::Update { name, catalog } => {
            run_tool_named(&paths, name, catalog, "update", operations::tool_update)
        }
        Commands::List => operations::tool_list(&paths),
        Commands::Path { name, catalog } => {
            run_tool_named(&paths, name, catalog, "path", operations::tool_path)
        }
        Commands::Status { name, catalog } => {
            run_tool_named(&paths, name, catalog, "status", operations::tool_status)
        }
        Commands::Run { shortcut } => match shortcut {
            Some(name) => launcher::run_shortcut(&paths, &name),
            None => {
                require_interactive("run", "SHORTCUT")?;
                let mut prompt = interactive::TerminalPrompt;
                launcher::run_interactive(&paths, &mut prompt)
            }
        },
        Commands::Shortcut {
            command: ShortcutCommands::Add,
        } => {
            require_interactive("shortcut add", "")?;
            let mut prompt = interactive::TerminalPrompt;
            launcher::add_shortcut(&paths, &mut prompt)
        }
        Commands::Catalog { command } => run_catalog(&paths, command),
        Commands::Rot { .. } => unreachable!(),
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
