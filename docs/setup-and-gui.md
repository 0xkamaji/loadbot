# Setup, repair, and GUI launch

Loadbot presents the same setup choices on Windows and Linux. From a source
checkout, run `./setup.sh` on Linux or `.\setup.ps1` on Windows and select:

1. CLI only
2. GUI only
3. CLI + GUI
4. Repair / verify installation
5. Exit

For automation, use exactly one of `--cli`, `--gui`, `--all`, or `--repair` on
Linux and `-Cli`, `-Gui`, `-All`, or `-Repair` on Windows. With redirected input,
an explicit mode is required.

CLI-only installs the Rust `loadbot` executable, shell completion, and the
managed PATH/profile block. It never checks for or installs Node, WebKitGTK, or
other GUI dependencies.

GUI-only builds and installs the same small `loadbot` executable as the stable
launcher, adds its directory to PATH, and installs `loadbot-desktop` beside it.
It does not generate shell completion. CLI + GUI installs both component sets.
The desktop executable contains the production frontend and the Rust backend;
normal launch does not need the source checkout, Node, npm, Vite, or a frontend
server.

Setup records the selected component set in
`${CARGO_HOME:-$HOME/.cargo}/loadbot-install-mode`. Repair repeats the recorded
plan, safely replacing stale binaries and managed integration while preserving
Loadbot configuration, catalogs, projects, shortcuts, and unrelated profile
content. Setup never deletes previously installed component artifacts when a
smaller mode is selected; repair follows the most recently recorded mode.

## Normal and development launch

`loadbot gui` starts only the installed sibling `loadbot-desktop` executable. It
does not run npm, Vite, Git, catalog synchronization, fixture preview, or setup.
If the desktop executable is missing, the command points to `loadbot setup`.

`loadbot gui --dev` is source-only. It finds the checkout from the current
directory, the `LOADBOT_SOURCE` override, or the source path recorded when the
CLI was compiled. A moved checkout can therefore be selected explicitly:

```bash
LOADBOT_SOURCE=/path/to/loadbot loadbot gui --dev
```

```powershell
$env:LOADBOT_SOURCE = "C:\path\to\loadbot"
loadbot gui --dev
```

Development launch checks Cargo, Node 22.12+, npm, the Tauri source, and Linux
native libraries. It compares `package-lock.json` byte-for-byte with
`node_modules/.loadbot-package-lock.json` and verifies the Tauri API package. If
dependencies are missing or stale, it runs one deterministic `npm ci`, records
the new lock state, and then starts Tauri development. An unchanged checkout
does not reinstall packages.

## Platform prerequisites

Linux setup distinguishes Debian/Ubuntu (`apt-get`) from Arch/CachyOS (`pacman`)
using `/etc/os-release` when both tools exist. GUI setup probes WebKitGTK 4.1,
GTK 3, librsvg, OpenSSL, a compiler, Make, and pkg-config. If missing, it shows
the full command and asks before invoking sudo.

Debian/Ubuntu packages follow Tauri 2's Linux prerequisites:

```text
libwebkit2gtk-4.1-dev build-essential wget file libxdo-dev libssl-dev
libayatana-appindicator3-dev librsvg2-dev pkg-config
```

Arch/CachyOS packages are:

```text
webkit2gtk-4.1 base-devel wget file openssl appmenu-gtk-module
libappindicator-gtk3 librsvg xdotool pkgconf
```

Unsupported Linux package managers are never guessed; setup prints the missing
dependency categories for manual installation. CLI-only never enters this GUI
dependency path.

Windows setup uses Winget for Git, Rustup, Node.js LTS, and WebView2 when they
are missing. It verifies the WebView2 runtime using Microsoft's registered
runtime version and detects the MSVC C++ build workload with Visual Studio's
`vswhere`. Because installing a Visual Studio workload is a consequential
system-wide choice, setup reports that prerequisite for manual installation
instead of silently changing Visual Studio.

Both bootstraps are safe to rerun. Privileged Linux package installation and
Windows prerequisite installation are displayed before approval. Setup never
modifies Loadbot's data/configuration, runs Git pulls, installs Git hooks, or
changes PowerShell execution policy.

Prerequisite references: [Tauri 2 platform prerequisites](https://v2.tauri.app/start/prerequisites/)
and [Microsoft WebView2 runtime detection](https://learn.microsoft.com/microsoft-edge/webview2/concepts/distribution#detect-if-a-suitable-webview2-runtime-is-already-installed).
