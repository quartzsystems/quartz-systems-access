# Testing

## Automated (CI, both platforms)

`cargo test --workspace` runs:

- unit tests in every crate;
- `tn5250/tests/replay.rs`: every recording in `fixtures/replay/` replays to its
  golden screen (see `fixtures/README.md`);
- `qsa-ui/tests/console_pixels.rs`: the 5250 grid at 1× matches the QS/400
  service console pixel for pixel (see `fixtures/console/README.md`);
- `qsaccess/tests/screenshots.rs`: every window and dialog against its golden
  PNG for the platform (`qsaccess/tests/goldens/{windows,linux,linux-wayland}`);
- `qap-client/tests/`: the QAP client against QS/400's own connection engine
  (`qapd`, pinned with the protocol crates), in process over loopback TLS 1.3,
  with QS/400's own test system (`qapd::testing`: profiles, sessions, Telnet
  tickets, test commands, a clock) behind a listener that takes any number of
  connections (`tests/common/mod.rs`): every connection, sign-on and session
  path of `docs/qap-protocol-spec.md`; and, in `tests/version1.rs`, the
  version-1 engine (`qapd_v1`, QS/400 at 58a7e6d3) with a small host of this
  crate's own, for the fallback: version 1 agreed, version-2 operations
  refused before they are sent;
- `qsaccess/tests/verify.rs`: Verify Connection's checks against the same
  server.
- `qsaccess/tests/sessions.rs`: one QAP session per system per process
  against the same server: every tool gets the same client, a running
  command and another request share its connection, and the GUI's look at
  the signed-on profile never waits on a client in use.

CI also runs `cargo fmt --check`, `cargo clippy -D warnings`, the clean-room
check (`ci/cleanroom.sh`), the copyright header check and `ci/deps.sh`
(`qsa-net`, `qap-client`, `tn5250` and `scs` must not depend on FLTK).

The clean-room check allows an explicit list of files and no others: each
protocol crate's `wire.rs` (today only `tn5250/src/wire.rs`).
The protocols' vendor-prefixed strings (5250 terminal types and environment
variable names) live there and nowhere else.
Other code uses their constants, and logs them only at debug level. A new
protocol crate adds its `wire.rs` to the list in `ci/cleanroom.sh`.

## Screenshot goldens

Goldens are made by CI's own runners, so they match what CI compares against:
Windows on `windows-2022`, Linux in `ubuntu:24.04` pinned by digest in
`.github/workflows/ci.yml`, under Xvfb (X11) and Weston (Wayland), with every
system font removed so only the bundled fonts exist.

Two crates have them: `qsaccess/tests/goldens/` (main window, System
Configurations, system and Session Properties, Sign On, Change Password, a
system's message, Verify Connection, Start, sessions, Keyboard Map
with the defaults and with `tests/data/operations.qkm` loaded, Load Keyboard
Map, certificate, Session Manager, Macro editor and step, Run Macro, a long
message) and `qsa-install/tests/goldens/` (every wizard page, Uninstall,
Repair). Both use `qsa_ui::goldens`. Locally, the Linux goldens can be made in
`ci/linux-ci.Dockerfile`, the same image as the CI job.

To regenerate after an intended change, from a clone with the commits to
bless (the bless job only writes goldens; it never commits or pushes):

1. Push the commits to a branch: `git push origin HEAD:refs/heads/<branch>`.
2. Run the bless workflow on it and wait for both jobs:
   `gh workflow run ci.yml --ref <branch> -f bless=true`, then
   `gh run list --workflow ci.yml --branch <branch> --limit 1` for its ID and
   `gh run watch <run-id>`. The screenshot steps pass in a bless run: they
   write what they capture.
3. Download the goldens into the checkout, from its top folder. Each artifact
   keeps the paths from the repository root:
   `gh run download <run-id> -n goldens-windows -D .` and
   `gh run download <run-id> -n goldens-linux -D .`
   (`goldens-linux` holds X11, Wayland and the PDF goldens).
4. Review: `git status --short -- '*/tests/goldens/'`, then look at every new
   and changed PNG, against the `screenshot-report` artifact (each view side
   by side across platforms) and the list below. Anything changed that is not
   on the list is a regression to fix, not to bless.
5. Commit the goldens, take the blessed views off the list below in the same
   commit, and push.

Locally, `QSA_BLESS=1 cargo test -p qsaccess --test screenshots` writes the
goldens for the machine you are on; use it to look, not to commit, unless the
machine matches the runner.

The printer views show job times in the local time zone; the runners are on
UTC, so a machine elsewhere shows those times differently.

### Waiting for a bless run

Views whose goldens are stale or missing until CI's bless run writes new ones,
on Windows, X11 and Wayland alike; take each off this list when it is blessed.

| View | Changed by |
|---|---|
| `main` | The task pane's groups and tasks as they now are, and the status bar's connection field for the selected system. |
| `system-configurations` | The Verify... button. |
| `properties-general` | The check box's label position (the check box control changed). |
| `properties-connection` | Passwords are never saved: one Default user ID field and a note in place of the three sign-on choices. |
| `properties-ports` | Systems file version 3: the Telnet and QAP ports replace the earlier ports and Port lookup. |
| `start-5250`, `macro-step`, `session-properties-general`, `session-properties-hotspots` | The check box's label position (the check box control changed), and the session properties' own changes since. |
| `message-long-path`, `session-manager` | Changed since the last bless; check against the screenshot report. |
| `start-5250-printer`, `printer-session`, `printer-settings` | New with the printer session. No golden yet. |
| `sign-on`, `change-password`, `message-system` | New: the QAP Sign On and Change Password dialogs, and a system's message shown as sent. No golden yet. |
| `verify-connection` | New: Verify Connection. No golden yet. |
| `run-command`, `run-command-results` | New: Run Command, empty and after runs and a check. No golden yet. |
| `prompt-simple`, `prompt-qualifiers`, `prompt-list`, `prompt-additional`, `prompt-elements`, `prompt-checked`, `prompt-help` | New: Prompt for QS/400's test commands (`qapd::testing`): CRTLIB; DSPOBJD with a qualified name, then a list with "+"; CRTPF with its additional parameters shown, then its element list changed; DSPOBJD after a failed check; a qualifier's help. No golden yet. |
| `qsa-install`: `wizard-0-welcome`, `wizard-1-licence`, `wizard-2-install-for`, `wizard-3-location`, `wizard-4-options`, `wizard-5-ready`, `wizard-7-finish`, `uninstall`, `repair` | The licence text, the check box control, and no saved passwords in the Welcome, Uninstall and Repair text. |

## PDF goldens

`qsa-print/tests/pdf_goldens.rs` reads SCS fixtures (the built-in test page,
the RFC 4777 example print record, a three-page 132-column listing, an A4 job
with overstrike and underline) through the page model to PDF, and checks:

- the text rendering against `qsa-print/tests/goldens/<name>.txt`, everywhere;
- that every embedded face is a subset of Courier Prime;
- every page, rasterised by `pdftoppm -r 50 -gray`, against
  `<name>-<page>.png`, at most 16 pixels off.

The page images need poppler 24.02.0 (`poppler-utils=24.02.0-1ubuntu9.11`,
pinned in the Linux CI job and `ci/linux-ci.Dockerfile`; a test tool, never
shipped). CI sets `QSA_REQUIRE_PDFTOPPM=1` there; elsewhere the images are
skipped when `pdftoppm` is missing, and `QSA_PDFTOPPM` can name one. Bless with
`QSA_BLESS=1` in the CI image only, and look at the PNGs before committing.

`qsa-print/tests/gdi_print_to_pdf.rs` prints the test page through the Windows
spooler to "Microsoft Print to PDF" (skipped when that printer is missing) and
leaves the PDF in `target/gdi-print/`.

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

For QAP, on QS/400 with `STRTCPSVR SERVER(*QAP)` and a certificate assigned
to `QS400_ACCESS_SERVER`:

- System Configurations > Verify... with no user ID: every check but the test
  sign-on OK (trust the certificate when asked);
- Actions > Sign On...; a wrong password shows the system's CPF message;
- a profile with an expired password: Change Password, then signed on;
- Actions > Change Password... while signed on;
- `ENDJOB` of the `QAPCNN` job: the session-ended message, then Sign On...;
- `ENDTCPSVR SERVER(*QAP)`: the server-ending message; Reconnect after
  `STRTCPSVR`;
- Actions > Sign Off.

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
   `.qsd`/`.qsp`/`.qkm`/`.qsm` ProgIDs and open commands, Start menu and desktop
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
