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
    ├── re-toolkit/
    └── other-tool/
```

- `config.toml` is machine-local. It registers catalog Git URLs, writable/read-only settings, and the default catalog.
- `catalogs/<name>/` is a complete Git clone. Its versioned `catalog.toml` is authoritative for portable tool definitions.
- `tools/<name>/` is the local clone of an installed tool repository.

The default root is `~/.local/share/loadbot/` on Linux and `%LOCALAPPDATA%\loadbot\` on native Windows. `LOADBOT_HOME` overrides the root for portable or isolated use. Read-only commands do not create it.

## Build And Install

Rust, Cargo, and Git must be installed and available in `PATH`.

```bash
cargo build --release
```

From a checked-out source tree, Linux users can run `./setup.sh`. Native Windows PowerShell users can run:

```powershell
.\setup.ps1
```

Both scripts install with Cargo, verify the executable, report its exact location, and print PATH guidance when needed. They do not modify shell startup files, PowerShell profiles, or PATH.

Private repositories and pushes use the user's existing Git configuration, SSH keys, credential helpers, and identity. Loadbot does not manage credentials or Git accounts.

## New Machine Bootstrap

Register and clone a personal writable catalog:

```bash
loadbot catalog add personal git@github.com:USER/loadbot-catalog.git --writable
```

Optional public catalogs should normally be registered read-only by omitting `--writable`:

```bash
loadbot catalog add community https://github.com/ORG/loadbot-catalog.git
```

The first registered catalog becomes `default_catalog` in local configuration. Catalog definitions are loaded from each clone's `catalog.toml`.

## End-To-End Example

```bash
loadbot catalog add personal git@github.com:USER/loadbot-catalog.git --writable
loadbot add re-toolkit git@github.com:USER/re-toolkit.git \
  --revision main \
  --catalog personal
loadbot pull re-toolkit
```

Direct `add` writes `catalog.toml` but does not commit or push unless explicitly requested, and it never pulls the tool automatically. Run `loadbot pull TOOL` separately after a direct add:

```bash
loadbot add re-toolkit git@github.com:USER/re-toolkit.git \
  --revision main \
  --catalog personal \
  --commit \
  --push
```

`--push` requires `--commit`. Loadbot never pushes catalog changes implicitly.

## Interactive Use

When stdin and stdout are terminals, omitted arguments start simple prompts:

```bash
loadbot catalog add
loadbot catalog sync
loadbot add
loadbot pull
loadbot update
loadbot status
loadbot path
```

Interactive tool addition asks for the tool name, Git URL, optional revision, writable catalog, confirmation, commit, push, and immediate pull. An empty revision uses the remote's default branch. Redirected or otherwise noninteractive incomplete commands fail instead of waiting.

`list` and `catalog list` are always directly printable and never prompt.

## Catalog Commands

```text
loadbot catalog add [NAME] [GIT_URL] [--writable]
loadbot catalog list
loadbot catalog sync [NAME]
loadbot catalog status [NAME]
loadbot catalog path [NAME]
loadbot catalog migrate NAME GIT_URL
```

`catalog sync` refuses dirty worktrees, fetches only from `origin`, and merges only with `--ff-only`. `catalog status` reports Git state plus whether `catalog.toml` is present and valid. `catalog path NAME` prints only its absolute path to stdout.

### Catalog Recovery

If catalog registration succeeds but its clone fails, the local registration is preserved and appears as `missing`. Healthy catalogs remain usable, and tool listing skips unhealthy catalogs with a warning. Run `loadbot catalog status NAME` for local details, then rerun the original `loadbot catalog add NAME URL` command to retry the missing clone. Loadbot does not automatically delete registrations or perform destructive repairs.

## Tool Commands

Tools are aggregated from every registered catalog:

```bash
loadbot list
loadbot pull re-toolkit
loadbot update re-toolkit
loadbot status re-toolkit
loadbot path re-toolkit
```

If a tool name appears in more than one catalog, `list` marks it ambiguous and commands require qualification:

```bash
loadbot pull re-toolkit --catalog personal
loadbot status re-toolkit --catalog personal
```

All catalog definitions for a tool share `tools/<tool-name>/`; Loadbot does not create duplicate installations. It verifies the selected definition's origin before treating that destination as installed.

## Safety

Loadbot:

- refuses unrelated files, directories, repositories, and symlink destinations;
- refuses dirty tool or catalog updates;
- fetches only from `origin` and merges only with `--ff-only`;
- never resets, discards changes, resolves conflicts, or pushes implicitly;
- cleans up only a destination newly created by its own failed clone;
- passes URLs and names directly as process arguments without a shell;
- never executes repository scripts or modifies shell configuration.

## Legacy Migration

Previous versions stored authoritative `[tools]` entries in `LOADBOT_HOME/config.toml`. Current Loadbot detects that format and stops with migration instructions rather than discarding it.

Create or choose an empty catalog repository that does not already contain `catalog.toml`, then run:

```bash
loadbot catalog migrate personal git@github.com:USER/loadbot-catalog.git
```

Migration clones the destination, writes the legacy definitions to its new `catalog.toml`, and replaces local configuration with a writable catalog registration. It refuses existing catalog destinations and existing `catalog.toml` files. It does not commit or push; inspect the result and do those Git operations explicitly.

## Current Limitations

- Only Git repository sources are supported.
- Safe updates require a checked-out branch; detached tags or commits are not updated.
- URL equivalence only removes surrounding whitespace, a trailing slash, and one trailing `.git`; equivalent SSH and HTTPS URLs remain distinct.
- There is no catalog discovery service, package resolution, dependency installation, plugin system, script execution, background service, AI feature, or Rotbot integration.
