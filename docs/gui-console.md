# Loadbot Console: Command and Activity

The lower workspace is the **Console**:

```text
Console
  ├── Command
  └── Activity
```

Command is a deterministic interface to Loadbot. Activity is the structured record
of real Loadbot operations. They share the parchment workspace and its persisted
pane size, but their state and purpose remain separate.

## Command boundary

Command is not an operating-system terminal. Loadbot does not start PowerShell,
`cmd.exe`, Bash, Fish, Zsh, or another shell; it has no PTY and cannot invoke an
arbitrary executable. The parser does not support pipes, redirects, variables,
substitution, chaining, backticks, or shell escape syntax. Windows and Linux use the
same command language.

The explicit registry initially provides only read-only operations that the current
semantic inventory supports:

```text
help
projects
shortcuts [project]
inspect <project> [shortcut]
```

`help` is generated from those registered definitions. `projects` lists the current
catalog context. With no argument, `shortcuts` uses the selected project. Project and
shortcut inspection returns the same real metadata already held by the application;
it does not parse human CLI output or reread files from React.

Whitespace separates tokens. Single or double quotes preserve names containing
spaces. A project may be qualified as `catalog/project`. An unqualified project uses
the current catalog when it uniquely matches. Ambiguous identities are never chosen
arbitrarily: Command returns qualified choices. A duplicate shortcut can be selected
with the displayed `source::name::path` identity.

The command registry and identity resolution live in the headless application layer.
It consumes the authoritative inventory snapshot obtained through `LoadbotAdapter`;
the view only renders structured results. No new Rust, Tauri, filesystem, Git, CLI,
or process capability is present. Future mutating commands must invoke the same
semantic controller action as the equivalent GUI control rather than establishing a
second implementation.

## Session state and Activity

Submitted commands and structured results form an in-memory transcript. Up and Down
recall the newest 100 submitted command lines; empty submissions are ignored. Neither
transcript nor recall history is persisted.

Read-only inventory queries do not add noisy Activity entries. Activity continues to
record actual reload, folder-open, catalog synchronization, and management events in
its existing bounded 250-entry session feed. An unknown, malformed, or unavailable
command reports an error in Command and never fabricates a successful Activity event.

## Product boundary

Loadbot remains a focused project/tool/shortcut manager and launcher. Shortcut
execution is not implemented by this Console phase. A future Rot application may own
a broader machine/workspace environment and a real system terminal; that capability
does not belong inside Loadbot and is not anticipated with a hidden shell abstraction.
