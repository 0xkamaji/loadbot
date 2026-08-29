use std::io::{self, IsTerminal, Write};

use anyhow::{Context, Result, bail};

use crate::paths;

#[derive(Debug, PartialEq, Eq)]
pub struct CatalogAddInput {
    pub name: String,
    pub url: String,
    pub writable: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CatalogMenuAction {
    UseKamajiCatalog,
    AddExisting,
    Initialize,
    List,
    Sync,
    Status,
    Path,
}

#[derive(Debug, PartialEq, Eq)]
pub struct CatalogInitializeInput {
    pub catalog: CatalogAddInput,
    pub commit: bool,
    pub push: bool,
}

const KAMAJI_CATALOG_NAME: &str = "personal";
const KAMAJI_CATALOG_URL: &str = "https://github.com/0xkamaji/loadbot-catalog.git";

pub fn collect_catalog_menu<P: Prompt>(prompt: &mut P) -> Result<Option<CatalogMenuAction>> {
    let choices = [
        "Use 0xkamaji's catalog",
        "Add an existing catalog",
        "Create or initialize a catalog",
        "List catalogs",
        "Sync a catalog",
        "Show catalog status",
        "Show catalog path",
        "Cancel",
    ]
    .map(str::to_owned);
    let Some(selection) = prompt.select("Catalog setup and management:", &choices)? else {
        return Ok(None);
    };
    Ok(match selection.as_str() {
        "Use 0xkamaji's catalog" => Some(CatalogMenuAction::UseKamajiCatalog),
        "Add an existing catalog" => Some(CatalogMenuAction::AddExisting),
        "Create or initialize a catalog" => Some(CatalogMenuAction::Initialize),
        "List catalogs" => Some(CatalogMenuAction::List),
        "Sync a catalog" => Some(CatalogMenuAction::Sync),
        "Show catalog status" => Some(CatalogMenuAction::Status),
        "Show catalog path" => Some(CatalogMenuAction::Path),
        "Cancel" => None,
        _ => bail!("invalid catalog menu selection"),
    })
}

pub fn confirm_kamaji_catalog<P: Prompt>(prompt: &mut P) -> Result<Option<CatalogAddInput>> {
    prompt.message(&format!(
        "\nUse this catalog?\n\n  Name:      {KAMAJI_CATALOG_NAME}\n  URL:       {KAMAJI_CATALOG_URL}\n  Writable:  yes\n"
    ))?;
    if prompt.confirm("Proceed?", true)? != Some(true) {
        return Ok(None);
    }
    Ok(Some(CatalogAddInput {
        name: KAMAJI_CATALOG_NAME.to_owned(),
        url: KAMAJI_CATALOG_URL.to_owned(),
        writable: true,
    }))
}

pub fn collect_catalog_initialize<P: Prompt>(
    prompt: &mut P,
) -> Result<Option<CatalogInitializeInput>> {
    let Some(name) = prompt_name(prompt, "Catalog name:", None)? else {
        return Ok(None);
    };
    let Some(url) = prompt_nonempty(prompt, "Existing empty Git repository URL:", None)? else {
        return Ok(None);
    };
    let Some(writable) = prompt.confirm("Writable?", true)? else {
        return Ok(None);
    };
    if !writable {
        bail!("a catalog initialized by Loadbot must be writable");
    }
    prompt.message(&format!(
        "\nInitialize this catalog?\n\n  Name:      {name}\n  URL:       {url}\n  Writable:  yes\n"
    ))?;
    if prompt.confirm("Proceed?", true)? != Some(true) {
        return Ok(None);
    }
    prompt.message(
        "Loadbot will only initialize an existing empty Git remote. It will create catalog.toml, but it will not commit or push unless you confirm each action.",
    )?;
    let Some(commit) = prompt.confirm("Commit the initial catalog.toml?", false)? else {
        return Ok(None);
    };
    let push = if commit {
        let Some(push) = prompt.confirm("Push the initial catalog commit?", false)? else {
            return Ok(None);
        };
        push
    } else {
        false
    };
    Ok(Some(CatalogInitializeInput {
        catalog: CatalogAddInput {
            name,
            url,
            writable,
        },
        commit,
        push,
    }))
}

#[derive(Debug, PartialEq, Eq)]
pub struct ToolAddInput {
    pub name: String,
    pub url: String,
    pub revision: Option<String>,
    pub catalog: String,
    pub commit: bool,
    pub push: bool,
}

pub trait Prompt {
    fn input(&mut self, label: &str, default: Option<&str>) -> Result<Option<String>>;
    fn confirm(&mut self, label: &str, default: bool) -> Result<Option<bool>>;
    fn select(&mut self, label: &str, choices: &[String]) -> Result<Option<String>>;
    fn message(&mut self, message: &str) -> Result<()>;
}

pub struct TerminalPrompt;

impl Prompt for TerminalPrompt {
    fn input(&mut self, label: &str, default: Option<&str>) -> Result<Option<String>> {
        write_stderr(&format!("{label}\n> "))?;
        let Some(value) = read_line()? else {
            return Ok(None);
        };
        let value = value.trim().to_owned();
        if value.is_empty()
            && let Some(default) = default
        {
            return Ok(Some(default.to_owned()));
        }
        Ok(Some(value))
    }

    fn confirm(&mut self, label: &str, default: bool) -> Result<Option<bool>> {
        let hint = if default { "Y/n" } else { "y/N" };
        loop {
            write_stderr(&format!("{label} [{hint}]:\n> "))?;
            let Some(value) = read_line()? else {
                return Ok(None);
            };
            match value.trim().to_ascii_lowercase().as_str() {
                "" => return Ok(Some(default)),
                "y" | "yes" => return Ok(Some(true)),
                "n" | "no" => return Ok(Some(false)),
                "q" | "quit" | "cancel" => return Ok(None),
                _ => write_stderr("Please answer yes or no.\n")?,
            }
        }
    }

    fn select(&mut self, label: &str, choices: &[String]) -> Result<Option<String>> {
        self.message(label)?;
        for (index, choice) in choices.iter().enumerate() {
            self.message(&format!("  {}. {choice}", index + 1))?;
        }
        loop {
            write_stderr("\nSelection:\n> ")?;
            let Some(value) = read_line()? else {
                return Ok(None);
            };
            if matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "q" | "quit" | "cancel"
            ) {
                return Ok(None);
            }
            let Ok(index) = value.trim().parse::<usize>() else {
                write_stderr("Enter the number of an available choice.\n")?;
                continue;
            };
            if let Some(choice) = index.checked_sub(1).and_then(|index| choices.get(index)) {
                return Ok(Some(choice.clone()));
            }
            write_stderr("Enter the number of an available choice.\n")?;
        }
    }

    fn message(&mut self, message: &str) -> Result<()> {
        write_stderr(message)?;
        write_stderr("\n")
    }
}

pub fn terminal_is_interactive() -> bool {
    is_interactive(io::stdin().is_terminal(), io::stdout().is_terminal())
}

pub fn is_interactive(stdin_is_terminal: bool, stdout_is_terminal: bool) -> bool {
    stdin_is_terminal && stdout_is_terminal
}

pub fn collect_catalog_add<P: Prompt>(
    prompt: &mut P,
    supplied_name: Option<String>,
    supplied_url: Option<String>,
    writable_default: bool,
) -> Result<Option<CatalogAddInput>> {
    let Some(name) = prompt_name(prompt, "Catalog name:", supplied_name)? else {
        return Ok(None);
    };
    let Some(url) = prompt_nonempty(prompt, "Git repository URL:", supplied_url)? else {
        return Ok(None);
    };
    let writable = if writable_default {
        true
    } else {
        let Some(writable) = prompt.confirm("Writable?", false)? else {
            return Ok(None);
        };
        writable
    };
    prompt.message(&format!(
        "\nAdd this catalog?\n\n  Name:      {name}\n  URL:       {url}\n  Writable:  {}\n",
        if writable { "yes" } else { "no" }
    ))?;
    if prompt.confirm("Proceed?", true)? != Some(true) {
        return Ok(None);
    }
    Ok(Some(CatalogAddInput {
        name,
        url,
        writable,
    }))
}

#[allow(clippy::too_many_arguments)]
pub fn collect_tool_add<P: Prompt>(
    prompt: &mut P,
    supplied_name: Option<String>,
    supplied_url: Option<String>,
    supplied_revision: Option<String>,
    supplied_catalog: Option<String>,
    writable_catalogs: &[String],
    commit_default: bool,
    push_default: bool,
) -> Result<Option<ToolAddInput>> {
    let Some(name) = prompt_name(prompt, "Tool name:", supplied_name)? else {
        return Ok(None);
    };
    let Some(url) = prompt_nonempty(prompt, "Git repository URL:", supplied_url)? else {
        return Ok(None);
    };
    let revision = match supplied_revision {
        Some(revision) => Some(revision),
        None => {
            let Some(revision) = prompt.input("Revision [remote default]:", None)? else {
                return Ok(None);
            };
            (!revision.is_empty()).then_some(revision)
        }
    };
    if writable_catalogs.is_empty() {
        bail!("No writable catalogs are configured.\nRun 'loadbot catalog add' to add one.");
    }
    let catalog = match supplied_catalog {
        Some(catalog) => {
            if !writable_catalogs.contains(&catalog) {
                bail!("catalog '{catalog}' is not configured as writable");
            }
            catalog
        }
        None => {
            let Some(catalog) = prompt.select("Select a writable catalog:\n", writable_catalogs)?
            else {
                return Ok(None);
            };
            catalog
        }
    };
    prompt.message(&format!(
        "\nAdd this tool?\n\n  Name:      {name}\n  URL:       {url}\n  Revision:  {}\n  Catalog:   {catalog}\n",
        revision.as_deref().unwrap_or("remote default")
    ))?;
    if prompt.confirm("Proceed?", true)? != Some(true) {
        return Ok(None);
    }
    let commit = if commit_default {
        true
    } else {
        let Some(commit) = prompt.confirm("Commit the catalog change?", false)? else {
            return Ok(None);
        };
        commit
    };
    let push = if commit {
        if push_default {
            true
        } else {
            let Some(push) = prompt.confirm("Push the catalog change?", false)? else {
                return Ok(None);
            };
            push
        }
    } else {
        false
    };
    Ok(Some(ToolAddInput {
        name,
        url,
        revision,
        catalog,
        commit,
        push,
    }))
}

fn prompt_name<P: Prompt>(
    prompt: &mut P,
    label: &str,
    supplied: Option<String>,
) -> Result<Option<String>> {
    if let Some(name) = supplied {
        paths::validate_name(&name)?;
        return Ok(Some(name));
    }
    loop {
        let Some(name) = prompt.input(label, None)? else {
            return Ok(None);
        };
        match paths::validate_name(&name) {
            Ok(()) => return Ok(Some(name)),
            Err(error) => prompt.message(&format!("{error:#}"))?,
        }
    }
}

fn prompt_nonempty<P: Prompt>(
    prompt: &mut P,
    label: &str,
    supplied: Option<String>,
) -> Result<Option<String>> {
    if let Some(value) = supplied {
        if value.is_empty() {
            bail!("Git URL must not be empty");
        }
        return Ok(Some(value));
    }
    loop {
        let Some(value) = prompt.input(label, None)? else {
            return Ok(None);
        };
        if !value.is_empty() {
            return Ok(Some(value));
        }
        prompt.message("Git URL must not be empty.")?;
    }
}

fn read_line() -> Result<Option<String>> {
    let mut value = String::new();
    match io::stdin().read_line(&mut value) {
        Ok(0) => Ok(None),
        Ok(_) => Ok(Some(value)),
        Err(error) if error.kind() == io::ErrorKind::Interrupted => Ok(None),
        Err(error) => Err(error).context("could not read terminal input"),
    }
}

fn write_stderr(message: &str) -> Result<()> {
    let mut stderr = io::stderr().lock();
    stderr
        .write_all(message.as_bytes())
        .context("could not write terminal prompt")?;
    stderr.flush().context("could not flush terminal prompt")
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;

    use super::*;

    #[derive(Default)]
    struct FakePrompt {
        inputs: VecDeque<Option<String>>,
        confirmations: VecDeque<Option<bool>>,
        selections: VecDeque<Option<String>>,
        messages: Vec<String>,
        selection_requests: Vec<(String, Vec<String>)>,
    }

    impl Prompt for FakePrompt {
        fn input(&mut self, _label: &str, _default: Option<&str>) -> Result<Option<String>> {
            Ok(self.inputs.pop_front().flatten())
        }

        fn confirm(&mut self, _label: &str, _default: bool) -> Result<Option<bool>> {
            Ok(self.confirmations.pop_front().flatten())
        }

        fn select(&mut self, label: &str, choices: &[String]) -> Result<Option<String>> {
            self.selection_requests
                .push((label.to_owned(), choices.to_vec()));
            Ok(self.selections.pop_front().flatten())
        }

        fn message(&mut self, message: &str) -> Result<()> {
            self.messages.push(message.to_owned());
            Ok(())
        }
    }

    #[test]
    fn tty_detection_requires_both_streams() {
        assert!(is_interactive(true, true));
        assert!(!is_interactive(true, false));
        assert!(!is_interactive(false, true));
    }

    #[test]
    fn catalog_flow_is_mockable_and_cancellable() {
        let mut prompt = FakePrompt {
            inputs: [Some("personal".to_owned()), Some("local.git".to_owned())].into(),
            confirmations: [Some(true)].into(),
            ..FakePrompt::default()
        };
        let input = collect_catalog_add(&mut prompt, None, None, true)
            .unwrap()
            .unwrap();
        assert_eq!(input.name, "personal");
        assert!(input.writable);

        let mut cancelled = FakePrompt {
            inputs: [None].into(),
            ..FakePrompt::default()
        };
        assert_eq!(
            collect_catalog_add(&mut cancelled, None, None, false).unwrap(),
            None
        );
    }

    #[test]
    fn bare_catalog_menu_lists_onboarding_and_management_choices() {
        let mut prompt = FakePrompt {
            selections: [Some("Use 0xkamaji's catalog".to_owned())].into(),
            ..FakePrompt::default()
        };
        assert_eq!(
            collect_catalog_menu(&mut prompt).unwrap(),
            Some(CatalogMenuAction::UseKamajiCatalog)
        );
        assert_eq!(
            prompt.selection_requests[0].1,
            [
                "Use 0xkamaji's catalog",
                "Add an existing catalog",
                "Create or initialize a catalog",
                "List catalogs",
                "Sync a catalog",
                "Show catalog status",
                "Show catalog path",
                "Cancel",
            ]
        );

        let mut cancelled = FakePrompt {
            selections: [Some("Cancel".to_owned())].into(),
            ..FakePrompt::default()
        };
        assert_eq!(collect_catalog_menu(&mut cancelled).unwrap(), None);
    }

    #[test]
    fn kamaji_preset_requires_confirmation_and_returns_exact_values() {
        let mut confirmed = FakePrompt {
            confirmations: [Some(true)].into(),
            ..FakePrompt::default()
        };
        assert_eq!(
            confirm_kamaji_catalog(&mut confirmed).unwrap(),
            Some(CatalogAddInput {
                name: "personal".to_owned(),
                url: "https://github.com/0xkamaji/loadbot-catalog.git".to_owned(),
                writable: true,
            })
        );
        assert!(confirmed.messages[0].contains("Writable:  yes"));

        let mut refused = FakePrompt {
            confirmations: [Some(false)].into(),
            ..FakePrompt::default()
        };
        assert_eq!(confirm_kamaji_catalog(&mut refused).unwrap(), None);
    }

    #[test]
    fn catalog_initialize_collects_separate_commit_and_push_choices() {
        let mut prompt = FakePrompt {
            inputs: [Some("new".to_owned()), Some("new.git".to_owned())].into(),
            confirmations: [Some(true), Some(true), Some(true), Some(false)].into(),
            ..FakePrompt::default()
        };
        let input = collect_catalog_initialize(&mut prompt).unwrap().unwrap();
        assert!(input.catalog.writable);
        assert!(input.commit);
        assert!(!input.push);
        assert!(prompt.messages.iter().any(|message| {
            message.contains("will not commit or push unless you confirm each action")
        }));
    }

    #[test]
    fn catalog_initialize_cancellation_returns_before_any_operation() {
        let mut proceed_refused = FakePrompt {
            inputs: [Some("new".to_owned()), Some("new.git".to_owned())].into(),
            confirmations: [Some(true), Some(false)].into(),
            ..FakePrompt::default()
        };
        assert_eq!(
            collect_catalog_initialize(&mut proceed_refused).unwrap(),
            None
        );

        let mut commit_cancelled = FakePrompt {
            inputs: [Some("new".to_owned()), Some("new.git".to_owned())].into(),
            confirmations: [Some(true), Some(true), None].into(),
            ..FakePrompt::default()
        };
        assert_eq!(
            collect_catalog_initialize(&mut commit_cancelled).unwrap(),
            None
        );

        let mut push_cancelled = FakePrompt {
            inputs: [Some("new".to_owned()), Some("new.git".to_owned())].into(),
            confirmations: [Some(true), Some(true), Some(true), None].into(),
            ..FakePrompt::default()
        };
        assert_eq!(
            collect_catalog_initialize(&mut push_cancelled).unwrap(),
            None
        );
    }

    #[test]
    fn tool_flow_collects_catalog_commit_and_push_choices() {
        let mut prompt = FakePrompt {
            inputs: [
                Some("demo".to_owned()),
                Some("demo.git".to_owned()),
                Some(String::new()),
            ]
            .into(),
            confirmations: [Some(true), Some(true), Some(false)].into(),
            selections: [Some("personal".to_owned())].into(),
            ..FakePrompt::default()
        };
        let input = collect_tool_add(
            &mut prompt,
            None,
            None,
            None,
            None,
            &["personal".to_owned()],
            false,
            false,
        )
        .unwrap()
        .unwrap();
        assert_eq!(input.catalog, "personal");
        assert!(input.commit);
        assert!(!input.push);
        assert_eq!(input.revision, None);
    }
}
