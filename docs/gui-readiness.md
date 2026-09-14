# GUI readiness and frontend contracts

Loadbot exposes a synchronous Rust library. A Tauri-based GUI can call this
library directly on a worker thread, constructing its `OperationContext` there.
The [Phase 1 standalone GUI](gui-phase1.md) now lives in `src/gui/`, with a separate
Tauri 2 host and an injected fixture adapter. The call flow below describes the
existing backend contracts for future connections; the GUI does not call them yet.

Rot is a separate Python application. The browser-only embedding example reuses
the menu inside a parent-owned container without Tauri. It demonstrates frontend
reuse only: future Rot hosting and backend transport still need design and
implementation. Phase 1 adds no Python bindings, server, cross-process protocol,
or terminal adapter, and does not modify Rot.

## Current module responsibilities

| Module | Responsibility |
| --- | --- |
| `lib.rs` | Public backend module surface; no CLI dependency. |
| `operations.rs` | Catalog/tool workflows, queries, validation, repository leases, and completed-step notices. |
| `catalog.rs` | Catalog/tool/command models, schema validation, and catalog document transactions. |
| `config.rs` | Catalog registrations, configuration models, and configuration transactions. |
| `shortcuts.rs` | Personal shortcut records, validation, load/save/removal, and name enumeration. |
| `paths.rs` | Explicit/platform locations and name/path rules; construction does not create state. |
| `git.rs` | Git inspection and guarded mutation, remote recovery, and versioned Rot identity discovery/retry. |
| `launcher.rs` | Project inventory, directory browsing, containment checks, interpreter selection, and managed launching. |
| `interaction.rs` | Frontend decisions, notices, operation context, reports, and outcome classification. |
| `persistence.rs` | Scoped OS leases and shared TOML replacement/recovery. |
| `process.rs`, `process_windows.rs` | Synchronous process events, bounded pipe draining, cancellation, and group/Job Object ownership. |
| `cli/mod.rs`, `cli/args.rs` | Argument parsing, dispatch, and CLI interactivity requirements. |
| `cli/menus.rs` | Terminal prompts and collection of menu choices. |
| `cli/operations.rs`, `cli/output.rs` | Backend adapters, immediate CLI rendering, and exit/error presentation. |
| `cli/launcher.rs`, `cli/shortcuts.rs` | Interactive inventory/browser/shortcut flows; execution and persistence stay in the backend. |
| `cli/completion.rs` | Shell and Rot completion using the CLI command tree and shortcut names. |

## Frontend call flow

1. Construct `Paths::with_directories(data, configuration)` for explicit storage,
   or use `Paths::discover()` for the established locations. Tests use temporary
   directories; CLI tests also set `LOADBOT_HOME` and `LOADBOT_CONFIG_HOME`.
2. Implement `Interaction`, or use `Unattended` for a policy that declines
   optional changes. Construct an `OperationContext` on the executing thread.
3. Set `context.process.terminal = false` and `context.tool_mode = Mode::Stream`
   for nonterminal work. Install a `process::Control::observer` for process and
   operation events; keep callbacks prompt and use bounded frontend storage.
4. Call `context.run(|context| operations::tool_list(&paths, context))`, or another
   public operation. Inspect the report even when `result` is an error.
5. Render the returned domain data in the frontend. Never parse CLI presentation
   to recover models, and never move backend validation into the view layer.

`OperationContext` is synchronous and is not a cross-thread job handle. The
caller can clone its cancellation token before starting work and cancel it from
another thread. Each independent operation/retry should get a fresh context/token.
There is no scheduler, serialization protocol, or GUI event queue in the library.

`Interaction::choose_identity`, `configure_push`, `reconcile_checkout`, and
`replace_push` carry decisions. A GUI supplies answers through those methods,
for example by synchronously coordinating with its UI from a worker thread.
Repository leases are released around decisions and relevant state is revalidated
afterward. Do not prompt in `notice` or process observer callbacks: they can run
while mutation leases are held. `Unattended` does not provide Git credentials;
configure noninteractive credentials separately when needed.

`Interaction::notice` delivers immediate domain progress, also retained in
`OperationReport::notices`. `report.status()` distinguishes `Succeeded`, `Busy`,
`Cancelled`, and `Failed`; `report.is_partial()` flags completed mutations or
uncertain persisted changes before an error. Keep the full error chain, including
`Busy`, `Cancelled`, `CleanupIncomplete`, `RemoteRecoveryIncomplete`, and
`DurabilityUncertain`. A launch error can contain `launcher::ChildExit`, whose
`code()` preserves the child's exit code. Do not infer rollback from failure.

The process observer receives operation start/finish and process arguments,
working directory, PID, stdout/stderr byte chunks, exit status, and errors or
cancellation. Output chunks can split text characters. Events describe sequential
processes within a synchronous operation; separate concurrent contexts should
have caller-owned correlation. Stream mode does not retain output for replay;
the frontend must retain what it needs within its own limits.

## Inventory, commands, and shortcuts

Use `operations::catalog_list`, `tool_list`, `catalog_status`, and `tool_status`
for lists/details, and `operations::all_tools` for resolved definitions. Load
`shortcuts::load(&paths.shortcuts()?)`, then pass the definitions and shortcut
file to `launcher::project_inventory`. Inventory merges catalog commands and
personal shortcuts; it is not an installation/launchability check and can include
broken shortcuts. Projects with no commands or shortcuts have no inventory row.

Pass the chosen entry's catalog, tool, path, runner, and source to
`launcher::launch_command`. It revalidates installation and canonical containment
and holds the managed repository lease. Use `launcher::run_shortcut` for a saved
name, and `browse_directory`/`safe_target` when selecting files. Explicit runners
and catalog commands use the tool root as working directory; inferred personal
scripts use the script directory. Preserve the inventory's `EntrySource`.

Create records with `Shortcut::new`, then use `shortcuts::save`, `load`, and
`shortcut_names`. After confirmation, use `remove_if_matches` with the displayed
record so a concurrent replacement is not deleted. These document APIs return
`Result` and do not emit operation-specific notices; `context.run` can wrap them
for reports and lifecycle events. Short persistence transactions finish safely
rather than being interrupted mid-replacement.

To add an operation, implement validation, leases, mutation, and notices once in
the appropriate backend module. Extend the result/notice types only as needed,
test the public operation using the existing isolated fixtures, then add thin CLI
and GUI adapters. Explicit commands and menus must keep using the same workflow.

## Terminal and cancellation limits

CLI tools default to inherited stdin/stdout/stderr. Git's captured CLI execution
can still use its terminal for credentials. GUI streaming uses null stdin and
separate stdout/stderr pipes; it is not a terminal. Interactive editors, prompts,
and full-screen terminal tools still need inherited execution in a real terminal
or a future terminal/PTY adapter. `terminal = false` disables Loadbot's terminal
handoff, not every child program's ability to open a terminal independently.

The backend drains both pipes concurrently and manages cancellation/cleanup.
Linux groups and Windows Job Objects have different ownership limits, especially
for detached processes, SSH, and WSL. See [the process and persistence notes](phase2.md)
for these limits, lock ordering, recovery, and partial outcomes. GUI-specific work
still includes worker/UI coordination, decisions, bounded display buffers,
incremental decoding, and any future terminal adapter.

## Rot compatibility and verification

Rot's external completion provider invokes
`loadbot rot complete <completed tokens> <current token>` with separate arguments.
The current token may be empty. Loadbot returns a JSON string array and no
presentation text; `[]` is a valid handled result, not a request for filesystem
fallback. Top-level and nested names come from the existing CLI tree; `shortcut`
includes `add`, `list`, and `remove`. `run` and `shortcut remove` complete only
saved names. Rot's helper also consumes `loadbot --version`. No command names or
argument shapes change in this pass.

Loadbot invokes `rot ssh identities --json` and accepts version 1 documents with
an `identities` array containing `alias`, nullable `username`, and `verification`.
It selects only verified safe aliases with a username, deduplicates aliases,
ignores unknown fields, and rejects malformed/unsupported documents. Discovery
is read-only; real verification can involve SSH probes, so isolated smoke tests
use an empty temporary home and no accounts or credentials.

`tests/library.rs::headless_library_session` exercises public queries, shortcut
transactions, inventory, streamed launching, notices, reports, and exit errors
without CLI parsing/rendering. Its parent captures the worker's actual output to
detect backend presentation leaks. Existing Phase 2 tests retain responsibility
for busy/partial/cancellation and process cleanup risks; CLI tests verify Rot JSON,
completion, argument shapes, and terminal passthrough. CI runs these on Linux and
Windows. A completed Pass 3 requires green required jobs on its final revision;
PowerShell setup tests remain distinct from Windows Rust coverage.

The frontend audit found no CLI rendering or decision logic in shared operations.
The expanded parallel tests exposed a Unix lease lifetime issue: a concurrent
fork could briefly retain a dropped lease's file description. Explicit unlock on
lease drop fixes that case; a deterministic duplicated-description regression
test covers it without timing-dependent sleeps.
