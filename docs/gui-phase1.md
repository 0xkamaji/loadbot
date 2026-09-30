# Standalone GUI development

Loadbot provides a Tauri 2 desktop target and a React/TypeScript menu built with
Vite. The menu uses the approved PNG controls and static mascot from
`src/gui/loadbot-gui-assets/`. Both reference images were visually inspected.
The normal native entry now displays [real local read-only inventory](gui-read-only.md).
Fixtures remain available explicitly for development. Mutations and execution are
still unavailable; the handoff's broader roadmap is not an implementation claim.

The consolidated `main` baseline has been launched natively successfully by the
maintainer. The subsequent structural pass and its local verification are described
in [GUI capability/application/presentation architecture](gui-architecture.md).

## Launch

From the repository root, with Node.js **22.12+** and npm installed:

```sh
npm --prefix src/gui ci
npm --prefix src/gui run desktop
```

The desktop is configured to open directly into the menu at 1000 × 680 logical
pixels, with native decorations and a minimum client size of 420 × 480. Native
resize/minimize/maximize/close behavior still needs manual verification on a
graphical desktop; the WSL1 verification environment cannot display this window.
Escape does not close the standalone menu. See the [verification record](gui-phase1-verification.md)
for separate local, remote CI, browser, and native results.

Browser-only development (no Rust or Tauri system dependencies):

```sh
npm --prefix src/gui run dev
```

- Fixture menu: <http://127.0.0.1:1420/fixture.html>
- Development-only parent overlay: <http://127.0.0.1:1420/embed.html>

The normal `/` entry requires Tauri for its real read. In an ordinary browser it
reports the missing native runtime rather than falling back to fixture data.

The overlay uses the same `LoadbotMenu` with the fixture adapter. Its parent owns
the dialog, close callback, Escape behavior, focus containment, and focus return.
The menu fits the parent's explicitly sized container; it never owns a viewport.
`embed.html` is not a production build entry, and its script is guarded by
`import.meta.env.DEV`. This is a reuse demonstration, not completed Rot integration.

For a compiled browser-only preview:

```sh
npm --prefix src/gui run build
npm --prefix src/gui run preview
```

Vite prints the preview URL (normally <http://127.0.0.1:4173/>).

## Platform prerequisites

Use a current stable Rust toolchain for desktop development, along with Node.js
22.12+, npm, and the [Tauri 2 platform prerequisites](https://v2.tauri.app/start/prerequisites/).
The root CLI retains its own Rust requirements and dependency graph.

- **Linux:** a graphical X11/Wayland session and development packages for WebKitGTK
  4.1, GTK 3, GLib, a C/C++ toolchain, pkg-config, OpenSSL, and the application
  indicator library. Ubuntu 24.04 is a suitable development baseline:

  ```sh
  sudo apt-get update
  sudo apt-get install -y build-essential pkg-config libwebkit2gtk-4.1-dev \
    libgtk-3-dev libayatana-appindicator3-dev librsvg2-dev libssl-dev patchelf
  ```

- **Windows:** native Windows Node/npm, Rust's MSVC toolchain, Visual Studio Build
  Tools with Desktop development with C++, Windows SDK, and WebView2 Runtime.
  Use a native Windows terminal or a properly configured Linux GUI environment;
  mixing Windows npm with WSL Linux Node is not supported.
- **macOS:** Xcode command-line tools and current stable Rust; the host uses the
  system WKWebView. Native macOS/Windows launch still needs platform verification.

The host uses the supplied header PNG as its window icon. On Windows, `build.rs`
places that exact PNG in an ICO container in Cargo's output directory for the
native resource compiler. No art is redrawn, resized, or added to the handoff.

### Finish native verification from Windows

For the Windows host of the current WSL1 environment, use **native PowerShell**
in a Windows-local checkout containing the reviewed verification fixes. Use
Windows-installed Node/npm, Git, Rust MSVC, Visual Studio's C++ workload/Windows
SDK, and WebView2 as described above. Do not reuse Linux `node_modules` or Cargo
output through the WSL filesystem.

From that checkout's repository root:

```powershell
Get-Command node, npm, git, cargo, rustc
node --version  # Must be 22.12 or newer
npm --version
rustup show active-toolchain  # Must be a Windows MSVC toolchain
git rev-parse HEAD
npm --prefix src/gui ci
npm --prefix src/gui run desktop
```

In the actual native window, compare the supplied `references/01-main-window.png`
and `references/02-interface-kit.png` beneath `src/gui/loadbot-gui-assets/`:

1. Check mascot, font, near-black/beige skins, and fixed pixel-border corners.
2. Select actual local projects and shortcuts; verify catalog/source qualification,
   descriptions and relative paths. Run must remain disabled, with no sample
   inputs injected into the native local-data view.
3. Toggle Console twice and check retained selection. The current drawer has
   `COMMAND` and `ACTIVITY` tabs; Command accepts only registered Loadbot commands,
   never shell input. See [Loadbot Console](gui-console.md).
4. Use Tab/Shift+Tab, arrow keys, Home/End, Enter/Space; check focus versus selection.
   Scroll the long project/shortcut labels and resize down to 420 × 480. Run and
   the toolbar must remain reachable by scrolling.
5. Exercise native minimize/maximize/restore and Close. Check that closing the
   window exits the native process; stop the development watcher with Ctrl+C.
6. Confirm the LOCAL INVENTORY label and disabled Run/catalog/folder actions throughout.
   Record the commit, OS, scale factor, screenshots, and any native-console errors
   in the verification record. Browser screenshots do not satisfy this step.

## Build isolation

`src/gui/src-tauri/Cargo.toml` is an **independent Cargo workspace** with its own
tracked `Cargo.lock`, and depends on the root `loadbot` library via `../../..`.
It registers only `read_loadbot_inventory`, delegating to the existing library's
read-only inventory composition. The root dependency graph and CLI contracts
remain independent of Tauri. Normal
`cargo build`, `cargo test`, and `cargo install --path .` do not resolve or compile
Tauri. Node dependencies and generated output are ignored by Git.

`custom-protocol` enables embedded production assets when building the desktop
target. Tauri development uses the Vite URL. Bundling is disabled; this phase
contains no installer configuration or terminal integration.

## Component and adapter boundaries

Paths below are relative to `src/gui/`:

| Location | Responsibility |
| --- | --- |
| `frontend/loadbot/LoadbotMenu.tsx` | Composition of injected capabilities, application state, and Loadbot presentation. |
| `frontend/loadbot/contract.ts`, `identity.ts` | Semantic inventory read interface and catalog/source-qualified identities; no widget metadata. |
| `frontend/loadbot/application/` | Headless deterministic state/actions and optional sample forms; thin React binding. |
| `frontend/loadbot/view/` | Loadbot layout, labels, sample widgets, unavailable-action presentation, mascot and responsive composition. |
| `frontend/ui/components.tsx` | Application-neutral frames, buttons, menu rows/lists, inputs/path selectors, checkboxes, status and drawer primitives. |
| `frontend/ui/theme.ts`, `theme.css` | Approved skin, tokens, asset URLs, font, spacing and control states, using manifest nine-slice measurements. |
| `frontend/loadbot/fixtures/` | Inventory adapter and separately injected UI-demo configuration. No filesystem/configuration access. |
| `frontend/hosts/fixtureComposition.ts` | Explicit choice of fixture adapter and sample forms for the development preview and overlay. |
| `frontend/hosts/realComposition.ts`, `tauriInventoryAdapter.ts` | Normal Windows/Linux native entry: real semantic read through the Tauri query, without sample forms. |
| `frontend/hosts/standalone.tsx`, `host.css` | Standalone mount and viewport sizing; also works in a normal browser. |
| `frontend/hosts/embed.tsx`, `embed.html` | Development-only parent-owned overlay and focus lifecycle. |
| `src-tauri/` | Tauri setup, native window configuration/lifetime, Rust library dependency. Future native calls belong here and in a host-side adapter. |

For another frontend parent:

```tsx
import { LoadbotMenu } from './frontend/loadbot/LoadbotMenu';
import { fixtureMenuDependencies } from './frontend/hosts/fixtureComposition';

<div style={{ width: '100%', height: 600 }}>
  <LoadbotMenu {...fixtureMenuDependencies} host={{ onClose: closeParentOverlay }} />
</div>
```

Loadbot-specific styles are scoped to `.lb-theme`. The parent's height must be
defined. Only the standalone/example hosts style `html`, `body`, and `#root`.
There are no Tauri frontend imports anywhere in the reusable menu.

## Explicit development fixture behavior

- Project selection updates shortcuts and selects the first available entry.
- Shortcut selection updates description, source, repository-relative path,
  runner, and supported **sample** controls. Entry identity includes catalog/tool
  and source/name/path, so duplicate project or shared/personal names do not collide.
- Sample path buttons set explicitly labeled in-memory values; editable text/path
  inputs and checkboxes update local form state. They are not native file pickers.
- Required fields treat whitespace as empty, expose `aria-invalid`, and describe
  missing inputs in a live status message. Completing the form says **Sample form
  ready. Execution is not connected.** Run remains disabled.
- Switching project/shortcut resets the form to that entry's sample defaults.
  Opening/closing the drawer leaves selection, values, and checkbox state intact.
- Add project, Refresh catalog, Open project folder, and Run are disabled with
  nearby explanations. No sample operation is presented as a successful mutation.
- The beige drawer from this historical phase is now the **Console**, with a narrow
  Loadbot-specific **COMMAND** view and structured **ACTIVITY**. It remains without
  a shell, PTY, arbitrary process input, or simulated execution output.
- Tab/Shift+Tab move among enabled controls. Arrow Up/Down and Home/End move focus
  within a list; Enter/Space activate a focused row. Focus and hover do not change
  selection. Selected skins stay selected on hover. Lists scroll independently;
  short windows allow workspace/panel scrolling, and narrow parent containers stack
  projects over shortcuts. Toolbar and drawer remain outside that scroll region.

## Asset fidelity

The runtime imports supplied PNG masters, not reference screenshots or exported
slice pieces. CSS border-image uses the manifest's fixed 12px window corners,
8px panel/titlebar/input/terminal corners, 6px button/menu corners, and 4px focus
overlay. Content padding subtracts the border thickness. Checkboxes/icons retain
their 24px/16px sizes. The mascot keeps its aspect ratio.

The bundled DejaVu Sans Mono is used for all real labels; it is intentionally not
an exact match for the generated reference lettering. Its original license stays
in the handoff and is emitted into `dist/licenses/LICENSE-DejaVu.txt`. Manifest
text colors accompany the original black/beige skins. The corrected handoff
instructions and path-base descriptions have updated `SHA256SUMS.txt` entries;
valid relative asset paths and all asset bytes are preserved.

## Checks

```sh
# Repository root — CLI/library checks need no desktop prerequisites.
cargo fmt --all -- --check
cargo fmt --manifest-path src/gui/src-tauri/Cargo.toml --all -- --check
cargo check --locked --all-targets --all-features
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo test --locked --all-targets --all-features
cargo test --locked --doc
sh tests/setup_sh_test.sh

# Frontend checks.
npm --prefix src/gui run build
npm --prefix src/gui test
npm --prefix src/gui exec -- playwright install chromium
npm --prefix src/gui run test:browser

# Desktop compilation, after frontend build and native prerequisite installation.
cargo check --locked --manifest-path src/gui/src-tauri/Cargo.toml --features custom-protocol
```

The browser tests capture screenshots under `src/gui/test-results/`: initial
1000 × 680 and 722 × 480 views, keyboard/input focus, open drawer, long labels,
420px/722px resized drawer views, and the parent overlay. They check selection vs
hover/focus, scrolling, form preservation, toolbar/Run reachability, container
bounds, parent close/Escape, and lack of a Tauri runtime. Component tests cover
input isolation, shared/personal identity, injected/empty/error data, stale adapter
responses, and the optional close callback.

See [the verification record](gui-phase1-verification.md) for the actual local
results and environment limitations.

## Current data boundary and next phase

The [read-only inventory guide](gui-read-only.md) documents the implemented Rust
query, Tauri boundary, error policy, path ownership, and separate Windows/Linux
verification results. It also lists exact native launch commands and Linux system
packages. The next review can add deliberate mutations/actions; shortcut execution,
output and terminal/PTY behavior remain separate future work. Sample forms are
never a real argument schema. Rot and personality/model integration remain future work.
