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
(`tn5250` and `scs` must not depend on FLTK).

The clean-room check allows exactly one file, `tn5250/src/wire.rs`: the
protocol's vendor-prefixed terminal types and environment variable names live
there and nowhere else. Other code uses its constants, and logs them only at
debug level.

## Screenshot goldens

Goldens are made by CI's own runners, so they match what CI compares against:
Windows on `windows-2022`, Linux in `ubuntu:24.04` pinned by digest in
`.github/workflows/ci.yml`, under Xvfb (X11) and Weston (Wayland), with every
system font removed so only the bundled fonts exist.

Two crates have them: `qsaccess/tests/goldens/` (main window, System
Configurations, system and Session Properties, Start, sessions, Keyboard Map
with the defaults and with `tests/data/operations.qkm` loaded, Load Keyboard
Map, certificate, Session Manager, Macro editor and step, Run Macro, a long
message) and `qsa-install/tests/goldens/` (every wizard page, Uninstall,
Repair). Both use `qsa_ui::goldens`. Locally, the Linux goldens can be made in
`ci/linux-ci.Dockerfile`, the same image as the CI job.

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

## Installer: Windows

`ci/installer-test-windows.ps1` runs the whole cycle silently and checks every
step. In a fresh Windows 11 VM, copy the installer and the script in, then:

```
powershell -ExecutionPolicy Bypass -File installer-test-windows.ps1 -Installer QS400-Access-V1R0M0-windows-x86_64.exe -Scope user
powershell -ExecutionPolicy Bypass -File installer-test-windows.ps1 -Installer QS400-Access-V1R0M0-windows-x86_64.exe -Scope machine   # as administrator
```

It snapshots the registry keys and folders an install can touch, then:

1. a damaged copy of the installer exits 2 and changes nothing;
2. silent install exits 0: program, uninstaller, manifest, version information,
   `.qsd`/`.qkm`/`.qsm` ProgIDs and open commands, Start menu and desktop
   shortcuts, the Apps entry;
3. (just me) a `.qsd` opened through the shell starts `qsaccess open <file>`,
   and `qsaccess --quit` closes it;
4. installing again over the top (upgrade) exits 0;
5. `uninstall.exe --repair --silent` exits 0 and keeps the program;
6. `uninstall.exe --uninstall --silent` exits 0, and the snapshot taken after
   it equals the one taken before (only user data may differ).

It refuses to start while QS/400 Access for Desktop is running, and puts the
user's `settings.toml` back afterwards. By hand, also run the wizard once for
each scope: All users must bring up the Windows consent prompt and nothing
else; Finish with **Start QS/400 Access for Desktop** ticked must open the main
window in the middle of the main monitor; Settings > Apps must list the
program with Uninstall and Modify (repair); uninstalling from there with
**Also remove my settings** ticked must remove `%APPDATA%\Quartz Systems\Access`
and leave `Documents\Quartz Access\Sessions` alone.

## Installer: Linux

Build the installer in the pinned glibc 2.31 image, then run the scripted
test in fresh Ubuntu 24.04 and AlmaLinux 9 containers (both pinned by
digest), as root for All users and as an ordinary user for Just me:

```
ci/dist-linux.sh
ci/installer-test-linux-containers.sh target/dist/QS400-Access-V1R0M0-linux-x86_64
```

Each container gets only the libraries the program loads (see README.md), the
desktop tools the installer runs (shared-mime-info, desktop-file-utils,
xdg-utils), GIO and Xvfb. `ci/installer-test-linux.sh` snapshots
`~/.local`, `~/.config` and `~/Desktop` (or `/opt`, `/usr/local` and `/etc`
for root), then: a damaged installer exits 2 and changes nothing; silent
install exits 0, with links, both desktop entries, the MIME package, icons and
the MIME database entry; (Just me) `gio` sees a `.qsd` as
`application/x-qsaccess-session` with QS/400 Access for Desktop as its default,
`gio open` on it (what a file manager does on a double-click) starts
`qsaccess open <file>`, and `qsaccess --quit` closes it; upgrade over the top,
repair and uninstall exit 0; and the snapshot after uninstall equals the one
before. User data is not compared: the program's settings, and
`~/.config/fltk.org`, where the toolkit keeps its preferences once the program
has run.

The same script runs in full VMs. In Ubuntu 24.04 and AlmaLinux 9 desktop VMs
also check by hand: double-clicking a `.qsd` in Files opens a connected session
to QS/400 at 10.42.70.150, a second double-click opens a second window in the
same process, and the desktop shortcut (GNOME: marked trusted through `gio`)
starts the program.
