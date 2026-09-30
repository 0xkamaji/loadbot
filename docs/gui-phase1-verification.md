# Phase 1 verification record

## Current consolidated baseline

`main` now contains the completed cleanup below, the PowerShell empty-PATH fix and
regressions, and the Vite/Tauri watcher fix. The maintainer has reported a successful
native GUI launch. The following sections are a **historical record** of the earlier
verification environment/publication state, not instructions to resume an old branch.
For the structural pass starting from `5503ae9ce0cbbd65d54f9b3bf0ae6c6b44e0549f`
and its fresh check results, see [GUI architecture](gui-architecture.md).
The subsequent [real read-only inventory phase](gui-read-only.md) supersedes the
fixture-only runtime limitations below and records Windows/WSL/Linux status separately.

## Historical follow-up status — 2026-09-14

The checkout started clean on `main` at
`bf4fff5f8167d8203ad5bb697c2b011ba2d0cd64`. Follow-up changes are on the local
`fix/gui-phase1-verification` branch and are **uncommitted/unpublished**. The local
results below cover that working tree; the remote results cover only the published
SHA above. Phase 1 is not fully signed off until the follow-up is published and
checked by CI and native window inspection is recorded.

### Formatting and separate fixture correction

- Root and independent desktop formatting checks were run before and after
  cleanup, including `--all`. Formatting-only changes affect `src/git.rs`,
  `src/persistence.rs`, `tests/cli.rs`, and `tests/library.rs`. The reviewed diff
  consists of rustfmt line breaks, trailing commas, and equivalent closure blocks;
  it introduces no runtime behavior changes. Desktop source already conformed.
- A separate hunk in `successful_registration_survives_sync_network_failure`
  corrects a test-fixture portability defect: register its temporary remote with
  a `file:///…` URL so Git uses its transport rather than local-clone hardlinks.
  There is no production backend fix. All original rename, failed-sync,
  byte-for-byte configuration preservation, checkout existence, and registered
  catalog assertions remain. No test was skipped, weakened, or removed.

### Rename failure investigation

Before the correction, the test failed both normally and under `strace`:

```text
rename("/tmp/.tmpy030DD/catalog.git", "/tmp/.tmpy030DD/catalog.git.offline")
    = -1 EACCES (Permission denied)
```

The trace shows Loadbot's lease handles closed and its process exited before the
failing rename. The user is UID/GID 1000, and `stat -f` identifies both `/tmp` and
the checkout as `wslfs`. A separate temporary Python reproducer, without Git or
Loadbot, created a UID-owned mode-0700 directory containing one closed file:

| File permissions | Link count | Rename directory | After unlinking external hardlink |
| --- | --- | --- | --- |
| 0644 | 1 | Success | — |
| 0444 | 1 | Success | — |
| 0644 | 2 | EACCES | Success |
| 0444 | 2 | EACCES | Success |

A Git-only control then initialized/pushed a bare remote and cloned it: a local
path produced remote object link counts `[2, 2]` and the same rename failure;
`file:///…` produced `[1, 1]` and a successful rename. This establishes the WSL1
hardlink/directory-rename limitation and the fixture's dependency on Git's local
optimization, rather than attributing the failure to an unchanged source file.
Diagnostic artifacts are `/tmp/opencode/rename-test.trace` and
`/tmp/opencode/rename-probe.py`; they are temporary and are not runtime code.

## Verified locally

Validation ran on Ubuntu 20.04 under **WSL1**, kernel
`4.4.0-19041-Microsoft`, using Rust/Cargo 1.98.0, existing temporary Linux Node.js
22.22.0/npm 10.9.4 and Git 2.47.3 from `/tmp/opencode`. Default PATH resolves Node
10.19.0 and Windows npm under `/mnt/c/Program Files/nodejs/`, which fails with
`WSL 1 is not supported`; the system Git 2.25.1 rejects `--initial-branch`.
Validation commands prepended the existing Linux tool directories to PATH, with
no user configuration or system prerequisite changes.

| Check | Result |
| --- | --- |
| `npm ci` in `src/gui/` | Passed from the tracked lockfile; audit reported zero vulnerabilities. |
| `npm run build` | TypeScript check and production Vite build passed. Font, original PNGs, and font license emitted; embedding entry excluded. |
| `npm test` | 4 focused component tests passed, including injected adapter rendering outside Tauri. |
| `npm run test:browser` | 2 Playwright tests passed under Chromium 145 with the WSL1 workaround below. |
| `cargo fmt --all -- --check` | Passed after formatting-only cleanup. |
| `cargo fmt --manifest-path src/gui/src-tauri/Cargo.toml --all -- --check` | Passed, including the local library dependency. |
| `cargo check --locked --all-targets --all-features` | Passed for the CLI/library, without desktop dependencies. |
| `cargo clippy --locked --all-targets --all-features -- -D warnings` | Passed. |
| Rust library unit tests | 58 passed. |
| Rust CLI unit tests | 36 passed. |
| `tests/cli.rs` | All 33 passed, including the corrected remote-unavailability fixture. |
| Focused file-URL path check | The corrected test also passed with `TMPDIR="/tmp/opencode/with spaces"`; this additional run supplements the full suite above. |
| Library/process integration | 14 library integration and 9 process/persistence tests passed. |
| `cargo test --locked --all-targets --all-features` | All 150 tests passed, zero ignored/filtered; includes all the suites above. |
| `cargo test --locked --doc` | 1 passed. |
| `sh tests/setup_sh_test.sh` | All 60 checks passed. |
| Handoff `sha256sum --check --quiet SHA256SUMS.txt` | Passed; this follow-up changes no handoff files or artwork. |

### Browser launch and visual inspection

Both supplied reference PNGs were opened before implementation. The actual React
menu was then launched through Vite and inspected using Playwright screenshots,
not just DOM tests or the handoff's static HTML. Screenshots were opened and
visually compared for composition, palette, frame corners, control skins, font,
mascot, hover/selection contrast, focus, long text, and the beige drawer.
The follow-up reran both browser tests and regenerated these screenshots, then
reopened the main-window screenshot and both supplied references for comparison.
This remains browser evidence only; no native-window visual inspection is claimed.

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

## Remote CI — published commit only

GitHub's public Actions API was checked during this follow-up for the latest runs
and job/step conclusions at `bf4fff5f8167d8203ad5bb697c2b011ba2d0cd64`:

| Job | Actual result |
| --- | --- |
| [Linux desktop compilation](https://github.com/0xkamaji/loadbot/actions/runs/34874484366/job/104078174915) | **Success**, including native prerequisite installation, frontend build, desktop formatting, and `cargo check --locked --manifest-path src/gui/src-tauri/Cargo.toml --features custom-protocol`. This is compilation, not native window launch. |
| [Menu and browser embedding](https://github.com/0xkamaji/loadbot/actions/runs/34874484366/job/104078175177) | **Success**, including npm build/tests and normal multiprocess Chromium tests. |
| [Windows backend tests](https://github.com/0xkamaji/loadbot/actions/runs/34874484324/job/104078178534) | **Success**, including `cargo test --all-targets --all-features`. This does not test the Windows GUI or PowerShell setup. |
| [Linux Rust checks](https://github.com/0xkamaji/loadbot/actions/runs/34874484324/job/104078178348) | **Failure at Check formatting**; Clippy, tests, and Linux setup were subsequently skipped by GitHub. Those checks passed locally after this follow-up. |

The overall published backend CI workflow is **not green**. No remote CI result
exists for the uncommitted follow-up, including its changed test fixture.

### Remaining publication step

The repository workflows run on PRs targeting `main` (and pushes to `main`). Work
is on `fix/gui-phase1-verification`; nothing was committed, pushed, merged, or
published directly to `main`. GitHub CLI was unavailable initially; temporary
`gh` 2.87.3 was downloaded, but `gh run list` requires authentication that this
session does not have. Read-only public API queries supplied the CI evidence.

An authenticated maintainer must review/commit the formatting cleanup and the
separate fixture correction/documentation, then publish the branch and open a PR:

```sh
gh auth login
# Review and commit the intended changes on fix/gui-phase1-verification first.
git push -u origin fix/gui-phase1-verification
gh pr create --base main --head fix/gui-phase1-verification \
  --title "Finish Phase 1 GUI verification" \
  --body "Resolve Rust formatting and the remote-unavailability fixture; record local, CI, and native verification limits."
gh pr checks --watch
```

Check all four jobs for the new PR head SHA. Publishing only the branch will not
trigger these workflows until the PR exists. Do not infer new Windows/Linux CI
success from the earlier commit or local results; do not merge as part of this task.

## Blocked or unverified checks

- **Native desktop launch:** the exact documented command
  `npm --prefix src/gui run desktop` was invoked with Linux Node/npm on PATH.
  Vite started and Tauri invoked `cargo run`; compilation failed on missing
  `gobject-2.0 >= 2.70`, `glib-2.0 >= 2.70`, and GTK-related development libraries.
  The watcher was bounded by a 30-second timeout. A direct desktop check with
  `--locked --features custom-protocol` also failed at native pkg-config checks.
  GTK 3, WebKitGTK 4.1, and GLib development packages are absent; neither
  `DISPLAY` nor `WAYLAND_DISPLAY` is set. No installation loop was attempted.
  **No native window launched.** Native asset/font loading, pixel borders,
  selection/forms, drawer/focus/scrolling/resizing, and native Close behavior
  could not be inspected. The successful remote compilation does not verify them.
- **Next native action:** in native Windows PowerShell, use a Windows-local
  checkout of the reviewed changes with Windows Node 22.12+, Git, Rust MSVC,
  Visual Studio C++/Windows SDK, and WebView2. Run `npm --prefix src/gui ci`, then
  **`npm --prefix src/gui run desktop`**. Exact preflight commands and the native
  inspection sequence are in [Finish native verification from Windows](gui-phase1.md#finish-native-verification-from-windows).
- **Platform coverage:** this follow-up's Windows Rust results require publication;
  PowerShell setup, Windows/macOS GUI, high-DPI scaling, and physical mouse/touch
  inspection were not run locally.

## Remaining visual and integration limits

The bundled DejaVu font is the approved implementation fallback, not the exact
generated pixel lettering in the references. Runtime PNG textures differ slightly
from the flattened reference art; the original skins and manifest measurements
are used directly. Short windows and an open drawer can require scrolling the
details to reach Run; at narrow widths the project/shortcut panels stack. Browser
tests verified reachability at 420 × 480, not simultaneous visibility of every
control at every size.

Source inspection confirms the standalone host still injects `fixtureAdapter`,
the adapter exposes only fixture reads, and Tauri registers no backend commands.
Browser tests confirm Run stays disabled. Native filesystem behavior cannot be
observed because no window launched. All menu data and input values are fixtures.
Run, catalog changes/refresh, native pickers, project-folder opening, and terminal
functionality are unconnected. No
execution, configuration mutation, catalog access, Python binding, backend server,
cross-process protocol, installer, or Rot change was implemented. Phase 2
connection points are documented in [the GUI guide](gui-phase1.md).
