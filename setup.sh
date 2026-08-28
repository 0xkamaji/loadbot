#!/usr/bin/env sh

set -eu

PROJECT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
INSTALL_ROOT="${CARGO_HOME:-$HOME/.cargo}"
INSTALL_BIN="$INSTALL_ROOT/bin"
LOADBOT_BIN="$INSTALL_BIN/loadbot"
COMPLETION_DIR="$INSTALL_ROOT/completions"

say() {
    printf '%s\n' "$1"
}

fail() {
    printf 'error: %s\n' "$1" >&2
    exit 1
}

require_command() {
    command_name=$1

    if ! command -v "$command_name" >/dev/null 2>&1; then
        fail "'$command_name' is required but was not found in PATH"
    fi
}

say "Checking Loadbot prerequisites..."

require_command cargo
require_command rustc
require_command git

[ -f "$PROJECT_DIR/Cargo.toml" ] ||
    fail "Cargo.toml was not found in $PROJECT_DIR"

say "Installing Loadbot from source..."

cargo install \
    --path "$PROJECT_DIR" \
    --root "$INSTALL_ROOT" \
    --locked \
    --force

[ -x "$LOADBOT_BIN" ] ||
    fail "Cargo completed, but $LOADBOT_BIN was not created"

"$LOADBOT_BIN" --help >/dev/null ||
    fail "Loadbot was installed but failed its verification check"

say "Generating shell completion scripts..."
mkdir -p "$COMPLETION_DIR"
COMPLETE=bash "$LOADBOT_BIN" >"$COMPLETION_DIR/loadbot.bash"
COMPLETE=zsh "$LOADBOT_BIN" >"$COMPLETION_DIR/loadbot.zsh"
COMPLETE=fish "$LOADBOT_BIN" >"$COMPLETION_DIR/loadbot.fish"
COMPLETE=powershell "$LOADBOT_BIN" >"$COMPLETION_DIR/loadbot.ps1"

say "Loadbot installed successfully:"
say "  $LOADBOT_BIN"
say "Completion scripts generated in:"
say "  $COMPLETION_DIR"
say "Source the script for your shell:"
say "  Bash:       . \"$COMPLETION_DIR/loadbot.bash\""
say "  Zsh:        . \"$COMPLETION_DIR/loadbot.zsh\""
say "  Fish:       source \"$COMPLETION_DIR/loadbot.fish\""
say "  PowerShell: . \"$COMPLETION_DIR/loadbot.ps1\""

case ":$PATH:" in
    *":$INSTALL_BIN:"*)
        say ""
        say "Run:"
        say "  loadbot --help"
        ;;
    *)
        say ""
        say "Cargo's binary directory is not currently in PATH."
        say "Add this line to your shell configuration:"
        say ""
        say "  export PATH=\"$INSTALL_BIN:\$PATH\""
        say ""
        say "Until then, run Loadbot directly:"
        say "  $LOADBOT_BIN --help"
        ;;
esac
