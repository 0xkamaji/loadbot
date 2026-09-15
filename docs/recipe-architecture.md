# Recipe architecture audit and v1 direction

This document records the Phase 1 audit and the Phase 2 core implementation. It
is not an authorization to connect GUI execution or build an editor.

> **Loadbot provides primitives. Catalogs provide knowledge. Recipes provide
> opinion.**

Loadbot must not know that a particular tool needs a sample, repository, profile,
or output format. It should validate a small vocabulary of ordered arguments and
resolve it into one program, one argument vector, and one working directory. A
catalog or personal shortcut supplies the tool-specific recipe.

## What exists today

There are two persisted sources which the launcher projects into one workspace:

| Source | Storage | Persisted entry | Ownership |
| --- | --- | --- | --- |
| Shared catalog command | `catalogs/<catalog>/catalog.toml`, under `[tools.<tool>.commands.<name>]` | `CommandConfig` | The Git-backed catalog |
| Personal shortcut | Platform config `loadbot/shortcuts.toml`, under `[shortcuts.<name>]` | `Shortcut` | The local user |

Both enclosing documents have `version = 1`. Both use Serde `flatten` maps to
retain unknown TOML fields through a load/save cycle. A catalog tool is a Git URL,
optional revision, and a sorted command map. A personal shortcut stores its
catalog and tool because its global map key contains only the shortcut name.

The current common entry facts are:

```text
name                 map key, not a field
catalog + tool       explicit on personal entries; inherited by shared commands
path                 required portable repository-relative file path
description          optional
runner               optional: direct, bash, sh, python, powershell
source               derived inventory fact: catalog or personal
```

Names accept conservative portable ASCII. Paths use `/`, are UTF-8 and relative,
and reject absolute paths, backslashes, colons, empty components, `.` and `..`.
Before a target is saved by the shared operation or launched, Rust resolves the
installed catalog-qualified project, canonicalizes the file, and rejects missing,
non-file, symlink-escape, or repository-escape targets.

The semantic inventory key for a project is `(catalog, tool)`. A selected entry is
qualified by its project plus `(source, name, path)`. Shared names need only be
unique inside one tool command map. Personal shortcut names are currently unique
across the entire user `shortcuts.toml`, not merely within a project. If a shared
and personal entry have the same display name, inventory preserves both and the
CLI/GUI labels their source rather than guessing.

### Creation paths

`operations::shortcut_add` is the authoritative personal-shortcut mutation used by
the native GUI and by current CLI save flows. It accepts qualified catalog/tool,
name, path, description and runner; verifies the installed project and target;
then uses the leased, atomic `shortcuts::save` transaction. Duplicate global
personal names are refused and existing data is not overwritten.

The native GUI form exposes all of those fields through `LoadbotAdapter`, a thin
Tauri command, and the shared Rust operation. The interactive CLI file browser
chooses an installed tool file and a name, then calls the same operation with no
description or runner. The interactive tool-add flow can enter that browser after
installing a tool. The generic `loadbot run` browser can launch a selected file and
then offer to save it. There is no operation that authors a shared catalog command;
catalog authors currently edit `catalog.toml` through their normal catalog workflow.

### Read and management paths

`launcher::project_inventory` merges resolved catalog commands with the personal
shortcut file. `read_project_inventory` is the complete, read-only query used by
Tauri. The TypeScript contract carries only name, relative path, optional
description/runner and source; React does not parse TOML or construct paths.

The CLI can list and remove personal shortcuts and run one by its globally unique
name. Its interactive project menu can launch either shared or personal entries.
The GUI can add and inspect entries but deliberately has no execution capability.
The Command Console can list and inspect inventory entries; it neither executes nor
mutates them.

### Current launch semantics

No current shortcut has stored arguments, parameter definitions, a configurable
working directory, environment overrides, or a detached-launch mode.

| Entry | Runner | Program and argv | Working directory |
| --- | --- | --- | --- |
| Shared catalog command | explicit | selected platform-aware runner; target is its sole argument | project root |
| Shared catalog command | omitted | target directly | project root |
| Personal shortcut | explicit | same runner behavior; target is its sole argument | project root |
| Personal shortcut | omitted | executable target directly, otherwise extension fallback for `.py`, `.sh`, `.ps1` | target's parent |
| Arbitrary file-browser launch | inferred as above | target or extension-selected interpreter plus target | target's parent |

Python resolves as `python3`/`python` on Linux and `python`/`python3` on Windows;
PowerShell resolves in platform-preferred `pwsh`/`powershell` order; Bash and `sh`
remain distinct. Windows POSIX runners receive a safe relative target from the
project-root cwd so a verbatim Windows path is not passed to Git Bash or WSL Bash.
The GUI's current no-runner option is worded “Use file association,” but the Rust
launcher does not ask the operating system to open an associated application: it
runs a native executable or performs the extension fallback above. That wording
must not be treated as an existing launch-application contract.

The current CLI uses `process::Mode::Inherit`: stdin/stdout/stderr and the parent
environment are inherited, Loadbot waits for exit, and nonzero status is an error.
The process layer already supports bounded concurrent stdout/stderr streaming,
typed lifecycle events, cancellation, Unix process groups and Windows Job Objects,
but it is not a PTY. A future GUI run must select `Mode::Stream` and null stdin; it
must not reuse the Tauri worker's current default inherited mode accidentally.

## Gap analysis

Reusable unchanged:

- catalog-qualified project lookup and installed-root validation;
- portable repository-relative path validation and canonical containment;
- runner selection and Windows/Linux interpreter details;
- process observation, bounded streaming, cancellation and exit classification;
- operation reports/notices and the adapter/controller dependency direction;
- format-preserving, leased, atomic persistence;
- inventory source and identity distinctions.

Missing for recipes:

- an explicit program choice beyond “runner plus one target”;
- ordered fixed and runtime-supplied argv pieces;
- stable parameter identities and input kinds;
- an explicit working-directory policy;
- observed-run versus handed-off-application behavior;
- semantic inspection and resolution results shared by CLI, Command and GUI.

The initial model does not need a shell grammar, PTY, pipeline, redirects, command
string parser, help scraper, dependency installer, secrets store, general scheduler,
or tool-specific fields. Recipe-specific environment overrides should also wait:
the current behavior of inheriting the host environment is sufficient for the first
model, while persistence of values—especially secrets—and cross-platform variable
rules require a separate explicit decision.

## Implemented Recipe core

A **Shortcut** remains the named, project-associated user concept and identity. A
**Recipe** is the optional structured invocation definition carried by either a
shared catalog command or a personal shortcut. Existing path-based entries are
legacy shortcuts, not migrated recipe documents.

Phase 2 implements the following shared Rust model:

```text
StoredInvocation
  Legacy { path, runner? }
  Recipe(RecipeDefinition)

RecipeDefinition
  version = 1
  behavior: run | launch
  program: ProjectFile(path) | Interpreter(runner) | Executable(name)
  working_directory: ProjectRoot | TargetParent | ProjectRelative(path)
  arguments: Vec<RecipeArgument>

RecipeArgument
  ProjectPath { path }
  Literal { value }
  Input { id, label, kind, required, default?, prefix? }
  Switch { id, label, value, default }

InputKind
  text | file | directory
```

`ProjectFile` is a validated project-relative executable. `Interpreter` uses the
existing platform-aware Bash/sh/Python/PowerShell selectors. `Executable` is one
explicit executable name resolved through the child environment; it is not a shell
string and receives dedicated validation. `ProjectPath` is a fixed, containment-
checked project file argument, useful for `python triage.py` without weakening a
literal into a native path.

The argument vector is expanded strictly in stored array order:

| Desired concept | Minimal representation | Expansion |
| --- | --- | --- |
| Fixed literal or always-on flag | `Literal` | exactly one argv value |
| Runtime value | `Input(kind = text)` | exactly one supplied value |
| Runtime file/directory | `Input(kind = file/directory)` | exactly one native path value |
| Toggleable flag | `Switch` | zero or one fixed argv value |
| Flag plus value | `Input(prefix = "--format")` | prefix and value together, or neither when optional and absent |

Every input/switch ID is unique within its recipe. Resolution rejects missing
required inputs, wrong input kinds and unknown supplied IDs. Labels are presentation
metadata; IDs are the stable contract. Fixed strings and user values each become a
single `OsString` argument. Nothing is split, expanded, quoted by a shell, or treated
as an operator. A rendered command line is only a preview.

For example, the product concept:

```text
python triage.py {Sample} --recursive --format {Format}
```

could be stored approximately as:

```toml
[shortcuts.triage]
catalog = "personal"
tool = "re-toolkit"
description = "Triage a sample"

[shortcuts.triage.recipe]
version = 1
behavior = "run"
program = { type = "interpreter", runner = "python" }
working_directory = { type = "project-root" }

[[shortcuts.triage.recipe.arguments]]
type = "project-path"
path = "triage.py"

[[shortcuts.triage.recipe.arguments]]
type = "input"
id = "sample"
label = "Sample"
kind = "file"
required = true

[[shortcuts.triage.recipe.arguments]]
type = "switch"
id = "recursive"
label = "Recursive"
value = "--recursive"
default = false

[[shortcuts.triage.recipe.arguments]]
type = "input"
id = "format"
label = "Format"
kind = "text"
prefix = "--format"
required = true
```

This is now the persisted Recipe shape. The same `RecipeDefinition` is used by
personal shortcuts and shared catalog commands.

Pure resolution accepts typed runtime values keyed by parameter ID. Text, file,
directory and switch values remain distinct. Runtime file/directory paths must be
absolute, may be outside the installed project, and are canonicalized and checked
for the declared filesystem type. Fixed `ProjectPath` values remain contained by
the installed project.

`ResolvedInvocation` contains behavior, ordered native `OsString` arguments, a
canonical `PathBuf` cwd, and a `ResolvedProgram`. A project-file program is a
canonical path. Interpreter programs retain both the semantic runner and the
existing ordered, platform-aware PATH candidates (for example, `python3` then
`python` on Linux) so pure resolution does not pretend to know which host executable
exists or spawn/search for it.

### Run Recipe versus Launch Application

`behavior = "run"` means a synchronous, observable Loadbot operation. CLI callers
may inherit terminal streams; GUI callers stream stdout/stderr with null stdin. Both
use the same resolved program/argv/cwd and report typed start/output/exit/failure
events. Interactive terminal programs remain unsupported by the GUI runner.

`behavior = "launch"` means start an explicitly resolved application and hand off
its lifecycle. It should use a separate backend launch operation, null or platform-
appropriate stdio, and record success/failure Activity without pretending to stream
a detached application's output. The current waiting `process::execute` path must
not be casually modified into a partly detached mode. Exact Unix/Windows handoff and
failure-observation semantics remain a pre-implementation decision.

## Backward compatibility and fail-closed evolution

All existing catalog commands and personal shortcuts remain the legacy variant and
retain their exact current runner, target, cwd, environment and stream semantics.
They need no rewrite, migration, generated recipe block, or document-version bump.
At runtime a resolver may project one into a `ResolvedRecipe`, but persistence stays
unchanged.

A new structured recipe should use `recipe = ...` **instead of** legacy `path` and
top-level `runner`, never beside them. The new loader must require exactly one valid
variant. This matters because an older Loadbot preserves unknown fields but still
understands `path`: storing both would let it silently ignore parameters and execute
the target incorrectly. With `path` omitted, an older binary rejects the unfamiliar
entry for its missing required field and therefore fails closed. Recipe-local
`version = 1` allows later recipe evolution without changing unrelated catalog or
shortcut document versions.

This preserves existing installations without migration, but it does not promise
that an old binary can consume newly authored recipes. Catalog publication must call
out the minimum Loadbot feature version. A whole-document version bump is not
recommended for this extension because it would reject unrelated legacy entries as
well; tests must prove the final Serde representation is unambiguous before Phase 2
freezes it.

## Shared semantic API direction

Later layers should consume qualified semantic operations, not TOML or CLI output:

1. `inspect_shortcut(identity)` returns source, description and either the legacy or
   recipe definition needed to render an editor/inspector.
2. `resolve_recipe(identity, inputs)` validates the authoritative definition,
   project installation, program, arguments and cwd, and returns a Rust-domain
   `ResolvedInvocation { behavior, program, argv, cwd }` containing native
   `OsString`/`PathBuf` values.
3. The existing personal `shortcut_add` evolves (or delegates) to one shared create
   operation accepting the validated invocation variant. A later update operation
   must preserve concurrency and format-preserving transaction guarantees.
4. A future `run_recipe` consumes only a resolved run invocation and the existing
   process observer/cancellation layer.
5. A future `launch_application` consumes only a resolved launch invocation and a
   deliberately defined platform handoff implementation.

The identity must include catalog, tool, source and name (plus any existing path
disambiguator used by the workspace). The backend chooses paths and executable
details. Tauri remains a thin structured bridge. The application/controller owns
form/run state and bounded output. GUI buttons, Command actions and CLI rendering
delegate to these same operations.

## Implemented in Phase 2

Phase 2 implements the shared versioned model, backward-compatible loader,
validation, semantic inventory inspection, typed runtime inputs, and pure resolution
to structured program/argv/cwd. Existing execution remains restricted to legacy
entries and retains its prior behavior; attempting to execute a Recipe reports that
Recipe execution is not implemented.

## Future phases

The Recipe Builder GUI, Recipe create/update workflow, process execution, detached
application launch, output UI, Command mutations, help discovery, environment
overrides and PTY support are not implemented. A later execution phase must consume
`ResolvedInvocation`; it must not reinterpret a preview string as a command.

Before implementing detached launch in a later phase, decide its precise Windows and
Linux ownership/error contract. Before catalog recipes are published, decide how the
minimum supporting Loadbot version is communicated. Those are the only material open
architecture decisions from this audit.
