# Setup, repair, and GUI launch

Windows releases include a current-user NSIS installer. It installs the desktop
application and CLI together under `%LOCALAPPDATA%\Programs\Loadbot`, creates a
Start Menu entry and uninstaller, and adds that directory to the user PATH. The
portable zip contains the same two executables without registering an
installation.

Git is a runtime prerequisite for catalog and project operations. Release setup
can install it through the supported platform package manager; the Windows NSIS
installer leaves an existing system Git installation unchanged and reports Git
errors normally if Git is unavailable.

Linux releases contain `loadbot`, `loadbot-desktop`, and `install.sh`. After
extracting the archive, run:

```bash
./install.sh
```

This installs both executables under `~/.local/bin`, adds the CLI directory to the
detected shell profile when needed, and creates an XDG desktop launcher and icon.
`LOADBOT_INSTALL_ROOT` selects a different release installation root.

`loadbot setup` works from either portable or installed release binaries and does
not inspect a source checkout. It presents the same choices on Windows and Linux:

1. CLI only
2. GUI only
3. CLI + GUI
4. Repair / verify installation
5. Exit

For automation, use exactly one of `loadbot setup --cli`, `--gui`, `--all`, or
`--repair`. With redirected input, an explicit mode is required.

CLI-only installs the `loadbot` executable, shell completion, and the
managed PATH/profile block. It never checks for or installs Node, WebKitGTK, or
other GUI dependencies.

GUI-only installs the same small `loadbot` executable as the stable launcher,
adds its directory to PATH, and installs `loadbot-desktop` beside it.
It does not generate shell completion. CLI + GUI installs both component sets.
The desktop executable contains the production frontend and the Rust backend;
normal launch does not need the source checkout, Node, npm, Vite, or a frontend
server.

Release setup records the selected component set under its installation root.
Repair verifies available binaries and restores managed integration while
preserving Loadbot configuration, catalogs, projects, shortcuts, and unrelated
profile content. Replacing a missing desktop payload requires rerunning setup
from a complete portable release or rerunning the Windows installer. Setup never
deletes previously installed component artifacts when a smaller mode is selected;
repair follows the most recently recorded mode.

Installations created before that record existed are detected from the expected
CLI and desktop executables plus PATH, managed profile, and completion state.
An unambiguous legacy mode can be adopted after confirmation (or automatically
for explicit noninteractive repair). Ambiguous or genuinely fresh state requires
an explicit CLI-only, GUI-only, or combined choice. Configuration directories are
reported as context but never treated as installation evidence. The record is
written atomically only after the selected repair completes successfully.

## Normal and development launch

`loadbot gui` starts only the installed sibling `loadbot-desktop` executable. It
does not run npm, Vite, Git, catalog synchronization, fixture preview, or setup.
If the desktop executable is missing, the command points to `loadbot setup`.

`loadbot gui --dev` is source-only. It finds a complete checkout from the current
directory or the explicit `LOADBOT_SOURCE` override. Release binaries do not
embed or search a build machine's source path:

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

The Linux desktop binary uses the distribution's WebKitGTK 4.1 and GTK runtime
libraries. Release builds use Ubuntu 22.04 as their compatibility baseline;
other distributions may need to install equivalent runtime packages.

## Source installation

Developers can still build and install from a checkout with `./setup.sh` on Linux
or `.\setup.ps1` on Windows. Those direct scripts retain the source-build modes
and prerequisite handling; the compiled `loadbot setup` command always uses the
current release payload instead.

## Platform prerequisites

Linux source setup distinguishes Debian/Ubuntu (`apt-get`) from Arch/CachyOS
(`pacman`) using `/etc/os-release` when both tools exist. Source GUI setup probes
WebKitGTK 4.1, GTK 3, librsvg, OpenSSL, a compiler, Make, and pkg-config. If
missing, it shows the full command and asks before invoking sudo.

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

Windows source setup uses Winget for Git, Rustup, Node.js LTS, and WebView2 when
they are missing. It verifies the WebView2 runtime using Microsoft's registered
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
