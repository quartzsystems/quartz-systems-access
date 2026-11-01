# Testing

## Automated (CI, both platforms)

`cargo test --workspace` runs:

- unit tests in every crate;
- `tn5250/tests/replay.rs`: every recording in `fixtures/replay/` replays to its
  golden screen (see `fixtures/README.md`);
- `qsa-ui/tests/console_pixels.rs`: the 5250 grid at 1× matches the QS/400
  service console pixel for pixel (see `fixtures/console/README.md`);
- `qsaccess/tests/screenshots.rs`: every window and dialog against its golden
  PNG for the platform (`qsaccess/tests/goldens/{windows,linux,linux-wayland}`).

CI also runs `cargo fmt --check`, `cargo clippy -D warnings`, the clean-room
check (`ci/cleanroom.sh`), the copyright header check and `ci/deps.sh`
(`tn5250` must not depend on FLTK).

## Screenshot goldens

Goldens are made by CI's own runners, so they match what CI compares against:
Windows on `windows-2022`, Linux in `ubuntu:24.04` pinned by digest in
`.github/workflows/ci.yml`, under Xvfb (X11) and Weston (Wayland), with every
system font removed so only the bundled fonts exist.

To regenerate after an intended change:

```
gh workflow run ci.yml --ref <branch> -f bless=true
gh run watch
gh run download <run-id> -n goldens-windows -D qsaccess/tests/goldens/windows
gh run download <run-id> -n goldens-linux -D qsaccess/tests/goldens
git diff --stat qsaccess/tests/goldens
```

Look at every changed PNG before committing. The Actions summary also has a
`screenshot-report` artifact with each view side by side across platforms.

Locally, `QSA_BLESS=1 cargo test -p qsaccess --test screenshots` writes the
goldens for the machine you are on; use it to look, not to commit, unless the
machine matches the runner.

## Wayland, by hand

CI runs the Wayland screenshots under Weston's X11 backend. A real compositor
is checked by hand before a release, on a GNOME or KDE Wayland session (not
XWayland: `echo $XDG_SESSION_TYPE` must print `wayland`):

1. `FLTK_BACKEND=wayland qsaccess`.
2. Open 5250 Display to a system. The grid is sharp at 1× and at every larger
   scale the window is resized to; no blur, no stretched glyphs.
3. With the desktop at 150 % or 200 % scaling, the grid is still sharp (the
   scale is chosen in device pixels).
4. Right Ctrl alone does Field Exit; Right Ctrl+C still copies; left Ctrl
   alone does nothing.
5. Copy a block with the mouse, paste it into another application; paste text
   from another application into an input field.
6. Print Screen writes a PDF; open it and zoom in: glyph edges stay square.
7. Close the window while connected: no message, the process ends when the
   last window closes.

## Live acceptance (Phase 1)

For each system (QS/400 at 10.42.70.150, the reference system, pub400.com by
hand only), at 24×80 and 27×132, plain and with TLS where the system offers
it:

- sign on at the system's sign-on display;
- move through menus;
- on a command line, type a command and press F4 to prompt it;
- `WRKACTJOB`, Page Down and Page Up;
- Help (F1) on a field;
- System Request (Shift+Esc) and Attention (Esc);
- sign off.

Record what failed, with `qsaccess.log` (negotiation is logged at debug
level; `QSACCESS_LOG=trace` adds every record in hex).

## Keyboard: Right Ctrl

Right Ctrl is told apart from left Ctrl by FLTK's key codes, which FLTK
reports on Windows (the extended-key flag), X11 (`Control_R`) and Wayland (the
xkb keysym `Control_R`). It acts on release, and only when no other key went
down while it was held. Check it by hand on each platform (step 4 above).
