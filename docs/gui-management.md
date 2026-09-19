# Phase 4B real Loadbot management

The native Loadbot workspace can maintain real catalogs, projects (Loadbot tool
definitions), and personal shortcuts. It still cannot run a shortcut or provide a
terminal session.

## Shared operation boundary

Catalog registration/synchronization and project definition creation continue to
use `operations::catalog_add`, `operations::catalog_sync`, and
`operations::tool_add`, which are the same semantic operations used by the CLI.
`operations::shortcut_add` adds the qualified installed-project and target checks
needed by a non-prompting caller, then delegates the durable document update and
duplicate rules to the existing `shortcuts::save` authority used by the CLI.

The native route is:

```text
React form
  -> headless application action and operation state
  -> LoadbotAdapter semantic method
  -> one typed Tauri command
  -> shared Rust Loadbot operation
  -> validation and durable Loadbot state
  -> authoritative inventory/catalog reread
```

React receives no native storage paths, edits no TOML, performs no Git operation,
and constructs no shell command. Tauri uses an unattended, non-terminal operation
context and returns structured catalog/project/shortcut identities. Failures do not
create optimistic frontend objects.

## Workspace behavior

Catalog remains compact session context in the header. The menu lists configured
catalogs with installed/missing/mismatch and writable/read-only facts. Selecting one
filters the current session without synchronizing it and without changing Loadbot's
persistent default catalog. `REFRESH CATALOG` is a separate deliberate Git update; on
success the GUI rereads local state. `RELOAD` only rereads state and never
contacts or updates a remote.

`ADD CATALOG` registers and clones an existing valid Loadbot catalog using its name,
Git URL, and writable flag, then uses it as the current GUI session context. It does
not initialize a new remote or silently push.

`ADD PROJECT` adds a real Git-backed Loadbot tool definition to the current installed
writable catalog. Its fields match the CLI operation: name, Git URL, optional
revision, and explicit commit/push choices. Configured projects now remain in the
inventory even with zero shortcuts, so the confirmed result can be selected after
the authoritative reread. Adding a definition does not silently pull/install its
repository.

`ADD SHORTCUT` targets the selected catalog-qualified installed project and opens
one target-first workflow: name, project-owned target, runner, optional description,
and ordered parameters. New GUI shortcuts are structured Run Recipes. Legacy is a
compatibility format rather than a creation choice, and Launch remains a supported
core behavior but is intentionally not offered for new GUI shortcuts yet. The form
projects directly onto the Phase 2 Recipe model; it has no GUI-only persistence
schema. Rust validates the installed project and fixed project-owned paths before
atomically saving `shortcuts.toml`.

Personal Run Recipes that map losslessly to target + runner + parameters can be
reopened in the same editor. Direct targets use `ProjectFile`; interpreted targets
use `Interpreter` with a leading fixed `ProjectPath`, which the form presents as the
target rather than a parameter. Name/identity changes are deliberately deferred;
updates preserve unrelated entries and unknown metadata and fail on a concurrent
definition change. Launch, Executable, and other Recipes outside this safe editor
subset remain inspectable and read-only instead of being normalized destructively.
Legacy entries are likewise never converted, and shared catalog Recipes remain
read-only because their authority is Git-backed `catalog.toml`. The preview of an
incomplete draft is presentation-only and never becomes an execution command.

The authoring UI speaks in shortcut terms: Target, Run with, Parameters, and an
Advanced `Run from` setting (Tool folder by default). Parameter choices are Value,
File, Directory, Flag, Flag + Value, and Fixed Argument. These map to `Input`,
`Switch`, and `Literal` without changing persistence. Stable parameter IDs are
generated from their labels and remain under an Advanced disclosure; once edited
explicitly, later label changes do not replace them.

`VIEW HELP` is a deliberately narrow authoring aid. The controller sends the current
qualified project, project-relative target, runner, and working directory through
the semantic adapter. Rust resolves the same Recipe primitives used by authoring,
then invokes the validated target with `--help`, falling back to `-h` only when the
first attempt returns no output. Output capture is bounded and each attempt has a
five-second timeout. The temporary panel shows raw stdout/stderr and exit status;
it does not parse help, infer parameters, mutate configuration, or use a shell.

Project-owned files and folders use the official native Tauri dialog plugin through
semantic `chooseProjectFile` and `chooseProjectDirectory` adapter capabilities. The
dialog starts in the installed project. Rust canonicalizes the result, rejects
outside-project paths and file/directory mismatches, and returns only a portable
project-relative path. Cancel returns no value and changes neither the draft nor
Activity. Runtime File/Directory options remain definitions only; authoring never
selects their future runtime values.

Personal shortcuts expose contextual deletion with a Loadbot confirmation that
states tools and files are untouched. Manage mode allows multiple personal
shortcuts to be selected while catalog commands remain visibly read-only. Bulk
deletion validates every qualified identity, then removes all definitions under one
shortcuts-file lease and one atomic save; a conflict deletes none. Both paths reread
authoritative inventory and emit semantic Activity. The frontend never edits TOML.

One controller-owned management state prevents concurrent duplicate submissions and
represents progress, success, and failure. A successful operation selects its known
identity after rereading real state. A failure also triggers a local authoritative
reread because shared operations can retain a completed durable step before a later
explicit commit/push step fails; the GUI never guesses or rolls that state back.
Sync/local reload preserve still-valid project and shortcut selections in the same
catalog. Switching catalogs deliberately clears cross-catalog selection.

The resizable bottom `Console` workspace has `COMMAND` and `ACTIVITY` views. Command
is a Loadbot-specific, read-only semantic interface—not a terminal, shell, or PTY.
Activity is a session-only, controller-owned feed capped at the newest 250 semantic entries. Explicit reload, folder opening, and
management actions record start/completion/failure; management mutations also record
their authoritative reread. Catalog sync streams typed core notices for validation,
repository verification, update start, and current/updated completion through a
Tauri channel. No CLI text, subprocess output, or Git command line is parsed, and no
activity log is persisted.

See [Loadbot Console](gui-console.md) for the registered read-only commands, qualified
identity behavior, session command history, and deliberate system-terminal boundary.

## Phase boundary

Fixtures remain isolated behind `LoadbotAdapter`; a fresh in-memory fixture adapter
can exercise project-path choices and personal deletion without touching real state.
Fixtures include Legacy, Run Recipe, and Launch Recipe inspection examples.
Background sync, catalog initialization, project pulling, Recipe/shortcut execution,
application launching, Output UI, PTY/system-terminal execution, Rot GUI integration,
help-to-parameter inference, and model/personality behavior are not implemented.
