#!/usr/bin/env sh

set -eu

PROJECT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
INSTALL_ROOT=${CARGO_HOME:-"$HOME/.cargo"}
INSTALL_BIN=$INSTALL_ROOT/bin
LOADBOT_BIN=$INSTALL_BIN/loadbot
COMPLETION_DIR=$INSTALL_ROOT/completions
START_MARKER='# >>> loadbot >>>'
END_MARKER='# <<< loadbot <<<'

say() {
    printf '%s\n' "$1"
}

fail() {
    printf 'error: %s\n' "$1" >&2
    exit 1
}

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

missing_prerequisites() {
    missing=
    for prerequisite in git cargo rustc; do
        if ! has_command "$prerequisite"; then
            missing="$missing $prerequisite"
        fi
    done
    printf '%s' "${missing# }"
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
            apt-get:cargo) package=cargo ;;
            apt-get:rustc) package=rustc ;;
            pacman:git) package=git ;;
            pacman:cargo) package=cargo ;;
            pacman:rustc) package=rust ;;
            *) continue ;;
        esac
        case " $packages " in
            *" $package "*) ;;
            *) packages="$packages $package" ;;
        esac
    done
    printf '%s' "${packages# }"
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
        case "$shell_name" in
            bash)
                cat <<'EOF'
# >>> loadbot >>>
export PATH="$HOME/.cargo/bin:$PATH"
[ -f "$HOME/.cargo/completions/loadbot.bash" ] && . "$HOME/.cargo/completions/loadbot.bash"
# <<< loadbot <<<
EOF
                ;;
            zsh)
                cat <<'EOF'
# >>> loadbot >>>
export PATH="$HOME/.cargo/bin:$PATH"
[ -f "$HOME/.cargo/completions/loadbot.zsh" ] && . "$HOME/.cargo/completions/loadbot.zsh"
# <<< loadbot <<<
EOF
                ;;
            fish)
                cat <<'EOF'
# >>> loadbot >>>
fish_add_path "$HOME/.cargo/bin"
test -f "$HOME/.cargo/completions/loadbot.fish"; and source "$HOME/.cargo/completions/loadbot.fish"
# <<< loadbot <<<
EOF
                ;;
        esac
    else
        quoted_bin=$(shell_quote "$INSTALL_BIN")
        quoted_completion=$(shell_quote "$COMPLETION_DIR/loadbot.$shell_name")
        case "$shell_name" in
            bash|zsh)
                printf '%s\n' "$START_MARKER" "export PATH=$quoted_bin:\$PATH" \
                    "[ -f $quoted_completion ] && . $quoted_completion" "$END_MARKER"
                ;;
            fish)
                printf '%s\n' "$START_MARKER" "fish_add_path $quoted_bin" \
                    "test -f $quoted_completion; and source $quoted_completion" "$END_MARKER"
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

missing=$(missing_prerequisites)
manager=
packages=
if [ -n "$missing" ]; then
    manager=$(detect_package_manager)
    if [ -n "$manager" ]; then
        packages=$(package_list "$manager" "$missing")
    fi
fi

say "LOADBOT SETUP PLAN"
say ""
say "Prerequisites:"
printf '  git:   %s\n' "$(command_status git)"
printf '  cargo: %s\n' "$(command_status cargo)"
printf '  rustc: %s\n' "$(command_status rustc)"
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
say ""
say "Would configure:"
if [ -n "$profile_path" ]; then
    say "  $profile_path ($profile_change)"
    say "  $COMPLETION_DIR/loadbot.$shell_name"
else
    say "  No profile (unsupported or unknown login shell: ${SHELL:-unset})"
    say "  Completion files in $COMPLETION_DIR"
fi

if [ -n "$missing" ] && [ -z "$manager" ]; then
    say ""
    say "Missing prerequisites: $missing"
    say "No supported package manager applies. Install the missing commands manually and rerun setup."
    say "Supported automatic managers are apt-get on Debian/Ubuntu and pacman on Arch/CachyOS."
    exit 1
fi

needs_approval=false
[ -n "$missing" ] && needs_approval=true
case "$profile_change" in create|append|replace) needs_approval=true ;; esac
if [ "$needs_approval" = true ]; then
    if [ ! -t 0 ] || [ ! -t 1 ]; then
        if [ -n "$missing" ]; then
            say ""
            say "Noninteractive setup cannot install missing prerequisites. Run the exact command(s) shown above manually, then rerun setup."
        else
            say ""
            say "Noninteractive setup cannot approve profile changes. Rerun in an interactive terminal."
        fi
        exit 1
    fi
    say ""
    if [ -n "$missing" ]; then
        printf 'Install these prerequisites? [y/N] '
    else
        printf 'Proceed? [y/N] '
    fi
    IFS= read -r answer || answer=
    case "$answer" in y|Y|yes|YES|Yes) ;; *) say "Setup cancelled; no changes were made."; exit 1 ;; esac
fi

[ "$(missing_prerequisites)" = "$missing" ] || fail "prerequisite state changed after approval; rerun setup"
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
    remaining=$(missing_prerequisites)
    [ -z "$remaining" ] || fail "prerequisite installation completed but these commands remain missing: $remaining"
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

say ""
say "Loadbot installed and verified successfully:"
say "  $LOADBOT_BIN"
if [ -n "$profile_path" ]; then
    say "Completion configured for $shell_name in:"
    say "  $profile_path"
    say "Open a new terminal, or reload this configuration now:"
    case "$shell_name" in
        bash) say "  source \"$profile_path\"" ;;
        zsh) say "  source \"$profile_path\"" ;;
        fish) say "  source \"$profile_path\"" ;;
    esac
else
    say "The login shell was not recognized, so no profile was changed."
    say "Configure PATH and completion manually for the shell you use:"
    say "  Bash: export PATH=\"$INSTALL_BIN:\$PATH\"; . \"$COMPLETION_DIR/loadbot.bash\""
    say "  Zsh:  export PATH=\"$INSTALL_BIN:\$PATH\"; . \"$COMPLETION_DIR/loadbot.zsh\""
    say "  Fish: fish_add_path \"$INSTALL_BIN\"; source \"$COMPLETION_DIR/loadbot.fish\""
fi
say "The already-running parent process was not modified."
