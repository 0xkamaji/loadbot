use std::io::{self, IsTerminal, Write};

use anyhow::{Context, Result, bail};

use crate::paths;

#[derive(Debug, PartialEq, Eq)]
pub struct AddInput {
    pub name: String,
    pub url: String,
    pub revision: Option<String>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum AddFlowOutcome {
    Cancelled,
    Registered,
    RegisteredPullCancelled,
    RegisteredAndPulled,
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
                write_stderr("Enter the number of a configured tool.\n")?;
                continue;
            };
            if let Some(choice) = index.checked_sub(1).and_then(|index| choices.get(index)) {
                return Ok(Some(choice.clone()));
            }
            write_stderr("Enter the number of a configured tool.\n")?;
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

pub fn infer_name(url: &str) -> Option<String> {
    let url = url.trim().trim_end_matches('/');
    let component = url
        .rsplit(['/', '\\', ':'])
        .next()
        .unwrap_or(url)
        .strip_suffix(".git")
        .unwrap_or_else(|| url.rsplit(['/', '\\', ':']).next().unwrap_or(url));
    paths::validate_name(component).ok()?;
    Some(component.to_owned())
}

pub fn sorted_choices<I>(names: I) -> Vec<String>
where
    I: IntoIterator<Item = String>,
{
    let mut names: Vec<_> = names.into_iter().collect();
    names.sort();
    names
}

pub fn select_tool<P: Prompt>(prompt: &mut P, names: Vec<String>) -> Result<Option<String>> {
    let choices = sorted_choices(names);
    if choices.is_empty() {
        bail!("No tools are configured.\nRun 'loadbot add' to add one.");
    }
    prompt.select("Select a tool:\n", &choices)
}

pub fn run_add_flow<P, A, L>(
    prompt: &mut P,
    supplied_name: Option<String>,
    supplied_url: Option<String>,
    supplied_revision: Option<String>,
    register: A,
    pull: L,
) -> Result<AddFlowOutcome>
where
    P: Prompt,
    A: FnOnce(&AddInput) -> Result<()>,
    L: FnOnce(&str) -> Result<()>,
{
    let Some(input) = collect_add_input(prompt, supplied_name, supplied_url, supplied_revision)?
    else {
        return Ok(AddFlowOutcome::Cancelled);
    };

    register(&input)?;
    match prompt.confirm("Pull it now?", true)? {
        Some(true) => {
            pull(&input.name).context("source was registered, but pulling it now failed")?;
            Ok(AddFlowOutcome::RegisteredAndPulled)
        }
        Some(false) => Ok(AddFlowOutcome::Registered),
        None => Ok(AddFlowOutcome::RegisteredPullCancelled),
    }
}

fn collect_add_input<P: Prompt>(
    prompt: &mut P,
    supplied_name: Option<String>,
    supplied_url: Option<String>,
    supplied_revision: Option<String>,
) -> Result<Option<AddInput>> {
    let url = match supplied_url {
        Some(url) => url,
        None => loop {
            let Some(url) = prompt.input("Git repository URL:", None)? else {
                return Ok(None);
            };
            if !url.is_empty() {
                break url;
            }
            prompt.message("Git repository URL must not be empty.")?;
        },
    };

    let name = match supplied_name {
        Some(name) => {
            paths::validate_name(&name)?;
            name
        }
        None => {
            let inferred = infer_name(&url);
            loop {
                let label = inferred
                    .as_deref()
                    .map_or_else(|| "Name:".to_owned(), |name| format!("Name [{name}]:"));
                let Some(name) = prompt.input(&label, inferred.as_deref())? else {
                    return Ok(None);
                };
                match paths::validate_name(&name) {
                    Ok(()) => break name,
                    Err(error) => prompt.message(&format!("{error:#}"))?,
                }
            }
        }
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

    let revision_summary = revision.as_deref().unwrap_or("remote default");
    prompt.message(&format!(
        "\nAdd this source?\n\n  Name:      {name}\n  URL:       {url}\n  Revision:  {revision_summary}\n"
    ))?;
    match prompt.confirm("Proceed?", true)? {
        Some(true) => Ok(Some(AddInput {
            name,
            url,
            revision,
        })),
        Some(false) | None => Ok(None),
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
    use std::fs;

    use tempfile::TempDir;

    use super::*;

    #[derive(Default)]
    struct FakePrompt {
        inputs: VecDeque<Option<String>>,
        confirmations: VecDeque<Option<bool>>,
        selection: Option<String>,
        messages: Vec<String>,
    }

    impl Prompt for FakePrompt {
        fn input(&mut self, _label: &str, default: Option<&str>) -> Result<Option<String>> {
            Ok(self.inputs.pop_front().flatten().map(|value| {
                if value.is_empty() {
                    default.unwrap_or_default().to_owned()
                } else {
                    value
                }
            }))
        }

        fn confirm(&mut self, _label: &str, _default: bool) -> Result<Option<bool>> {
            Ok(self.confirmations.pop_front().flatten())
        }

        fn select(&mut self, _label: &str, _choices: &[String]) -> Result<Option<String>> {
            Ok(self.selection.take())
        }

        fn message(&mut self, message: &str) -> Result<()> {
            self.messages.push(message.to_owned());
            Ok(())
        }
    }

    fn add_inputs() -> VecDeque<Option<String>> {
        [
            Some("https://example.test/demo.git".to_owned()),
            Some(String::new()),
            Some(String::new()),
        ]
        .into()
    }

    #[test]
    fn infers_names_from_common_git_urls() {
        assert_eq!(
            infer_name("https://github.com/0xkamaji/rot-tools.git"),
            Some("rot-tools".to_owned())
        );
        assert_eq!(
            infer_name("git@github.com:0xkamaji/rot-tools.git"),
            Some("rot-tools".to_owned())
        );
        assert_eq!(
            infer_name("ssh://git@github.com/0xkamaji/rot-tools.git"),
            Some("rot-tools".to_owned())
        );
        assert_eq!(
            infer_name("/path/to/local/repository.git"),
            Some("repository".to_owned())
        );
    }

    #[test]
    fn inference_removes_a_trailing_slash_and_git_suffix() {
        assert_eq!(
            infer_name("https://github.com/owner/project.git/"),
            Some("project".to_owned())
        );
        assert_eq!(
            infer_name("https://github.com/owner/project/"),
            Some("project".to_owned())
        );
    }

    #[test]
    fn inference_rejects_unsafe_or_absent_names() {
        assert_eq!(infer_name("https://example.test/..git"), None);
        assert_eq!(infer_name("https://example.test/bad%name.git"), None);
        assert_eq!(infer_name("https://example.test/.git"), None);
        assert_eq!(infer_name("/"), None);
    }

    #[test]
    fn tty_detection_requires_both_streams() {
        assert!(is_interactive(true, true));
        assert!(!is_interactive(true, false));
        assert!(!is_interactive(false, true));
        assert!(!is_interactive(false, false));
    }

    #[test]
    fn choices_are_sorted_by_name() {
        assert_eq!(
            sorted_choices(["third", "first", "second"].map(str::to_owned)),
            ["first", "second", "third"]
        );
    }

    #[test]
    fn declining_add_confirmation_performs_no_operation() {
        let temporary = TempDir::new().unwrap();
        let config = temporary.path().join("config.toml");
        let installed = temporary.path().join("installed");
        let mut prompt = FakePrompt {
            inputs: add_inputs(),
            confirmations: [Some(false)].into(),
            ..FakePrompt::default()
        };

        let outcome = run_add_flow(
            &mut prompt,
            None,
            None,
            None,
            |_| {
                fs::write(&config, "registered")?;
                Ok(())
            },
            |_| {
                fs::write(&installed, "pulled")?;
                Ok(())
            },
        )
        .unwrap();

        assert_eq!(outcome, AddFlowOutcome::Cancelled);
        assert!(!config.exists());
        assert!(!installed.exists());
    }

    #[test]
    fn declining_pull_registers_without_installing() {
        let temporary = TempDir::new().unwrap();
        let config = temporary.path().join("config.toml");
        let installed = temporary.path().join("installed");
        let mut prompt = FakePrompt {
            inputs: add_inputs(),
            confirmations: [Some(true), Some(false)].into(),
            ..FakePrompt::default()
        };

        let outcome = run_add_flow(
            &mut prompt,
            None,
            None,
            None,
            |_| {
                fs::write(&config, "registered")?;
                Ok(())
            },
            |_| {
                fs::write(&installed, "pulled")?;
                Ok(())
            },
        )
        .unwrap();

        assert_eq!(outcome, AddFlowOutcome::Registered);
        assert!(config.is_file());
        assert!(!installed.exists());
    }

    #[test]
    fn accepting_pull_runs_registration_then_pull() {
        let temporary = TempDir::new().unwrap();
        let config = temporary.path().join("config.toml");
        let installed = temporary.path().join("installed");
        let mut prompt = FakePrompt {
            inputs: add_inputs(),
            confirmations: [Some(true), Some(true)].into(),
            ..FakePrompt::default()
        };

        let outcome = run_add_flow(
            &mut prompt,
            None,
            None,
            None,
            |_| {
                fs::write(&config, "registered")?;
                Ok(())
            },
            |_| {
                assert!(config.is_file());
                fs::write(&installed, "pulled")?;
                Ok(())
            },
        )
        .unwrap();

        assert_eq!(outcome, AddFlowOutcome::RegisteredAndPulled);
        assert!(config.is_file());
        assert!(installed.is_file());
    }

    #[test]
    fn cancellation_does_not_perform_the_pending_operation() {
        let temporary = TempDir::new().unwrap();
        let marker = temporary.path().join("operation");
        let mut prompt = FakePrompt {
            inputs: [None].into(),
            ..FakePrompt::default()
        };

        let outcome = run_add_flow(
            &mut prompt,
            None,
            None,
            None,
            |_| {
                fs::write(&marker, "registered")?;
                Ok(())
            },
            |_| Ok(()),
        )
        .unwrap();

        assert_eq!(outcome, AddFlowOutcome::Cancelled);
        assert!(!marker.exists());
    }

    #[test]
    fn cancellation_after_registration_does_not_pull() {
        let temporary = TempDir::new().unwrap();
        let config = temporary.path().join("config.toml");
        let installed = temporary.path().join("installed");
        let mut prompt = FakePrompt {
            inputs: add_inputs(),
            confirmations: [Some(true), None].into(),
            ..FakePrompt::default()
        };

        let outcome = run_add_flow(
            &mut prompt,
            None,
            None,
            None,
            |_| {
                fs::write(&config, "registered")?;
                Ok(())
            },
            |_| {
                fs::write(&installed, "pulled")?;
                Ok(())
            },
        )
        .unwrap();

        assert_eq!(outcome, AddFlowOutcome::RegisteredPullCancelled);
        assert!(config.is_file());
        assert!(!installed.exists());
    }

    #[test]
    fn empty_tool_selection_is_actionable_and_cancellation_is_clean() {
        let mut prompt = FakePrompt::default();
        let error = select_tool(&mut prompt, Vec::new()).unwrap_err();
        assert_eq!(
            error.to_string(),
            "No tools are configured.\nRun 'loadbot add' to add one."
        );

        assert_eq!(
            select_tool(&mut prompt, vec!["demo".to_owned()]).unwrap(),
            None
        );
    }
}
