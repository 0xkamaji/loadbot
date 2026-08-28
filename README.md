# Loadbot

Loadbot is a small cross-platform CLI for managing catalogs of Git-backed tool repositories. Catalogs make tool definitions portable between machines, while installed repositories remain local. Loadbot invokes the user's installed `git` command and never parses or executes downloaded repository content.

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

The Loadbot executable and Loadbot source repository do not have to live beneath `LOADBOT_HOME`.

## Build and Install

Rust, Cargo, and Git must be installed and available in `PATH`.

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

Both scripts install with Cargo, verify the executable, report its exact location, and print PATH guidance when needed. They do not modify shell startup files, PowerShell profiles, or PATH.

Private repositories and pushes use the user's existing Git configuration, SSH keys, credential helpers, and identity. Loadbot does not manage credentials or Git accounts.

## Global Options

```bash
loadbot --help
loadbot --version
```

- `--help` displays CLI help.
- `--version` displays the version from `Cargo.toml`.

Running `loadbot` without a subcommand does not currently open a top-level menu.

## New Machine Bootstrap

Register and clone a personal writable catalog:

```bash
loadbot catalog add personal git@github.com:USER/loadbot-catalog.git --writable
```

Optional public catalogs should normally be registered read-only by omitting `--writable`:

```bash
loadbot catalog add community https://github.com:ORG/loadbot-catalog.git
```

The first registered catalog becomes `default_catalog` in local configuration. Catalog definitions are loaded from each clone's `catalog.toml`.

## Complete Command Reference

### `loadbot catalog add`

```text
loadbot catalog add [NAME] [GIT_URL] [--writable]
```

Registers and clones a catalog repository.

Writable personal catalog:

```bash
loadbot catalog add personal \
  git@github.com:0xkamaji/loadbot-catalog.git \
  --writable
```

Read-only public catalog:

```bash
loadbot catalog add community \
  https://github.com/example/loadbot-catalog.git
```

Behavior:

- Validates the catalog name.
- Records the catalog in local `config.toml`.
- Clones it into `LOADBOT_HOME/catalogs/<name>/`.
- Makes the first registered catalog the default.
- Marks it writable only when `--writable` is supplied.
- Reuses the user's existing Git and SSH configuration.
- Refuses unrelated folders, repositories with a different origin, and symlink destinations.
- Behaves idempotently when the expected repository is already installed.
- Preserves a successful registration if cloning fails so the operation can be retried.

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

When the name is omitted in an interactive terminal, Loadbot presents a numbered catalog selection. It does not synchronize every catalog automatically.

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
  git@github.com:0xkamaji/loadbot-catalog.git
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
- Actual origin URL

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

## Catalog Recovery

If catalog registration succeeds but its clone fails, the local registration is preserved and appears as `missing`.

Healthy catalogs remain usable. Tool listing skips unhealthy catalogs with a warning and recommends:

```bash
loadbot catalog status NAME
```

After resolving the remote or connectivity problem, rerun the original command:

```bash
loadbot catalog add NAME GIT_URL
```

Use the same `--writable` setting originally registered. Loadbot retries the missing clone. It does not automatically delete registrations or perform destructive repairs.

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
  git@github.com:0xkamaji/loadbot-catalog.git \
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
- Never executes repository scripts.
- Never modifies shell configuration.
- Never manages SSH keys, credentials, or Git accounts.
- Preserves unknown TOML fields when reading and writing catalog and local configuration.

## Current Limitations

- Only Git repository sources are supported.
- Safe updates require a checked-out branch; detached tags or commits are not updated.
- URL equivalence removes only surrounding whitespace, a trailing slash, and one trailing `.git`; equivalent SSH and HTTPS URLs remain distinct.
- There is no catalog discovery service, package resolution, dependency installation, plugin system, script execution, background service, AI feature, or Rotbot integration.
