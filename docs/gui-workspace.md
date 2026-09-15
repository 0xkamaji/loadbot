# Phase 4A workspace and read-only interaction

Phase 4A started from clean `main` at
`697ef45c1847371224d9ef30118ff81272462295`. It evolves the real local-inventory
window into a workspace shell without adding catalog, project, or shortcut mutation
and without adding shortcut or terminal execution.

## Workspace model

Catalog is quiet session context in the header. The displayed catalog follows the
currently selected catalog-qualified project. The chevron control is deliberately
disabled in this phase: the existing inventory contract does not provide a clean
catalog-switch operation, so the GUI neither invents one nor writes a GUI catalog
preference. Projects remain primary navigation in the left pane. The right workspace
contains the selected project's shortcut list above the selected shortcut's real
metadata. The terminal remains a clearly non-connected future workspace area.

Three small, keyboard-focusable separators resize:

1. Projects versus the right workspace.
2. Shortcut list versus selected-shortcut details.
3. Main workspace versus terminal.

The separators support pointer drag, arrow keys, bounds derived from their current
container, and double-click reset. Content scrolls inside its owning pane. Their
pixel dimensions are best-effort presentation preferences stored under the versioned
browser-local key `loadbot.workspace.panes.v1`. Missing, malformed, old, non-finite,
or unavailable storage falls back to defaults; restored and window-resized values
are clamped. This state never enters Loadbot configuration, catalogs, projects, or
shortcuts.

`RELOAD LOCAL` starts the same complete local inventory read used at startup. It is
not catalog refresh or remote synchronization. Catalog-qualified project and
source/name/path-qualified shortcut selections survive when still present. An
invalid project falls back to the first project; an invalid shortcut falls back to
the first shortcut in the preserved project. Empty and failed reads clear selection
and retain their distinct UI states.

## Open Project Folder capability

Each project row has a separate, accessible folder icon. Activating it does not
activate the selection row. The operation path is:

```text
project row folder action
    ↓ projectKey lookup in headless application
LoadbotAdapter.openProjectFolder({ catalog, tool })
    ↓
Tauri open_loadbot_project(catalog, tool)
    ↓
launcher::resolve_project_directory
    ↓ existing operations::installed_tool_path validation
    ↓
explorer.exe <one literal path argument>  (Windows)
xdg-open <one literal path argument>      (Linux)
```

The frontend receives no native path, performs no path construction, chooses no
operating-system command, and invokes no shell. Rust resolves and validates the
installed Git repository using existing Loadbot configuration and qualified
identity. A missing, invalid, or mismatched project is a controlled adapter error.
Fixture composition implements the same adapter seam with a controlled unavailable
error and never opens a real directory.

The native capability is narrowly limited to the main window's two commands:
inventory read and qualified project-folder open. No general shell, filesystem,
opener plugin, command bus, mutation, catalog synchronization, or execution
capability was introduced.

## Phase boundary

Local-mode shortcut details show only existing semantic facts: name, optional
description, project, catalog, source, optional runner, and configured relative
target. There is no Run button in the real view. Fixture-only sample forms remain
available from explicit fixture hosts for frontend testing.

The terminal is resizable and initially visible, but it has no PTY, shell, input,
streaming, history, or fake output. Add/edit/delete actions, catalog management,
catalog synchronization, shortcut execution, and terminal implementation remain
future work. Phase 4B should preserve these seams and separately design mutation
reports/confirmation; it should not reuse folder opening as a generic action bus.
