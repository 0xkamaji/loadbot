mod cli;
mod config;
mod git;
mod interactive;
mod paths;

use std::fs;
use std::path::Path;

use anyhow::{Context, Result, bail};
use clap::Parser;

use cli::{Cli, Commands};
use config::{Config, ToolConfig};
use paths::Paths;

fn main() {
    if let Err(error) = run() {
        eprintln!("error: {error:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();
    let paths = Paths::discover()?;

    match cli.command {
        Commands::Add {
            name,
            git_url,
            revision,
        } => run_add(&paths, name, git_url, revision),
        Commands::Pull { name } => run_named(&paths, name, "pull", pull),
        Commands::Update { name } => run_named(&paths, name, "update", update),
        Commands::List => list(&paths),
        Commands::Path { name } => run_named(&paths, name, "path", print_path),
        Commands::Status { name } => run_named(&paths, name, "status", status),
    }
}

fn run_add(
    paths: &Paths,
    name: Option<String>,
    url: Option<String>,
    revision: Option<String>,
) -> Result<()> {
    if let (Some(name), Some(url)) = (name.as_deref(), url.as_ref()) {
        return add(paths, name, url.clone(), revision);
    }
    require_interactive("add", "NAME URL")?;

    let mut prompt = interactive::TerminalPrompt;
    let outcome = interactive::run_add_flow(
        &mut prompt,
        name,
        url,
        revision,
        |input| {
            add(
                paths,
                &input.name,
                input.url.clone(),
                input.revision.clone(),
            )
        },
        |name| pull(paths, name),
    )?;
    match outcome {
        interactive::AddFlowOutcome::Cancelled => eprintln!("cancelled"),
        interactive::AddFlowOutcome::RegisteredPullCancelled => {
            eprintln!("pull cancelled; source remains registered");
        }
        interactive::AddFlowOutcome::Registered
        | interactive::AddFlowOutcome::RegisteredAndPulled => {}
    }
    Ok(())
}

fn run_named(
    paths: &Paths,
    name: Option<String>,
    command: &str,
    operation: fn(&Paths, &str) -> Result<()>,
) -> Result<()> {
    if let Some(name) = name {
        return operation(paths, &name);
    }
    require_interactive(command, "NAME")?;

    let config = config::load(&paths.config())?;
    let mut prompt = interactive::TerminalPrompt;
    match interactive::select_tool(&mut prompt, config.tools.keys().cloned().collect())? {
        Some(name) => operation(paths, &name),
        None => {
            eprintln!("cancelled");
            Ok(())
        }
    }
}

fn require_interactive(command: &str, arguments: &str) -> Result<()> {
    if !interactive::terminal_is_interactive() {
        bail!(
            "missing required argument for '{command}'; run it in an interactive terminal or supply: loadbot {command} {arguments}"
        );
    }
    Ok(())
}

fn add(paths: &Paths, name: &str, url: String, revision: Option<String>) -> Result<()> {
    paths::validate_name(name)?;
    if url.is_empty() {
        bail!("Git URL must not be empty");
    }
    if revision.as_deref() == Some("") {
        bail!("revision must not be empty");
    }

    let mut config = config::load(&paths.config())?;
    let tool = ToolConfig::git(url, revision);
    if let Some(existing) = config.tools.get(name) {
        if existing == &tool {
            println!("tool '{name}' is already configured");
            return Ok(());
        }
        bail!("tool '{name}' is already configured with different settings");
    }

    config.tools.insert(name.to_owned(), tool);
    config::save(&paths.config(), &config)?;
    println!("registered tool '{name}'");
    Ok(())
}

fn pull(paths: &Paths, name: &str) -> Result<()> {
    paths::validate_name(name)?;
    let config = config::load(&paths.config())?;
    let tool = configured_tool(&config, name)?;
    let destination = paths.tool(name);

    if path_exists(&destination) {
        if git::is_expected_repository(&destination, &tool.url)? {
            println!("tool '{name}' is already installed");
            return Ok(());
        }
        if git::is_repository(&destination)? {
            bail!("destination exists but is not the configured Git repository");
        }
        bail!("destination exists but is not a Git repository");
    }

    fs::create_dir_all(paths.tools())
        .with_context(|| format!("could not create {}", paths.tools().display()))?;
    if let Err(error) = git::clone_repository(&tool.url, tool.revision.as_deref(), &destination) {
        cleanup_failed_clone(&destination);
        return Err(error).context(format!("could not clone tool '{name}'"));
    }
    println!("installed tool '{name}' at {}", destination.display());
    Ok(())
}

fn update(paths: &Paths, name: &str) -> Result<()> {
    paths::validate_name(name)?;
    let config = config::load(&paths.config())?;
    let tool = configured_tool(&config, name)?;
    let destination = paths.tool(name);

    if !path_exists(&destination) {
        bail!("tool '{name}' is not installed; run 'loadbot pull {name}' first");
    }
    if !git::is_repository(&destination)? {
        bail!("destination exists but is not a Git repository");
    }
    if !git::is_expected_repository(&destination, &tool.url)? {
        bail!("destination is not the configured Git repository");
    }

    let (old_commit, new_commit) = git::update(&destination, tool.revision.as_deref())
        .with_context(|| format!("refusing to update '{name}'"))?;
    if old_commit == new_commit {
        println!("tool '{name}' is already current at {new_commit}");
    } else {
        println!("updated tool '{name}' from {old_commit} to {new_commit}");
    }
    Ok(())
}

fn list(paths: &Paths) -> Result<()> {
    let config = config::load(&paths.config())?;
    if config.tools.is_empty() {
        println!("no tools configured");
        return Ok(());
    }

    println!("NAME\tTYPE\tSTATE\tREVISION");
    for (name, tool) in &config.tools {
        let destination = paths.tool(name);
        let installed = if path_exists(&destination) {
            git::is_expected_repository(&destination, &tool.url)?
        } else {
            false
        };
        println!(
            "{name}\t{}\t{}\t{}",
            tool.source_type.as_str(),
            if installed { "installed" } else { "missing" },
            tool.revision.as_deref().unwrap_or("-")
        );
    }
    Ok(())
}

fn print_path(paths: &Paths, name: &str) -> Result<()> {
    paths::validate_name(name)?;
    let config = config::load(&paths.config())?;
    configured_tool(&config, name)?;
    println!("{}", paths.tool(name).display());
    Ok(())
}

fn status(paths: &Paths, name: &str) -> Result<()> {
    paths::validate_name(name)?;
    let config = config::load(&paths.config())?;
    let tool = configured_tool(&config, name)?;
    let destination = paths.tool(name);

    println!("Name: {name}");
    println!("Path: {}", destination.display());
    println!(
        "Installed: {}",
        if path_exists(&destination) && git::is_expected_repository(&destination, &tool.url)? {
            "yes"
        } else {
            "no"
        }
    );
    println!("Configured URL: {}", tool.url);
    println!(
        "Configured revision: {}",
        tool.revision.as_deref().unwrap_or("(default)")
    );

    if path_exists(&destination) && git::is_repository(&destination)? {
        let repository = git::status(&destination)?;
        println!(
            "Current branch: {}",
            repository.branch.as_deref().unwrap_or("(detached)")
        );
        println!("Current commit: {}", repository.commit);
        println!(
            "Working tree: {}",
            if repository.dirty { "dirty" } else { "clean" }
        );
        println!(
            "Origin URL: {}",
            repository.origin.as_deref().unwrap_or("(none)")
        );
    } else {
        println!("Current branch: -");
        println!("Current commit: -");
        println!("Working tree: -");
        println!("Origin URL: -");
    }
    Ok(())
}

fn configured_tool<'a>(config: &'a Config, name: &str) -> Result<&'a ToolConfig> {
    config
        .tools
        .get(name)
        .with_context(|| format!("tool '{name}' is not configured"))
}

fn cleanup_failed_clone(destination: &Path) {
    let Ok(metadata) = fs::symlink_metadata(destination) else {
        return;
    };
    if metadata.is_dir() && !metadata.file_type().is_symlink() {
        let _ = fs::remove_dir_all(destination);
    } else {
        let _ = fs::remove_file(destination);
    }
}

fn path_exists(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok()
}
