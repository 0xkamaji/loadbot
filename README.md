# Loadbot

Loadbot is a small cross-platform CLI for managing catalogs of Git-backed tool repositories. Catalogs make tool definitions and explicitly selected shared commands portable between machines, while installed repositories remain local. Loadbot invokes the user's installed `git` command and never executes repository content automatically.

## Data Model

All runtime data managed by Loadbot lives beneath one root:

```text
LOADBOT_HOME/
├── config.toml
├── catalogs/
│   ├── personal/
│   │   └── catalog.toml
│   └── public/
│       └── catalog.toml
└── tools/
    ├── personal/
    │   └── re-toolkit/
    └── public/
        └── other-tool/
```

- `config.toml` is machine-local. It registers catalog Git URLs, writable/read-only settings, and the default catalog.
- `catalogs/<name>/` is a complete Git clone. Its versioned `catalog.toml` is authoritative for portable tool definitions.
- `tools/<catalog>/<name>/` is the local clone of a tool installed from that catalog. Catalog namespaces allow the same tool name to be installed independently from multiple catalogs.

The default root is `~/.local/share/loadbot/` on Linux and `%LOCALAPPDATA%\loadbot\` on native Windows. `LOADBOT_HOME` overrides the root for portable or isolated use. Read-only commands do not create it.

Personal launcher shortcuts are stored separately in the platform configuration directory at `loadbot/shortcuts.toml` (`~/.config/loadbot/shortcuts.toml` on Linux). They contain a catalog name, tool name, and repository-relative file path, never an absolute installation path. Shortcuts are not written to `config.toml`, catalog repositories, or `catalog.toml`.

Catalog authors may define shared commands directly on a tool in the existing version-1 `catalog.toml`:

```toml
version = 1

[tools.re-toolkit]
type = "git"
url = "https://github.com/0xkamaji/re-toolkit.git"
revision = "main"

[tools.re-toolkit.commands.print-strings]
path = "recipes/BinaryNinja/print_strings.py"
description = "Print strings from the current Binary Ninja database"
runner = "python"

[tools.re-toolkit.commands.install]
path = "scripts/install.sh"
runner = "bash"
```

The command name is the key below `[tools.<tool>.commands]` and must be unique within that tool. `path` is required. `description` and `runner` are optional. Existing catalogs without `commands` remain valid version-1 catalogs. This release changes only Loadbot's schema and behavior; it does not add commands to the real 0xkamaji catalog.

The Loadbot executable and Loadbot source repository do not have to live beneath `LOADBOT_HOME`.

## Build and Install

Loadbot requires Git, Rust, and Cargo. Cargo downloads the crate dependencies declared by `Cargo.toml` and `Cargo.lock`; the setup scripts do not install individual Rust crates.

```bash
cargo build --release
```

From a checked-out source tree, Linux users can run:

```bash
./setup.sh
```

Native Windows PowerShell users can run:

```powershell
.\setup.ps1
```

The setup scripts display a consolidated plan before changing PATH, a profile, or system prerequisites. Automatic prerequisite installation is limited to `apt-get` on Ubuntu/Debian, `pacman` on Arch/CachyOS, and Winget on native Windows. The exact package command, package list, and elevation behavior are displayed first, and package installation always requires an explicit `Install these prerequisites? [y/N]` confirmation. A noninteractive setup with missing prerequisites prints the required command and exits. Unsupported package managers receive manual guidance and are never guessed.

Linux setup installs beneath `${CARGO_HOME:-$HOME/.cargo}`, adds its `bin` directory and the appropriate generated completion to one of these login-shell files, and leaves other shells untouched:

```text
Bash: ~/.bashrc
Zsh:  ~/.zshrc
Fish: ~/.config/fish/config.fish
```

Windows setup installs beneath `$env:CARGO_HOME` or `$HOME\.cargo`, adds its `bin` directory once to the current-user PATH (never machine PATH), and configures the current-user/current-host `$PROFILE` to dot-source the generated PowerShell completion. Existing profiles must be normal user files, not symlinks or reparse points. Before a real profile change, setup creates a timestamped backup beside the profile and atomically replaces only a clearly marked Loadbot block. Exact existing blocks require no edit or backup; malformed or duplicate markers stop setup safely, so repeated setup does not grow PATH or profile content.

Both scripts run `--version` and `--help` through the absolute installed executable path. The terminal that launched setup does not inherit a child process's environment changes: open a new terminal after setup, or run the reload command it prints (`source ~/.bashrc`, `source ~/.zshrc`, `source ~/.config/fish/config.fish`, or dot-source `$PROFILE`). PowerShell setup reports restrictive execution policy but never changes it.

Users who decline automatic prerequisite installation can run the displayed command themselves. The supported commands are:

```bash
# Ubuntu/Debian (the displayed package list is reduced to missing prerequisites)
sudo apt-get update
sudo apt-get install -y git cargo rustc

# Arch/CachyOS (the displayed package list is reduced to missing prerequisites)
sudo pacman -S --needed git rust cargo
```

```powershell
# Run only the missing package installation(s).
winget install --id Git.Git --exact --source winget --scope user --accept-package-agreements --accept-source-agreements
winget install --id Rustlang.Rustup --exact --source winget --scope user --accept-package-agreements --accept-source-agreements
rustup toolchain install stable
rustup default stable
```

The complete manual Loadbot installation remains:

```bash
INSTALL_ROOT="${CARGO_HOME:-$HOME/.cargo}"
cargo install --path . --root "$INSTALL_ROOT" --locked --force
mkdir -p "$INSTALL_ROOT/completions"
COMPLETE=bash "$INSTALL_ROOT/bin/loadbot" >"$INSTALL_ROOT/completions/loadbot.bash"
COMPLETE=zsh "$INSTALL_ROOT/bin/loadbot" >"$INSTALL_ROOT/completions/loadbot.zsh"
COMPLETE=fish "$INSTALL_ROOT/bin/loadbot" >"$INSTALL_ROOT/completions/loadbot.fish"
```

```powershell
$InstallRoot = if ($env:CARGO_HOME) { $env:CARGO_HOME } else { Join-Path $HOME ".cargo" }
cargo install --path . --root $InstallRoot --locked --force
$CompletionDir = Join-Path $InstallRoot "completions"
New-Item -ItemType Directory -Force $CompletionDir | Out-Null
$env:COMPLETE = "powershell"
& (Join-Path $InstallRoot "bin\loadbot.exe") | Set-Content (Join-Path $CompletionDir "loadbot.ps1")
Remove-Item Env:COMPLETE
```

Installer tests use isolated temporary homes and mocked/fake external commands:

```bash
./tests/setup_sh_test.sh
```

```powershell
Invoke-Pester .\tests\setup_ps1.Tests.ps1
```

Private repositories and pushes first use the user's existing Git configuration,
SSH keys, credential helpers, and identity. If a canonical GitHub SSH operation
fails specifically with public-key authentication, Loadbot can query
`rot ssh identities --json` and retry with a verified Rot-managed SSH alias.
Loadbot does not manage credentials, keys, SSH configuration, or Git accounts.

Catalog Git URLs are portable canonical fetch URLs. For a public GitHub HTTPS
tool, interactive `loadbot pull` can use a verified Rot-managed host alias to
set a machine-local `remote.origin.pushurl` while leaving `remote.origin.url`
and `catalog.toml` unchanged. This affects only that checkout and does not
change other machines or global Git configuration. Private tools whose
canonical catalog URL is SSH continue to use SSH for fetching and pushing.

## Global Options

```bash
loadbot --help
loadbot --version
```

- `--help` displays CLI help.
- `--version` displays the version from `Cargo.toml`.

Running `loadbot` without a subcommand does not currently open a top-level menu.

## New Machine Bootstrap

Open the interactive catalog onboarding and management menu:

```bash
loadbot catalog
```

```text
1. Use 0xkamaji's catalog
2. Add an existing catalog
3. Create or initialize a catalog
4. List catalogs
5. Sync a catalog
6. Show catalog status
7. Show catalog path
8. Cancel
```

The first choice displays and confirms this preset before changing anything:

```text
Name:      personal
URL:       https://github.com/0xkamaji/loadbot-catalog.git
Writable:  yes
```

It delegates to the normal safe catalog-add operation. The equivalent noninteractive command for automation is:

```bash
loadbot catalog add personal https://github.com/0xkamaji/loadbot-catalog.git --writable
```

After registration, catalog and tool repositories use separate catalog namespaces:

```text
LOADBOT_HOME/
├── config.toml
├── catalogs/
│   └── personal/
│       └── catalog.toml
└── tools/
    └── personal/
        └── rot-tools/
```

The first registered catalog becomes `default_catalog`. Because the preset is writable, unambiguous tool lookup and direct tool addition can use `personal` without another catalog-selection prompt.

Fresh-machine workflow:

```bash
# Select "Use 0xkamaji's catalog" and confirm the displayed values.
loadbot catalog

# Resolves rot-tools from the newly registered personal catalog.
loadbot pull rot-tools

# Add and publish another definition.
loadbot add rot-infra \
  git@github.com:0xkamaji/rot-infra.git \
  --revision main \
  --catalog personal \
  --commit \
  --push
```

`Add an existing catalog` asks for a name, Git URL, writable/read-only access, and confirmation, then clones the existing catalog without changing its repository contents. `Create or initialize a catalog` is only for an existing empty Git remote: it safely registers and clones the remote, refuses existing or conflicting data, and creates an empty version-1 `catalog.toml`. It asks separately before committing and before pushing. Loadbot never creates a GitHub repository, calls the GitHub API, manages credentials, or pushes implicitly.

Optional public catalogs should normally be registered read-only by omitting `--writable`:

```bash
loadbot catalog add community https://github.com:ORG/loadbot-catalog.git
```

## Complete Command Reference

### `loadbot catalog`

In an interactive terminal, bare `loadbot catalog` opens the onboarding and management menu shown above. Add, list, sync, status, and path choices delegate to the same operations used by their direct commands. In redirected or otherwise noninteractive use, bare `loadbot catalog` fails immediately and directs the user to run it in an interactive terminal.

The create/initialize choice requires a writable existing empty Git remote. It refuses a nonempty remote, a dirty working tree, unrelated tracked files, conflicting `catalog.toml` data, origin mismatches, and unsafe destinations. Creating `catalog.toml`, committing it, and pushing its initial commit are distinct steps; commit and push require separate confirmations, and the push is a normal non-force `git push origin HEAD`.

### `loadbot catalog add`

```text
loadbot catalog add [NAME] [GIT_URL] [--writable]
```

Registers and clones a catalog repository.

Writable personal catalog:

```bash
loadbot catalog add personal \
  https://github.com/0xkamaji/loadbot-catalog.git \
  --writable
```

Read-only public catalog:

```bash
loadbot catalog add community \
  https://github.com/example/loadbot-catalog.git
```

Behavior:

- Validates the catalog name.
- Clones it into `LOADBOT_HOME/catalogs/<name>/` and validates `catalog.toml`.
- Records the catalog in local `config.toml` only after cloning and validation succeed.
- Makes the first registered catalog the default.
- Marks it writable only when `--writable` is supplied.
- Reuses the user's existing Git and SSH configuration.
- Refuses unrelated folders, repositories with a different origin, and symlink destinations.
- Behaves idempotently when the expected repository is already installed.
- Removes only a clone created by the failed operation and leaves no first-time registration behind if cloning, validation, or registration fails.

Interactive form:

```bash
loadbot catalog add
```

The interactive flow asks for:

1. Catalog name
2. Git repository URL
3. Whether the catalog is writable
4. Confirmation

### `loadbot catalog list`

```bash
loadbot catalog list
```

Lists locally registered catalogs without contacting the network.

Output columns:

```text
NAME    STATE    ACCESS    DEFAULT    URL
```

Catalog states include:

- `installed`: the expected repository is present.
- `missing`: the catalog is registered but not cloned.
- `mismatch`: the local destination is not the configured repository.

Access is shown as `writable` or `read-only`.

### `loadbot catalog sync`

```text
loadbot catalog sync [NAME]
```

Safely updates a catalog from its remote.

```bash
loadbot catalog sync personal
```

Behavior:

- Verifies that the destination contains the configured repository.
- Refuses a dirty working tree.
- Requires a checked-out branch.
- Fetches only from `origin`.
- Merges only with `--ff-only`.
- Never resets, discards changes, or resolves conflicts.

When the name is omitted in an interactive terminal, Loadbot presents valid installed catalogs only. Missing, mismatched, or invalid registrations remain available through `catalog list`, `catalog status NAME`, and explicit repair commands. Loadbot does not synchronize every catalog automatically.

### `loadbot catalog status`

```text
loadbot catalog status [NAME]
```

Displays detailed local catalog information:

- Name
- Absolute path
- Configured URL
- Writable status
- Current branch
- Current commit
- Clean or dirty working tree
- Actual origin URL
- Whether `catalog.toml` is valid, invalid, missing, or unavailable

```bash
loadbot catalog status personal
```

When the name is omitted in an interactive terminal, Loadbot presents catalog selection.

### `loadbot catalog path`

```text
loadbot catalog path [NAME]
```

Prints only the catalog's absolute local path to stdout:

```bash
loadbot catalog path personal
```

Example output:

```text
/home/kamaji/.local/share/loadbot/catalogs/personal
```

This is designed for shell use:

```bash
cd "$(loadbot catalog path personal)"
```

When the name is omitted in an interactive terminal, Loadbot presents catalog selection.

### `loadbot catalog migrate`

```text
loadbot catalog migrate NAME GIT_URL
```

Migrates the legacy Loadbot format, where authoritative `[tools]` entries lived in local `config.toml`, into a Git-backed catalog.

```bash
loadbot catalog migrate personal \
  https://github.com/0xkamaji/loadbot-catalog.git
```

Behavior:

- Requires an existing legacy `config.toml` containing `[tools]`.
- Clones the supplied catalog repository.
- Requires that the repository not already contain `catalog.toml`.
- Writes the legacy definitions into a new `catalog.toml`.
- Replaces the legacy local configuration with a writable catalog registration.
- Refuses existing catalog destinations.
- Does not commit or push.
- Preserves the legacy configuration if migration fails.

Inspect and publish the migration manually:

```bash
cd "$(loadbot catalog path personal)"
git status
git add catalog.toml
git commit -m "Migrate tools to Loadbot catalog"
git push
```

### `loadbot add`

```text
loadbot add [NAME] [GIT_URL]
  [--revision REVISION]
  [--catalog CATALOG]
  [--commit]
  [--push]
```

Adds a tool definition to a writable catalog.

```bash
loadbot add re-toolkit \
  git@github.com:0xkamaji/re-toolkit.git \
  --revision main \
  --catalog personal
```

This modifies `catalogs/personal/catalog.toml`. Direct `add` does not clone the tool repository.

Options:

- `--revision REVISION` selects a branch or tag. Without it, Git uses the remote's default branch.
- `--catalog CATALOG` selects the writable catalog receiving the definition. If omitted during direct use, Loadbot uses the default catalog only when it is writable.
- `--commit` commits the `catalog.toml` change with a message such as `Add re-toolkit to Loadbot catalog`.
- `--push` pushes the resulting commit to `origin`. It requires `--commit`.

Loadbot never pushes catalog changes implicitly. Its catalog commit excludes unrelated staged files.

Interactive form:

```bash
loadbot add
```

The interactive flow asks for:

1. Tool name
2. Git URL
3. Optional revision
4. Writable catalog
5. Confirmation
6. Whether to commit
7. Whether to push
8. Whether to pull the tool immediately

### `loadbot list`

```bash
loadbot list
```

Aggregates tools from all healthy registered catalogs without contacting the network.

Output columns:

```text
NAME    CATALOG    TYPE    STATE    REVISION    AMBIGUOUS
```

Tool states include `installed` and `missing`.

If a catalog is missing, mismatched, or invalid, Loadbot prints a warning to stderr, skips it, and continues listing tools from healthy catalogs.

If the same tool name appears in multiple catalogs, every definition is shown and marked ambiguous. Commands referencing that tool must use `--catalog`.

### `loadbot pull`

```text
loadbot pull [NAME] [--catalog CATALOG]
```

Clones a configured tool repository into `LOADBOT_HOME/tools/<catalog>/<name>/`.

```bash
loadbot pull re-toolkit
```

For an ambiguous tool name:

```bash
loadbot pull re-toolkit --catalog personal
```

Behavior:

- Resolves the tool definition from registered catalogs.
- Uses the configured revision when present.
- Refuses unrelated destinations and repositories with a different origin.
- Behaves idempotently when the expected repository is already installed.
- Cleans up only a destination newly created by its own failed clone.
- Never executes downloaded repository content.
- For public GitHub HTTPS tools, may interactively offer to configure an SSH
  push URL from a verified Rot identity; the default answer is No.
- Can interactively reconcile an equivalent GitHub SSH origin into the exact
  catalog HTTPS fetch URL while preserving the old SSH URL for pushes.
- Never reconciles unknown SSH aliases, different owners, different
  repositories, or non-GitHub hosts.

The SSH push setting is local to the installed repository. Noninteractive
pulls never prompt or change remotes, and a declined reconciliation reports
both URLs without modifying the checkout. Existing separate push URLs are
preserved unless an explicit replacement is confirmed.

When the name is omitted in an interactive terminal, Loadbot presents catalog-qualified selections such as `personal/re-toolkit`.

### `loadbot update`

```text
loadbot update [NAME] [--catalog CATALOG]
```

Safely updates an installed tool repository.

```bash
loadbot update re-toolkit
```

For an ambiguous tool name:

```bash
loadbot update re-toolkit --catalog personal
```

Behavior:

- Verifies that the repository origin matches the selected definition.
- Refuses dirty working trees.
- Requires a checked-out branch.
- Refuses when the configured revision differs from the checked-out branch.
- Fetches only from `origin`.
- Merges only with `--ff-only`.
- Never resets, discards changes, or resolves conflicts.

When the name is omitted in an interactive terminal, Loadbot presents catalog-qualified tool selection.

### `loadbot status`

```text
loadbot status [NAME] [--catalog CATALOG]
```

Shows a tool definition and its local Git state:

- Tool name
- Source catalog
- Absolute installation path
- Installed or missing state
- Configured URL
- Configured revision
- Current branch
- Current commit
- Clean or dirty working tree
- Actual fetch URL
- Effective push URL

`Fetch URL:` is the repository's `remote.origin.url`. `Push URL:` shows
`remote.origin.pushurl` when configured; otherwise it shows the fetch URL and
notes that pushes use it.

```bash
loadbot status re-toolkit
```

For an ambiguous tool name:

```bash
loadbot status re-toolkit --catalog personal
```

When the name is omitted in an interactive terminal, Loadbot presents catalog-qualified tool selection.

### `loadbot path`

```text
loadbot path [NAME] [--catalog CATALOG]
```

Prints only the tool's absolute destination path:

```bash
loadbot path re-toolkit
```

Example output:

```text
/home/kamaji/.local/share/loadbot/tools/personal/re-toolkit
```

The tool may be defined but not yet cloned. This makes the command suitable for scripts:

```bash
cd "$(loadbot path re-toolkit)"
```

For an ambiguous name, supply `--catalog`. When the name is omitted in an interactive terminal, Loadbot presents catalog-qualified tool selection.

### `loadbot run`

```text
loadbot run [SHORTCUT]
```

Without a shortcut name, opens an interactive project-first launcher:

```bash
loadbot run
```

The first menu contains only projects that have at least one shared catalog command or personal shortcut. A tool name is used as the label when unique; duplicate tool names are qualified with their catalog. Selecting a project opens its merged command menu. Descriptions appear beside command names when present. Back returns to project selection and Exit closes the launcher.

`Browse installed tools...` preserves the file navigator. It reads only the directory currently displayed. Directories descend into that directory, files may be selected for launch, and `../` choices navigate back to the previous directory, tool selection, or catalog selection. Symlinks are not shown. After a selected file exits successfully, Loadbot optionally offers to save it as a personal shortcut; the default is No.

Run a saved shortcut directly:

```bash
loadbot run print-strings
```

A shortcut stores logical location information:

```toml
version = 1

[shortcuts.print-strings]
catalog = "personal"
tool = "re-toolkit"
path = "recipes/BinaryNinja/print_strings.py"
description = "My local print workflow"
runner = "python"
```

`description` and `runner` are optional for personal shortcuts. Existing version-1 files containing only `catalog`, `tool`, and `path` continue to load unchanged, and unknown fields continue to be preserved. Personal shortcut changes are written only to the user-level `shortcuts.toml`, never to a catalog or installed tool clone.

At launch time Loadbot resolves the catalog-qualified tool through the current catalog definition and installation path. It rejects missing tools, missing files, origin mismatches, path traversal, and targets that resolve outside the tool repository. Duplicate shortcut names are not overwritten. If a shared command and personal shortcut have the same name in one project, both are retained and visibly labeled `[shared]` and `[personal]`; Loadbot never silently chooses one.

Supported runner values are `direct`, `bash`, `sh`, `python`, and `powershell`. Loadbot selects conventional platform-aware executable names from `PATH`: `bash`, `sh`, `python3`/`python`, and `pwsh`/`powershell`. An explicit runner always invokes that runner, so `runner = "bash"` works for a non-executable file and never substitutes `sh`. A shared command with no runner uses direct execution and therefore must be executable on platforms that require an executable bit. A legacy personal shortcut with no runner preserves the existing behavior: executable files run directly, while non-executable `.py`, `.sh`, and `.ps1` files use the existing extension-based interpreter fallback.

Paths are portable repository-relative strings separated by `/`. They must be nonempty, must not be absolute, and may not contain `.` or `..` components, empty components, backslashes, or colons. Loadbot also canonicalizes the selected target and rejects symlinks or other resolution that escapes the installed tool root. Shared commands run with the installed tool root as their working directory. Children explicitly inherit stdin, stdout, stderr, the environment, and the working terminal, so passphrase prompts, `sudo`, and ordinary script questions remain interactive. Loadbot does not install dependencies or execute arbitrary shell command strings.

Shared commands are read from each registered catalog clone. They become available after the catalog is cloned or after `loadbot catalog sync NAME` fast-forwards it; Loadbot does not create a separate local copy of command definitions. The corresponding tool repository must still be installed before a command can run.

Saved shortcut names are available as dynamic completions for the positional argument to `loadbot run`. Completion never scans installed repositories or suggests tool names and files. Enable it for the current shell with:

```bash
# Bash
source <(COMPLETE=bash loadbot)

# Zsh
source <(COMPLETE=zsh loadbot)

# Fish
COMPLETE=fish loadbot | source
```

PowerShell:

```powershell
$env:COMPLETE = "powershell"
loadbot | Out-String | Invoke-Expression
Remove-Item Env:COMPLETE
```

`setup.sh` generates Bash, Zsh, Fish, and PowerShell registration scripts beneath Cargo's `completions` install directory and configures the detected Bash, Zsh, or Fish profile. `setup.ps1` generates the PowerShell registration script and configures the current-user/current-host PowerShell profile. The setup scripts use one idempotent managed block and back up an existing profile before changing it.

## Catalog Recovery

First-time catalog addition is transactional. Loadbot clones and validates the catalog before saving its registration. A failed clone, invalid catalog, or failed configuration save leaves no new registration and removes only the clone created by that operation. Rerun the same `catalog add` command after correcting the problem.

Healthy catalogs remain usable. Tool listing skips unhealthy catalogs with a warning and recommends:

```bash
loadbot catalog status NAME
```

If a previously registered catalog directory is later deleted, `catalog list` reports it as `missing`, while normal writable and sync selections omit it. Restore it by rerunning the original command:

```bash
loadbot catalog add NAME GIT_URL
```

Use the same `--writable` setting originally registered. Read-only commands do not silently rewrite `config.toml`, and temporary sync or network failures never remove a successful registration.

A command explicitly qualified with a missing, mismatched, or invalid catalog fails instead of silently selecting another catalog.

## Common Workflows

### Add and publish a tool

```bash
loadbot add re-toolkit \
  git@github.com:0xkamaji/re-toolkit.git \
  --revision main \
  --catalog personal \
  --commit \
  --push

loadbot pull re-toolkit
```

### Set up another computer

```bash
loadbot catalog add personal \
  https://github.com/0xkamaji/loadbot-catalog.git \
  --writable

loadbot list
loadbot pull re-toolkit
```

### Receive catalog changes made elsewhere

```bash
loadbot catalog sync personal
loadbot list
```

### Update an installed tool

```bash
loadbot update re-toolkit
```

### Inspect local state

```bash
loadbot catalog status personal
loadbot status re-toolkit
```

## Interactive and Noninteractive Behavior

When stdin and stdout are terminals, omitted names and arguments start prompts or numbered selection:

```bash
loadbot catalog add
loadbot catalog sync
loadbot catalog status
loadbot catalog path
loadbot add
loadbot pull
loadbot update
loadbot status
loadbot path
```

`loadbot list` and `loadbot catalog list` are always directly printable and never prompt.

Fully specified commands remain noninteractive. Incomplete commands with redirected stdin or stdout fail with instructions instead of waiting for input.

## Safety

Loadbot:

- Refuses unrelated files, directories, repositories, and symlink destinations.
- Refuses dirty tool or catalog updates.
- Fetches only from `origin` and merges only with `--ff-only`.
- Never resets, discards changes, resolves conflicts, or pushes implicitly.
- Cleans up only a destination newly created by its own failed clone.
- Passes URLs and names directly as process arguments without a shell.
- Never executes repository scripts automatically; `loadbot run` executes only the file explicitly selected by the user.
- The runtime CLI never modifies shell configuration; the setup scripts modify only the user profile described above after confirmation.
- Never manages SSH keys, credentials, or Git accounts.
- Keeps configured and origin URLs canonical; a selected Rot SSH alias is applied
  only to the individual retrying Git process.
- Preserves unknown TOML fields when reading and writing catalog and local configuration.

## Current Limitations

- Only Git repository sources are supported.
- Safe updates require a checked-out branch; detached tags or commits are not updated.
- URL equivalence removes only surrounding whitespace, a trailing slash, and one trailing `.git`; equivalent SSH and HTTPS URLs remain distinct.
- There is no catalog discovery service, package resolution, dependency installation, plugin system, arbitrary shell-command execution, background service, or AI feature. Rot integration is limited to read-only SSH identity discovery after a GitHub SSH public-key authentication failure.
