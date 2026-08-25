# Loadbot

Loadbot is a small cross-platform CLI that registers and manages whole Git repositories in a predictable, Loadbot-owned tools directory. It invokes the installed `git` command and never parses, installs, copies, or executes scripts from downloaded repositories.

## Build

Rust and Git must be installed and available in `PATH`.

```bash
cargo build --release
```

Cargo produces a normal native executable for the selected target (`loadbot` on Linux and `loadbot.exe` on Windows). Private repositories use the user's existing Git configuration, credential helpers, and SSH keys; Loadbot does not manage credentials.

## Storage

Loadbot keeps all of its files under one root:

- Linux: `~/.local/share/loadbot/`
- Native Windows: `%LOCALAPPDATA%\loadbot\`

The root contains `config.toml` and a `tools` directory. Set `LOADBOT_HOME` to override the root, which is useful for portable use and isolated testing. Read-only commands do not create these directories.

## Interactive Use

Omit arguments in an interactive terminal to use prompts and configured-tool selection:

```bash
loadbot add
loadbot pull
loadbot update
loadbot status
loadbot path
```

Interactive `add` asks for the Git URL, suggests a safe name when one can be inferred, and accepts an optional revision. Leaving the revision empty lets Git use the remote's default branch. After showing a confirmation summary, Loadbot can optionally pull the repository immediately.

Prompts start only when both stdin and stdout are terminals. An incomplete command with redirected input or output fails instead of waiting. `list` remains directly printable and noninteractive, and a top-level action menu is not implemented yet.

## Direct Use

Supplying all required arguments preserves automation-friendly, noninteractive behavior:

```bash
loadbot add rot-tools \
  git@github.com:0xkamaji/rot-tools.git \
  --revision main

loadbot pull rot-tools
```

The complete direct workflow remains:

```bash
loadbot add re-toolbox git@github.com:0xkamaji/re-toolbox.git --revision main
loadbot pull re-toolbox
loadbot status re-toolbox
loadbot path re-toolbox
loadbot update re-toolbox
```

Direct `add` only records the repository and never asks whether to pull. `pull` clones the entire repository. `list` reports configured tools without network access, and `path` prints only the absolute destination path on stdout in both usage styles.

Updates refuse dirty working trees, fetch only from `origin`, and merge only with `--ff-only`. Loadbot never resets, discards changes, resolves conflicts, modifies shell startup files, or executes repository content.

## Current Limitations

- Only Git repository sources are supported.
- Safe updates currently require a checked-out branch. Cloned tags are usable, but their detached working trees cannot be updated by this version.
- URL equivalence only normalizes surrounding whitespace, a trailing slash, and a trailing `.git`; SSH and HTTPS forms of the same repository are intentionally not treated as identical.
- Loadbot does not provide package resolution, dependency installation, plugins, manifests, registries, script execution, or Rotbot integration.
