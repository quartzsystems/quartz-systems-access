#!/usr/bin/env bash
# Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
# licensed under the terms of the MIT license which can be found in the
# root directory of this project.
#
# Installer test for Linux: install, open a session file as a file manager does,
# upgrade, repair and uninstall, silently, then check that nothing is left
# that was not there before (apart from user data). See TESTING.md.
#
#   ci/installer-test-linux.sh INSTALLER user|machine
#
# "machine" must run as root. Run it as the user being installed for; a
# display is needed for the xdg-open step (xvfb-run is used if there is
# none). Exits 0 when every check passes.
set -uo pipefail

installer="$(readlink -f "$1")"
scope="${2:-user}"
failures=0
check() {
    if eval "$1"; then echo "ok    $2"; else echo "FAIL  $2"; failures=$((failures + 1)); fi
}

if [ "$scope" = machine ]; then
    dir=/opt/quartz-access
    bin=/usr/local/bin
    share=/usr/local/share
    watch=(/opt /usr/local /etc)
else
    dir="$HOME/.local/opt/quartz-access"
    bin="$HOME/.local/bin"
    share="${XDG_DATA_HOME:-$HOME/.local/share}"
    watch=("$HOME/.local" "$HOME/.config" "$HOME/Desktop")
fi
config="${XDG_CONFIG_HOME:-$HOME/.config}"
settings="$config/quartz-access"

# Everything under the folders an install can touch, except user data: the
# program's settings, and FLTK's own preferences, which running the program
# writes. The config folder itself holds user data, so it is not listed.
snapshot() {
    for w in "${watch[@]}"; do
        [ -e "$w" ] && find "$w" -xdev \( -path "$settings" -o -path "$config/fltk.org" \) -prune \
            -o ! -path "$config" -printf '%p %y\n'
    done | sort
}

run() { "$@" >/dev/null 2>&1; echo $?; }

if pgrep -x qsaccess >/dev/null; then
    echo "QS/400 Access for Desktop is running; close it and run the test again"
    exit 2
fi
before="$(mktemp)"; snapshot >"$before"

# A damaged installer is refused before anything changes.
damaged="$(mktemp)"
cp "$installer" "$damaged"
size=$(stat -c %s "$damaged")
printf '\xff' | dd of="$damaged" bs=1 seek=$((size - 1000)) conv=notrunc status=none
chmod +x "$damaged"
check '[ "$(run "$damaged" --silent --scope=$scope)" = 2 ]' "a damaged installer exits 2"
rm -f "$damaged"
check 'diff -q "$before" <(snapshot) >/dev/null' "a damaged installer changes nothing"

# Install.
check '[ "$(run "$installer" --silent --scope=$scope)" = 0 ]' "silent install exits 0"
check '[ -x "$dir/qsaccess" ]' "qsaccess installed"
check '[ -x "$dir/quartz-access-uninstall" ]' "uninstaller installed"
check '[ -f "$dir/install-manifest.toml" ]' "install manifest written"
check '[ "$(readlink "$bin/qsaccess")" = "$dir/qsaccess" ]' "link $bin/qsaccess"
check '[ -L "$bin/quartz-access-uninstall" ]' "link $bin/quartz-access-uninstall"
check 'grep -q "^Exec=\"$dir/qsaccess\" open %f$" "$share/applications/quartz-access.desktop"' "desktop entry"
check 'grep -q "^Exec=\"$dir/qsaccess\" --session-manager$" "$share/applications/quartz-access-sessions.desktop"' "Session Manager desktop entry"
check 'grep -q "application/x-qsaccess-session" "$share/mime/packages/quartz-access.xml"' "MIME package"
check '[ -f "$share/icons/hicolor/256x256/apps/quartz-access.png" ]' "application icon"
check '[ -f "$share/icons/hicolor/48x48/mimetypes/application-x-qsaccess-session.png" ]' "file-type icon"
check 'grep -q "application/x-qsaccess-printer" "$share/mime/packages/quartz-access.xml"' "MIME package has printer sessions"
check '[ -f "$share/icons/hicolor/48x48/mimetypes/application-x-qsaccess-printer.png" ]' "printer session icon"
check 'grep -q "application/x-qsaccess-printer" "$share/applications/quartz-access.desktop"' "desktop entry opens printer sessions"
if command -v update-mime-database >/dev/null; then
    check 'grep -q "application/x-qsaccess-session" "$share/mime/globs2"' "MIME database knows *.qsd"
    check 'grep -q "application/x-qsaccess-printer:\*\.qsp" "$share/mime/globs2"' "MIME database knows *.qsp"
fi

# Open a session file the way a file manager does.
qsd="$(mktemp --suffix=.qsd)"
cat >"$qsd" <<QSD
[qsaccess]
kind = "display"
version = 1

[system]
name = "10.42.70.150"
host = "10.42.70.150"

[session]
title = "Installer test"
auto_connect = false
QSD
# GIO is what file managers use to open a file that is double-clicked.
if [ "$scope" = user ] && command -v gio >/dev/null; then
    check 'gio info -a standard::content-type "$qsd" | grep -q application/x-qsaccess-session' "a .qsd is application/x-qsaccess-session"
    check '[ "$(xdg-mime query default application/x-qsaccess-session)" = quartz-access.desktop ]' "QS/400 Access for Desktop opens .qsd"
    mkdir -p "$settings"
    made_settings=0
    if [ ! -f "$settings/systems.toml" ]; then
        printf '[[system]]\nname = "10.42.70.150"\ndefault_user = "QATEST"\n' >"$settings/systems.toml"
        made_settings=1
    fi
    display=()
    [ -z "${DISPLAY:-}${WAYLAND_DISPLAY:-}" ] && display=(xvfb-run -a)
    "${display[@]}" bash -c "gio open '$qsd' & sleep 6; pgrep -af 'qsaccess open' >/tmp/qsa-open.$$; '$dir/qsaccess' --quit; sleep 2" >/dev/null 2>&1
    check 'grep -q "qsaccess.* open .*\.qsd" /tmp/qsa-open.$$' "opening a .qsd with gio open runs qsaccess open"
    qsp="$(mktemp --suffix=.qsp)"
    cat >"$qsp" <<QSP
[qsaccess]
kind = "printer"
version = 1

[system]
name = "10.42.70.150"
host = "10.42.70.150"

[printer]
device_name = "QSATSTPRT"

[session]
auto_connect = false
QSP
    check 'gio info -a standard::content-type "$qsp" | grep -q application/x-qsaccess-printer' "a .qsp is application/x-qsaccess-printer"
    "${display[@]}" bash -c "gio open '$qsp' & sleep 6; pgrep -af 'qsaccess open' >/tmp/qsa-open.$$; '$dir/qsaccess' --quit; sleep 2" >/dev/null 2>&1
    check 'grep -q "qsaccess.* open .*\.qsp" /tmp/qsa-open.$$' "opening a .qsp with gio open runs qsaccess open"
    rm -f "$qsp"
    check '! pgrep -x qsaccess >/dev/null' "qsaccess --quit closes it"
    rm -f /tmp/qsa-open.$$
    [ "$made_settings" = 1 ] && rm -rf "$settings"
fi
rm -f "$qsd"

# Upgrade over the top, then repair.
check '[ "$(run "$installer" --silent --scope=$scope)" = 0 ]' "upgrade over the top exits 0"
check '[ "$(run "$bin/quartz-access-uninstall" --repair --silent)" = 0 ]' "repair exits 0"
check '[ -x "$dir/qsaccess" ] && [ -x "$dir/quartz-access-uninstall" ]' "repair keeps the program and the uninstaller"

# Uninstall.
check '[ "$(run "$bin/quartz-access-uninstall" --uninstall --silent)" = 0 ]' "silent uninstall exits 0"
if ! diff "$before" <(snapshot) >/tmp/qsa-diff.$$; then
    sed 's/^/      /' /tmp/qsa-diff.$$
fi
check '[ ! -s /tmp/qsa-diff.$$ ]' "nothing left behind after uninstall"
rm -f /tmp/qsa-diff.$$ "$before"

if [ "$failures" -gt 0 ]; then
    echo "$failures check(s) failed"
    exit 1
fi
echo "every check passed"
