use std::ffi::OsStr;
use std::io;
use std::path::Path;

use anyhow::Result;
use clap::CommandFactory;
use clap_complete::CompleteEnv;
use clap_complete::engine::CompletionCandidate;

use crate::cli::Cli;
use crate::paths::Paths;
use crate::shortcuts;

pub fn complete() {
    CompleteEnv::with_factory(Cli::command).complete();
}

pub fn rot_complete(words: &[String]) -> Result<()> {
    let candidates = rot_candidates(words);
    serde_json::to_writer(io::stdout().lock(), &candidates)?;
    println!();
    Ok(())
}

fn rot_candidates(words: &[String]) -> Vec<String> {
    let Some((current, completed)) = words.split_last() else {
        return Vec::new();
    };
    let mut command = Cli::command();
    let mut path = Vec::new();
    for word in completed {
        let Some(subcommand) = command
            .get_subcommands()
            .find(|subcommand| subcommand.get_name() == word)
            .cloned()
        else {
            return Vec::new();
        };
        path.push(word.as_str());
        command = subcommand;
    }

    let mut candidates = if path == ["run"] {
        shortcut_names()
    } else {
        command
            .get_subcommands()
            .filter(|subcommand| !subcommand.is_hide_set())
            .map(|subcommand| subcommand.get_name().to_owned())
            .collect()
    };
    candidates.retain(|candidate| candidate.starts_with(current));
    candidates.sort();
    candidates.dedup();
    candidates
}

fn shortcut_names() -> Vec<String> {
    let Ok(paths) = Paths::discover() else {
        return Vec::new();
    };
    let Ok(path) = paths.shortcuts() else {
        return Vec::new();
    };
    shortcuts::shortcut_names(&path).unwrap_or_default()
}

pub fn shortcut_candidates(current: &OsStr) -> Vec<CompletionCandidate> {
    let Ok(paths) = Paths::discover() else {
        return Vec::new();
    };
    let Ok(path) = paths.shortcuts() else {
        return Vec::new();
    };
    shortcut_candidates_from(&path, current)
}

fn shortcut_candidates_from(path: &Path, current: &OsStr) -> Vec<CompletionCandidate> {
    let Some(current) = current.to_str() else {
        return Vec::new();
    };
    shortcuts::shortcut_names(path)
        .unwrap_or_default()
        .into_iter()
        .filter(|name| name.starts_with(current))
        .map(CompletionCandidate::new)
        .collect()
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    #[test]
    fn candidates_are_only_matching_shortcut_names() {
        let temporary = tempfile::TempDir::new().unwrap();
        let shortcuts = temporary.path().join("shortcuts.toml");
        fs::write(
            &shortcuts,
            r#"version = 1

[shortcuts.print-strings]
catalog = "personal"
tool = "re-toolkit"
path = "recipes/print_strings.py"

[shortcuts.bn-triage]
catalog = "personal"
tool = "re-toolkit"
path = "triage.py"
"#,
        )
        .unwrap();
        fs::write(temporary.path().join("arbitrary-tool-file.py"), "").unwrap();

        let all: Vec<_> = shortcut_candidates_from(&shortcuts, OsStr::new(""))
            .into_iter()
            .map(|candidate| candidate.get_value().to_owned())
            .collect();
        assert_eq!(all, [OsStr::new("bn-triage"), OsStr::new("print-strings")]);

        let partial: Vec<_> = shortcut_candidates_from(&shortcuts, OsStr::new("pri"))
            .into_iter()
            .map(|candidate| candidate.get_value().to_owned())
            .collect();
        assert_eq!(partial, [OsStr::new("print-strings")]);
        assert!(!all.contains(&OsStr::new("re-toolkit").to_owned()));
        assert!(!all.contains(&OsStr::new("arbitrary-tool-file.py").to_owned()));
    }

    #[test]
    fn rot_candidates_follow_the_clap_command_tree() {
        assert_eq!(
            rot_candidates(&[String::new()]),
            [
                "add", "catalog", "list", "path", "pull", "run", "shortcut", "status", "update"
            ]
        );
        assert_eq!(rot_candidates(&["p".to_owned()]), ["path", "pull"]);
        assert_eq!(
            rot_candidates(&["catalog".to_owned(), String::new()]),
            ["add", "list", "migrate", "path", "status", "sync"]
        );
        assert_eq!(
            rot_candidates(&["shortcut".to_owned(), String::new()]),
            ["add"]
        );
    }
}
