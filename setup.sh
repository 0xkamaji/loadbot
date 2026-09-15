#!/usr/bin/env sh

set -eu

PROJECT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
INSTALL_ROOT=${CARGO_HOME:-"$HOME/.cargo"}
INSTALL_BIN=$INSTALL_ROOT/bin
LOADBOT_BIN=$INSTALL_BIN/loadbot
COMPLETION_DIR=$INSTALL_ROOT/completions
MIN_CARGO_MAJOR=1
MIN_CARGO_MINOR=85
RUSTUP_INIT_URL=https://sh.rustup.rs
START_MARKER='# >>> loadbot >>>'
END_MARKER='# <<< loadbot <<<'

say() {
    printf '%s\n' "$1"
}

fail() {
    printf 'error: %s\n' "$1" >&2
    exit 1
}

MODE=
case ${1:-} in
    --cli) MODE=cli ;;
    --gui) MODE=gui ;;
    --all) MODE=all ;;
    --repair) MODE=repair ;;
    '') ;;
    *) fail "unknown setup option '$1'; use --cli, --gui, --all, or --repair" ;;
esac
[ "$#" -le 1 ] || fail "setup accepts only one mode option"

has_command() {
    command -v "$1" >/dev/null 2>&1
}

command_status() {
    if has_command "$1"; then
        printf 'ready'
    else
        printf 'missing'
    fi
}

cargo_version() {
    has_command cargo || return 1
    cargo --version 2>/dev/null | awk 'NR == 1 { print $2; exit }'
}

cargo_is_supported() {
    version=$(cargo_version) || return 1
    numeric=${version%%-*}
    major=${numeric%%.*}
    remainder=${numeric#*.}
    [ "$remainder" != "$numeric" ] || return 1
    minor=${remainder%%.*}
    case "$major" in ''|*[!0-9]*) return 1 ;; esac
    case "$minor" in ''|*[!0-9]*) return 1 ;; esac
    [ "$major" -gt "$MIN_CARGO_MAJOR" ] ||
        { [ "$major" -eq "$MIN_CARGO_MAJOR" ] && [ "$minor" -ge "$MIN_CARGO_MINOR" ]; }
}

rust_toolchain_status() {
    if ! has_command cargo || ! has_command rustc; then
        printf 'missing'
    elif cargo_is_supported; then
        printf 'ready'
    else
        printf 'outdated'
    fi
}

cargo_status() {
    if ! has_command cargo; then
        printf 'missing'
        return
    fi
    version=$(cargo_version || true)
    if cargo_is_supported; then
        printf 'ready (%s)' "$version"
    elif [ -n "$version" ]; then
        printf 'too old or unsupported (%s; need %s.%s+)' "$version" "$MIN_CARGO_MAJOR" "$MIN_CARGO_MINOR"
    else
        printf 'version unreadable (need %s.%s+)' "$MIN_CARGO_MAJOR" "$MIN_CARGO_MINOR"
    fi
}

rust_action() {
    [ "$(rust_toolchain_status)" != ready ] || { printf 'none'; return; }
    if has_command rustup; then
        printf 'update'
    else
        printf 'install'
    fi
}

rust_toolchain_signature() {
    printf '%s|%s|%s|%s' "$(rust_toolchain_status)" "$(cargo_version || true)" \
        "$(command -v cargo 2>/dev/null || true)" "$(command -v rustup 2>/dev/null || true)"
}

missing_system_prerequisites() {
    missing=
    for prerequisite in git; do
        if ! has_command "$prerequisite"; then
            missing="$missing $prerequisite"
        fi
    done
    if [ "$(rust_toolchain_status)" != ready ] && ! has_command rustup && ! has_command curl; then
        missing="$missing curl"
    fi
    if [ "${WANT_GUI:-false}" = true ]; then
        node_is_supported || missing="$missing nodejs"
        has_command npm || missing="$missing npm"
        gui_native_ready || missing="$missing gui-libraries"
    fi
    printf '%s' "${missing# }"
}

node_is_supported() {
    has_command node || return 1
    version=$(node --version 2>/dev/null | sed 's/^v//')
    major=$(printf '%s\n' "$version" | awk -F. 'NR == 1 { print $1 }')
    minor=$(printf '%s\n' "$version" | awk -F. 'NR == 1 { print $2 }')
    case "$major" in ''|*[!0-9]*) return 1 ;; esac
    case "$minor" in ''|*[!0-9]*) return 1 ;; esac
    [ "$major" -gt 22 ] || { [ "$major" -eq 22 ] && [ "$minor" -ge 12 ]; }
}

gui_native_ready() {
    has_command pkg-config && has_command cc && has_command make || return 1
    pkg-config --exists webkit2gtk-4.1 gtk+-3.0 librsvg-2.0 openssl 2>/dev/null &&
        { pkg-config --exists ayatana-appindicator3-0.1 2>/dev/null || pkg-config --exists appindicator3-0.1 2>/dev/null; }
}

verify_configuration_directories() {
    for directory in "${LOADBOT_HOME:-${XDG_DATA_HOME:-$HOME/.local/share}/loadbot}" \
        "${LOADBOT_CONFIG_HOME:-${XDG_CONFIG_HOME:-$HOME/.config}/loadbot}"; do
        if [ -e "$directory" ]; then
            [ -d "$directory" ] || fail "Loadbot configuration path is not a directory: $directory"
            [ -r "$directory" ] && [ -w "$directory" ] || fail "Loadbot configuration directory is not readable and writable: $directory"
        fi
    done
}

frontend_dependencies_current() {
    gui=$PROJECT_DIR/src/gui
    [ -f "$gui/node_modules/@tauri-apps/api/package.json" ] &&
        [ -f "$gui/node_modules/.loadbot-package-lock.json" ] &&
        cmp -s "$gui/package-lock.json" "$gui/node_modules/.loadbot-package-lock.json"
}

record_install_mode() {
    [ ! -L "$INSTALL_MODE_FILE" ] || fail "refusing to replace symlink installation record $INSTALL_MODE_FILE"
    [ ! -e "$INSTALL_MODE_FILE" ] || [ -f "$INSTALL_MODE_FILE" ] || fail "installation record is not a regular file: $INSTALL_MODE_FILE"
    temporary=$INSTALL_ROOT/.loadbot-install-mode.tmp.$$
    printf '%s\n' "$MODE" >"$temporary"
    chmod 600 "$temporary"
    mv -f "$temporary" "$INSTALL_MODE_FILE"
}

os_family() {
    [ -r /etc/os-release ] || return 0
    awk -F= '
        $1 == "ID" || $1 == "ID_LIKE" {
            value = $2
            gsub(/^[[:space:]\047\"]+|[[:space:]\047\"]+$/, "", value)
            print value
        }
    ' /etc/os-release | awk '
        {
            for (i = 1; i <= NF; i++) {
                if ($i == "debian" || $i == "ubuntu") debian = 1
                if ($i == "arch" || $i == "cachyos") arch = 1
            }
        }
        END {
            if (debian && !arch) print "debian"
            else if (arch && !debian) print "arch"
        }
    '
}

detect_package_manager() {
    apt_available=false
    pacman_available=false
    has_command apt-get && apt_available=true
    has_command pacman && pacman_available=true

    if [ "$apt_available" = true ] && [ "$pacman_available" = true ]; then
        family=$(os_family)
        case "$family" in
            debian) printf 'apt-get' ;;
            arch) printf 'pacman' ;;
        esac
    elif [ "$apt_available" = true ]; then
        printf 'apt-get'
    elif [ "$pacman_available" = true ]; then
        printf 'pacman'
    fi
}

package_list() {
    manager=$1
    prerequisites=$2
    packages=
    for prerequisite in $prerequisites; do
        case "$manager:$prerequisite" in
            apt-get:git) package=git ;;
            apt-get:curl) package=curl ;;
            pacman:git) package=git ;;
            pacman:curl) package=curl ;;
            apt-get:nodejs) package=nodejs ;;
            apt-get:npm) package=npm ;;
            apt-get:gui-libraries) package='libwebkit2gtk-4.1-dev build-essential wget file libxdo-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev pkg-config' ;;
            pacman:nodejs) package=nodejs ;;
            pacman:npm) package=npm ;;
            pacman:gui-libraries) package='webkit2gtk-4.1 base-devel wget file openssl appmenu-gtk-module libappindicator-gtk3 librsvg xdotool pkgconf' ;;
            *) continue ;;
        esac
        case " $packages " in
            *" $package "*) ;;
            *) packages="$packages $package" ;;
        esac
    done
    printf '%s' "${packages# }"
}

install_rust_toolchain() {
    action=$1
    if [ "$action" = update ]; then
        rustup toolchain install stable
        rustup default stable
    else
        temporary=$(mktemp "${TMPDIR:-/tmp}/loadbot-rustup-init.XXXXXX") ||
            fail "could not create rustup installer temporary file"
        trap 'rm -f "$temporary"' EXIT HUP INT TERM
        curl --proto '=https' --tlsv1.2 -sSf "$RUSTUP_INIT_URL" -o "$temporary" ||
            fail "could not download rustup from $RUSTUP_INIT_URL"
        sh "$temporary" -y --no-modify-path || fail "rustup installation failed"
        rm -f "$temporary"
        trap - EXIT HUP INT TERM
    fi

    PATH=$INSTALL_BIN:$PATH
    export PATH
    has_command rustup || fail "rustup completed, but rustup is not available in $INSTALL_BIN"
    if [ "$action" = install ]; then
        rustup default stable
    fi
    has_command cargo || fail "rustup completed, but cargo is still unavailable"
    has_command rustc || fail "rustup completed, but rustc is still unavailable"
    cargo_is_supported ||
        fail "rustup completed, but Cargo $(cargo_version || printf unknown) is older than $MIN_CARGO_MAJOR.$MIN_CARGO_MINOR"
}

shell_quote() {
    # Paths containing a single quote are represented with the standard shell splice.
    printf "'%s'" "$(printf '%s' "$1" | sed "s/'/'\\\\''/g")"
}

profile_signature() {
    path=$1
    if [ ! -e "$path" ] && [ ! -L "$path" ]; then
        printf 'missing'
    elif [ -L "$path" ]; then
        printf 'symlink'
    elif [ -f "$path" ]; then
        cksum "$path"
    else
        printf 'unsafe'
    fi
}

validate_profile() {
    path=$1
    [ ! -L "$path" ] || fail "refusing to modify symlink profile $path"
    if [ -e "$path" ]; then
        [ -f "$path" ] || fail "profile is not a regular file: $path"
        owner=$(stat -c '%u' "$path") || fail "could not inspect profile ownership: $path"
        [ "$owner" = "$(id -u)" ] || fail "profile is not owned by the current user: $path"
    fi
}

validate_markers() {
    path=$1
    [ -f "$path" ] || return 0
    awk -v start="$START_MARKER" -v end="$END_MARKER" '
        $0 == start {
            starts++
            if (inside) bad = 1
            inside = 1
        }
        $0 == end {
            ends++
            if (!inside) bad = 1
            inside = 0
        }
        END {
            if (inside || bad || starts != ends || starts > 1) exit 1
        }
    ' "$path" || fail "malformed or duplicate Loadbot managed markers in $path"
}

make_managed_block() {
    shell_name=$1
    if [ "$INSTALL_ROOT" = "$HOME/.cargo" ]; then
        case "$shell_name:${WANT_COMPLETION:-true}" in
            bash:true)
                cat <<'EOF'
# >>> loadbot >>>
export PATH="$HOME/.cargo/bin:$PATH"
[ -f "$HOME/.cargo/completions/loadbot.bash" ] && . "$HOME/.cargo/completions/loadbot.bash"
# <<< loadbot <<<
EOF
                ;;
            zsh:true)
                cat <<'EOF'
# >>> loadbot >>>
export PATH="$HOME/.cargo/bin:$PATH"
[ -f "$HOME/.cargo/completions/loadbot.zsh" ] && . "$HOME/.cargo/completions/loadbot.zsh"
# <<< loadbot <<<
EOF
                ;;
            fish:true)
                cat <<'EOF'
# >>> loadbot >>>
fish_add_path "$HOME/.cargo/bin"
test -f "$HOME/.cargo/completions/loadbot.fish"; and source "$HOME/.cargo/completions/loadbot.fish"
# <<< loadbot <<<
EOF
                ;;
            bash:false|zsh:false)
                cat <<'EOF'
# >>> loadbot >>>
export PATH="$HOME/.cargo/bin:$PATH"
# <<< loadbot <<<
EOF
                ;;
            fish:false)
                cat <<'EOF'
# >>> loadbot >>>
fish_add_path "$HOME/.cargo/bin"
# <<< loadbot <<<
EOF
                ;;
        esac
    else
        quoted_bin=$(shell_quote "$INSTALL_BIN")
        quoted_completion=$(shell_quote "$COMPLETION_DIR/loadbot.$shell_name")
        case "$shell_name:${WANT_COMPLETION:-true}" in
            bash:true|zsh:true)
                printf '%s\n' "$START_MARKER" "export PATH=$quoted_bin:\$PATH" \
                    "[ -f $quoted_completion ] && . $quoted_completion" "$END_MARKER"
                ;;
            fish:true)
                printf '%s\n' "$START_MARKER" "fish_add_path $quoted_bin" \
                    "test -f $quoted_completion; and source $quoted_completion" "$END_MARKER"
                ;;
            bash:false|zsh:false)
                printf '%s\n' "$START_MARKER" "export PATH=$quoted_bin:\$PATH" "$END_MARKER"
                ;;
            fish:false)
                printf '%s\n' "$START_MARKER" "fish_add_path $quoted_bin" "$END_MARKER"
                ;;
        esac
    fi
}

profile_action() {
    path=$1
    desired=$2
    [ -f "$path" ] || { printf 'create'; return; }
    if ! grep -Fqx "$START_MARKER" "$path"; then
        printf 'append'
        return
    fi
    existing=$(awk -v start="$START_MARKER" -v end="$END_MARKER" '
        $0 == start { inside = 1 }
        inside { print }
        $0 == end && inside { exit }
    ' "$path")
    if [ "$existing" = "$desired" ]; then
        printf 'unchanged'
    else
        printf 'replace'
    fi
}

write_profile() {
    path=$1
    desired=$2
    action=$3
    directory=$(dirname -- "$path")
    mkdir -p "$directory"
    temporary=$(mktemp "$directory/.loadbot-profile.XXXXXX") || fail "could not create profile temporary file"
    trap 'rm -f "$temporary"' EXIT HUP INT TERM

    if [ "$action" = replace ]; then
        awk -v start="$START_MARKER" -v end="$END_MARKER" -v block="$desired" '
            $0 == start { print block; skipping = 1; next }
            $0 == end && skipping { skipping = 0; next }
            !skipping { print }
        ' "$path" >"$temporary"
    else
        if [ -f "$path" ]; then
            cat "$path" >"$temporary"
            [ ! -s "$path" ] || printf '\n' >>"$temporary"
        fi
        printf '%s\n' "$desired" >>"$temporary"
    fi

    if [ -f "$path" ]; then
        backup="$path.loadbot-backup.$(date +%Y%m%d%H%M%S)"
        suffix=0
        while [ -e "$backup" ]; do
            suffix=$((suffix + 1))
            backup="$path.loadbot-backup.$(date +%Y%m%d%H%M%S).$suffix"
        done
        cp -p "$path" "$backup" || fail "could not back up $path"
        chmod --reference="$path" "$temporary" || fail "could not preserve profile permissions"
        say "Backed up profile to:"
        say "  $backup"
    else
        chmod 600 "$temporary"
    fi
    mv -f "$temporary" "$path" || fail "could not atomically update $path"
    trap - EXIT HUP INT TERM
}

[ "$(id -u)" -ne 0 ] || fail "run setup as a normal user, not as root"
[ -f "$PROJECT_DIR/Cargo.toml" ] || fail "Cargo.toml was not found in $PROJECT_DIR"

if [ -z "$MODE" ]; then
    [ -t 0 ] && [ -t 1 ] || fail "setup mode is required without an interactive terminal; use --cli, --gui, --all, or --repair"
    say "LOADBOT SETUP"
    say ""
    say "What would you like to configure?"
    say ""
    say "  1. CLI only"
    say "  2. GUI only"
    say "  3. CLI + GUI"
    say "  4. Repair / verify installation"
    say "  5. Exit"
    say ""
    printf '> '
    IFS= read -r selection || selection=5
    case "$selection" in
        1) MODE=cli ;;
        2) MODE=gui ;;
        3) MODE=all ;;
        4) MODE=repair ;;
        5) say "Setup cancelled; no changes were made."; exit 0 ;;
        *) fail "invalid setup selection '$selection'" ;;
    esac
fi

INSTALL_MODE_FILE=$INSTALL_ROOT/loadbot-install-mode
IS_REPAIR=false
if [ "$MODE" = repair ]; then
    IS_REPAIR=true
    [ -f "$INSTALL_MODE_FILE" ] || fail "no recorded Loadbot installation was found; choose CLI only, GUI only, or CLI + GUI"
    MODE=$(sed -n '1p' "$INSTALL_MODE_FILE")
    case "$MODE" in cli|gui|all) ;; *) fail "invalid installation record in $INSTALL_MODE_FILE" ;; esac
    say "Repairing recorded $MODE installation."
fi
[ "$IS_REPAIR" = false ] || verify_configuration_directories
WANT_GUI=false
WANT_COMPLETION=false
[ "$MODE" != gui ] && WANT_COMPLETION=true
[ "$MODE" = gui ] || [ "$MODE" = all ] && WANT_GUI=true

shell_name=
profile_path=
case ${SHELL:-} in
    */bash) shell_name=bash; profile_path=$HOME/.bashrc ;;
    */zsh) shell_name=zsh; profile_path=$HOME/.zshrc ;;
    */fish) shell_name=fish; profile_path=$HOME/.config/fish/config.fish ;;
esac

profile_change=none
profile_before=none
managed_block=
if [ -n "$profile_path" ]; then
    validate_profile "$profile_path"
    validate_markers "$profile_path"
    managed_block=$(make_managed_block "$shell_name")
    profile_change=$(profile_action "$profile_path" "$managed_block")
    profile_before=$(profile_signature "$profile_path")
fi

rust_before=$(rust_toolchain_signature)
toolchain_action=$(rust_action)
missing=$(missing_system_prerequisites)
manager=
packages=
if [ -n "$missing" ]; then
    manager=$(detect_package_manager)
    if [ -n "$manager" ]; then
        packages=$(package_list "$manager" "$missing")
    fi
fi

say "LOADBOT SETUP PLAN"
say "Mode: $MODE"
say ""
say "Prerequisites:"
printf '  git:   %s\n' "$(command_status git)"
printf '  cargo: %s\n' "$(cargo_status)"
printf '  rustc: %s\n' "$(command_status rustc)"
printf '  rustup: %s\n' "$(command_status rustup)"
if [ "$WANT_GUI" = true ]; then
    printf '  node:  %s\n' "$(if node_is_supported; then node --version; else printf 'missing or older than 22'; fi)"
    printf '  npm:   %s\n' "$(command_status npm)"
    printf '  native GUI libraries: %s\n' "$(if gui_native_ready; then printf ready; else printf missing; fi)"
fi
if [ "$toolchain_action" != none ]; then
    say ""
    say "Rust toolchain:"
    if [ "$toolchain_action" = update ]; then
        say "  Update/install stable with the existing rustup"
        say "  rustup toolchain install stable"
        say "  rustup default stable"
    else
        say "  Install stable with rustup (Cargo $MIN_CARGO_MAJOR.$MIN_CARGO_MINOR+ required)"
        say "  Download $RUSTUP_INIT_URL over HTTPS, then run it with --no-modify-path"
    fi
fi
say ""
say "Package manager:"
say "  ${manager:-none required or supported}"
if [ -n "$missing" ] && [ -n "$manager" ]; then
    say ""
    say "Would run (elevation required through sudo):"
    case "$manager" in
        apt-get)
            say "  sudo apt-get update"
            say "  sudo apt-get install -y $packages"
            ;;
        pacman)
            say "  sudo pacman -S --needed $packages"
            ;;
    esac
fi
say ""
say "Would install:"
say "  $LOADBOT_BIN"
if [ "$WANT_GUI" = true ]; then
    say "  $INSTALL_BIN/loadbot-desktop"
fi
say ""
say "Would configure:"
if [ -n "$profile_path" ]; then
    say "  $profile_path ($profile_change)"
    if [ "$WANT_COMPLETION" = true ]; then say "  $COMPLETION_DIR/loadbot.$shell_name"; fi
else
    say "  No profile (unsupported or unknown login shell: ${SHELL:-unset})"
    say "  Completion files in $COMPLETION_DIR"
fi

if [ -n "$missing" ] && [ -z "$manager" ]; then
    say ""
    say "Missing system prerequisites: $missing"
    say "No supported package manager applies. Install the missing commands manually and rerun setup."
    say "Supported automatic managers are apt-get on Debian/Ubuntu and pacman on Arch/CachyOS."
    exit 1
fi

needs_approval=false
[ -n "$missing" ] && needs_approval=true
[ "$toolchain_action" != none ] && needs_approval=true
case "$profile_change" in create|append|replace) needs_approval=true ;; esac
if [ "$needs_approval" = true ]; then
    if [ ! -t 0 ] || [ ! -t 1 ]; then
        if [ -n "$missing" ] || [ "$toolchain_action" != none ]; then
            say ""
            say "Noninteractive setup cannot install or update prerequisites. Run setup in an interactive terminal."
        else
            say ""
            say "Noninteractive setup cannot approve profile changes. Rerun in an interactive terminal."
        fi
        exit 1
    fi
    say ""
    if [ -n "$missing" ] || [ "$toolchain_action" != none ]; then
        printf 'Install/update these prerequisites? [y/N] '
    else
        printf 'Proceed? [y/N] '
    fi
    IFS= read -r answer || answer=
    case "$answer" in y|Y|yes|YES|Yes) ;; *) say "Setup cancelled; no changes were made."; exit 1 ;; esac
fi

[ "$(missing_system_prerequisites)" = "$missing" ] || fail "prerequisite state changed after approval; rerun setup"
[ "$(rust_toolchain_signature)" = "$rust_before" ] || fail "Rust toolchain changed after approval; rerun setup"
if [ -n "$profile_path" ]; then
    [ "$(profile_signature "$profile_path")" = "$profile_before" ] ||
        fail "profile changed after approval; rerun setup"
fi

if [ -n "$missing" ]; then
    case "$manager" in
        apt-get)
            sudo apt-get update
            sudo apt-get install -y $packages
            ;;
        pacman)
            sudo pacman -S --needed $packages
            ;;
    esac
    remaining=$(missing_system_prerequisites)
    case " $remaining " in
        *" nodejs "*) fail "Node.js 22.12 or newer is still unavailable; install a current Node.js LTS release and rerun setup" ;;
    esac
    [ -z "$remaining" ] || fail "prerequisite installation completed but these commands remain missing: $remaining"
fi

if [ "$toolchain_action" != none ]; then
    say "Installing a supported Rust toolchain with rustup..."
    install_rust_toolchain "$toolchain_action"
fi

say "Installing Loadbot from source..."
cargo install \
    --path "$PROJECT_DIR" \
    --root "$INSTALL_ROOT" \
    --locked \
    --force

[ -x "$LOADBOT_BIN" ] || fail "Cargo completed, but $LOADBOT_BIN was not created"
"$LOADBOT_BIN" --version || fail "Loadbot failed its version verification check"
"$LOADBOT_BIN" --help >/dev/null || fail "Loadbot failed its help verification check"

if [ "$WANT_GUI" = true ]; then
    if frontend_dependencies_current; then
        say "Lockfile-pinned GUI dependencies are current."
    else
        say "Restoring lockfile-pinned GUI dependencies..."
        npm --prefix "$PROJECT_DIR/src/gui" ci || fail "npm ci failed while restoring GUI dependencies"
        cp "$PROJECT_DIR/src/gui/package-lock.json" "$PROJECT_DIR/src/gui/node_modules/.loadbot-package-lock.json"
    fi
    say "Building the native Loadbot GUI..."
    npm --prefix "$PROJECT_DIR/src/gui" run desktop:build || fail "native Loadbot GUI build failed"
    GUI_BUILD=$PROJECT_DIR/src/gui/src-tauri/target/release/loadbot-desktop
    [ -x "$GUI_BUILD" ] || fail "Tauri completed, but $GUI_BUILD was not created"
    GUI_TEMP=$INSTALL_BIN/.loadbot-desktop.tmp.$$
    cp "$GUI_BUILD" "$GUI_TEMP"
    chmod 755 "$GUI_TEMP"
    mv -f "$GUI_TEMP" "$INSTALL_BIN/loadbot-desktop"
fi

if [ "$WANT_COMPLETION" = true ]; then
    say "Generating shell completion scripts..."
    mkdir -p "$COMPLETION_DIR"
    for completion_shell in bash zsh fish powershell; do
        extension=$completion_shell
        [ "$completion_shell" != powershell ] || extension=ps1
        destination=$COMPLETION_DIR/loadbot.$extension
        temporary=$COMPLETION_DIR/.loadbot.$extension.tmp.$$
        if COMPLETE=$completion_shell "$LOADBOT_BIN" >"$temporary"; then
            mv -f "$temporary" "$destination"
        else
            rm -f "$temporary"
            fail "Loadbot failed to generate $completion_shell completions"
        fi
    done
fi

if [ -n "$profile_path" ] && [ "$profile_change" != unchanged ]; then
    [ "$(profile_signature "$profile_path")" = "$profile_before" ] ||
        fail "profile changed while Loadbot was being installed; rerun setup"
    if [ "$profile_change" = replace ]; then
        say "Updating the existing Loadbot managed block in $profile_path"
    fi
    write_profile "$profile_path" "$managed_block" "$profile_change"
fi

case ":$PATH:" in
    *":$INSTALL_BIN:"*) ;;
    *) PATH=$INSTALL_BIN:$PATH; export PATH ;;
esac
record_install_mode

say ""
say "Loadbot installed and verified successfully:"
say "  $LOADBOT_BIN"
if [ -n "$profile_path" ]; then
    if [ "$WANT_COMPLETION" = true ]; then
        say "PATH and completion configured for $shell_name in:"
    else
        say "PATH configured for $shell_name in:"
    fi
    say "  $profile_path"
    say "Open a new terminal, or reload this configuration now:"
    case "$shell_name" in
        bash) say "  source \"$profile_path\"" ;;
        zsh) say "  source \"$profile_path\"" ;;
        fish) say "  source \"$profile_path\"" ;;
    esac
else
    say "The login shell was not recognized, so no profile was changed."
    if [ "$WANT_COMPLETION" = true ]; then
        say "Configure PATH and completion manually for the shell you use:"
        say "  Bash: export PATH=\"$INSTALL_BIN:\$PATH\"; . \"$COMPLETION_DIR/loadbot.bash\""
        say "  Zsh:  export PATH=\"$INSTALL_BIN:\$PATH\"; . \"$COMPLETION_DIR/loadbot.zsh\""
        say "  Fish: fish_add_path \"$INSTALL_BIN\"; source \"$COMPLETION_DIR/loadbot.fish\""
    else
        say "Add $INSTALL_BIN to PATH in the shell you use."
    fi
fi
say "The already-running parent process was not modified."
