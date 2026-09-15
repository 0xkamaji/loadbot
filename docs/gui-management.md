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
persistent default catalog. `SYNC CATALOG` is a separate deliberate Git update; on
success the GUI rereads local state. `RELOAD LOCAL` only rereads state and never
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

`ADD SHORTCUT` targets the selected catalog-qualified installed project. It accepts a
name, repository-relative existing file, optional description, and an existing
Loadbot runner value. Rust validates that the project is installed, the path remains
inside it, the target is a file, and the shortcut name is not already present before
atomically saving `shortcuts.toml`.

One controller-owned management state prevents concurrent duplicate submissions and
represents progress, success, and failure. A successful operation selects its known
identity after rereading real state. A failure also triggers a local authoritative
reread because shared operations can retain a completed durable step before a later
explicit commit/push step fails; the GUI never guesses or rolls that state back.
Sync/local reload preserve still-valid project and shortcut selections in the same
catalog. Switching catalogs deliberately clears cross-catalog selection.

The resizable bottom workspace has `TERMINAL` and `ACTIVITY` views. Terminal remains
an explicitly unconnected placeholder. Activity is a session-only, controller-owned
feed capped at the newest 250 semantic entries. Explicit reload, folder opening, and
management actions record start/completion/failure; management mutations also record
their authoritative reread. Catalog sync streams typed core notices for validation,
repository verification, update start, and current/updated completion through a
Tauri channel. No CLI text, subprocess output, or Git command line is parsed, and no
activity log is persisted.

## Phase boundary

Fixtures remain isolated behind `LoadbotAdapter`; their management calls return a
controlled unavailable error and never touch real state. Edit, delete, background
sync, catalog initialization, project pulling, shortcut execution, PTY/terminal
execution, Rot GUI integration, and model/personality behavior are not implemented.
