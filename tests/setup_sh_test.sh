#!/usr/bin/env sh

set -eu

REPOSITORY=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
SYSTEM_PATH=$PATH
TEST_ROOT=${TMPDIR:-/tmp}/loadbot-setup-tests.$$
PASS=0
FAIL=0

cleanup() {
    rm -rf "$TEST_ROOT"
}
trap cleanup EXIT HUP INT TERM
mkdir -p "$TEST_ROOT"

pass() {
    PASS=$((PASS + 1))
    printf 'ok %s - %s\n' "$PASS" "$1"
}

fail_test() {
    FAIL=$((FAIL + 1))
    printf 'not ok - %s\n' "$1" >&2
}

assert_contains() {
    description=$1
    needle=$2
    file=$3
    if grep -F "$needle" "$file" >/dev/null; then pass "$description"; else fail_test "$description"; fi
}

assert_not_exists() {
    description=$1
    path=$2
    if [ ! -e "$path" ] && [ ! -L "$path" ]; then pass "$description"; else fail_test "$description"; fi
}

assert_count() {
    description=$1
    expected=$2
    needle=$3
    file=$4
    actual=$(grep -F -c "$needle" "$file" || true)
    if [ "$actual" = "$expected" ]; then pass "$description"; else fail_test "$description (expected $expected, got $actual)"; fi
}

new_case() {
    name=$1
    CASE_DIR="$TEST_ROOT/$name with spaces"
    HOME="$CASE_DIR/home with spaces"
    CARGO_HOME="$HOME/.cargo"
    PROJECT="$CASE_DIR/project with spaces"
    FAKE_BIN="$CASE_DIR/fake bin"
    COMMAND_LOG="$CASE_DIR/commands.log"
    OUTPUT="$CASE_DIR/output.txt"
    mkdir -p "$HOME" "$PROJECT" "$FAKE_BIN"
    cp "$REPOSITORY/setup.sh" "$PROJECT/setup.sh"
    cp "$REPOSITORY/Cargo.toml" "$PROJECT/Cargo.toml"
    : >"$COMMAND_LOG"

    for utility in sh awk sed grep cksum cmp mkdir mktemp cat date cp chmod mv rm dirname ln wc; do
        utility_path=$(PATH=$SYSTEM_PATH command -v "$utility")
        ln -s "$utility_path" "$FAKE_BIN/$utility"
    done
    cat >"$FAKE_BIN/id" <<'EOF'
#!/bin/sh
[ "${1:-}" = -u ] && { printf '%s\n' 1000; exit 0; }
exit 1
EOF
    chmod +x "$FAKE_BIN/id"
    cat >"$FAKE_BIN/stat" <<'EOF'
#!/bin/sh
[ "${1:-}" = -c ] && [ "${2:-}" = %u ] && { printf '%s\n' 1000; exit 0; }
exit 1
EOF
    chmod +x "$FAKE_BIN/stat"
    make_prerequisite git
    make_prerequisite rustc
    cp "$FAKE_BIN/rustc" "$CASE_DIR/rustc-template"
    make_cargo
    make_rustup
    make_curl
    make_sudo
    make_manager apt-get
    PATH=$FAKE_BIN
    SHELL=/bin/bash
    export CASE_DIR HOME CARGO_HOME PROJECT FAKE_BIN COMMAND_LOG OUTPUT PATH SHELL
    unset FAKE_SUDO_FAIL FAKE_CARGO_FAIL FAKE_INSTALL_PREREQUISITES \
        FAKE_CARGO_VERSION FAKE_INSTALLED_CARGO_VERSION FAKE_CURL_FAIL \
        FAKE_RUSTUP_FAIL FAKE_RUSTUP_INSTALL_FAIL
}

make_prerequisite() {
    name=$1
    cat >"$FAKE_BIN/$name" <<EOF
#!/bin/sh
printf '%s\\n' '$name \$*' >>"\$COMMAND_LOG"
exit 0
EOF
    chmod +x "$FAKE_BIN/$name"
}

make_cargo() {
    cat >"$FAKE_BIN/cargo" <<'EOF'
#!/bin/sh
if [ "${1:-}" = --version ]; then
    case "$0" in
        "$CARGO_HOME/bin/cargo") version=${FAKE_INSTALLED_CARGO_VERSION:-1.85.0} ;;
        *) version=${FAKE_CARGO_VERSION:-1.85.0} ;;
    esac
    printf 'cargo %s (fake)\n' "$version"
    exit 0
fi
printf '%s\n' "cargo $*" >>"$COMMAND_LOG"
[ "${FAKE_CARGO_FAIL:-0}" != 1 ] || exit 42
root=
while [ "$#" -gt 0 ]; do
    if [ "$1" = --root ]; then shift; root=$1; fi
    shift
done
mkdir -p "$root/bin"
cat >"$root/bin/loadbot" <<'LOADBOT'
#!/bin/sh
printf '%s\n' "loadbot $* COMPLETE=${COMPLETE:-}" >>"$COMMAND_LOG"
case ${COMPLETE:-} in
    '') [ "${1:-}" = --version ] && printf '%s\n' 'loadbot 0.1.0';;
    *) printf '%s\n' "completion for $COMPLETE";;
esac
exit 0
LOADBOT
chmod +x "$root/bin/loadbot"
EOF
    chmod +x "$FAKE_BIN/cargo"
    cp "$FAKE_BIN/cargo" "$CASE_DIR/cargo-template"
}

make_rustup() {
    cat >"$CASE_DIR/rustup-template" <<'EOF'
#!/bin/sh
printf '%s\n' "rustup $*" >>"$COMMAND_LOG"
[ "${FAKE_RUSTUP_FAIL:-0}" != 1 ] || exit 44
mkdir -p "$CARGO_HOME/bin"
cp "$CASE_DIR/cargo-template" "$CARGO_HOME/bin/cargo"
cp "$CASE_DIR/rustc-template" "$CARGO_HOME/bin/rustc"
cp "$CASE_DIR/rustup-template" "$CARGO_HOME/bin/rustup"
chmod +x "$CARGO_HOME/bin/cargo" "$CARGO_HOME/bin/rustc" "$CARGO_HOME/bin/rustup"
exit 0
EOF
    chmod +x "$CASE_DIR/rustup-template"

    cat >"$CASE_DIR/rustup-init-template" <<'EOF'
#!/bin/sh
printf '%s\n' "rustup-init $*" >>"$COMMAND_LOG"
[ "${FAKE_RUSTUP_INSTALL_FAIL:-0}" != 1 ] || exit 45
mkdir -p "$CARGO_HOME/bin"
cp "$CASE_DIR/cargo-template" "$CARGO_HOME/bin/cargo"
cp "$CASE_DIR/rustc-template" "$CARGO_HOME/bin/rustc"
cp "$CASE_DIR/rustup-template" "$CARGO_HOME/bin/rustup"
chmod +x "$CARGO_HOME/bin/cargo" "$CARGO_HOME/bin/rustc" "$CARGO_HOME/bin/rustup"
exit 0
EOF
    chmod +x "$CASE_DIR/rustup-init-template"
}

make_curl() {
    cat >"$FAKE_BIN/curl" <<'EOF'
#!/bin/sh
printf '%s\n' "curl $*" >>"$COMMAND_LOG"
[ "${FAKE_CURL_FAIL:-0}" != 1 ] || exit 43
output=
while [ "$#" -gt 0 ]; do
    if [ "$1" = -o ]; then shift; output=$1; fi
    shift
done
[ -n "$output" ] || exit 46
cp "$CASE_DIR/rustup-init-template" "$output"
exit 0
EOF
    chmod +x "$FAKE_BIN/curl"
}

make_sudo() {
    cat >"$FAKE_BIN/sudo" <<'EOF'
#!/bin/sh
printf '%s\n' "sudo $*" >>"$COMMAND_LOG"
[ "${FAKE_SUDO_FAIL:-0}" != 1 ] || exit 23
if [ "${FAKE_INSTALL_PREREQUISITES:-0}" = 1 ]; then
    case " $* " in *" cargo "*) cp "$CASE_DIR/cargo-template" "$FAKE_BIN/cargo"; chmod +x "$FAKE_BIN/cargo";; esac
    case " $* " in *" rustc "*|*" rust "*) printf '#!/bin/sh\nexit 0\n' >"$FAKE_BIN/rustc"; chmod +x "$FAKE_BIN/rustc";; esac
    case " $* " in *" git "*) printf '#!/bin/sh\nexit 0\n' >"$FAKE_BIN/git"; chmod +x "$FAKE_BIN/git";; esac
    case " $* " in
        *"libwebkit2gtk-4.1-dev"*|*"webkit2gtk-4.1"*)
            for tool in pkg-config cc make; do printf '#!/bin/sh\nexit 0\n' >"$FAKE_BIN/$tool"; chmod +x "$FAKE_BIN/$tool"; done
            ;;
    esac
fi
exit 0
EOF
    chmod +x "$FAKE_BIN/sudo"
}

make_manager() {
    name=$1
    cat >"$FAKE_BIN/$name" <<EOF
#!/bin/sh
printf '%s\\n' '$name \$*' >>"\$COMMAND_LOG"
exit 99
EOF
    chmod +x "$FAKE_BIN/$name"
}

run_interactive() {
    answer=$1
    login_shell=$SHELL
    set +e
    printf '%s\n' "$answer" | SHELL=/bin/sh /usr/bin/script -qefc \
        "SHELL='$login_shell' /bin/sh '$PROJECT/setup.sh' --cli" /dev/null >"$OUTPUT" 2>&1
    STATUS=$?
    set -e
}

run_noninteractive() {
    set +e
    /bin/sh "$PROJECT/setup.sh" --cli >"$OUTPUT" 2>&1
    STATUS=$?
    set -e
}

run_mode_interactive() {
    mode=$1
    answer=$2
    login_shell=$SHELL
    set +e
    printf '%s\n' "$answer" | SHELL=/bin/sh /usr/bin/script -qefc \
        "SHELL='$login_shell' /bin/sh '$PROJECT/setup.sh' '--$mode'" /dev/null >"$OUTPUT" 2>&1
    STATUS=$?
    set -e
}

run_menu() {
    answers=$1
    login_shell=$SHELL
    set +e
    printf '%s\n' "$answers" | SHELL=/bin/sh /usr/bin/script -qefc \
        "SHELL='$login_shell' /bin/sh '$PROJECT/setup.sh'" /dev/null >"$OUTPUT" 2>&1
    STATUS=$?
    set -e
}

prepare_gui() {
    mkdir -p "$PROJECT/src/gui/src-tauri/target/release"
    printf '{}\n' >"$PROJECT/src/gui/package.json"
    printf '{"lockfileVersion":3}\n' >"$PROJECT/src/gui/package-lock.json"
    for name in cc make; do make_prerequisite "$name"; done
    cat >"$FAKE_BIN/node" <<'EOF'
#!/bin/sh
printf 'v22.12.0\n'
EOF
    chmod +x "$FAKE_BIN/node"
    cat >"$FAKE_BIN/pkg-config" <<'EOF'
#!/bin/sh
exit 0
EOF
    chmod +x "$FAKE_BIN/pkg-config"
    cat >"$FAKE_BIN/npm" <<'EOF'
#!/bin/sh
printf '%s\n' "npm $*" >>"$COMMAND_LOG"
mkdir -p "$PROJECT/src/gui/node_modules"
case " $* " in
    *" ci "*|*" ci") mkdir -p "$PROJECT/src/gui/node_modules/@tauri-apps/api"; printf '{}\n' >"$PROJECT/src/gui/node_modules/@tauri-apps/api/package.json" ;;
    *" desktop:build "*)
        printf '#!/bin/sh\nexit 0\n' >"$PROJECT/src/gui/src-tauri/target/release/loadbot-desktop"
        chmod +x "$PROJECT/src/gui/src-tauri/target/release/loadbot-desktop"
        ;;
esac
EOF
    chmod +x "$FAKE_BIN/npm"
}

install_legacy_binary() {
    path=$1
    mkdir -p "$(dirname -- "$path")"
    printf '#!/bin/sh\nexit 0\n' >"$path"
    chmod +x "$path"
}

run_without_mode() {
    set +e
    /bin/sh "$PROJECT/setup.sh" >"$OUTPUT" 2>&1
    STATUS=$?
    set -e
}

# Ready prerequisites, absolute verification, Bash creation, completions, and paths with spaces.
new_case ready
run_interactive y
[ "$STATUS" -eq 0 ] && pass "all prerequisites already installed" || fail_test "all prerequisites already installed"
assert_contains "absolute version verification" "loadbot --version" "$COMMAND_LOG"
assert_contains "absolute help verification" "loadbot --help" "$COMMAND_LOG"
assert_not_exists "Cargo bin was not required in initial PATH" "$FAKE_BIN/loadbot"
assert_contains "Bash profile created" '# >>> loadbot >>>' "$HOME/.bashrc"
assert_contains "Bash PATH configured" 'export PATH="$HOME/.cargo/bin:$PATH"' "$HOME/.bashrc"
assert_contains "Bash completion configured" 'loadbot.bash' "$HOME/.bashrc"
assert_contains "completion generated" 'completion for bash' "$CARGO_HOME/completions/loadbot.bash"
assert_contains "path containing spaces installed" "$CARGO_HOME/bin/loadbot" "$OUTPUT"

# Existing profile preservation, backup, replacement, and repeat idempotency.
new_case existing
printf '%s\n' '# existing content' >"$HOME/.bashrc"
run_interactive y
assert_contains "existing Bash profile preserved" '# existing content' "$HOME/.bashrc"
backup=$(printf '%s\n' "$HOME"/.bashrc.loadbot-backup.*)
[ -f "$backup" ] && pass "profile backup made before change" || fail_test "profile backup made before change"
before_backups=$(printf '%s\n' "$HOME"/.bashrc.loadbot-backup.* | wc -l)
run_noninteractive
[ "$STATUS" -eq 0 ] && pass "repeated complete setup succeeds without prompting" || fail_test "repeated complete setup succeeds without prompting"
assert_count "managed block is idempotent" 1 '# >>> loadbot >>>' "$HOME/.bashrc"
after_backups=$(printf '%s\n' "$HOME"/.bashrc.loadbot-backup.* | wc -l)
[ "$before_backups" = "$after_backups" ] && pass "idempotent setup avoids backup" || fail_test "idempotent setup avoids backup"

new_case replace
cat >"$HOME/.bashrc" <<'EOF'
before
# >>> loadbot >>>
old loadbot settings
# <<< loadbot <<<
after
EOF
run_interactive y
assert_contains "differing managed block update announced" "Updating the existing Loadbot managed block" "$OUTPUT"
assert_contains "content before replacement preserved" "before" "$HOME/.bashrc"
assert_contains "content after replacement preserved" "after" "$HOME/.bashrc"
assert_count "only managed block replaced" 1 '# >>> loadbot >>>' "$HOME/.bashrc"

# Rust version handling, rustup installation, package managers, and failures.
new_case rustup_install
rm "$FAKE_BIN/cargo" "$FAKE_BIN/rustc"
run_interactive y
[ "$STATUS" -eq 0 ] && pass "missing Rust toolchain is installed with rustup" || fail_test "missing Rust toolchain is installed with rustup"
assert_contains "rustup install is proposed" "Install stable with rustup" "$OUTPUT"
assert_contains "rustup installer is downloaded over HTTPS" "curl --proto =https --tlsv1.2 -sSf https://sh.rustup.rs" "$COMMAND_LOG"
assert_contains "rustup installer avoids editing profiles" "rustup-init -y --no-modify-path" "$COMMAND_LOG"

new_case outdated
FAKE_CARGO_VERSION=1.75.0; export FAKE_CARGO_VERSION
run_interactive y
[ "$STATUS" -eq 0 ] && pass "outdated Cargo is replaced with rustup stable" || fail_test "outdated Cargo is replaced with rustup stable"
assert_contains "outdated Cargo version is explained" "too old or unsupported (1.75.0; need 1.85+)" "$OUTPUT"
assert_contains "outdated Cargo triggers rustup" "rustup-init -y --no-modify-path" "$COMMAND_LOG"

new_case rustup_update
FAKE_CARGO_VERSION=1.75.0; export FAKE_CARGO_VERSION
cp "$CASE_DIR/rustup-template" "$FAKE_BIN/rustup"
run_interactive y
[ "$STATUS" -eq 0 ] && pass "existing rustup updates an outdated toolchain" || fail_test "existing rustup updates an outdated toolchain"
assert_contains "existing rustup installs stable" "rustup toolchain install stable" "$COMMAND_LOG"
assert_contains "existing rustup selects stable" "rustup default stable" "$COMMAND_LOG"

new_case apt
rm "$FAKE_BIN/git"
FAKE_INSTALL_PREREQUISITES=1; export FAKE_INSTALL_PREREQUISITES
run_interactive y
assert_contains "apt manager detected" "  apt-get" "$OUTPUT"
assert_contains "apt update displayed before approval" "sudo apt-get update" "$OUTPUT"
assert_contains "exact apt packages displayed" "sudo apt-get install -y git" "$OUTPUT"
assert_contains "apt package command executed" "sudo apt-get install -y git" "$COMMAND_LOG"

new_case decline
rm "$FAKE_BIN/cargo"
run_interactive n
[ "$STATUS" -ne 0 ] && pass "declining prerequisite installation fails safely" || fail_test "declining prerequisite installation fails safely"
assert_not_exists "decline leaves profile untouched" "$HOME/.bashrc"
[ ! -s "$COMMAND_LOG" ] && pass "decline invokes no package manager or Cargo" || fail_test "decline invokes no package manager or Cargo"

new_case noninteractive
rm "$FAKE_BIN/cargo"
run_noninteractive
[ "$STATUS" -ne 0 ] && pass "noninteractive missing prerequisites fails" || fail_test "noninteractive missing prerequisites fails"
assert_contains "noninteractive requests an interactive terminal" "cannot install or update prerequisites" "$OUTPUT"
[ ! -s "$COMMAND_LOG" ] && pass "noninteractive run invokes no package manager" || fail_test "noninteractive run invokes no package manager"

new_case no_mode
run_without_mode
[ "$STATUS" -ne 0 ] && pass "noninteractive setup requires an explicit mode" || fail_test "noninteractive setup requires an explicit mode"
assert_contains "noninteractive mode guidance lists automation flags" "use --cli, --gui, --all, or --repair" "$OUTPUT"
[ ! -s "$COMMAND_LOG" ] && pass "missing mode mutates nothing" || fail_test "missing mode mutates nothing"

new_case menu_exit
run_menu 5
[ "$STATUS" -eq 0 ] && pass "interactive setup menu can exit successfully" || fail_test "interactive setup menu can exit successfully"
assert_contains "interactive menu offers GUI and repair choices" "4. Repair / verify installation" "$OUTPUT"
assert_not_exists "menu exit does not install Loadbot" "$CARGO_HOME/bin/loadbot"

new_case menu_cli
run_menu '1
y'
[ "$STATUS" -eq 0 ] && pass "interactive CLI-only selection installs CLI" || fail_test "interactive CLI-only selection installs CLI"
assert_contains "interactive menu records CLI mode" "Mode: cli" "$OUTPUT"

new_case gui_only
prepare_gui
run_mode_interactive gui y
[ "$STATUS" -eq 0 ] && pass "GUI-only setup builds and installs the native GUI" || fail_test "GUI-only setup builds and installs the native GUI"
assert_contains "GUI install uses npm ci" "npm --prefix $PROJECT/src/gui ci" "$COMMAND_LOG"
assert_contains "GUI install uses no-bundle build script" "npm --prefix $PROJECT/src/gui run desktop:build" "$COMMAND_LOG"
[ -x "$CARGO_HOME/bin/loadbot-desktop" ] && pass "GUI binary installed beside launcher" || fail_test "GUI binary installed beside launcher"
assert_not_exists "GUI-only setup omits shell completion" "$CARGO_HOME/completions/loadbot.bash"
assert_count "GUI-only PATH block remains idempotent" 1 '# >>> loadbot >>>' "$HOME/.bashrc"

run_mode_interactive repair ''
[ "$STATUS" -eq 0 ] && pass "repair reuses the recorded GUI-only mode" || fail_test "repair reuses the recorded GUI-only mode"
assert_contains "repair announces recorded component set" "Repairing recorded gui installation." "$OUTPUT"
assert_count "repair does not duplicate managed profile block" 1 '# >>> loadbot >>>' "$HOME/.bashrc"

rm "$CARGO_HOME/loadbot-install-mode"
run_mode_interactive repair y
[ "$STATUS" -eq 0 ] && pass "legacy GUI-only installation is adopted" || fail_test "legacy GUI-only installation is adopted"
assert_contains "legacy GUI-only mode is inferred from integration" "Adopt this as a GUI-only installation" "$OUTPUT"
assert_contains "legacy GUI-only adoption creates record" "gui" "$CARGO_HOME/loadbot-install-mode"

new_case legacy_cli_repair
run_interactive y
rm "$CARGO_HOME/loadbot-install-mode"
: >"$COMMAND_LOG"
run_menu '4
y'
[ "$STATUS" -eq 0 ] && pass "legacy CLI installation is adopted interactively" || fail_test "legacy CLI installation is adopted interactively"
assert_contains "legacy CLI state is reported" "CLI executable:       installed" "$OUTPUT"
assert_contains "legacy CLI mode is inferred" "Adopt this as a CLI-only installation" "$OUTPUT"
assert_contains "legacy CLI repair uses adopted mode" "Repairing cli installation." "$OUTPUT"
assert_contains "legacy CLI adoption creates record" "cli" "$CARGO_HOME/loadbot-install-mode"

run_mode_interactive repair ''
[ "$STATUS" -eq 0 ] && pass "repair after legacy adoption is idempotent" || fail_test "repair after legacy adoption is idempotent"
assert_contains "repair after adoption uses record" "Repairing recorded cli installation." "$OUTPUT"

new_case legacy_all_repair
prepare_gui
run_mode_interactive all y
rm "$CARGO_HOME/loadbot-install-mode"
run_mode_interactive repair y
[ "$STATUS" -eq 0 ] && pass "legacy CLI and GUI installation is adopted" || fail_test "legacy CLI and GUI installation is adopted"
assert_contains "legacy GUI executable is reported" "GUI executable:       installed" "$OUTPUT"
assert_contains "legacy complete mode is inferred" "Adopt this as a CLI + GUI installation" "$OUTPUT"
assert_contains "legacy complete adoption creates record" "all" "$CARGO_HOME/loadbot-install-mode"

new_case fresh_repair
run_mode_interactive repair 4
[ "$STATUS" -ne 0 ] && pass "fresh repair requires an explicit component choice" || fail_test "fresh repair requires an explicit component choice"
assert_contains "fresh repair reports no installation" "No existing Loadbot installation was detected." "$OUTPUT"
assert_not_exists "fresh repair cancellation creates no record" "$CARGO_HOME/loadbot-install-mode"

new_case config_only_repair
mkdir -p "$HOME/.local/share/loadbot"
run_mode_interactive repair 4
[ "$STATUS" -ne 0 ] && pass "configuration alone is treated as a fresh installation" || fail_test "configuration alone is treated as a fresh installation"
assert_contains "configuration is reported without becoming install evidence" "Data directory:       present" "$OUTPUT"
assert_contains "configuration-only repair still reports no installation" "No existing Loadbot installation was detected." "$OUTPUT"
assert_not_exists "configuration-only cancellation creates no record" "$CARGO_HOME/loadbot-install-mode"

new_case ambiguous_repair
prepare_gui
install_legacy_binary "$CARGO_HOME/bin/loadbot"
install_legacy_binary "$CARGO_HOME/bin/loadbot-desktop"
run_mode_interactive repair '3
y'
[ "$STATUS" -eq 0 ] && pass "ambiguous legacy state asks for and uses a mode" || fail_test "ambiguous legacy state asks for and uses a mode"
assert_contains "ambiguous legacy state is explained" "cannot be determined safely" "$OUTPUT"
assert_contains "ambiguous repair offers explicit choices" "Which installation should Loadbot repair?" "$OUTPUT"
assert_contains "ambiguous selection creates chosen record" "all" "$CARGO_HOME/loadbot-install-mode"

new_case ambiguous_cancel
install_legacy_binary "$CARGO_HOME/bin/loadbot"
install_legacy_binary "$CARGO_HOME/bin/loadbot-desktop"
before_cli=$(cksum "$CARGO_HOME/bin/loadbot")
before_gui=$(cksum "$CARGO_HOME/bin/loadbot-desktop")
run_mode_interactive repair 4
[ "$STATUS" -ne 0 ] && pass "ambiguous legacy adoption can be cancelled" || fail_test "ambiguous legacy adoption can be cancelled"
assert_not_exists "ambiguous cancellation creates no record" "$CARGO_HOME/loadbot-install-mode"
[ "$(cksum "$CARGO_HOME/bin/loadbot")" = "$before_cli" ] && [ "$(cksum "$CARGO_HOME/bin/loadbot-desktop")" = "$before_gui" ] && pass "ambiguous cancellation leaves binaries untouched" || fail_test "ambiguous cancellation leaves binaries untouched"

new_case legacy_noninteractive
run_interactive y
rm "$CARGO_HOME/loadbot-install-mode"
set +e
/bin/sh "$PROJECT/setup.sh" --repair >"$OUTPUT" 2>&1
STATUS=$?
set -e
[ "$STATUS" -eq 0 ] && pass "noninteractive unambiguous legacy repair succeeds" || fail_test "noninteractive unambiguous legacy repair succeeds"
assert_contains "noninteractive legacy mode is adopted" "Adopting unambiguous legacy mode: cli." "$OUTPUT"
assert_contains "noninteractive adoption creates record" "cli" "$CARGO_HOME/loadbot-install-mode"

new_case ambiguous_noninteractive
install_legacy_binary "$CARGO_HOME/bin/loadbot"
install_legacy_binary "$CARGO_HOME/bin/loadbot-desktop"
set +e
/bin/sh "$PROJECT/setup.sh" --repair >"$OUTPUT" 2>&1
STATUS=$?
set -e
[ "$STATUS" -ne 0 ] && pass "noninteractive ambiguous repair fails without prompting" || fail_test "noninteractive ambiguous repair fails without prompting"
assert_contains "noninteractive ambiguity has explicit guidance" "use --cli, --gui, or --all" "$OUTPUT"
assert_not_exists "noninteractive ambiguity creates no record" "$CARGO_HOME/loadbot-install-mode"

new_case fresh_noninteractive_repair
set +e
/bin/sh "$PROJECT/setup.sh" --repair >"$OUTPUT" 2>&1
STATUS=$?
set -e
[ "$STATUS" -ne 0 ] && pass "noninteractive fresh repair fails without prompting" || fail_test "noninteractive fresh repair fails without prompting"
assert_contains "noninteractive fresh repair has setup guidance" "no existing Loadbot installation was detected" "$OUTPUT"
assert_not_exists "noninteractive fresh repair creates no record" "$CARGO_HOME/loadbot-install-mode"

new_case cli_only_scope
run_interactive y
if ! grep -F 'npm ' "$COMMAND_LOG" >/dev/null; then pass "CLI-only setup never invokes npm"; else fail_test "CLI-only setup never invokes npm"; fi
assert_not_exists "CLI-only setup does not install GUI binary" "$CARGO_HOME/bin/loadbot-desktop"

new_case repair_bad_config
mkdir -p "$CARGO_HOME"
printf 'cli\n' >"$CARGO_HOME/loadbot-install-mode"
LOADBOT_HOME="$CASE_DIR/not a directory"
printf 'unsafe\n' >"$LOADBOT_HOME"
export LOADBOT_HOME
set +e
/bin/sh "$PROJECT/setup.sh" --repair >"$OUTPUT" 2>&1
STATUS=$?
set -e
[ "$STATUS" -ne 0 ] && pass "repair rejects an invalid configuration path" || fail_test "repair rejects an invalid configuration path"
assert_contains "repair reports the invalid configuration path" "configuration path is not a directory" "$OUTPUT"
unset LOADBOT_HOME

new_case gui_apt
prepare_gui
rm "$FAKE_BIN/pkg-config"
FAKE_INSTALL_PREREQUISITES=1; export FAKE_INSTALL_PREREQUISITES
run_mode_interactive gui y
assert_contains "Debian GUI plan names WebKitGTK 4.1 development package" "libwebkit2gtk-4.1-dev" "$OUTPUT"
assert_contains "Debian GUI dependency installation is explicit" "sudo apt-get install -y" "$COMMAND_LOG"

new_case all_mode
prepare_gui
run_mode_interactive all y
[ "$STATUS" -eq 0 ] && pass "CLI + GUI setup installs both component sets" || fail_test "CLI + GUI setup installs both component sets"
[ -x "$CARGO_HOME/bin/loadbot-desktop" ] && [ -f "$CARGO_HOME/completions/loadbot.bash" ] && pass "CLI + GUI includes desktop and completion" || fail_test "CLI + GUI includes desktop and completion"

new_case gui_pacman
rm "$FAKE_BIN/apt-get"
make_manager pacman
prepare_gui
rm "$FAKE_BIN/pkg-config"
FAKE_INSTALL_PREREQUISITES=1; export FAKE_INSTALL_PREREQUISITES
run_mode_interactive gui y
assert_contains "Arch GUI plan names WebKitGTK 4.1 package" "webkit2gtk-4.1" "$OUTPUT"
assert_contains "Arch GUI dependency installation uses --needed" "sudo pacman -S --needed" "$COMMAND_LOG"

new_case package_failure
rm "$FAKE_BIN/git"
FAKE_SUDO_FAIL=1; export FAKE_SUDO_FAIL
run_interactive y
[ "$STATUS" -ne 0 ] && pass "package-manager failure stops setup" || fail_test "package-manager failure stops setup"
assert_not_exists "package failure stops profile configuration" "$HOME/.bashrc"

new_case recheck
rm "$FAKE_BIN/git"
run_interactive y
[ "$STATUS" -ne 0 ] && pass "prerequisites are rechecked after installation" || fail_test "prerequisites are rechecked after installation"
assert_contains "recheck reports remaining command" "remain missing: git" "$OUTPUT"

new_case cargo_failure
FAKE_CARGO_FAIL=1; export FAKE_CARGO_FAIL
run_interactive y
[ "$STATUS" -ne 0 ] && pass "Cargo failure stops setup" || fail_test "Cargo failure stops setup"
assert_not_exists "Cargo failure stops profile configuration" "$HOME/.bashrc"

# Pacman and unsupported manager behavior.
new_case pacman
rm "$FAKE_BIN/apt-get" "$FAKE_BIN/git"
make_manager pacman
FAKE_INSTALL_PREREQUISITES=1; export FAKE_INSTALL_PREREQUISITES
run_interactive y
assert_contains "pacman manager detected" "  pacman" "$OUTPUT"
assert_contains "exact pacman command displayed" "sudo pacman -S --needed git" "$OUTPUT"
assert_contains "exact pacman command executed" "sudo pacman -S --needed git" "$COMMAND_LOG"

new_case unsupported
rm "$FAKE_BIN/apt-get" "$FAKE_BIN/cargo" "$FAKE_BIN/curl"
run_noninteractive
[ "$STATUS" -ne 0 ] && pass "unsupported package manager fails safely" || fail_test "unsupported package manager fails safely"
assert_contains "unsupported manager gives manual guidance" "Install the missing commands manually" "$OUTPUT"
[ ! -s "$COMMAND_LOG" ] && pass "unsupported manager mutates nothing" || fail_test "unsupported manager mutates nothing"

# Zsh and Fish select only their own safe profiles.
new_case zsh
SHELL=/usr/bin/zsh; export SHELL
run_interactive y
assert_contains "Zsh profile configured" 'loadbot.zsh' "$HOME/.zshrc"
assert_not_exists "Zsh does not create Bash profile" "$HOME/.bashrc"

new_case fish
SHELL=/usr/bin/fish; export SHELL
run_interactive y
assert_contains "Fish uses fish_add_path" 'fish_add_path "$HOME/.cargo/bin"' "$HOME/.config/fish/config.fish"
assert_contains "Fish completion uses valid source syntax" 'and source "$HOME/.cargo/completions/loadbot.fish"' "$HOME/.config/fish/config.fish"

# Unsafe and malformed profiles are rejected before any command runs.
new_case symlink
printf '%s\n' untouched >"$HOME/target"
ln -s "$HOME/target" "$HOME/.bashrc"
run_noninteractive
[ "$STATUS" -ne 0 ] && pass "symlink profile refused" || fail_test "symlink profile refused"
assert_contains "symlink refusal reported" "refusing to modify symlink profile" "$OUTPUT"
[ "$(cat "$HOME/target")" = untouched ] && pass "symlink target unchanged" || fail_test "symlink target unchanged"

new_case malformed
printf '%s\n' '# >>> loadbot >>>' >"$HOME/.bashrc"
run_noninteractive
[ "$STATUS" -ne 0 ] && pass "malformed marker refused" || fail_test "malformed marker refused"
assert_contains "malformed marker reported" "malformed or duplicate" "$OUTPUT"

new_case duplicate
cat >"$HOME/.bashrc" <<'EOF'
# >>> loadbot >>>
# <<< loadbot <<<
# >>> loadbot >>>
# <<< loadbot <<<
EOF
run_noninteractive
[ "$STATUS" -ne 0 ] && pass "duplicate markers refused" || fail_test "duplicate markers refused"

printf '%s\n' "1..$((PASS + FAIL))"
[ "$FAIL" -eq 0 ] || exit 1
