use std::ffi::OsStr;
use std::path::Path;

use clap::CommandFactory;
use clap_complete::CompleteEnv;
use clap_complete::engine::CompletionCandidate;

use crate::cli::Cli;
use crate::paths::Paths;
use crate::shortcuts;

pub fn complete() {
    CompleteEnv::with_factory(Cli::command).complete();
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
}
