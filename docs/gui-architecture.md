# GUI capability / application / presentation boundary

## Baseline and scope

The structural pass started on clean, fetched, up-to-date `main` at
`5503ae9ce0cbbd65d54f9b3bf0ae6c6b44e0549f`. This is the authoritative consolidated
baseline, including the successfully launched native Phase 1 shell (reported by
the maintainer), GUI verification cleanup, PowerShell empty-PATH fix/regression
coverage, and Windows Vite/Tauri watcher exclusion. Do not restart from an older
`fix/*` branch.

The subsequent [real read-only data phase](gui-read-only.md) started from clean
`main` at `6cbc60964b394b9ce7e514509635e87776c81410`, after the maintainer launched
the refactored native Windows shell successfully. The normal standalone entry
now uses the existing Rust inventory through one platform-neutral Tauri query.
Fixtures remain explicit development/test composition. No mutation, execution,
terminal, Rot integration, or model service is connected.

The [Phase 4A workspace pass](gui-workspace.md) starts from clean `main` at
`697ef45c1847371224d9ef30118ff81272462295`. It adds read-only workspace navigation,
local inventory reload, a qualified project-folder capability, and presentation-only
split persistence. It does not add management mutation or execution.

## Before and after

Previously, `frontend/menu/LoadbotMenu.tsx` combined asynchronous adapter reads,
selection, form defaults/validation, drawer state, wording, and layout. The adapter
types contained both inventory records and `previewFields` describing controls.
`menu/components.tsx` contained reusable controls but also a hard-coded Loadbot
frame label, fixture path-selection behavior, and terminal-placeholder text.
One stylesheet mixed skins and application composition.

Now the dependency direction is:

```text
Loadbot core: launcher::{read_project_inventory, resolve_project_directory}
           ↓ thin Tauri worker commands
LoadbotAdapter contract ← real adapter OR explicit fixture adapter
           ↓
headless Loadbot application state/actions ← injected sample-form configuration
           ↓ React binding
Loadbot layout and wording
           ↓
local reusable UI primitives + approved skin

Host composition supplies the adapter, optional demo configuration, and Close.
Native window ownership / browser dialog lifecycle remain outside the menu.
```

### Module map (relative to `src/gui/frontend/`)

| Module | Responsibility |
| --- | --- |
| `loadbot/contract.ts` | `LoadbotAdapter`, `LoadbotProject`, `LoadbotShortcut`: consumed read capability and semantic inventory data. No React, host, fixture, filesystem, Rust, or control types. |
| `loadbot/identity.ts` | Catalog-qualified project and source-qualified shortcut identity. No absolute installation paths. |
| `loadbot/application/controller.ts` | `createLoadbotApplication`, `LoadbotState`, `LoadbotActions`: loading/error/ready state, qualified local selection, local reload, qualified folder-open state, sample values/validation, drawer state, subscriptions and read lifetime. Pure TypeScript; no DOM or React runtime. |
| `loadbot/application/sampleForms.ts` | Optional local demonstration fields/defaults and validation helpers. Separate from the backend contract. |
| `loadbot/application/useLoadbotApplication.ts` | Thin React binding using `useSyncExternalStore`; owns subscription/effect cleanup. |
| `loadbot/LoadbotMenu.tsx` | Public composition component accepting an adapter, optional sample forms, and optional shell callbacks. |
| `loadbot/view/LoadbotMenuView.tsx`, `ShortcutDetails.tsx`, `menu.css`, `workspaceLayout.ts` | Loadbot presentation: quiet catalog context, project navigation/folder affordances, shortcut list/details, local reload, GUI-local pane preferences, fixture sample widgets, mascot, and non-executing terminal workspace. Receives state/actions, not an adapter. |
| `loadbot/fixtures/adapter.ts` | Fictional inventory behind `LoadbotAdapter`; each read returns an independent snapshot. |
| `loadbot/fixtures/sampleForms.ts` | Separate UI-demo configuration keyed by qualified selection identity. Never sent to a backend. |
| `ui/components.tsx`, `ui/Splitter.tsx` | Application frame, panel, button/icon button, menu row/list, splitter, input/path-selector, checkbox, status and drawer primitives. Generic labels, values, content and callbacks; no Loadbot data imports. |
| `ui/theme.ts`, `ui/theme.css` | Approved PNG asset mapping, nine-slice tokens, colors, font, spacing, control states and shared shell skins. |
| `hosts/tauriInventoryAdapter.ts` | One Windows/Linux real adapter: invokes inventory read and qualified folder-open commands, checks the structured projection, and normalizes errors, not paths. |
| `hosts/realComposition.ts` | Normal standalone composition: real adapter, local/read-only presentation, no sample forms. |
| `hosts/fixtureComposition.ts` | Explicit development/test composition choosing the fixture adapter and sample forms. |
| `hosts/standalone.tsx`, `embed.tsx`, `host.css` | Viewport or parent-overlay ownership, mount/unmount, parent Close/Escape/focus behavior. The browser overlay is still dev-only. |
| `../src-tauri/` | Native lifecycle and independent workspace; inventory read delegates to the library, while folder opening resolves catalog/tool identity in Rust and passes one literal path to the platform file manager. |

## Consumed capability contract

```ts
interface LoadbotAdapter {
  readInventory(): Promise<readonly LoadbotProject[]>;
  openProjectFolder(project: { catalog: string; tool: string }): Promise<void>;
}
```

This remains a small semantic contract, rather than a speculative action bus.
The existing backend inventory groups command/shortcut entries under catalog/tool
projects (`launcher::project_inventory`), and the current UI consumes a complete
snapshot. A second asynchronous `listShortcuts` request is unnecessary for this
UI: selecting a project derives its entries locally.

Records retain catalog/tool, name, repository-relative path, optional description,
optional runner identifier, and catalog/personal source. These are Loadbot domain
concepts, not executable strings or Rust implementation handles. The adapter
returns caller-owned snapshots and rejects on failure. Callers treat returned
records and application snapshots as readonly. The Tauri bridge serializes this
minimal projection; it adds no capability-discovery system or versioned helper
protocol to the TypeScript port.

The old fixture-only `mode` discriminator is removed from the backend interface;
being a fixture is a composition choice. Presentation receives a separate
`local`/`fixture` mode, defaulting to local. Only explicit fixture composition
retains fixture labels and sample forms. See [read semantics and failures](gui-read-only.md#observational-startup-and-failures)
for the complete-snapshot policy: a skipped catalog is an explicit read failure,
not an invented partial-health state or silent empty success.

`LoadbotActions` contains only working local transitions:

- `selectProject(id)` / `selectShortcut(id)`
- `reloadInventory()` (local reread only; no fetch, pull, or synchronization)
- `openProjectFolder(id)` (qualified identity to adapter; controlled result state)
- `changeSampleInput(id, value)` / `useSamplePath(id)`
- `toggleDrawer()`

Only reload and folder-open call the adapter. `start()` belongs to controller
lifecycle and reads inventory; neither initial read nor reload is Git catalog
synchronization. Add/edit/delete, catalog management, Run, and terminal execution
remain absent and have no pretend adapter methods.

## Application state and lifetime

`createLoadbotApplication(adapter, sampleForms?)` creates an independent controller.
`getSnapshot()` is stable between transitions; `subscribe()` returns an unsubscribe
callback. `start()` returns cleanup that invalidates the read generation. Responses
or errors arriving after cleanup or a newer start are ignored, including React
StrictMode's setup/cleanup/setup cycle. This is stale-response suppression, not
cancellation of backend work.

Initial successful loading selects the first project/shortcut. Reload preserves
qualified selections when present and otherwise safely selects the first valid
project/shortcut. Unknown selection IDs are
ignored; reselecting the current item preserves inputs. Changing selection resets
to that selection's sample defaults. Drawer toggles preserve selection and form
values. Each controller has isolated state. The React binding creates a new
controller when the injected adapter or sample-form object changes, so parents
should keep those dependencies stable for the life of a menu session.

Required-input state is a list of field IDs, not user-facing success text. The
view chooses wording and widgets (for example, a semantic boolean sample value
is presented as a checkbox). Input readiness never implies execution success.
Backend failures are distinct from empty successful inventory snapshots.

### Sample forms are not a Loadbot input schema

The Rust inventory has no input/output argument schema. The fixture's old
`previewFields` have therefore been removed from its inventory records. Hosts
explicitly inject `SampleForms` alongside the adapter to preserve the demonstration.
Omitting that configuration produces no sample inputs, even for the same inventory.
The application owns sample defaults and whitespace validation; the view decides
how to render them. `ui/PathSelector` simply composes an input and supplied action;
it neither chooses a sample path nor opens a filesystem picker.

## Reuse and limits

The UI primitives are genuinely application-neutral in semantics. Tests render
an example utility frame, destination selector, status and drawer with no Loadbot
adapter or project/shortcut records. They retain the approved Loadbot skin and
`.lb-*` token namespace: this is a local reusable set, not a theme-independent
framework/package. A second design system could instead consume the headless
application and semantic contract without importing this UI directory.

Projects, catalogs, shortcuts, logical identities, selection rules, demo forms,
runner/source wording, source labeling, and Loadbot's two-column layout remain
explicitly Loadbot-specific. DOM focus and arrow-key behavior stay in the list
primitive; native/window focus and Close stay in the host. Loading/empty/error
messages remain Loadbot presentation over a generic status primitive; no unnecessary
generic screen framework was extracted.

Future direction, **not implemented integration**:

```text
                 Loadbot core
                      ↑
                Loadbot adapter
                      ↑
       ┌───────────────┬───────────────┐
       │ standalone UI │ future Rot UI │
       └───────────────┴───────────────┘

Helper A ─ adapter ─┐
Helper B ─ adapter ─┼─> Rot host/UI
Loadbot  ─ adapter ─┘
```

Each helper may be Rust, Python, or another language. A future host-side adapter
translates that helper's real capabilities; the helper does not choose buttons,
menus, layout, theme, or its host. Language/transport differences stay at that
implementation boundary. No generic helper registry, plugin lifecycle, Python
binding, or shared repository is needed to prove the seam here.

## Optional personality seam (future only)

Application facts and transitions are deterministic and separate from rendered
text. A future personality service may observe a completed deterministic event
and supply **cosmetic text only** in a separate presentation slot/state. It must
not receive the adapter, action callbacks, mutable application state, execution
authority, filesystem access, or permission decisions. Do not parse personality
text back into functional state or replace result/status messages with it.

The future service must be asynchronous, failure-isolated, and optional; a static
text fallback must retain all functionality. Never await it in the action/result
path. Subscription infrastructure here is for application/UI state; it is not a
model API or an excuse to run a model inside state transitions. No events such as
`shortcut_completed` are emitted now because execution does not exist here.

## Read-only implementation and next review

The real adapter uses the original semantic inventory and qualified identities.
Path/configuration discovery stays in Rust's `Paths`; no frontend OS checks or
native-path reconstruction were added. Folder opening returns catalog/tool identity
to Rust, which reuses existing installation validation before choosing the OS file
manager. The inventory projection deliberately does
not expose catalog health or installation status. Local presentation omits sample
forms and does not claim launchability. The controller's `start()` remains a local
initial read, not catalog synchronization.

Review explicit management mutations as the next phase, including report/decision
semantics. Execution/output/terminal integration remains separately reviewable.
No management or execution actions are implemented. See the
[read-only guide](gui-read-only.md) for current Windows/Linux verification status
and launch instructions; the structural results below are historical.

## Historical verification of the structural pass

- Root and independent-desktop `cargo fmt ... --all -- --check`: passed.
- `cargo clippy --locked --all-targets --all-features -- -D warnings`: passed.
- `cargo test --locked --all-targets --all-features`: 150 passed; doctest: 1 passed.
- `sh tests/setup_sh_test.sh`: 60 passed. Native Windows PowerShell 5.1 via WSL
  interop ran `tests/setup_path_test.ps1`: all 12 PATH regressions passed. PowerShell
  7 was not run in this session. The platform-fix files and CI coverage have
  no diff from the starting commit, including Vite's `**/src-tauri/**` exclusion.
- Frontend `npm run build` (strict TypeScript + Vite): passed; `npm test`: 10 passed,
  including headless state/fixture isolation, import-boundary enforcement, and
  non-Loadbot primitive rendering. No separate frontend formatter/linter is
  configured; existing style is retained and TypeScript's strict checks are run.
- Existing Playwright browser/UI scenarios: 2 passed. All **nine** before/after
  screenshot PNGs have identical SHA-256 hashes, including small sizes, focus,
  long labels, drawer, and overlay. Before images were captured from the clean
  baseline under `/tmp/opencode/gui-boundary-before/`; after images are in
  `src/gui/test-results/`. Main/small-window screenshots were visually reopened.
- Handoff `SHA256SUMS.txt`: passed; artwork/font/license are unchanged.
- Native Linux `cargo check --locked --manifest-path src/gui/src-tauri/Cargo.toml
  --features custom-protocol`: blocked by missing `gobject-2.0 >= 2.70`, GLib, GTK
  and WebKitGTK development prerequisites. This is WSL1, with no DISPLAY/Wayland
  session; no native GUI was visually verified in this pass. The maintainer's
  successful native baseline launch is acknowledged, not claimed as a new test.

Validation used the existing temporary Linux Node 22.22.0/npm 10.9.4 and Git 2.47.3
under `/tmp/opencode`, Rust 1.98.0, and Chromium's existing WSL1 single-process
test option. No dependencies, lockfiles, native targets, or setup scripts changed.
No publication/remote-CI claim is made for this uncommitted structural pass.
From a Windows-local checkout with native prerequisites, recheck with
`npm --prefix src/gui ci` and `npm --prefix src/gui run desktop`.
