//! Structured, shell-free recipe definitions and pure invocation resolution.

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::catalog::Runner;
use crate::shortcuts;

pub const RECIPE_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoredInvocation {
    Legacy(LegacyInvocation),
    Recipe(RecipeDefinition),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LegacyInvocation {
    pub path: String,
    pub runner: Option<Runner>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecipeDefinition {
    pub version: u32,
    pub behavior: InvocationBehavior,
    pub program: RecipeProgram,
    pub working_directory: WorkingDirectory,
    #[serde(default)]
    pub arguments: Vec<RecipeArgument>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum InvocationBehavior {
    Run,
    Launch,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case", deny_unknown_fields)]
pub enum RecipeProgram {
    ProjectFile { path: String },
    Interpreter { runner: Runner },
    Executable { name: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case", deny_unknown_fields)]
pub enum WorkingDirectory {
    ProjectRoot,
    TargetParent,
    ProjectRelative { path: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case", deny_unknown_fields)]
pub enum RecipeArgument {
    ProjectPath {
        path: String,
    },
    Literal {
        value: String,
    },
    Input {
        id: String,
        label: String,
        kind: InputKind,
        required: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        default: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        prefix: Option<String>,
    },
    Switch {
        id: String,
        label: String,
        value: String,
        default: bool,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum InputKind {
    Text,
    File,
    Directory,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeInput {
    Text(String),
    File(PathBuf),
    Directory(PathBuf),
    Switch(bool),
}

pub type RuntimeInputs = BTreeMap<String, RuntimeInput>;

/// A direct project file or an ordered set of executable names for PATH lookup.
/// Keeping runner fallbacks explicit preserves existing platform behavior without
/// making pure resolution inspect or spawn programs from the host environment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolvedProgram {
    ProjectFile(PathBuf),
    Interpreter {
        runner: Runner,
        candidates: Vec<OsString>,
    },
    SearchPath {
        candidates: Vec<OsString>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedInvocation {
    pub behavior: InvocationBehavior,
    pub program: ResolvedProgram,
    pub argv: Vec<OsString>,
    pub cwd: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecipeError {
    UnsupportedVersion(u32),
    MissingInvocation,
    MixedInvocation,
    RunnerWithoutLegacyPath,
    InvalidProjectPath {
        field: &'static str,
        value: String,
    },
    InvalidExecutable(String),
    DirectIsNotInterpreter,
    InvalidParameterId(String),
    EmptyLabel(String),
    EmptyPrefix(String),
    EmptySwitchValue(String),
    DuplicateParameterId(String),
    UnknownInput(String),
    MissingRequiredInput(String),
    InputKindMismatch {
        id: String,
        expected: &'static str,
    },
    ProjectRoot(String),
    ProjectEntry {
        path: String,
        expected: &'static str,
    },
    TargetParentWithoutProjectFile,
    RuntimePathMustBeAbsolute(String),
    RuntimePath {
        id: String,
        expected: InputKind,
    },
}

impl fmt::Display for RecipeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedVersion(version) => {
                write!(formatter, "unsupported recipe version {version}")
            }
            Self::MissingInvocation => {
                formatter.write_str("entry must contain either a legacy path or a recipe")
            }
            Self::MixedInvocation => {
                formatter.write_str("entry must not contain both a legacy path and a recipe")
            }
            Self::RunnerWithoutLegacyPath => {
                formatter.write_str("legacy runner requires a legacy path")
            }
            Self::InvalidProjectPath { field, value } => {
                write!(formatter, "invalid {field} project path '{value}'")
            }
            Self::InvalidExecutable(name) => write!(formatter, "invalid executable name '{name}'"),
            Self::DirectIsNotInterpreter => {
                formatter.write_str("direct is not an interpreter; use a project-file program")
            }
            Self::InvalidParameterId(id) => write!(formatter, "invalid recipe parameter id '{id}'"),
            Self::EmptyLabel(id) => write!(formatter, "recipe parameter '{id}' has an empty label"),
            Self::EmptyPrefix(id) => write!(formatter, "recipe input '{id}' has an empty prefix"),
            Self::EmptySwitchValue(id) => {
                write!(formatter, "recipe switch '{id}' has an empty value")
            }
            Self::DuplicateParameterId(id) => {
                write!(formatter, "duplicate recipe parameter id '{id}'")
            }
            Self::UnknownInput(id) => write!(formatter, "unknown runtime input '{id}'"),
            Self::MissingRequiredInput(id) => {
                write!(formatter, "missing required runtime input '{id}'")
            }
            Self::InputKindMismatch { id, expected } => {
                write!(formatter, "runtime input '{id}' must be {expected}")
            }
            Self::ProjectRoot(detail) => {
                write!(formatter, "invalid installed project root: {detail}")
            }
            Self::ProjectEntry { path, expected } => write!(
                formatter,
                "project path '{path}' is missing, escapes the project, or is not {expected}"
            ),
            Self::TargetParentWithoutProjectFile => formatter
                .write_str("target-parent working directory requires a project-file program"),
            Self::RuntimePathMustBeAbsolute(id) => {
                write!(formatter, "runtime path input '{id}' must be absolute")
            }
            Self::RuntimePath { id, expected } => write!(
                formatter,
                "runtime input '{id}' does not resolve to an existing {expected}"
            ),
        }
    }
}

impl std::error::Error for RecipeError {}

impl fmt::Display for InputKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Text => "text",
            Self::File => "file",
            Self::Directory => "directory",
        })
    }
}

impl RecipeDefinition {
    pub fn validate(&self) -> Result<(), RecipeError> {
        if self.version != RECIPE_VERSION {
            return Err(RecipeError::UnsupportedVersion(self.version));
        }
        match &self.program {
            RecipeProgram::ProjectFile { path } => validate_project_path("program", path)?,
            RecipeProgram::Interpreter {
                runner: Runner::Direct,
            } => {
                return Err(RecipeError::DirectIsNotInterpreter);
            }
            RecipeProgram::Interpreter { .. } => {}
            RecipeProgram::Executable { name } => validate_executable(name)?,
        }
        if let WorkingDirectory::ProjectRelative { path } = &self.working_directory {
            validate_project_path("working-directory", path)?;
        }
        if matches!(self.working_directory, WorkingDirectory::TargetParent)
            && !matches!(self.program, RecipeProgram::ProjectFile { .. })
        {
            return Err(RecipeError::TargetParentWithoutProjectFile);
        }

        let mut ids = BTreeSet::new();
        for argument in &self.arguments {
            match argument {
                RecipeArgument::ProjectPath { path } => validate_project_path("argument", path)?,
                RecipeArgument::Literal { .. } => {}
                RecipeArgument::Input {
                    id, label, prefix, ..
                } => {
                    validate_parameter(id, label, &mut ids)?;
                    if prefix.as_deref() == Some("") {
                        return Err(RecipeError::EmptyPrefix(id.clone()));
                    }
                }
                RecipeArgument::Switch {
                    id, label, value, ..
                } => {
                    validate_parameter(id, label, &mut ids)?;
                    if value.is_empty() {
                        return Err(RecipeError::EmptySwitchValue(id.clone()));
                    }
                }
            }
        }
        Ok(())
    }
}

impl StoredInvocation {
    pub fn legacy(path: String, runner: Option<Runner>) -> Self {
        Self::Legacy(LegacyInvocation { path, runner })
    }

    pub fn validate(&self) -> Result<(), RecipeError> {
        match self {
            Self::Legacy(legacy) => validate_project_path("legacy", &legacy.path),
            Self::Recipe(recipe) => recipe.validate(),
        }
    }

    pub fn as_legacy(&self) -> Option<&LegacyInvocation> {
        match self {
            Self::Legacy(legacy) => Some(legacy),
            Self::Recipe(_) => None,
        }
    }

    pub fn as_recipe(&self) -> Option<&RecipeDefinition> {
        match self {
            Self::Legacy(_) => None,
            Self::Recipe(recipe) => Some(recipe),
        }
    }

    pub(crate) fn from_parts(
        path: Option<String>,
        runner: Option<Runner>,
        recipe: Option<RecipeDefinition>,
    ) -> Result<Self, RecipeError> {
        match (path, runner, recipe) {
            (Some(path), runner, None) => Ok(Self::legacy(path, runner)),
            (None, None, Some(recipe)) => Ok(Self::Recipe(recipe)),
            (Some(_), _, Some(_)) => Err(RecipeError::MixedInvocation),
            (None, Some(_), None) => Err(RecipeError::RunnerWithoutLegacyPath),
            (None, Some(_), Some(_)) => Err(RecipeError::MixedInvocation),
            (None, None, None) => Err(RecipeError::MissingInvocation),
        }
    }

    pub(crate) fn parts(&self) -> (Option<String>, Option<Runner>, Option<RecipeDefinition>) {
        match self {
            Self::Legacy(legacy) => (Some(legacy.path.clone()), legacy.runner, None),
            Self::Recipe(recipe) => (None, None, Some(recipe.clone())),
        }
    }
}

pub fn resolve_recipe(
    project_root: &Path,
    recipe: &RecipeDefinition,
    inputs: &RuntimeInputs,
) -> Result<ResolvedInvocation, RecipeError> {
    recipe.validate()?;
    let declared: BTreeSet<&str> = recipe
        .arguments
        .iter()
        .filter_map(|argument| match argument {
            RecipeArgument::Input { id, .. } | RecipeArgument::Switch { id, .. } => {
                Some(id.as_str())
            }
            _ => None,
        })
        .collect();
    if let Some(unknown) = inputs.keys().find(|id| !declared.contains(id.as_str())) {
        return Err(RecipeError::UnknownInput(unknown.clone()));
    }

    let root = canonical_project_root(project_root)?;
    let (program, target) = match &recipe.program {
        RecipeProgram::ProjectFile { path } => {
            let path = project_entry(&root, path, EntryKind::File)?;
            (ResolvedProgram::ProjectFile(path.clone()), Some(path))
        }
        RecipeProgram::Interpreter { runner } => (
            ResolvedProgram::Interpreter {
                runner: *runner,
                candidates: runner
                    .executable_candidates()
                    .iter()
                    .map(OsString::from)
                    .collect(),
            },
            None,
        ),
        RecipeProgram::Executable { name } => (
            ResolvedProgram::SearchPath {
                candidates: vec![OsString::from(name)],
            },
            None,
        ),
    };
    let cwd = match &recipe.working_directory {
        WorkingDirectory::ProjectRoot => root.clone(),
        WorkingDirectory::TargetParent => target
            .as_deref()
            .and_then(Path::parent)
            .map(Path::to_owned)
            .ok_or(RecipeError::TargetParentWithoutProjectFile)?,
        WorkingDirectory::ProjectRelative { path } => {
            project_entry(&root, path, EntryKind::Directory)?
        }
    };

    let mut argv = Vec::new();
    for argument in &recipe.arguments {
        match argument {
            RecipeArgument::ProjectPath { path } => {
                argv.push(project_entry(&root, path, EntryKind::Any)?.into_os_string())
            }
            RecipeArgument::Literal { value } => argv.push(OsString::from(value)),
            RecipeArgument::Input {
                id,
                kind,
                required,
                default,
                prefix,
                ..
            } => {
                let value = match inputs.get(id) {
                    Some(value) => Some(resolve_input(id, *kind, value)?),
                    None => default
                        .as_ref()
                        .map(|value| resolve_default(id, *kind, value))
                        .transpose()?,
                };
                match value {
                    Some(value) => {
                        if let Some(prefix) = prefix {
                            argv.push(OsString::from(prefix));
                        }
                        argv.push(value);
                    }
                    None if *required => return Err(RecipeError::MissingRequiredInput(id.clone())),
                    None => {}
                }
            }
            RecipeArgument::Switch {
                id, value, default, ..
            } => {
                let enabled = match inputs.get(id) {
                    Some(RuntimeInput::Switch(enabled)) => *enabled,
                    Some(_) => {
                        return Err(RecipeError::InputKindMismatch {
                            id: id.clone(),
                            expected: "a switch",
                        });
                    }
                    None => *default,
                };
                if enabled {
                    argv.push(OsString::from(value));
                }
            }
        }
    }

    Ok(ResolvedInvocation {
        behavior: recipe.behavior,
        program,
        argv,
        cwd,
    })
}

/// Validate the stored definition and every project-owned path without requiring
/// runtime inputs. Authoring surfaces use this before persistence; execution still
/// performs the complete validation again in [`resolve_recipe`].
pub fn validate_recipe_for_project(
    project_root: &Path,
    recipe: &RecipeDefinition,
) -> Result<(), RecipeError> {
    recipe.validate()?;
    let root = canonical_project_root(project_root)?;
    match &recipe.program {
        RecipeProgram::ProjectFile { path } => {
            project_entry(&root, path, EntryKind::File)?;
        }
        RecipeProgram::Interpreter { .. } | RecipeProgram::Executable { .. } => {}
    }
    match &recipe.working_directory {
        WorkingDirectory::ProjectRoot | WorkingDirectory::TargetParent => {}
        WorkingDirectory::ProjectRelative { path } => {
            project_entry(&root, path, EntryKind::Directory)?;
        }
    }
    for argument in &recipe.arguments {
        match argument {
            RecipeArgument::ProjectPath { path } => {
                project_entry(&root, path, EntryKind::Any)?;
            }
            RecipeArgument::Input {
                id,
                kind,
                default: Some(default),
                ..
            } => {
                resolve_default(id, *kind, default)?;
            }
            RecipeArgument::Literal { .. }
            | RecipeArgument::Input { default: None, .. }
            | RecipeArgument::Switch { .. } => {}
        }
    }
    Ok(())
}

fn validate_project_path(field: &'static str, value: &str) -> Result<(), RecipeError> {
    shortcuts::relative_path(value)
        .map(|_| ())
        .map_err(|_| RecipeError::InvalidProjectPath {
            field,
            value: value.to_owned(),
        })
}

fn validate_executable(name: &str) -> Result<(), RecipeError> {
    let starts_safely = name
        .bytes()
        .next()
        .is_some_and(|byte| byte.is_ascii_alphanumeric());
    if !starts_safely || crate::paths::validate_name(name).is_err() {
        return Err(RecipeError::InvalidExecutable(name.to_owned()));
    }
    Ok(())
}

fn validate_parameter(
    id: &str,
    label: &str,
    ids: &mut BTreeSet<String>,
) -> Result<(), RecipeError> {
    if crate::paths::validate_name(id).is_err() {
        return Err(RecipeError::InvalidParameterId(id.to_owned()));
    }
    if label.trim().is_empty() {
        return Err(RecipeError::EmptyLabel(id.to_owned()));
    }
    if !ids.insert(id.to_owned()) {
        return Err(RecipeError::DuplicateParameterId(id.to_owned()));
    }
    Ok(())
}

fn canonical_project_root(root: &Path) -> Result<PathBuf, RecipeError> {
    let root =
        fs::canonicalize(root).map_err(|error| RecipeError::ProjectRoot(error.to_string()))?;
    if !root.is_dir() {
        return Err(RecipeError::ProjectRoot(
            "path is not a directory".to_owned(),
        ));
    }
    Ok(root)
}

#[derive(Clone, Copy)]
enum EntryKind {
    Any,
    File,
    Directory,
}

fn project_entry(root: &Path, portable: &str, kind: EntryKind) -> Result<PathBuf, RecipeError> {
    let relative =
        shortcuts::relative_path(portable).map_err(|_| RecipeError::InvalidProjectPath {
            field: "resolved",
            value: portable.to_owned(),
        })?;
    let path = fs::canonicalize(root.join(relative)).map_err(|_| RecipeError::ProjectEntry {
        path: portable.to_owned(),
        expected: kind.description(),
    })?;
    let right_kind = match kind {
        EntryKind::Any => path.is_file() || path.is_dir(),
        EntryKind::File => path.is_file(),
        EntryKind::Directory => path.is_dir(),
    };
    if !path.starts_with(root) || !right_kind {
        return Err(RecipeError::ProjectEntry {
            path: portable.to_owned(),
            expected: kind.description(),
        });
    }
    Ok(path)
}

impl EntryKind {
    fn description(self) -> &'static str {
        match self {
            Self::Any => "a file or directory",
            Self::File => "a file",
            Self::Directory => "a directory",
        }
    }
}

fn resolve_default(id: &str, kind: InputKind, value: &str) -> Result<OsString, RecipeError> {
    match kind {
        InputKind::Text => Ok(OsString::from(value)),
        InputKind::File => resolve_runtime_path(id, kind, Path::new(value)),
        InputKind::Directory => resolve_runtime_path(id, kind, Path::new(value)),
    }
}

fn resolve_input(
    id: &str,
    expected: InputKind,
    value: &RuntimeInput,
) -> Result<OsString, RecipeError> {
    match (expected, value) {
        (InputKind::Text, RuntimeInput::Text(value)) => Ok(OsString::from(value)),
        (InputKind::File, RuntimeInput::File(path)) => resolve_runtime_path(id, expected, path),
        (InputKind::Directory, RuntimeInput::Directory(path)) => {
            resolve_runtime_path(id, expected, path)
        }
        _ => Err(RecipeError::InputKindMismatch {
            id: id.to_owned(),
            expected: expected.description(),
        }),
    }
}

fn resolve_runtime_path(id: &str, kind: InputKind, path: &Path) -> Result<OsString, RecipeError> {
    if !path.is_absolute() {
        return Err(RecipeError::RuntimePathMustBeAbsolute(id.to_owned()));
    }
    let path = fs::canonicalize(path).map_err(|_| RecipeError::RuntimePath {
        id: id.to_owned(),
        expected: kind,
    })?;
    let right_kind = match kind {
        InputKind::File => path.is_file(),
        InputKind::Directory => path.is_dir(),
        InputKind::Text => unreachable!(),
    };
    if !right_kind {
        return Err(RecipeError::RuntimePath {
            id: id.to_owned(),
            expected: kind,
        });
    }
    Ok(path.into_os_string())
}

impl InputKind {
    fn description(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::File => "a file",
            Self::Directory => "a directory",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn definition(
        behavior: InvocationBehavior,
        program: RecipeProgram,
        working_directory: WorkingDirectory,
        arguments: Vec<RecipeArgument>,
    ) -> RecipeDefinition {
        RecipeDefinition {
            version: RECIPE_VERSION,
            behavior,
            program,
            working_directory,
            arguments,
        }
    }

    fn project() -> tempfile::TempDir {
        let project = tempfile::tempdir().unwrap();
        fs::create_dir_all(project.path().join("scripts/data")).unwrap();
        fs::write(project.path().join("scripts/triage.py"), "pass\n").unwrap();
        project
    }

    #[test]
    fn ordered_arguments_runtime_paths_prefixes_and_spaces_resolve_without_shell_splitting() {
        let project = project();
        let external = tempfile::tempdir().unwrap();
        let sample = external.path().join("sample with spaces.bin");
        fs::write(&sample, b"sample").unwrap();
        let output = external.path().join("output directory");
        fs::create_dir(&output).unwrap();
        let recipe = definition(
            InvocationBehavior::Run,
            RecipeProgram::Interpreter {
                runner: Runner::Python,
            },
            WorkingDirectory::ProjectRoot,
            vec![
                RecipeArgument::Literal {
                    value: "literal with spaces".into(),
                },
                RecipeArgument::ProjectPath {
                    path: "scripts/triage.py".into(),
                },
                RecipeArgument::Input {
                    id: "sample".into(),
                    label: "Sample".into(),
                    kind: InputKind::File,
                    required: true,
                    default: None,
                    prefix: None,
                },
                RecipeArgument::Switch {
                    id: "recursive".into(),
                    label: "Recursive".into(),
                    value: "--recursive".into(),
                    default: false,
                },
                RecipeArgument::Input {
                    id: "format".into(),
                    label: "Format".into(),
                    kind: InputKind::Text,
                    required: true,
                    default: None,
                    prefix: Some("--format".into()),
                },
                RecipeArgument::Input {
                    id: "output".into(),
                    label: "Output".into(),
                    kind: InputKind::Directory,
                    required: true,
                    default: None,
                    prefix: Some("--output".into()),
                },
                RecipeArgument::Literal {
                    value: "echo foo | bar && $(nope)".into(),
                },
            ],
        );
        let inputs = BTreeMap::from([
            ("sample".into(), RuntimeInput::File(sample.clone())),
            ("recursive".into(), RuntimeInput::Switch(true)),
            (
                "format".into(),
                RuntimeInput::Text("markdown report".into()),
            ),
            ("output".into(), RuntimeInput::Directory(output.clone())),
        ]);

        let resolved = resolve_recipe(project.path(), &recipe, &inputs).unwrap();
        assert_eq!(resolved.behavior, InvocationBehavior::Run);
        assert_eq!(resolved.cwd, fs::canonicalize(project.path()).unwrap());
        assert_eq!(
            resolved.program,
            ResolvedProgram::Interpreter {
                runner: Runner::Python,
                candidates: Runner::Python
                    .executable_candidates()
                    .iter()
                    .map(OsString::from)
                    .collect()
            }
        );
        assert_eq!(
            resolved.argv,
            vec![
                OsString::from("literal with spaces"),
                fs::canonicalize(project.path().join("scripts/triage.py"))
                    .unwrap()
                    .into_os_string(),
                fs::canonicalize(sample).unwrap().into_os_string(),
                OsString::from("--recursive"),
                OsString::from("--format"),
                OsString::from("markdown report"),
                OsString::from("--output"),
                fs::canonicalize(output).unwrap().into_os_string(),
                OsString::from("echo foo | bar && $(nope)"),
            ]
        );
    }

    #[test]
    fn defaults_and_disabled_optional_arguments_expand_atomically() {
        let project = project();
        let recipe = definition(
            InvocationBehavior::Launch,
            RecipeProgram::Executable {
                name: "demo-tool".into(),
            },
            WorkingDirectory::ProjectRelative {
                path: "scripts/data".into(),
            },
            vec![
                RecipeArgument::Input {
                    id: "format".into(),
                    label: "Format".into(),
                    kind: InputKind::Text,
                    required: true,
                    default: Some("markdown".into()),
                    prefix: Some("--format".into()),
                },
                RecipeArgument::Input {
                    id: "optional".into(),
                    label: "Optional".into(),
                    kind: InputKind::Text,
                    required: false,
                    default: None,
                    prefix: Some("--optional".into()),
                },
                RecipeArgument::Switch {
                    id: "verbose".into(),
                    label: "Verbose".into(),
                    value: "--verbose".into(),
                    default: true,
                },
                RecipeArgument::Switch {
                    id: "quiet".into(),
                    label: "Quiet".into(),
                    value: "--quiet".into(),
                    default: false,
                },
            ],
        );

        let resolved = resolve_recipe(project.path(), &recipe, &RuntimeInputs::new()).unwrap();
        assert_eq!(resolved.behavior, InvocationBehavior::Launch);
        assert_eq!(
            resolved.program,
            ResolvedProgram::SearchPath {
                candidates: vec!["demo-tool".into()]
            }
        );
        assert_eq!(
            resolved.argv,
            ["--format", "markdown", "--verbose"].map(OsString::from)
        );
        assert_eq!(
            resolved.cwd,
            fs::canonicalize(project.path().join("scripts/data")).unwrap()
        );
    }

    #[test]
    fn parameter_definition_and_runtime_maps_are_strict() {
        let project = project();
        let input = |id: &str, kind| RecipeArgument::Input {
            id: id.into(),
            label: "Value".into(),
            kind,
            required: true,
            default: None,
            prefix: None,
        };
        let duplicate = definition(
            InvocationBehavior::Run,
            RecipeProgram::Executable {
                name: "tool".into(),
            },
            WorkingDirectory::ProjectRoot,
            vec![
                input("same", InputKind::Text),
                RecipeArgument::Switch {
                    id: "same".into(),
                    label: "Same".into(),
                    value: "--same".into(),
                    default: false,
                },
            ],
        );
        assert_eq!(
            duplicate.validate(),
            Err(RecipeError::DuplicateParameterId("same".into()))
        );

        let recipe = definition(
            InvocationBehavior::Run,
            RecipeProgram::Executable {
                name: "tool".into(),
            },
            WorkingDirectory::ProjectRoot,
            vec![input("value", InputKind::Text)],
        );
        assert_eq!(
            resolve_recipe(project.path(), &recipe, &RuntimeInputs::new()).unwrap_err(),
            RecipeError::MissingRequiredInput("value".into())
        );
        assert_eq!(
            resolve_recipe(
                project.path(),
                &recipe,
                &BTreeMap::from([("other".into(), RuntimeInput::Text("x".into()))])
            )
            .unwrap_err(),
            RecipeError::UnknownInput("other".into())
        );
        assert_eq!(
            resolve_recipe(
                project.path(),
                &recipe,
                &BTreeMap::from([("value".into(), RuntimeInput::Switch(true))])
            )
            .unwrap_err(),
            RecipeError::InputKindMismatch {
                id: "value".into(),
                expected: "text"
            }
        );
    }

    #[test]
    fn project_program_target_parent_and_project_relative_paths_are_contained() {
        let project = project();
        let recipe = definition(
            InvocationBehavior::Launch,
            RecipeProgram::ProjectFile {
                path: "scripts/triage.py".into(),
            },
            WorkingDirectory::TargetParent,
            vec![],
        );
        let resolved = resolve_recipe(project.path(), &recipe, &RuntimeInputs::new()).unwrap();
        let target = fs::canonicalize(project.path().join("scripts/triage.py")).unwrap();
        assert_eq!(
            resolved.program,
            ResolvedProgram::ProjectFile(target.clone())
        );
        assert_eq!(resolved.cwd, target.parent().unwrap());

        for path in ["../outside", "/absolute", "scripts/../outside"] {
            let invalid = definition(
                InvocationBehavior::Run,
                RecipeProgram::ProjectFile { path: path.into() },
                WorkingDirectory::ProjectRoot,
                vec![],
            );
            assert!(matches!(
                invalid.validate(),
                Err(RecipeError::InvalidProjectPath { .. })
            ));
        }
        let invalid_cwd = definition(
            InvocationBehavior::Run,
            RecipeProgram::Executable {
                name: "tool".into(),
            },
            WorkingDirectory::ProjectRelative {
                path: "../outside".into(),
            },
            vec![],
        );
        assert!(matches!(
            invalid_cwd.validate(),
            Err(RecipeError::InvalidProjectPath {
                field: "working-directory",
                ..
            })
        ));
        let missing_program = definition(
            InvocationBehavior::Run,
            RecipeProgram::ProjectFile {
                path: "missing.exe".into(),
            },
            WorkingDirectory::ProjectRoot,
            vec![],
        );
        assert!(matches!(
            resolve_recipe(project.path(), &missing_program, &RuntimeInputs::new()),
            Err(RecipeError::ProjectEntry { .. })
        ));
        let ambiguous = definition(
            InvocationBehavior::Run,
            RecipeProgram::Executable {
                name: "tool".into(),
            },
            WorkingDirectory::TargetParent,
            vec![],
        );
        assert_eq!(
            resolve_recipe(project.path(), &ambiguous, &RuntimeInputs::new()).unwrap_err(),
            RecipeError::TargetParentWithoutProjectFile
        );
    }

    #[test]
    fn external_runtime_objects_are_typed_absolute_and_not_project_contained() {
        let project = project();
        let external = tempfile::tempdir().unwrap();
        let file = external.path().join("outside.bin");
        fs::write(&file, b"outside").unwrap();
        let recipe = definition(
            InvocationBehavior::Run,
            RecipeProgram::Executable {
                name: "tool".into(),
            },
            WorkingDirectory::ProjectRoot,
            vec![RecipeArgument::Input {
                id: "sample".into(),
                label: "Sample".into(),
                kind: InputKind::File,
                required: true,
                default: None,
                prefix: None,
            }],
        );
        let resolved = resolve_recipe(
            project.path(),
            &recipe,
            &BTreeMap::from([("sample".into(), RuntimeInput::File(file.clone()))]),
        )
        .unwrap();
        assert_eq!(
            resolved.argv,
            [fs::canonicalize(file).unwrap().into_os_string()]
        );

        let relative =
            BTreeMap::from([("sample".into(), RuntimeInput::File("outside.bin".into()))]);
        assert_eq!(
            resolve_recipe(project.path(), &recipe, &relative).unwrap_err(),
            RecipeError::RuntimePathMustBeAbsolute("sample".into())
        );
        let directory =
            BTreeMap::from([("sample".into(), RuntimeInput::File(external.path().into()))]);
        assert_eq!(
            resolve_recipe(project.path(), &recipe, &directory).unwrap_err(),
            RecipeError::RuntimePath {
                id: "sample".into(),
                expected: InputKind::File
            }
        );
    }

    #[test]
    fn versions_programs_and_invocation_variants_fail_closed() {
        let mut recipe = definition(
            InvocationBehavior::Run,
            RecipeProgram::Interpreter {
                runner: Runner::Direct,
            },
            WorkingDirectory::ProjectRoot,
            vec![],
        );
        assert_eq!(recipe.validate(), Err(RecipeError::DirectIsNotInterpreter));
        recipe.program = RecipeProgram::Executable {
            name: "bash -c".into(),
        };
        assert_eq!(
            recipe.validate(),
            Err(RecipeError::InvalidExecutable("bash -c".into()))
        );
        recipe.program = RecipeProgram::Executable {
            name: "tool".into(),
        };
        recipe.version = 2;
        assert_eq!(recipe.validate(), Err(RecipeError::UnsupportedVersion(2)));
        assert_eq!(
            StoredInvocation::from_parts(None, None, None),
            Err(RecipeError::MissingInvocation)
        );
        assert_eq!(
            StoredInvocation::from_parts(Some("run.sh".into()), None, Some(recipe.clone())),
            Err(RecipeError::MixedInvocation)
        );
        assert_eq!(
            StoredInvocation::from_parts(None, Some(Runner::Bash), None),
            Err(RecipeError::RunnerWithoutLegacyPath)
        );
    }

    #[cfg(unix)]
    #[test]
    fn fixed_project_paths_reject_symlink_escape() {
        use std::os::unix::fs::symlink;

        let project = project();
        let external = tempfile::tempdir().unwrap();
        let outside = external.path().join("outside");
        fs::write(&outside, b"outside").unwrap();
        symlink(&outside, project.path().join("scripts/escape")).unwrap();
        let recipe = definition(
            InvocationBehavior::Run,
            RecipeProgram::Executable {
                name: "tool".into(),
            },
            WorkingDirectory::ProjectRoot,
            vec![RecipeArgument::ProjectPath {
                path: "scripts/escape".into(),
            }],
        );
        assert!(matches!(
            resolve_recipe(project.path(), &recipe, &RuntimeInputs::new()),
            Err(RecipeError::ProjectEntry { .. })
        ));
    }

    #[cfg(unix)]
    #[test]
    fn external_runtime_paths_preserve_non_utf8_native_values() {
        use std::os::unix::ffi::OsStringExt;

        let project = project();
        let external = tempfile::tempdir().unwrap();
        let path = external.path().join(OsString::from_vec(vec![b's', 0xff]));
        fs::write(&path, b"sample").unwrap();
        let recipe = definition(
            InvocationBehavior::Run,
            RecipeProgram::Executable {
                name: "tool".into(),
            },
            WorkingDirectory::ProjectRoot,
            vec![RecipeArgument::Input {
                id: "sample".into(),
                label: "Sample".into(),
                kind: InputKind::File,
                required: true,
                default: None,
                prefix: None,
            }],
        );
        let resolved = resolve_recipe(
            project.path(),
            &recipe,
            &BTreeMap::from([("sample".into(), RuntimeInput::File(path.clone()))]),
        )
        .unwrap();
        assert_eq!(
            resolved.argv,
            [fs::canonicalize(path).unwrap().into_os_string()]
        );
    }
}
