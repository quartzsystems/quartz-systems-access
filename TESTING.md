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
- `qap-client/tests/version2.rs`: every version-2 operation and status
  against the test system: `RunCommand` (`Ok`, `Error`, `NotFound`,
  `NotAuthorized`, `Cancelled`, interactive only), `CheckCommand`,
  `DescribeCommand`, `PasswordStatus`, `TelnetTicket` (and the ticket as the
  Telnet server takes it).
- `qap-client/tests/version3.rs`: every version-3 `Spool` operation and
  status against the test system's output queues and spooled files
  (`qapd::testing::fixtures`); batched reads that join byte for byte into
  a whole-file read once each later batch's leading state is taken off;
  batches written as received that render the whole file's page model;
  cancel in a batch and between batches; a read beside a long command; and
  the 64 MiB limit on a joined reply, the session resumed after it.
  `tests/connection.rs` also has replies that come back in the reverse
  order of their requests, both kinds of cancel, and a long request that a
  timeout never cancels.
- `qap-client/tests/older.rs`: QS/400's engine at 8306ec41 (`qapd_v2`, its
  own `qapd::testing`, `tests/common/v2.rs`): version 2 agreed, every
  `Spool` operation refused before it is sent, version 2 still working.
- `qap-client/tests/bodies.rs`: every QAP version 8 and 9 request, reply
  and event body encoded, decoded, framed, and as a reply split with `More`
  and joined; a `ReadMember` reply over 1 MiB and a `WriteMember` of three
  pieces, from QS/400's conformance `COMPILE42.rpgle` and debug `ASSETS`
  (`qap-client/tests/fixtures/`, each file's origin in the test's header).
- `qap-client/tests/version8.rs` and `version9.rs`: `Develop`, `Jobs`,
  `Sql` 12 and 13 and `Debug` against the test system: a member over 1 MiB
  read whole and written back in pieces, dates kept, the library list per
  connection job, `Compile`'s diagnostic on line 42, a debug session's
  `Stopped` and `DebugEnded` on its receiver while other requests are out;
  and the gate: the default still agrees version 6, and below 8 (or 9 for
  `Debug`) each new method is refused before anything is sent.
- `qsaccess/tests/fallback.rs`: against the version-1 engine, sign-on works
  and every version-2 function is `QAD5001` (the expiry warning silent);
  against version 1 and version 2, Printer Output is `QAD5001` and every
  earlier function still works.
- `qsaccess/tests/runcmd.rs`: Run Command's results with the test system's
  severities 00 to 40, and a cancelled command's reply.
- `qsaccess/tests/printer_output.rs`: Printer Output's filters, a read on
  its own thread to a temporary file, Save as PDF and as Text laid out as a
  printer session lays out the same data, refused reads with the system's
  message as sent, and a cancelled read.
- `qsaccess/tests/spool_goldens.rs`: the page-model goldens below.
- `qsaccess/tests/secrets.rs`: a full run with every trace line in the log
  file, then the log, a session recording and the `Debug` output searched
  for every password, the session token and the Telnet ticket.
- `qsaccess/src/prompt/form.rs`: the command string builder against the test
  system's CRTLIB, DSPOBJD, CRTPF and DLYJOB definitions.
- `qsaccess/tests/autosignon.rs`: 5250 automatic sign-on end to end: a
  ticket from the test system, sent by tn5250's negotiator, taken by the test
  system as its Telnet server takes one (`System::redeem_ticket`): once over
  TLS, then refused when used again, over plain Telnet, after a minute, for
  another profile, or when `QRMTSIGN` does not allow it (`TCP1A55`); and
  never in the trace.
- `qsaccess/tests/sessions.rs`: one QAP session per system per process
  against the same server: every tool gets the same client, a running
  command and another request share its connection, and the GUI's look at
  the signed-on profile never waits on a client in use.

Phase 7 (QAP version 4), against QS/400's engine at dd1f61a4 and its test
database (`qapd::testing::sql`):

- `qap-client/tests/attach.rs`: `Attach` and `JobInfo`: a connection of its
  own with its own job as the session's profile; refused before version 4.
- `qap-client/tests/sql.rs`: every `Sql` operation and status (`SqlError`
  with SQLSTATE and SQLCODE, `NotFound`, `NotAuthorized` with `SQL0551`,
  `InUse` with `SQL0752`, `Cancelled`); `Attach` first or `InvalidRequest`,
  and not again on the same connection; the session's 16 connections
  (`TCP1A59`); `SessionEnded` (`TCP1A58`) after a sign-off elsewhere; the
  three cancel bodies; cursors that survive Commit and Rollback; a failing
  row's index; `SQL7008` on the unjournaled table; rollback when a
  connection closes; every type (DECFLOAT's special values, CCSID 65535
  bytes, `TIMESTAMP(12)`, BOOLEAN, NULLs); a result over 1 MiB streamed in
  blocks of 500; and a forced drop mid-fetch (`System::drop_connection`):
  the cursor lost, said once, never sent to the new job.
- `qap-client/tests/drops.rs`: connections dropped under their requests by
  the test system's forced close: read-only requests resumed and sent once
  more (a spooled file batch included), a command never sent again, a
  window's connection attached anew; every close logged at info.
- `qsa-net`'s unit tests: polled reads, and os error 997 as no data.
- `qsa-sql/tests/props.rs`: 20,000 generated values each: decimal and
  DECFLOAT text, integers, binary and CCSID 65535 data come back unchanged
  through CSV fields (either decimal point) and spreadsheet cells; dates,
  times and timestamps read back in every date style.
- `qsa-transfer/tests/download.rs` and `upload.rs`: downloads to every
  format with their exact text; columns, Where clause and row limit;
  append; cancel and failure leaving no file; uploads mapped by name,
  position and hand; a failing row in one transaction, with a commit every
  batch (resume from the first row not committed) and without commitment
  control after `SQL7008` (resume from the failing row); Replace in one
  transaction; a misfit; a cancel; and the all-types table downloaded and
  uploaded back in CSV, TSV, XLSX, ODS and fixed width, CCSID 65535 data as
  hex and as text, reproducing every value exactly (ODS refuses CCSID 65535
  text with control characters, which XML cannot hold).
- `qsaccess/tests/runsql.rs`: Run SQL's connection worker: a script's
  messages as sent with SQLSTATE, SQLCODE and script line, Stop at First
  Error, Fetch More and Fetch All, a cancelled query keeping its rows
  (incomplete), the commit level and `InUse`, a drop mid-fetch (`QAD7010`,
  nothing run again, the next run attached anew), `SessionEnded` after a
  sign-off on the shared connection, and a close that rolls back and never
  signs off.
- `qsaccess/tests/runcmd.rs`: Run Command windows on their own attached
  connections beside a long command, the seventeenth connection as
  `QAD7003`, and a sign-off elsewhere.
- `qsaccess/tests/fallback.rs`: before version 4, Run SQL, Schemas and Data
  Transfer are `QAD5001`, nothing of `Sql` or `Attach` is sent, and Run
  Command keeps the shared connection.
- `qsaccess/tests/run_request.rs`: `qsaccess run` delegated to the running
  instance with only the request's path, the instance running it on its own
  attached connection and sending back progress, the summary and the exit
  code, every message both ways searched for the session token; exit code 1
  for bad arguments and request files.
- `qsaccess`'s `ipc` unit tests: frames both ways on one link, and a peer of
  another user refused both ways (simulated by expecting another SID or uid,
  since a test cannot run as another user), counted and closed unread.
- `qsaccess/tests/secrets.rs` also attaches and runs SQL, and searches for
  the session token too.
- `qap-client/tests/ifs.rs`: version 5's `Ifs` service against QS/400's
  test file system (`qapd::testing::files`): every operation and status
  with the file-system commands' messages (`CPFA0A9`, `CPFA09C`, `CPFA0A0`,
  `CPFA09E`, `CPFA0AC`, `TCP1A5A`), 1,200 entries in pages of 500, links
  and a broken one, `InUse` on `LOCKED` and the read after
  `System::release_lock()`, `CHECKED_OUT`, the 200 MiB file both ways byte
  for byte, the 37, 273, 819, 1208 and mixed 939 files read in chunks that
  cut inside characters joining into the system's whole conversion, text
  written in pieces making the same file as one write, a text download
  resumed from its last `next_offset` on another connection, a read and a
  binary upload resumed after `System::drop_connection`, a cancel, and a
  session at version 4 sending nothing of `Ifs`.
- `qsa-transfer/tests/files.rs`: `TREE` counted and deleted depth-first,
  stopping at `TREE_HELD` (checked out to `ANN`) with `CPFA09E`, what came
  before it deleted and its folders refusing as not empty (`CPFA0AC`); a
  folder copied on the system and deleted whole.
- `qsa-transfer/tests/file_transfer.rs`: one file's transfer: binary both
  ways resumed after a cancel, the partial download beside its target, the
  existing-file policies (Ask with each answer, Skip, Replace, Replace if
  newer, Keep both), text converted by the system with CR LF here matching
  the system's whole-file conversion, a text download resumed from its last
  confirmed `next_offset`, folders made a job per file, and a saved
  request's files planned (patterns, recurse or not, a refused folder).
- `qsa-transfer`'s `files::lines` unit tests: LF to CR LF and back, a CR LF
  pair split across chunks and across every buffer size.
- `qsa-transfer`'s `request` unit tests: `.qdx` version 2 for tables and
  files both ways, version 1 read as a table's, and what is refused.
- `qsaccess/tests/ifs_editor.rs`: the editor's reads (each CCSID, the mixed
  file, a link, binary as a hex dump read-only, over 8 MiB read-only, the
  200 MiB file refused, `InUse` then the read after the hold ends) and
  saves (the CCSID kept, Save As in another CCSID, `Exists` for a name
  taken). What a save checks: on version 6 `Stat`'s change count, moved by
  a save with no wait and by `Files::change_twice` (two changes in one
  millisecond, `modified` held), a link's by its target's; on version 5
  (`max_version` 5) `GetInfo`'s change time, after a 20 ms wait.
- `qsaccess/tests/run_request.rs` also runs a files request delegated to the
  instance: exit 0 with the files written, exit 3 for a folder refused.
- Certificate Management. `qsa-net`'s `x509` unit tests: the certificates
  in a PEM file, a DER file, several in one file in order, and files that
  hold none (a key, text, cut-short DER). `qsaccess`'s `certs` unit tests:
  what Import takes and refuses, one row per certificate with the systems
  that trust it, a referenced file read as PEM or DER, adding once and
  removing while keeping a file's other certificates, the Valid to column
  (Expired, the days left), the confirmation's wording and View's text.
  `qsaccess/tests/certs_sync.rs` (a plain program, `harness = false`, since
  it makes the real windows): the window and the Security tab of an open
  System Properties show one list, each sees what the other adds and
  removes at once, OK saves the dialog's list and Cancel drops it, and
  **Trust for this system** shows in the window.
- Help. Building `qsaccess` fails on a help link to a topic or heading that
  does not exist, on a topic file not listed in `help/contents.txt` and on a
  link out of Help (`qsaccess/build/help.rs`). `qsaccess`'s `help` unit
  tests: every topic a window or dialog opens exists, every internal link
  resolves, Search finds a known phrase (plain substring, title matches
  first), the Messages reference has every catalogue message with its cause
  and recovery, the keyboard reference has the default map, and a topic
  prints as pages with its headings in bold and makes a PDF.
  `qsaccess/tests/help_window.rs` (a plain program too): Home, Back and
  Forward, every link of every topic followed through FLTK's help view as a
  click follows it, a link from a showing no longer on screen, Search's
  list, and `QAD0050` for a topic that does not exist.
- `qsa-config`'s `documents` unit tests: the earlier Documents folders
  moved whole into the one folder, a file whose name is taken left where it
  was with nothing overwritten, and paths re-rooted.
- `qsaccess/tests/macro_editor.rs` (a plain program): the Macro editor has
  unsaved changes after an edit and none once it is undone, always for a
  recorded macro with no file, never for an untouched one.
- `qsa-net`'s `x509` unit tests also cover `details`: the key (EC curves,
  RSA size, Ed25519), the signature algorithm, alternative names of each
  kind and the basic constraints.
- `ci/version.sh names`, a CI step: no file names a release that is not
  `VERSION`'s (docs/releasing.md).
- `qsaccess/tests/fallback.rs` also: the Integrated File System is
  `QAD5001` before version 5, against a version 2 server and in a session
  that agreed version 4.
- `qap-client/tests/sql.rs`: a slow select, plain and under
  `RepeatableRead`, holding up no Run Command, Printer Output list or other
  window's query; `DECFLOAT` read back as the engine's text exactly and `=`
  by value. `qsa-transfer/tests/upload.rs`: the same `DECFLOAT` texts
  through a download and upload in every format.
- `qap-client/tests/connection.rs`: the server's end over `System::listen`:
  connects refused while ended, taken after `System::restart()`, and a
  connection opened before the end still ending with it after a quick
  restart.

Phase 9 (QAP version 6), against QS/400's engine at 93c6350f and its test
message queues and spool changes (`qapd::testing::messages`,
`System::inject_message`, `spool_add`, `spool_change`, `spool_remove`;
watches looking every 20 ms with `Config::watch_poll`):

- `qap-client/tests/messages.rs`: every `Messages` operation and status:
  lists strictly newest and oldest first and paged over `QSYSOPR`'s 149;
  `ReplyMessage` with `CPF2420` (a reply the inquiry does not take), the
  default for an empty reply, `CPF2422` (answered, or not an inquiry) and
  `CPF2410`; `RemoveMessages` all or none; `SendMessage` naming the
  signed-on profile, `CPF2204`, `CPF2403` for a profile that never signed
  on, `CPD0071`; `CPF9802` for `QGPL/SECRETQ`; `UnwatchQueue` and
  `UnwatchSpool` of nothing watched (`TCP1A5B`), `*ALL` refused. The watch
  manager: injected arrivals reported once with the inquiries counted; a
  passive watch beside `QGPL/TEAMQ`'s break-mode holder; a forced close
  (`System::drop_all`) with messages injected in the gap, caught up exactly
  once after `Resume`; a gap of 250 (three pages); the newest known key
  removed in the gap (stopped by time); `NotAuthorized` on a watch again
  reported and the watch taken away; a new sign-on of the next client
  watching again and catching up, and `Own` started afresh for another
  profile; an inquiry's reply matched through the sender's copy and
  `answers`, to another queue and to one's own; spool watches reporting an
  add, a hold, a move between two watched queues (`Removed` then `Added`,
  `Changed` for the user's target) and a delete, and watched again after a
  resume. Refused before version 6.
- `qap-client/tests/ifs.rs` also: `Stat`'s entry and change count (two
  changes in one millisecond, a write, a directory's entries), a link
  stamped by its target, refused before version 6 with `GetInfo`'s stamp
  in its place.
- `qsaccess/tests/messages.rs`: the Messages-waiting count across a drop
  and resume, a catch-up, an inquiry answered elsewhere while away, a
  session ended under a window's own connection and an operator's `ENDJOB`
  (`TCP1A54`): the watches handed to the system's next client, which
  watches again and catches up at Sign On.
- `qsaccess/tests/printer_output.rs` also: each filter's watch target (one
  queue, one user, or none for every user everywhere), and pages kept by
  real spool events alone, a move between two watched queues included,
  equal to fresh lists after each change.
- `qsaccess/tests/fallback.rs` also: Messages is `QAD5001` before version
  6, and nothing of version 6 goes to a session at version 5.
- `qsaccess`'s `messages::model` unit tests: rows, bold inquiries, the
  detail pane and its link, what Remove takes without the second
  confirmation, what opens Message Arrived; `qsa-config`'s message options
  per system in `settings.toml`; `qsa-msg`'s `QAD9001` to `QAD9021`.

What is checked by hand only: Message Arrived never taking the focus from
a 5250 session (Windows, X11 and Wayland; see the Phase 9 checklist).

CI also runs `cargo fmt --check`, `cargo clippy -D warnings`, the clean-room
check (`ci/cleanroom.sh`), the copyright header check and `ci/deps.sh`
(`qsa-net`, `qap-client`, `qsa-sql`, `qsa-transfer`, `tn5250` and `scs` must
not depend on FLTK).

The clean-room check allows an explicit list of files and no others: each
protocol crate's `wire.rs` (today only `tn5250/src/wire.rs`). The read-only
spec mirror `docs/qap-protocol-spec.md` is checked like any other file, and
the check fails if it is ever allowlisted.
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
message, Certificate Management with its dialogs, Help) and
`qsa-install/tests/goldens/` (every wizard page, Uninstall,
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

None: every view is blessed on CI's runners. The main window and the
installer's pages show the release, so they wait for a bless again after
each `ci/version.sh prepare` (docs/releasing.md).

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

### Spooled file goldens

`qsaccess/tests/spool_goldens.rs` reads QS/400's test system's SCS files
`QPJOBLOG`, `QSYSPRT`, `LANDSCAPE`, `MIXED` and `OVERFLOW` through qap-client
in batches of 25 pages, written as received, and whole in one request. Both
must give the same page model and text (checked against
`qsaccess/tests/goldens/spool/<name>.pages` and `<name>.txt`) and the same
PDF pages: with `pdftoppm` (required where `QSA_REQUIRE_PDFTOPPM=1`) both
PDFs are rasterised and compared page for page. The goldens are text, the
same on every platform; `QSA_BLESS=1` writes them.

`OVERFLOW`'s page 26 is begun by overflowing the page length, not by a form
feed, exactly at a 25-page batch boundary: the second batch's leading state
moves to the line with AVPP (`X'34C4'`), and the page lays out as in the
whole file. `qap-client/tests/version3.rs`
`one_page_batches_render_the_whole_files_pages` checks every page of every
file read a page at a time.

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

## Live acceptance (Phase 5: QAP version 2)

On QS/400 at 10.42.70.150, prepared first:

1. a QS/400 build at or after 8306ec41 installed (`STRTCPSVR SERVER(*QAP)`,
   a certificate assigned to `QS400_ACCESS_SERVER`);
2. `CHGSYSVAL QRMTSIGN '*VERIFY'` (a system installed earlier keeps the old
   shipped `*FRCSIGNON`, which turns automatic sign-on off);
3. Telnet TLS on 992 with a certificate this computer trusts (the one
   assigned to `QS400_TELNET_SERVER`, else the system's self-signed one,
   trusted when asked);
4. a test profile whose password expires inside the warning period, for
   example `CRTUSRPRF USRPRF(QATEST) PASSWORD(...) PWDEXPITV(5)` with
   `QPWDEXPWRN` 7 (the shipped value).

Then, with `QSACCESS_LOG=trace`:

- [ ] **Verify.** System Configurations > Verify...: QAP Hello says
  "Version 2".
- [ ] **Run a command.** Run Command, `CRTLIB LIB(QATESTLIB)`, Run:
  `QAD5010` and `CPC2102` as the system sent it; the job line shows
  `QAPCNN`, `QUSER` and a number; the ID opens its second-level text. Run it
  again: `QAD5011` and `CPF2111`. `DLTLIB LIB(QATESTLIB)` afterwards.
- [ ] **Prompt a command.** Type `DSPOBJD`, F4: the title is the command's
  text and name; `OBJ` with indented name and library rows; `OBJTYPE` with
  "+"; F1 on a row shows the prompt text with the no-help note; F10 shows
  the additional parameters. Fill `OBJ(QGPL/QSYS)` and `OBJTYPE(*LIB)`,
  Show Command String, OK: the string is on the command line. Prompt a
  command with an element list (`CRTPF`, `SIZE` under F10).
- [ ] **Check errors.** Prompt `DSPOBJD`, leave `OBJ` empty, OK: the check's
  `CPD0071` and `CPF0006` are listed in the dialog with `QAD5020` and it
  stays open. Check on the Run Command window does the same in the results.
- [ ] **Cancel a long command.** Run `DLYJOB DLY(60)`, Cancel after a few
  seconds: `QAD5012` well before the minute. The window keeps redrawing and
  Cancel answers while the command runs.
- [ ] **Not authorized and interactive only.** As a profile without
  `*SECADM`, `DLTUSRPRF USRPRF(QATEST)` shows the system's escape message on
  the status line; `STRSEU` ends with its escape message (`QAD5011`).
- [ ] **Expiry warning.** Sign on as `QATEST`: `QAD4031` with the days left,
  once; Change Password... changes it. Sign off and on as a profile whose
  password does not expire: nothing.
- [ ] **Auto sign-on over TLS.** A display session on 992 with Use TLS and
  Sign on automatically: Connect goes straight to the profile's initial menu
  (Sign On is asked first if this program had no session). `WRKACTJOB` shows
  the device job under the signed-on profile.
- [ ] **Refused over plain Telnet, and a reused ticket refused.** The same
  session file edited by hand to `tls = false` (the check box is disabled
  without TLS): the status bar says `QAD5030` and the sign-on display comes
  up. Then, for what the program itself never does:
  `QSA_CHECK_PASSWORD=... cargo run -p qsaccess --example ticket_check --
  10.42.70.150 QATEST` signs on over QAP, connects on 992 with a ticket
  (PASS: signed on), connects again with the same ticket (PASS: the sign-on
  display), and offers a new ticket on 23 (PASS: the sign-on display). It
  prints no ticket and signs off at the end.
- [ ] **TCP1A55.** `CHGSYSVAL QRMTSIGN '*FRCSIGNON'`, Connect: `TCP1A55` shown
  as the system sent it, then the sign-on display. Set `*VERIFY` back.
- [ ] **Nothing secret logged.** Search `qsaccess.log` for the passwords
  used and for any 64-hexadecimal-character run: none.

Against a version-1 server (QS/400 before 8306ec41, if one is at hand; the
automated tests cover it with QS/400's engine at 58a7e6d3): Run Command
opens disabled with `QAD5001`, Session Properties' Sign on automatically is
disabled with `QAD5001`, no expiry warning, and sign-on works.

## Live acceptance (Phase 6: QAP version 3)

On QS/400 at 10.42.70.150, prepared first:

1. a QS/400 build at or after 3134e9ea installed (`STRTCPSVR SERVER(*QAP)`;
   Verify... says "Version 3");
2. a second profile, for example `CRTUSRPRF USRPRF(QATEST2) PASSWORD(...)`,
   and an output queue that does not show its files to others:
   `CRTOUTQ OUTQ(QGPL/QATESTQ) DSPDTA(*NO)`. Signed on as `QATEST2` in a
   5250 session: `CHGJOB OUTQ(QGPL/QATESTQ)`, then
   `DSPOBJD OBJ(QGPL/*ALL) OBJTYPE(*ALL) OUTPUT(*PRINT)`: one of its
   spooled files on that queue;
3. as the profile the tests sign on with, a large listing on `QGPL/QPRINT`,
   for example `DSPOBJD OBJ(QSYS/*ALL) OBJTYPE(*ALL) OUTPUT(*PRINT)` (well
   over 25 pages), and some small ones (`DSPLIB LIB(QGPL) OUTPUT(*PRINT)`);
4. a 5250 printer session of this program (device `QSAPRT01`) with its
   writer started, for the printing case and the comparison below.

Then, with `QSACCESS_LOG=trace`:

- [ ] **List and page `QGPL/QPRINT`.** Printer Output, Output queue
  `QGPL/QPRINT`, Go: `QAD6001` with the total; Next and Previous page
  through it, the paging line following. With `rows_per_page = 50` in
  `settings.toml`'s `[printer_output]`, a page has 50 rows.
- [ ] **Filter.** Status Held, then User `*ALL`, then a Form type and a
  User data of one of the files: each Go lists only what matches. A column
  heading sorts the page, and the status bar says so.
- [ ] **Open a large listing.** Open the large listing: the viewer shows
  page 1 while the status bar still counts pages (`QAD6010`) and the page
  count reads "of N...". Find a word on several pages: matches highlighted,
  F3 goes on through the pages; Copy puts the page's text on the clipboard.
- [ ] **Cancel a read.** Open it again and Cancel Reading before it ends:
  `QAD6011`, the pages read stay, and the trace shows the QAP cancel for
  the batch in flight.
- [ ] **Save PDF and text.** Save as PDF... and Save as Text...: `QAD6020`
  each. Move the same file to the printer session's queue so `QSAPRT01`
  receives it: its PDF in Received output and the saved one have the same
  pages and the same layout. Received output itself is as before.
- [ ] **Print to a local printer.** Print...: the printer list is this
  computer's; `QAD6021`; the printed copy looks as the printer session
  prints the file.
- [ ] **Hold, release, move, delete.** Select several files: Hold (each of
  Immediately and At End of Page), Release, Move to another queue, then
  Move with To the top of the queue (the file is first on that queue),
  Delete (the confirmation lists every file): each ends with the summary,
  every file's result in it.
- [ ] **A printing file.** While `QSAPRT01` shows a file Printing, Delete it,
  then Move it: the system's message (`CPF3330`) as it sent it.
- [ ] **An unauthorized file.** User `*ALL`, Output queue `QGPL/QATESTQ` (or
  `*ALL`): `QATEST2`'s file is listed; Open it, and Hold it: the system's
  message as it sent it (`CPF3303` or what QS/400 sends), Details giving its
  second-level text.
- [ ] **Auto-refresh.** Auto-refresh 15 seconds, then spool a new file from
  a 5250 session (`DSPLIB LIB(QGPL) OUTPUT(*PRINT)`): it is listed within
  15 seconds, the selection and scroll position kept. Minimised, the trace
  shows no `ListSpooledFiles`.
- [ ] **Run Command while a large read runs.** Open the large listing and,
  while it reads, Run Command `DLYJOB DLY(5)` then `CRTLIB LIB(QATESTLIB)`:
  both complete, and the read goes on and ends meanwhile.
- [ ] **Output Queues.** View > Output Queues...: `*ALL`, `*LIBL` and `QGPL`
  list their queues with files and writers; a double-click opens Printer
  Output on the queue.
- [ ] **Other device types.** If the system has an AFP, ASCII or line file:
  Open shows QS/400's `TCP1A56` as it sent it.
- [ ] **Nothing secret logged.** Search `qsaccess.log` for the passwords
  used and for any 64-hexadecimal-character run: none.

Against a version-2 server (QS/400 before 3134e9ea, if one is at hand; the
automated tests cover it with QS/400's engine at 8306ec41): Printer Output
is dimmed in the task pane with `QAD5001` as its tip and says the same when
opened; Run Command, the expiry warning and automatic sign-on work as
before.

## Live acceptance (Phase 7: QAP version 4)

On QS/400 at 10.42.70.150, prepared first:

1. a QS/400 build at or after dd1f61a4 installed (`STRTCPSVR SERVER(*QAP)`;
   Verify... says "Version 4");
2. the test data from QS/400's `tools/conformance/qap/` (its `README.md`
   says how): `RUNSQLSTM` of `hlinv.sql`, then each command of `hlinv.cl`
   (profiles `BOB` and `ANN`, `HLTEST.NOJRN` unjournaled, `HLPAY.SALARIES`
   for `ANN` only). Names differ from the in-process test database where that
   README says (the view is `HLINV.ASSETSACT`; there is no `ASSETSL1`);
3. the long query, from the comment in `hlinv.sql` above `HLTEST.SLOW`:
   `SELECT COUNT(*) FROM HLTEST.SLOW A WHERE EXISTS (SELECT 1 FROM HLTEST.SLOW B WHERE EXISTS (SELECT 1 FROM HLTEST.SLOW C WHERE C.ID > A.ID + B.ID + 1000000))`.
   Never cancel-test with a join of several copies (see that README);
4. this program installed by its installer (for the `.qdx` file type), signed
   on as `BOB`.

Then, with `QSACCESS_LOG=trace`:

- [ ] **Query.** Run SQL, `SELECT * FROM HLINV.ASSETS`, Ctrl+Enter: twelve
  rows; `COST` with two decimals; `ACQUIRED` in the job's date format;
  `NOTES` a grey `-`; a header's tooltip shows its label and type; the title
  names the system and the window's job (`QAD7001` on the status line);
  `WRKACTJOB` shows that job beside the shared connection's. Copy Cell and
  Copy Row paste as tab-separated text.
- [ ] **The long query cancelled.** Run the long query, Cancel after a few
  seconds: `SQL0952` in Messages as the system sent it; the result tab says
  Incomplete (cancelled), as does the status bar (`QAD7011`).
- [ ] **Run Command while a long query runs.** Start the long query; in Run
  Command, `CRTLIB LIB(QATESTLIB)` completes at once (`QAD5010`), in a job
  of its own; `DLTLIB LIB(QATESTLIB)` afterwards.
- [ ] **Printer Output and a second Run SQL window while a long query
  runs.** Three times, each with one long statement running in a first Run
  SQL window: the long query above (an aggregate); a slow plain select,
  `SELECT A.ID, A.NAME FROM HLTEST.SLOW A WHERE EXISTS (SELECT 1 FROM
  HLTEST.SLOW B WHERE EXISTS (SELECT 1 FROM HLTEST.SLOW C WHERE C.ID + B.ID =
  600 + A.ID * 0))`, every row found only at the last `B` and `C`, so its 300
  rows come slowly (Fetch All; never a join, which builds its rows in memory); and the long query again at commit level
  Repeatable read. Meanwhile Run Command `CRTLIB LIB(QATESTLIB)` completes
  at once, Printer Output lists and opens a file, and a second Run SQL
  window queries `HLINV.LOCATIONS`: none waits for the statement (at
  58e189df no statement in one job holds up another; see `docs/QAP.md`'s
  "Version 4 Sql, as built"). Under Repeatable read, the rows read stay
  read-locked until Commit or Rollback: in the second window, an `UPDATE` of
  a row of `HLTEST.SLOW` the first has read waits up to the table's record
  wait (60 s) and fails with `SQL0913`, a database rule rather than a delay,
  while its `SELECT`s still answer at once. Commit in the first window
  releases them. Do not use `CLRPFM`, `INZPFM`, save or restore commands,
  `CPYTOSTMF`, `CPYFRMSTMF` or `RCLSTG` for this: those still run to their
  end.
- [ ] **Commit and Rollback.** Commit level Read committed: Commit and
  Rollback are enabled. `INSERT INTO HLTEST.JRN VALUES(1, 'one')`
  (`QAD7020`), Rollback, `SELECT * FROM HLTEST.JRN`: empty. Insert again,
  Commit: it stays. Insert once more and change the commit level: the
  program asks Commit, Roll Back or Cancel (`QAD7021`); so does closing the
  window. With a result half fetched, Commit: Fetch More still works.
- [ ] **An unjournaled table.** Under Read committed,
  `INSERT INTO HLTEST.NOJRN VALUES(1, 'x')`: the system's `SQL7008`
  (SQLSTATE `55019`) in Messages.
- [ ] **Not authorized.** As `BOB`, `SELECT * FROM HLPAY.SALARIES`:
  `SQL0551` as sent; Schemas lists `SALARIES` and opening it shows the same.
  As `ANN` in a second session (another computer or user): two rows.
- [ ] **Schemas.** The tree lists `ARCH001` and on in pages of 100 (More
  schemas...), `HLMANY`'s 150 tables in pages; `HLINV.ASSETS`'s columns in
  the right pane; View Data opens Run SQL with the first 1,000 rows; Transfer
  Data from... and to... open Data Transfer with the table set.
- [ ] **Download to each format.** `HLTEST.ALLTYPES` to CSV, TSV, XLSX, ODS
  and fixed width (`QAD7030` each). Open the XLSX and ODS in a spreadsheet
  program: `C_DECIMAL` a number with two decimals, `C_BIGINT` and the
  DECFLOAT special values text, `C_BOOLEAN` TRUE or FALSE, `C_TS12` with
  twelve fractional digits. Cancel a download of the long query: no file.
- [ ] **DECFLOAT as the engine writes it.** In Run SQL, run
  `UPDATE HLTEST.ALLTYPES SET C_DECFLOAT = 1.50 WHERE ID = 7` (and `1E+2`,
  `1.00E+2`, `1e2`, `+1.5`, `0.0000001`, `-0`), each followed by `SELECT
  C_DECFLOAT FROM HLTEST.ALLTYPES WHERE ID = 7`: `1.50`, `1E+2`, `100`,
  `1E+2`, `1.5`, `1E-7` and `-0`, compared as text. With `1.50` set,
  `WHERE C_DECFLOAT = 1.5` finds row 7, and with `-0`, `= 0` does. Download
  row 7 to each format and upload it back: the same text each time.
- [ ] **Upload from each format.** `DELETE FROM HLTEST.ALLTYPES`, then
  upload each downloaded file back (Replace): `SELECT * FROM
  HLTEST.ALLTYPES` shows the same rows as before, NULLs, decimals, DECFLOAT
  special values, `TIMESTAMP(12)`, BOOLEAN and the CCSID 65535 bytes
  included. The Mapping page shows a misfit (`QAD7035`) for a hand-edited
  CSV with a text in `C_INTEGER`.
- [ ] **A failing upload in both modes, with resume.** A CSV of `ID,NAME`
  rows 1 to 30 with row 17 repeating ID 3, into `HLTEST.JRN` (empty):
  - One transaction: `QAD7031` names row 17, line 18 and `SQL0803`; the
    table is empty.
  - Commit every 1 batch: `QAD7033` says how many rows were committed and
    offers Resume from Row X, X the first row not committed (not 17).
    Correct row 17, resume: rows 1 to 30, each once.
- [ ] **The unjournaled-table fallback.** The same CSV into `HLTEST.NOJRN`:
  the system's `SQL7008` with `QAD7034` and Upload Without Commitment
  Control. Uploading without it stops at row 17 with rows 1 to 16 kept, and
  offers Resume from Row 17.
- [ ] **A `.qdx` double-clicked.** Save Request... from a download, then
  double-click the `.qdx` in Explorer (or the file manager): the window
  opens with the request, ready to run (`QAD7040` when saved).
- [ ] **`qsaccess run` with a running instance.** With the program running
  and signed on: `start /wait "" qsaccess run assets.qdx` (Windows) or
  `qsaccess run assets.qdx` (Linux): no Sign On; the running program shows
  the progress window, progress on stderr; `QAD7030` on stdout; exit code 0;
  `WRKACTJOB` shows the transfer's job attached to the program's session.
  Cancel in the progress window: exit code 4. With `QSACCESS_LOG=trace`,
  the program's log shows `request from another instance: ["--run", ...]`
  and nothing else from the run. Close the console window during a long
  download: the program cancels it at its next progress line.
- [ ] **`qsaccess run` without one.** Close the program, run it again: the
  Sign On dialog, then the transfer; exit code 0. Cancel the Sign On: exit
  code 2. A request from a missing table: `QAD7036`, exit code 3.
- [ ] **A sign-off under a window.** With a Run SQL window open, Actions >
  Sign Off in the main window: the window shows the system's `TCP1A58` and
  offers Sign On...; its results stay.
- [ ] **Nothing secret logged.** Search `qsaccess.log` for the passwords
  used and for any 64-hexadecimal-character run: none.

Against a version-3 server (QS/400 at 3134e9ea, if one is at hand; the
automated tests cover version 2 with QS/400's engine at 8306ec41): Run SQL,
Schemas and Data Transfer are dimmed with `QAD5001`; Run Command works on
the shared connection; everything earlier works as before.

## Live acceptance (Phase 8: QAP version 5)

On QS/400 at 10.42.70.150, prepared first:

1. a QS/400 build at or after 58e189df installed (`STRTCPSVR SERVER(*QAP)`;
   Verify... says "Version 5"), with the Phase 7 setup done (profiles `BOB`
   and `ANN`, the `HLINV` and `HLTEST` data);
2. `fixtures/live/ifs-setup.sh` put in `/home/QSECOFR/` (with `sftp`, say)
   and run as `QSECOFR`: `QSH CMD('/home/QSECOFR/ifs-setup.sh')`. It makes
   `/home/BOB/qsa8` (1,200 files in `many`, the text files in CCSIDs 1208,
   37, 273, 819 and mixed 939 and a binary one in `text`, a link and a
   broken one, `edit37.txt`, `checkedout.txt` and `tree/src/lib/m.txt`
   checked out to `ANN`, and `tree`) and `/home/ANN/qsa8private`, which
   `BOB` may not read, with `MKDIR`, `CPY ... TOCCSID`, `ADDLNK`, `CHGAUT`
   and `CHKOUT`; its header says how to run it again. Check both `CHKOUT`
   jobs ended normally;
3. on this computer, a 200 MiB file of random bytes, `big.bin`, and its
   SHA-256 (`certutil -hashfile big.bin SHA256` on Windows, `sha256sum` on
   Linux);
4. this program installed by its installer, signed on as `BOB`.

Then, with `QSACCESS_LOG=trace`:

- [ ] **Home and `/`.** Integrated File System (General) opens on
  `/home/BOB`; the tree has `/home/BOB` and `/`; opening `/` lists `home`,
  `QSYS.LIB` and the rest; `/QSYS.LIB/QGPL.LIB` lists its objects. The
  status line says `QAD8001` with the count.
- [ ] **A large folder.** `/home/BOB/qsa8/many`: `QAD8001` says 1,200; the
  first 500 show, and scrolling down loads the next pages until
  `f1199.txt`. A pattern `f11??.txt` shows 100. Sorting by Size or Name
  sorts those loaded, folders first.
- [ ] **Links.** In `/home/BOB/qsa8`, `link1208 -> .../text1208.txt` is a
  Link to file and opens (Edit) as the text; `broken` is red, Broken link,
  and does not open.
- [ ] **An unauthorized folder.** Path `/home/ANN/qsa8private`, Go: the list
  area shows `CPFA09C` as the system sent it; Back returns.
- [ ] **Binary both ways, interrupted and resumed.** Upload... `big.bin` to
  `/home/BOB/qsa8` (Binary): the queue shows Upload, Progress and Speed.
  Part way, Cancel (`QAD8022`); Retry: `QAD8023` names where it goes on, and
  it completes (`QAD8020`). Download it back to another folder, cancelling
  part way: `big.bin.qsapart` is beside the target and the target is not
  there; Retry resumes and completes, the part file gone. The downloaded
  file's SHA-256 equals the original's. Also pull the network cable (or
  disable the adapter) for a few seconds during a download: it fails
  (`QAD8021`); Retry resumes from the last confirmed part.
- [ ] **Text round trips.** Download `text37.txt` and `text1208.txt` in Text
  mode with CR LF: both open in Notepad as the same German text with CR LF
  line ends. Edit the local copy of `text37.txt` (add a line with `ß`) and
  Upload it in Text mode, CCSID for new files 37, Replace: Properties says
  CCSID 37; Edit shows the new line; `DSPF '/home/BOB/qsa8/text37.txt'`
  (5250) shows it as the system's own conversion. Upload a new UTF-8 file
  in Text mode with CCSID 1208: Properties says 1208.
- [ ] **A text download resumed.** Make a large mixed file: Upload a few
  MiB of Japanese text (UTF-8) in Text mode with CCSID 939. Download it in
  Text mode, cancel part way, Retry (`QAD8023`): the result equals the
  original UTF-8 text exactly (`fc /b` or `cmp`), its CR LF aside.
- [ ] **Edit and save.** Edit `edit37.txt`: CCSID 37 on the status bar, line
  and column follow the cursor. Change a line, Ctrl+S: saved, still CCSID 37.
  Change it again; meanwhile in 5250 `EDTF '/home/BOB/qsa8/edit37.txt'` and
  save a change there; Ctrl+S here: `QAD8011` with Overwrite, Reload and
  Cancel; Reload shows the 5250 change. `binary.dat` opens read-only as a
  hex dump (`QAD8013`); Open on `text939.txt` opens a read-only copy in the
  default application.
- [ ] **Server-side copy.** Copy `edit37.txt`, open `many`, Paste:
  `QAD8005`; Properties of the copy says CCSID 37. Paste again: `QAD8010`
  with Replace, Skip and Cancel. Copy the folder `text`, Paste into
  `/home/BOB`: the folder and its files are there. No data passed through
  this computer (the trace shows `Copy` requests, no `ReadFile`).
- [ ] **Cut and paste.** Cut the copied `text` folder, open `/home/BOB/qsa8`,
  Paste: it moves (`QAD8003`); Paste is then empty.
- [ ] **Recursive delete.** Delete `tree`: the confirmation gives 7 files and
  4 folders. Delete: it stops at `src/lib/m.txt` with `QAD8006` and the
  system's `CPFA09E`; `docs` and `readme.txt` are gone, `src` is still there.
  Deleting `checkedout.txt` gives `CPFA09E` too.
- [ ] **Drag and drop.** Drag three files and a folder from Explorer (or
  Files) onto the list: the options dialog, then each in the queue, the
  folder's files with it; the list shows them when done.
- [ ] **A files `.qdx` double-clicked.** In the transfer options of a
  download of `text`, Save Request... (`QAD7040`). Double-click it in
  Explorer: the Integrated File System window opens with the request's
  options; OK queues its files. Open Request... in the options dialog runs
  another saved request instead.
- [ ] **`qsaccess run`, both ways.** With the program running and signed
  on, `start /wait "" qsaccess run text.qdx` (Windows) or
  `qsaccess run text.qdx` (Linux): no Sign On, file progress on stderr,
  `QAD8020` on stdout, exit 0; a request for `/home/ANN/qsa8private`: exit 3
  with the system's `CPFA09C`. Close the program and run `text.qdx` again:
  the Sign On dialog, then the files, exit 0.
- [ ] **Responsiveness during a transfer and a long query.** While the 200
  MiB upload runs: Run SQL `SELECT * FROM HLINV.ASSETS` and Printer Output's
  list answer at once, and the IFS window still browses. Then, with each of
  the three long statements of the Phase 7 item running in a Run SQL
  window (the aggregate, the slow plain select, the aggregate under
  Repeatable read): the IFS window lists and transfers, Run Command
  `CRTLIB LIB(QATESTLIB)` completes and Printer Output lists, none waiting.
- [ ] **Connections.** `WRKACTJOB`: each IFS window with transfers has one
  job of its own beside the shared connection's. With 15 Run SQL windows
  open, a transfer shows `QAD8030` once and runs on the shared connection.
- [ ] **Nothing secret logged.** Search `qsaccess.log` for the passwords
  used and for any 64-hexadecimal-character run: none.

Against a version-4 server (QS/400 at dd1f61a4, if one is at hand; the
automated tests cover version 2 with QS/400's engine at 8306ec41 and a
session that agrees version 4 with the current one): Integrated File System
is dimmed with `QAD5001`, and opening a files `.qdx` says the same; Run SQL,
Schemas, Data Transfer and everything earlier work as before.

## Live acceptance (Phase 9: QAP version 6)

On QS/400 at 10.42.70.150, prepared first:

1. QS/400 at 93c6350f installed (`STRTCPSVR SERVER(*QAP)`; Verify... says
   "Version 6"), with the Phase 7 setup done (profiles `BOB` and `ANN`), and
   `BOB` and `ANN` each signed on once in 5250, so their own message queues
   exist;
2. `fixtures/live/msgs-setup.sh` put in `/home/QSECOFR/` and run as
   `QSECOFR`: `QSH CMD('/home/QSECOFR/msgs-setup.sh')`. It makes
   `QGPL/QSA9FAIL`, an ILE CL program that ends with `CEE9901`, and
   excludes `ANN` from `QSYSOPR`; its header says how to undo both;
3. `STRSBS QSYS/QBATCH`, for the batch job below;
4. this program installed by its installer, signed on as `BOB`, with
   `QSACCESS_LOG=trace`, and a 5250 session of `QSECOFR`'s open beside it.

Then:

- [ ] **Messages, and an arrival from 5250.** Messages (General) opens on My
  messages (BOB), newest first, `QAD9001` with the count. In the 5250
  session, `SNDMSG MSG('Hello from 5250') TOUSR(BOB)`: within a second the
  message is at the top of the list (Type Informational, From user
  `QSECOFR`); with the default options it opens no Message Arrived.
  `SNDMSG MSG('Coffee?') TOUSR(BOB) MSGTYPE(*INQ)`: it comes in bold,
  Message Arrived opens (below), the status bar says Messages waiting: 1
  more than before, and a click on it opens Messages.
- [ ] **`QSYSOPR` opted in.** Messages > Options: System operator is offered
  (BOB may read it); turn it on, OK. Messages' queue list has System
  operator; the log says `QAD9010` for `QSYSOPR`. Sign on as `ANN` instead
  (Sign Off, Sign On): Options says ANN may not display `QSYSOPR`'s
  messages and the box is dimmed; with the option set on in `settings.toml`
  by hand for this system, signing on as `ANN` gives `QAD9012` and the
  option turns off. Sign on as `BOB` again.
- [ ] **A batch job's inquiry answered.** In 5250,
  `SBMJOB CMD(CALL PGM(QGPL/QSA9FAIL)) JOB(QSA9FAIL) JOBQ(QBATCH) INQMSGRPY(*RQD)`.
  `WRKACTJOB`: `QSA9FAIL` in `MSGW`. Here, `CEE9901` arrives on `QSYSOPR`
  (Message Arrived, and in bold in Messages on System operator). Reply `X`:
  the system's `CPF2420` as it sent it, and the job still waits. Reply `C`:
  `QAD9002`, the inquiry is no longer bold, its reply `C` shows in the
  detail pane, a `Reply` follows it on the queue and links back to it, and
  the job ends (`WRKACTJOB`; its job log has `CEE9901`).
- [ ] **A break-mode holder beside the watch.** In 5250,
  `CHGMSGQ MSGQ(QSYSOPR) DLVRY(*BREAK)` and stay signed on. Submit
  `QSA9FAIL` again: the 5250 session breaks with the message as it always
  does, and here it arrives on `QSYSOPR` too; nothing is refused. Answer it
  here (`C`). `CHGMSGQ MSGQ(QSYSOPR) DLVRY(*HOLD)` afterwards.
- [ ] **Send Message with an inquiry.** Messages > File > Send Message...
  To user profile `QSECOFR`, "Can you restart the writer?", Inquiry: Send
  gives `QAD9004` and "Waiting for reply...". In 5250 `DSPMSG`, answer it
  "Done". Here the dialog shows "Reply received from QSECOFR: Done"; on My
  messages the sender's copy shows the reply and links to the `Reply`.
  Send another, close the dialog, answer in 5250: the reply still arrives
  in Messages.
- [ ] **Remove.** Select two informational messages and an unanswered
  inquiry, Remove: the separate confirmation says the sender gets no reply;
  Remove gives `QAD9003`. Remove All Answered and Informational: the count
  in the confirmation is what goes; inquiries waiting stay.
- [ ] **`ENDJOB` of the shared connection's job.** Messages, then
  `WRKACTJOB` in 5250: end the shared connection's `QAPCNN` job (Run
  Command's `JobInfo` names it, or the log line at sign-on) with
  `ENDJOB ... OPTION(*IMMED)`. Here: the session ended by an operator
  (`TCP1A54` as sent) with Sign On...; meanwhile `SNDMSG MSG('While away')
  TOUSR(BOB)` in 5250. Sign On: the log says `QAD9010` for both queues
  and `QAD9013` with 1; 'While away' is in Messages once.
- [ ] **A dropped network connection.** Disable the network adapter for 30
  seconds; meanwhile send two messages to BOB from 5250. Enable it: the
  session resumes (the log says so), `QAD9013` with 2 on the status bar,
  each message once in Messages and the count right. Repeat with an
  inquiry sent in the gap: Message Arrived opens for it once.
- [ ] **Printer Output live.** Printer Output with User `*CURRENT` (the
  default): `QAD9020` on the status bar, Auto-refresh dimmed. In 5250, as
  BOB, `DSPLIB QGPL OUTPUT(*PRINT)`: the file appears within a second, with
  no refresh. `HLDSPLF` it: its status turns Held in place. `DLTSPLF` it:
  it goes. Choose output queue `QGPL/QPRINT` with User `*ALL`: watched
  too. Set User `*ALL` with Output queue `*ALL`: `QAD9021` and the list
  refreshes on the Auto-refresh timer. The trace shows no
  `ListSpooledFiles` between changes while watched.
- [ ] **Message Arrived does not take the focus (by hand).** In a 5250
  session of this program, type steadily (a long `SNDMSG` text, say) while
  someone sends `BOB` an inquiry from another session. Message Arrived
  appears on the main monitor without the keyboard leaving the 5250
  session: no typed character goes to Message Arrived; the taskbar entry
  flashes (Windows) or the window is marked urgent (X11). Several
  inquiries queue in it with Previous and Next. Check on Windows, on Linux
  under X11, and under Wayland, where the compositor decides: note what
  it did (focus kept or taken) in the run's record.
- [ ] **Nothing secret logged.** Search `qsaccess.log` for the passwords
  used and for any 64-hexadecimal-character run: none. Message texts are
  not in it either (a watch's event is logged by kind only).

Against a version-5 server (QS/400 at 58e189df, if one is at hand; the
automated tests cover a session that agrees version 5 with the current
engine): Messages is dimmed with `QAD5001`, Printer Output polls as before,
the editor's changed-on-system check uses the last-change time, and
everything earlier works as before.

## Live acceptance (QAP versions 8 and 9, qap-client)

For `qap-client` alone: the desktop app does not call these services and
still agrees version 6, so nothing in its windows changes. On QS/400 at
7f82a43f (`QSHQ400`, at 10.42.70.150), prepared first:

1. QS/400 at 7f82a43f installed (`STRTCPSVR SERVER(*QAP)`; the check below
   prints "QAP version 9"), with the Phase 7 setup (library `HLINV` and its
   source file `QRPGLESRC` with member `ASSETS`), and `CWELLMAN` allowed to
   change `HLINV/QRPGLESRC`;
2. the system in this user's System Configurations, as the other examples
   use it, and a 5250 session of `CWELLMAN`'s open beside it.

Then, from `qsaccess/examples/qap_v9_check.rs` (it adds members
`ASSETSCPY`, `COMPILE42` and `DBGASSETS` to `HLINV/QRPGLESRC` and removes
them at the end; programs go to `QTEMP`):

```sh
QSA_CHECK_PASSWORD=... cargo run -p qsaccess --example qap_v9_check -- 10.42.70.150 CWELLMAN
```

- [ ] **`ReadMember`.** It prints every record of `HLINV/QRPGLESRC ASSETS`
  with its sequence number and date. In 5250,
  `DSPPFM FILE(HLINV/QRPGLESRC) MBR(ASSETS)`, F20 to the right for the
  sequence numbers and dates: each matches.
- [ ] **`WriteMember` back.** PASS: `CPYSRCF` (`CopyMember`) to
  `ASSETSCPY` first, the records written back, then both members read and
  compared record by record: identical, dates unchanged. QS/400 ships no
  `CMPPFM`, so the check compares them itself, as QS/400's QEMU step 160
  does; on a system that has it, `CMPPFM NEWFILE(HLINV/QRPGLESRC)
  NEWMBR(ASSETS) OLDFILE(HLINV/QRPGLESRC) OLDMBR(ASSETSCPY)` while the
  copy is there says the same.
- [ ] **`Compile`.** PASS: `CRTBNDRPG` of `COMPILE42` (QS/400's
  `tools/conformance/qap/COMPILE42.rpgle`) answers one diagnostic at line
  42; its ID (`RNF7416`) and text are those of the compile listing
  (`WRKSPLF`, the `COMPILE42` listing).
- [ ] **`ListJobs`.** PASS: `*CURRENT` includes the connection's job as
  `JobInfo` names it; `WRKUSRJOB` in 5250 shows the same `QAPCNN` job.
- [ ] **`Debug`.** PASS: `Start Own`, `AddProgram` of `QTEMP/DBGASSETS`
  (the conformance `ASSETS`, `DBGVIEW(*SOURCE)`), a breakpoint on line 11
  landing on 12, `CALL` through `RunCommand`: `Stopped` at line 12 on the
  receiver, `Variables` lists `COUNT`, `NAME` and `ASSET`, `Resume Go` and
  the `CALL`'s reply. `DebugEnded` follows at the sign-off (`TCP1A76`):
  by the spec neither `Resume` nor `End` sends it.
- [ ] **Nothing secret logged.** No password and no 64-hexadecimal-character
  run in the output.

## Keyboard: Right Ctrl

Right Ctrl is told apart from left Ctrl by FLTK's key codes, which FLTK
reports on Windows (the extended-key flag), X11 (`Control_R`) and Wayland (the
xkb keysym `Control_R`). It acts on release, and only when no other key went
down while it was held. Check it by hand on each platform (step 4 above).

## Installer: Windows

`ci/installer-test-windows.ps1` runs the whole cycle silently and checks every
step. In a fresh Windows 11 VM, copy the installer and the script in, then:

```
powershell -ExecutionPolicy Bypass -File installer-test-windows.ps1 -Installer QS400-Access-V1R2M0-windows-x86_64.exe -Scope user
powershell -ExecutionPolicy Bypass -File installer-test-windows.ps1 -Installer QS400-Access-V1R2M0-windows-x86_64.exe -Scope machine   # as administrator
```

It snapshots the registry keys and folders an install can touch, then:

1. a damaged copy of the installer exits 2 and changes nothing;
2. silent install exits 0: program, uninstaller, manifest, version information,
   `.qsd`/`.qsp`/`.qkm`/`.qsm`/`.qdx` ProgIDs and open commands, Start menu and desktop
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
and leave `Documents\QS400 Access for Desktop\Sessions` alone.

## Installer: Linux

Build the installer in the pinned glibc 2.31 image, then run the scripted
test in fresh Ubuntu 24.04 and AlmaLinux 9 containers (both pinned by
digest), as root for All users and as an ordinary user for Just me:

```
ci/dist-linux.sh
ci/installer-test-linux-containers.sh target/dist/QS400-Access-V1R2M0-linux-x86_64
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
