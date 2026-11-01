#!/usr/bin/env bash
# Runs the screenshot tests under Wayland: Weston on its X11 backend inside
# Xvfb, then the tests as a Wayland client of it. Extra arguments go to
# `cargo test` (for example `--target x86_64-unknown-linux-gnu`).
set -euo pipefail
cd "$(dirname "$0")/.."
export XDG_RUNTIME_DIR="${XDG_RUNTIME_DIR:-$(mktemp -d)}"
chmod 0700 "$XDG_RUNTIME_DIR"
inner="$(mktemp)"
trap 'rm -f "$inner"' EXIT
cat > "$inner" <<INNER
set -u
weston --backend=x11 --socket=wl-qsa --idle-time=0 --width=1280 --height=1024 &
w=\$!
for _ in \$(seq 100); do [ -S "\$XDG_RUNTIME_DIR/wl-qsa" ] && break; sleep 0.1; done
status=0
env -u DISPLAY WAYLAND_DISPLAY=wl-qsa FLTK_BACKEND=wayland QSA_SCREENSHOT_TAG=wayland \
  cargo test -p qsaccess --test screenshots $* || status=\$?
kill \$w || true
exit \$status
INNER
xvfb-run -a -s "-screen 0 1600x1200x24 -dpi 96" bash "$inner"
