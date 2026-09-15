# Real local read-only inventory — Windows and Linux

## Baseline and result

Started from clean, fetched, up-to-date `main` at
`6cbc60964b394b9ce7e514509635e87776c81410`, containing the completed architecture
pass and the maintainer-verified native Windows shell. This work is uncommitted;
there is no final commit SHA or new remote CI result to report.

The normal standalone entry now reads the current machine's **existing Loadbot
state**, using the same code on Windows and Linux:

```text
Paths::discover()
    ↓
launcher::read_project_inventory(paths, context)
    ├── operations::all_tools(paths, context)
    ├── shortcuts::load(paths.shortcuts())
    └── launcher::project_inventory(tools, shortcuts)
    ↓
Tauri read_loadbot_inventory() — one argument-free query, blocking worker
    ↓
createTauriLoadbotAdapter().readInventory()
    ↓
existing headless application → existing Loadbot view → existing UI skin
```

No CLI subprocess or human-readable CLI parsing is used. No new database,
inventory file, catalog copy, configuration layer, or platform-specific frontend
model was introduced. The headless controller and `LoadbotAdapter` signature are
unchanged.

## Authoritative inventory semantics and path ownership

| Information | Existing source and what is exposed |
| --- | --- |
| Configuration locations | `Paths::discover()` in Rust, honoring existing `LOADBOT_HOME` / `LOADBOT_CONFIG_HOME` overrides and platform defaults. React never locates files or expands environment variables. |
| Catalog registrations and tool definitions | `operations::all_tools` reads `config.toml`, checks local catalog repositories/origins, and loads their `catalog.toml` through existing parsers/validation. |
| Project inventory | Existing `launcher::project_inventory`: only projects with shared commands or personal shortcuts appear. Tools without either do not become invented empty project rows. |
| Project identity/provenance | Existing `catalog` and `tool` strings; both are retained. The sidebar shows the catalog. |
| Shortcut identity | `source`, `name`, `path`, qualified by project identity. Shared/personal collisions remain separate, including identical names and paths. |
| Description/runner/source | Existing metadata, when present. `Runner` uses its existing lowercase serialization; missing runner is displayed as **Runner not specified**, not guessed. |
| Path displayed | The backend's portable repository-relative shortcut `path`, passed through unchanged. No native paths are constructed, normalized, or converted by TypeScript. |
| Native project location | Rust `Paths::tool` can compute the expected installation destination, but existence is not established by inventory. It is deliberately not added to this read projection or used to enable folder opening. Future folder actions must send catalog/tool identity back to Rust. |
| Catalog/tool status | Other library queries can establish specific repository/status facts. They are not called or exposed by this inventory-only phase. No installed/healthy/synced/executable/ready badges are inferred. |

Personal shortcuts referencing missing tools or unregistered catalogs remain
visible, exactly as in the existing launcher inventory. They are saved definitions,
not proof of launchability. Run is always disabled.

The serialization projection is intentionally limited to existing `Project`,
`ProjectEntry`, and `EntrySource` semantic records. `Serialize` was added to those
types; optional description/runner fields are omitted when absent. Raw
`LocalConfig`, `ResolvedTool`, extra TOML fields, remote URLs, native filesystem
handles, and mutation/process reports are not serialized to the GUI.

The shared JSON test fixture at `tests/fixtures/gui-inventory.json` is checked
against the Rust query result and consumed by frontend boundary tests. It is test
data only, never runtime storage.

## Observational startup and failures

`read_project_inventory` is a small library composition of the established queries,
not a second discovery algorithm. Local Git calls are limited to repository-root
inspection (`rev-parse --show-toplevel`) and reading `remote.origin.url` with
`config --get`. Startup does not fetch, pull, clone, synchronize, inspect tool
contents, run scripts, invoke Rot, acquire write leases, or create configuration.
There is no automatic repair or persisted cache.

The original `all_tools` can return tools from readable sources while recording
typed `Notice::SkippedCatalog` diagnostics for missing, mismatched, malformed,
or unreadable catalogs. The CLI displays those warnings and keeps its current
behavior. This GUI's existing array contract cannot carry partial-read notices.
**The new complete-snapshot query therefore rejects any read with skipped
catalogs**, including their diagnostics in the error. It does not silently discard
warnings, substitute fixtures, or label partial data complete. Old notices in a
reused operation context do not poison a subsequent read.

The three outcomes remain distinct:

1. Complete read with projects: display the real inventory and select the first
   project/shortcut using the existing controller rules.
2. Complete read with no projects: display an explicit empty-inventory message.
   Absent optional config/shortcut files mean empty data according to existing
   Loadbot loaders; a clean machine is not automatically initialized.
3. Failed read: display **Inventory read failed** and the backend diagnostic.
   Invalid configuration/shortcuts, unreadable files, and any skipped registered
   catalog are errors, not empty successes.

Reads are not a transaction across independently changing source files. The
controller suppresses superseded responses as before; it does not introduce
stale-data caching, user refresh, or network synchronization. React development
StrictMode may issue an extra initial read; each is equally observational.

## Thin native boundary and compositions

- `src/gui/src-tauri/src/main.rs` registers **only** `read_loadbot_inventory()`.
  It creates `Paths`, `Unattended`, and `OperationContext` on a blocking worker,
  disables terminal access, and calls the library query. It accepts no path,
  executable, shell string, or operation selector from JavaScript.
- Success is a serialized project array; failure is `{ "message": "..." }`.
  `frontend/hosts/tauriInventoryAdapter.ts` invokes the query, validates the small
  record shape, preserves strings, and converts serialized errors into `Error`
  for the existing controller. There is one adapter for both operating systems.
- `build.rs` declares the single application command for Tauri's ACL;
  `permissions/inventory.toml` and `capabilities/main.json` permit that read on
  the local `main` window. No filesystem, shell, opener, execution, or mutation
  plugin permission was added. Native window controls retain native behavior.
- `frontend/hosts/realComposition.ts` supplies the real adapter and `mode: 'local'`.
  `standalone.tsx` always uses it. Missing native runtime is an explicit error,
  never a reason to fall back to fictional data.
- `fixtureComposition.ts` explicitly supplies the fixture adapter, demo forms,
  and `mode: 'fixture'`. The standalone fixture preview moved to **`/fixture.html`**;
  **`/embed.html`** remains the parent-owned fixture overlay. Both are dev-only
  HTML entries, excluded from the production build. There is no user-facing mode
  toggle and no Windows/Linux composition split.
- `LoadbotMenu` defaults to local presentation. It ignores supplied sample-form
  configuration in local mode. The adapter contract still has no input schema.

The small presentation-only mode changes source labels, loading/error/empty text,
and the no-inputs explanation. Real mode has no **Sample form ready** state or demo
fields. Layout, artwork, mascot, fonts, nine-slice skins, and selection controls
retain their established implementations. Add Project, Refresh Catalog, Run, and
Open Project Folder remain disabled. The drawer is still a labeled placeholder.

## Development and maintainer verification

Windows and Linux are first-class native targets, not separate applications.
WSL checks below are not a substitute for Linux desktop verification.

### Windows native

Use a Windows-local checkout of these changes, Windows Git, Node.js 22.12+ / npm,
current stable Rust MSVC, Visual Studio C++ Build Tools with the Windows SDK, and
WebView2 Runtime. Open a native PowerShell terminal at the repository root:

```powershell
Get-Command node, npm, git, cargo, rustc
git rev-parse HEAD
npm --prefix src/gui ci
npm --prefix src/gui run desktop
```

The existing empty-PATH PowerShell fix and regression coverage are preserved.
Vite continues to ignore `**/src-tauri/**`, avoiding locked Cargo DLL watcher
failures. Restart a previously running Vite/Tauri session after updating the
checkout; it may otherwise continue serving the earlier fixture entry.

### Linux native desktop

Use a graphical X11/Wayland Linux desktop with Git, Node.js 22.12+ / npm, and
current stable Rust. On Ubuntu 24.04, install Tauri development prerequisites:

```sh
sudo apt-get update
sudo apt-get install -y build-essential pkg-config libwebkit2gtk-4.1-dev \
  libgtk-3-dev libayatana-appindicator3-dev librsvg2-dev libssl-dev patchelf

# From the updated checkout's repository root:
git rev-parse HEAD
npm --prefix src/gui ci
npm --prefix src/gui run desktop
```

For other supported distributions, use their equivalent packages from the
[Tauri prerequisites guide](https://v2.tauri.app/start/prerequisites/). These
dependencies are documented here, not vendored or installed by CLI setup scripts.

### What to inspect on each machine

Check **LOCAL INVENTORY**, actual catalog-qualified projects, shared/personal
entries, descriptions/runners and unchanged relative paths. Confirm sample fields
and fixture labels are absent. Compare with that machine's existing Loadbot
launcher definitions (not an expectation that every installed tool has a project
row). Check selection, keyboard focus, long-list scrolling, resizing, drawer,
native close, and that all action controls remain disabled. Record platform and
commit separately. Inventory errors should be explicit without fabricated rows.

Both platforms use their existing local configuration conventions; they need not
show the same inventory. If using the established environment overrides for an
isolated verification directory, use absolute paths and the same overrides as
the CLI. No GUI-specific configuration file is involved.

Browser fixture development remains:

```sh
npm --prefix src/gui run dev
# http://127.0.0.1:1420/fixture.html
# http://127.0.0.1:1420/embed.html
```

The normal `/` entry in an ordinary browser reports that the native application
is required. Browser tests mock only the Tauri transport; they cannot verify
native IPC permissions, native lifecycle, or the maintainer's real inventory.

## Verification record

All backend verification used isolated temporary directories. No maintainer
configuration/catalog was read or modified by the new query during verification.

**WSL1 / Linux toolchain (not a native Linux desktop):**

- Rust formatting: root and independent desktop workspace passed.
- Clippy, `--locked --all-targets --all-features -- -D warnings`: passed.
- Rust tests: **155 passed**, plus **1 doctest**. New tests cover complete/empty/
  failed reads, original partial notices, qualified collisions, optional fields,
  broken references, JSON serialization, process allowlisting, and unchanged file
  bytes/directory entries including Git metadata and absence of lock sidecars.
- Shell setup checks: **60 passed**.
- Frontend strict TypeScript/Vite build: passed. Frontend tests: **16 passed**.
- Playwright: **6 passed** on the current checkout's test-owned Vite server.
  The four new browser scenarios cover real composition with a mocked structured
  native read, empty/error replies, and unavailable runtime without fixture fallback.
- Fixture screenshots: all **nine** PNG hashes still match the structural baseline.
  A screenshot of the local-mode projection was also captured with mocked transport.
- Native Linux check: attempted, blocked at missing GLib/GTK/WebKitGTK development
  libraries (`glib-2.0 >= 2.70`, etc.). No graphical session is available. No
  native Linux window or real-machine inventory was visually verified. This is
  a WSL environment limitation, not a dropped Linux product target.

**Native Windows through available PowerShell interop:**

- PowerShell 5.1 setup PATH regressions: **12 passed**.
- Native Windows Rust/Cargo **1.98.1** were located at the standard user-toolchain
  location after the inherited PowerShell PATH did not resolve them.
- **Windows Tauri `cargo check --locked --features custom-protocol`: passed**, using
  this checkout and its built frontend assets. This compiled the new command,
  permission configuration, semantic serialization, and Windows icon resources.
  The UNC checkout initially failed an incremental-cache lock operation; setting
  `CARGO_INCREMENTAL=0` only for the check resolved that filesystem limitation.
  Output is isolated under `src/gui/src-tauri/target/windows-check/`.
- **New native Windows inventory tests: 5 passed**, including the read-only file
  snapshot/process checks and shared JSON projection. Fixture repositories need
  no branch or commit, so these tests use plain `git init` on both platforms.
- Full Windows backend tests were also attempted: the library unit suite reported
  **39 passed / 13 failed** because the installed Git **2.24.1.windows.2** rejects
  existing tests' `git init --initial-branch` (requires Git 2.28+). Cargo stopped
  before the later suites; the new inventory suite was run separately. No existing
  check was weakened/skipped and the full Windows suite is not claimed green.
  Re-run with current Git for Windows, as provided by the Windows CI runner.
- Frontend build/tests: run under WSL above, not claimed as Windows results.
- Native GUI launch / real local inventory visual check: not performed here.

The maintainer's earlier successful native launch covers the prior baseline, not
this bridge. Native compilation and isolated inventory tests are not a claim of
visual verification against the maintainer's real data.

**CI:** the existing GUI workflow now runs native Tauri compilation on both
`ubuntu-24.04` (with system prerequisites) and `windows-latest`. Windows also runs
frontend tests; Linux retains the existing frontend/build/Playwright job. Existing
backend Linux/Windows jobs and PowerShell/shell regression steps remain intact,
and run the new Rust inventory tests. No new workflow or macOS symmetry job was
added. These changes are uncommitted/unpublished; no remote success is claimed.

Playwright now owns port **1421** and refuses to reuse an existing server. During
validation, port 1420 was found serving the maintainer's other Windows checkout,
which would have tested stale fixture code. That session was left untouched;
only final tests against the dedicated current-checkout server count above.
The native development port and watcher exclusion remain unchanged.

## Next phase, deliberately not implemented

Review explicit mutations/actions next: catalog sync, project operations, and
semantic project-folder opening, using backend-owned platform behavior and real
operation reports. Consider partial-inventory presentation separately if retaining
readable rows alongside failed catalogs becomes necessary; currently one skipped
catalog rejects the complete snapshot. Shortcut execution, output, cancellation,
and terminal/PTY behavior need their own later review. Rot integration and optional
cosmetic personality/model services remain unimplemented.
