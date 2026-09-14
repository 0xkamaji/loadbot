# Phase 1 verification record

## Verified locally

Validation ran on Linux under WSL1, using Rust/Cargo 1.98.0, temporary Node.js
22.22.0, npm 10.9.4 (npm 11 was used once to resolve a dependency-upgrade issue),
and temporary Git 2.47.3 for the repository tests. The system's Node 10 and Git
2.25.1 were too old; the latter rejects the tests' `git init --initial-branch`.
Temporary tools were installed beneath `/tmp/opencode`, without changing the
repository's setup scripts or user configuration.

| Check | Result |
| --- | --- |
| `npm ci` in `src/gui/` | Passed from the tracked lockfile; audit reported zero vulnerabilities. |
| `npm run build` | TypeScript check and production Vite build passed. Font, original PNGs, and font license emitted; embedding entry excluded. |
| `npm test` | 4 focused component tests passed, including injected adapter rendering outside Tauri. |
| `npm run test:browser` | 2 Playwright tests passed under Chromium 145 with the WSL1 workaround below. |
| `cargo check --locked --all-targets` | Passed for the CLI/library, without desktop dependencies. |
| `cargo clippy --locked --all-targets -- -D warnings` | Passed. |
| Rust library unit tests | 58 passed. |
| Rust CLI unit tests | 36 passed. |
| `tests/cli.rs` | 32 passed; one existing test failed on a filesystem rename, described below. |
| `cargo test --locked --test library --test phase2` | 14 library integration and 9 process/persistence tests passed. |
| `cargo test --locked --doc` | 1 passed. |
| `sh tests/setup_sh_test.sh` | All 60 checks passed. |
| Desktop crate `cargo fmt --check` | Passed. |
| Handoff `sha256sum --check --quiet SHA256SUMS.txt` | Passed after updating the four edited document/manifest entries. |

### Browser launch and visual inspection

Both supplied reference PNGs were opened before implementation. The actual React
menu was then launched through Vite and inspected using Playwright screenshots,
not just DOM tests or the handoff's static HTML. Screenshots were opened and
visually compared for composition, palette, frame corners, control skins, font,
mascot, hover/selection contrast, focus, long text, and the beige drawer.

Screenshots are generated under `src/gui/test-results/` (ignored build/test
artifacts). The browser test names determine the containing directory:

- `main-1000x680.png`: full initial two-column menu with all three initial shortcuts.
- `main-722x480.png`: short two-column client area; contents scroll vertically.
- `keyboard-focus.png`: focused unselected project remains distinct from selection.
- `input-focus.png`: supplied transparent 4px ring over the input's 8px skin.
- `long-labels.png`: wrapped project/detail text and persistent selection on hover.
- `drawer-1000x680.png`: selected terminal toggle and clearly labeled placeholder.
- `small-drawer-722.png`, `small-drawer-420.png`: resized view after scrolling Run
  into view; the toolbar and open drawer remain reachable.
- `parent-overlay.png`: the same menu inside a parent-owned 1000 × 680 container
  in a 1200 × 850 browser, with a parent close action.

The first visual pass exposed a clipped third shortcut and an overly specific
hover selector that replaced the selected skin. Layout and selectors were fixed;
the final browser tests include a selected-on-hover regression check. Tests also
verified keyboard activation, required input state, retained values/checkboxes
across drawer toggles, a 19-entry shortcut list scrolling to its last row, and
parent close/Escape/focus return without a Tauri runtime.

Chromium's normal multiprocess renderer hangs on this WSL1 host. For local
validation only, the Playwright configuration supports:

```sh
LOADBOT_BROWSER_SINGLE_PROCESS=1 npm --prefix src/gui run test:browser
```

This adds `--no-zygote --single-process --disable-gpu`. The two independent browser
tests run in separate workers. The normal configuration and CI use standard
Chromium launch behavior. This workaround is unrelated to application behavior.

## Blocked or unverified checks

- **Native desktop launch:** `npm run desktop -- --no-watch` successfully started
  Vite and invoked the separate Cargo application, then compilation failed in
  `glib-sys`: `glib-2.0 >= 2.70` was unavailable. A direct desktop `cargo check`
  failed at the same native prerequisite boundary. GTK 3 and WebKitGTK 4.1
  development packages are also absent, and neither `DISPLAY` nor
  `WAYLAND_DISPLAY` is set. No native window was displayed or photographed.
  Native decoration/lifecycle, WebKit rendering, Windows icon/resource handling,
  and Windows/macOS launch remain unverified. The added GUI CI workflow includes
  a Linux desktop compilation job with prerequisites; that remote job was not run
  during this session.
- **One existing CLI integration test:**
  `successful_registration_survives_sync_network_failure` fails at
  `tests/cli.rs:290` with `PermissionDenied` from renaming its temporary Git remote
  directory. It also fails when run alone. The root backend/test sources are
  unchanged by this phase; the full `cargo test --locked` result is therefore not
  green on this host. The later library/process suites were run separately.
- **Root formatting:** `cargo fmt --check` reports existing formatting differences
  in `src/git.rs`, `src/persistence.rs`, `tests/cli.rs`, and `tests/library.rs`.
  Those files were not edited. Formatting of the new desktop crate passes.
- **Platform coverage:** Windows Rust tests, PowerShell setup tests, native
  Windows/macOS desktop checks, high-DPI scaling, and physical mouse/touch use
  were not run here.

## Remaining visual and integration limits

The bundled DejaVu font is the approved implementation fallback, not the exact
generated pixel lettering in the references. Runtime PNG textures differ slightly
from the flattened reference art; the original skins and manifest measurements
are used directly. Short windows and an open drawer can require scrolling the
details to reach Run; at narrow widths the project/shortcut panels stack. Browser
tests verified reachability at 420 × 480, not simultaneous visibility of every
control at every size.

All menu data and input values are fixtures. Run, catalog changes/refresh, native
pickers, project-folder opening, and terminal functionality are unconnected. No
execution, configuration mutation, catalog access, Python binding, backend server,
cross-process protocol, installer, or Rot change was implemented. Phase 2
connection points are documented in [the GUI guide](gui-phase1.md).
