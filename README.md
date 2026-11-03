# QS/400 Access for Desktop

QS/400 Access for Desktop is the native desktop client for QS/400, the operating system from Quartz Systems, on Windows and Linux. It is written in Rust on FLTK, with plain Windows-style controls on every platform (see Look below). It provides 5250 display and printer sessions, session files, System Configurations and QAP sign-on; Run Command, the password expiry warning and 5250 automatic sign-on by ticket (QAP version 2); and Printer Output, spooled files on the system with a viewer (QAP version 3). The phases to come each follow a QAP version that QS/400 ships first: 7 (version 4) Run SQL, Schemas and Data Transfer; 8 (version 5) the Integrated File System; 9 (version 6) Messages; 10 the 5250 Console and Control Panel (the service console, not QAP). `docs/connecting.md` keeps the list. Build with `cargo build --release`; the binary is `qsaccess`, and `qsaccess --ui-sampler` opens a window that shows every widget once.

## Look

- **Controls.** Plain Windows-style desktop controls, drawn by the program
  from a fixed palette on every platform; the desktop theme is never read.
- **UI text.** On Windows, the system's own Segoe UI. Elsewhere, the bundled
  Selawik.
- **System output** (certificate fingerprints and PEM text, Run Command's
  command line): the bundled Cascadia Mono, everywhere.
- **The 5250 grid** of display sessions: the QS/400 service console's font and
  palette, everywhere.

Because the UI text face differs, and each platform rasterises text its own
way, windows do not look pixel-identical across platforms. Screenshot goldens
are kept per platform (`qsaccess/tests/goldens/{windows,linux,linux-wayland}`);
only the 5250 grid is checked against the service console pixel for pixel.

## 5250 display

Select **5250 Display** in the task pane, choose the screen size, code page,
device name and colour map, and Start. The display draws exactly like the QS/400
service console: its 9×16 bitmap font and its palette, scaled by whole numbers
only. Sessions can be recorded for the replay tests with `qsaccess --record
FILE` (or `QSACCESS_RECORD=FILE`); see `fixtures/README.md`.

The `tn5250` crate holds the protocol (Telnet, TN5250E, the 5250 data stream,
the keyboard) and has no UI dependency.

## Connecting to QS/400

5250 display and printer sessions use the system's Telnet port (23, or 992
with TLS). Everything else (sign-on, password changes, and the functions to
come) uses QAP, the Quartz Access Protocol (`docs/qap-protocol-spec.md`), on
one port: **2320** unless changed on the Ports tab. QAP runs over **TLS 1.3
only**. The system's QAP server presents the certificate assigned to
application ID **`QS400_ACCESS_SERVER`** in its certificate manager and does
not start until one is assigned; start it with `STRTCPSVR SERVER(*QAP)`. A
certificate this computer does not trust goes through the same Certificate Not
Trusted dialog as TLS 5250 sessions.

**Actions > Sign On...**, **Change Password...** and **Sign Off** work on the
system selected in the task pane. The system's own messages are shown as it
sent them. Passwords and session tokens are never saved or logged: a system
configuration keeps only a **Default user ID** for the Sign On dialog. The
program signs on to a system once per run, when a task first needs it, and
every task uses that session. A session survives a dropped connection and is
resumed quietly; the program asks for a sign-on again only when the session
has ended or expired, or an operator ended it, and when the QAP server ends it
says so and offers to reconnect. Releases before systems file version 4 could
save passwords in the platform credential store; the first start after the
upgrade deletes them.

**Run Command** (General) runs CL commands on the selected system over its
QAP session and lists every message each sent, as the system sent it, with
the job it ran in; Prompt (F4), Check, Cancel and a per-system history are
there too. `docs/connecting.md` has the details.

**Run SQL** and **Schemas** (Database) and **Data Transfer** (General) need
QAP version 4. Run SQL runs SQL scripts with results a tab each, commitment
control and the system's messages as sent; Schemas browses the catalog; Data
Transfer moves rows between a table or query and CSV, TSV, XLSX, ODS or
fixed-width files, exactly (decimals never go through floating point). Each
Run SQL and Run Command window and each transfer has a connection of its own,
attached to the session without a new sign-on, so a long query or command
holds up nothing else; a window never signs off. `docs/connecting.md` has the
details.

### Transfer requests and `qsaccess run`

A transfer can be saved as a `.qdx` request (**Save Request...** in either
Data Transfer window): everything but passwords, which are never saved, with
the file's path relative to the request when it is in the request's folder.
Opening one (a double-click, or **Open Request...**) shows it ready to run.

```
qsaccess run <file.qdx>
```

runs a request without its window. If the program is running, the run hands
it the request file's path over the per-user instance endpoint and nothing
else: the program runs the transfer on its own session and a connection of
its own (showing Sign On first if it has no session on the request's
system) and sends back the progress, the summary and the exit code. The
session token never leaves the running program. The endpoint takes
connections from the same user only (on Windows a named pipe that refuses
remote clients and lets the current user alone open it, on Linux a socket of
mode 0600), and each end checks the other's user; another user's connection
is refused and logged as `QAD8040`. With no running program, the run shows
the Sign On dialog itself and signs off at the end. Progress goes to stderr,
and to a progress window with **Cancel**; the summary goes to stdout. A download replaces an existing
output file even when the request says to ask. Exit codes:

| Code | Meaning |
|---|---|
| 0 | The transfer is done. |
| 1 | Bad arguments, or a request file that cannot be read or names a system not configured here. |
| 2 | Sign-on cancelled or failed (or the system's QAP version has no `Sql`). |
| 3 | The transfer failed (the summary says where, and the row to resume from). |
| 4 | The transfer was cancelled. |

Fully unattended runs, with no sign-on prompt, are not offered: passwords are
never saved, so a run with no running program signed on to the system always
asks for one. On Windows `qsaccess` is a windowed program, so `cmd.exe` does
not wait for it: use `start /wait "" qsaccess run file.qdx` and then
`%ERRORLEVEL%`, or in PowerShell
`(Start-Process qsaccess -ArgumentList 'run','file.qdx' -Wait -PassThru).ExitCode`.

A 5250 display session can sign on automatically over Telnet TLS with a
one-time QAP ticket instead of a password (Session Properties, **Sign on
automatically**); `docs/connecting.md` has what the system needs.

**System Configurations > Verify...** checks the name lookup, the Telnet port
(and its TLS when used), the QAP port, the TLS 1.3 handshake and certificate,
and QAP Hello, with an optional test sign-on, and shows each as OK, Failed or
Skipped. A failed test sign-on counts toward `QMAXSIGN` and is audited like
any sign-on.

`docs/connecting.md` has the details: ports, the server commands, sign-on and
password rules, what happens when a session or the server ends, and every
Verify Connection check with its messages.

## Installing

Each platform has one installer file, built with `cargo xtask dist` (Linux:
`ci/dist-linux.sh`, which builds in a pinned glibc 2.31 image):

- `QS400-Access-V1R0M0-windows-x86_64.exe`
- `QS400-Access-V1R0M0-linux-x86_64` (make it executable and run it)

Run it for the Setup wizard: Welcome, Licence, Install for (Just me or All
users), Location, Options (menu entries, desktop shortcut, file types),
Ready, Progress, Finish. The installer checks its own checksum before it
changes anything (`QAD2103` if the file is damaged). Every file, folder,
registry key and desktop entry it makes is listed in `install-manifest.toml`
in the install folder, and uninstall removes exactly those. Settings, system
configurations and session files are never removed unless you tick **Also
remove my settings**, and the Sessions folder never is.

On Windows, Just me installs to `%LOCALAPPDATA%\Programs\Quartz Systems\Access`
without administrator rights; All users installs to
`%ProgramFiles%\Quartz Systems\Access` and Windows asks for administrator
rights for that choice only. The Start menu folder is **Quartz Systems**; a
file name cannot hold `/`, so the program's shortcut is named
*QS400 Access for Desktop*.

On Linux, Just me installs to `~/.local/opt/quartz-access`, with links in
`~/.local/bin` and menu entries, MIME types and icons under `~/.local/share`;
All users (run the installer as root) installs to `/opt/quartz-access`, with
links in `/usr/local/bin` and entries under `/usr/local/share`. `.qsd`, `.qkm`,
`.qsm` and `.qdx` are registered as `application/x-qsaccess-session`, `-keymap`,
`-macro` and `-transfer`, by name and by their `[qsaccess]` header. The installer runs
`update-mime-database`, `update-desktop-database` and `xdg-mime default` when
they are there, and warns (without failing) when they are not, or when
`~/.local/bin` is not on `PATH`.

The Linux binaries are built against glibc 2.31 (Ubuntu 20.04, in
`ci/dist-linux.Dockerfile`) and need no glibc newer than 2.30. They run under
Wayland and X11 alike, and load only these libraries, which every desktop
installation has:

| Library | For |
|---|---|
| `libwayland-client.so.0`, `libwayland-cursor.so.0`, `libxkbcommon.so.0` | Wayland |
| `libX11.so.6`, `libXinerama.so.1`, `libXcursor.so.1`, `libXfixes.so.3` | X11 |
| `libfontconfig.so.1`, `libpango-1.0.so.0`, `libpangocairo-1.0.so.0`, `libcairo.so.2`, `libgobject-2.0.so.0` | Text and drawing |
| `libgcc_s.so.1`, `libc.so.6`, `libm.so.6`, `libdl.so.2`, `libpthread.so.0` | The C runtime |

**Windows SmartScreen.** The installer is not code-signed yet, so the first
time it is run from a download Windows shows *Windows protected your PC*.
Select **More info**, check that the file name is
`QS400-Access-V1R0M0-windows-x86_64.exe`, then **Run anyway**. The program
installed has the same publisher details in its properties (Quartz Systems,
1.0.0 (V1R0M0)) but is not signed either.

### Silent install, uninstall and repair

```
QS400-Access-V1R0M0-windows-x86_64.exe --silent --scope=user|machine [--dir=PATH]
    [--no-desktop-shortcut] [--no-associations] [--no-start-menu]
    [--close-running] [--log=FILE]
uninstall.exe --uninstall [--silent] [--remove-settings] [--close-running]
uninstall.exe --repair [--silent] [--close-running]
```

On Linux the uninstaller is `quartz-access-uninstall` (on `PATH` through
`~/.local/bin` or `/usr/local/bin`). `--scope=machine` needs administrator
rights (Windows) or root (Linux). Without `--close-running`, a silent run stops
with exit code 6 if QS/400 Access for Desktop is running. The log goes to
`qsa-install.log` in the temporary folder unless `--log` names another file.

| Exit code | Meaning |
|---|---|
| 0 | Done |
| 1 | Arguments not understood |
| 2 | Installer payload damaged (`QAD2103`) |
| 3 | Administrator or root rights needed for `--scope=machine` (`QAD2104`) |
| 4 | Cancelled |
| 5 | A file or registry key could not be written or removed; see the log |
| 6 | QS/400 Access for Desktop is running |
| 7 | No install found to uninstall or repair |

## Tests and goldens

`cargo test --workspace` runs everything, including the screenshot tests
against the goldens for your platform. Goldens are made on CI's pinned runners:

```
gh workflow run ci.yml --ref <branch> -f bless=true
gh run download <run-id> -n goldens-windows -D .
gh run download <run-id> -n goldens-linux -D .
```

The Linux job runs in `ubuntu:24.04` pinned by digest, under Xvfb, with only the
bundled fonts. `TESTING.md` has the details, the manual Wayland check and the
live acceptance list.
