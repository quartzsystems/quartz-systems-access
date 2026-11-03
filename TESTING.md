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
- `qsaccess/tests/run_request.rs`: the running instance's session handed to
  `qsaccess run` over the instance endpoint, and exit code 1 for bad
  arguments and request files.
- `qsaccess/tests/secrets.rs` also attaches, runs SQL and hands the session
  over, and searches for the session token too.

CI also runs `cargo fmt --check`, `cargo clippy -D warnings`, the clean-room
check (`ci/cleanroom.sh`), the copyright header check and `ci/deps.sh`
(`qsa-net`, `qap-client`, `qsa-sql`, `qsa-transfer`, `tn5250` and `scs` must
not depend on FLTK).

The clean-room check allows an explicit list of files and no others: each
protocol crate's `wire.rs` (today only `tn5250/src/wire.rs`), and, while QS/400's
version 4 build notes name another platform, the read-only spec mirror
`docs/qap-protocol-spec.md`.
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
| `run-command`, `run-command-results` | New: Run Command, empty and after runs and a check; `run-command-results` also shows CHGAUT and CPYFRMSTMF with the test system's severities 10 and 20 (red from 20). No golden yet. |
| `printer-output`, `printer-output-held`, `printer-output-empty` | New: Printer Output with the default filter (page one of BOB's files on QS/400's test system), with a status filter, and with nothing matching. Times are shown in UTC here, so they match on every machine. No golden yet. |
| `viewer`, `viewer-landscape`, `viewer-find` | New: the spooled file viewer, fit to width, on page 1 of QS/400's test listing `QSYSPRT`, on page 2 of `LANDSCAPE` (198 columns, 15 pitch, 8 lines per inch), and on `QPJOBLOG` finding `TEST0`, every match on the page highlighted and the current one darker. No golden yet. |
| `output-queues` | New: Output Queues listing QS/400's test system's queues in `*ALL`. No golden yet. |
| `printer-output-hold`, `printer-output-move`, `printer-output-delete`, `printer-output-properties`, `printer-output-summary` | New: the hold choice, Move, the delete confirmation listing every file, Properties, and the summary of actions on several files with a refusal as the system sent it. No golden yet. |
| `session-properties-auto-sign-on`, `session-properties-auto-sign-on-disabled` | New: Session Properties with automatic sign-on enabled (TLS, QAP version 2) and disabled without TLS (`QAD5030`). `session-properties-general` changes with them. No golden yet. |
| `password-expiry` | New: the password expiry warning, `QAD4031` with Change Password.... No golden yet. |
| `prompt-simple`, `prompt-qualifiers`, `prompt-list`, `prompt-additional`, `prompt-elements`, `prompt-checked`, `prompt-help` | New: Prompt for QS/400's test commands (`qapd::testing`): CRTLIB; DSPOBJD with a qualified name, then a list with "+"; CRTPF with its additional parameters shown, then its element list changed; DSPOBJD after a failed check; a qualifier's help. No golden yet. |
| `run-sql`, `run-sql-results`, `run-sql-error`, `run-sql-incomplete`, `run-sql-commit`, `run-sql-uncommitted` | New: Run SQL empty; with two result tabs (`HLINV.ASSETS`, NULL notes a grey dash, dates in `*MDY`); with `SQL0104` as the system sent it; with a result cut short by a cancel (`QAD7011`, the tab marked Incomplete); under Read committed with Commit and Rollback enabled after `QAD7020`; and the uncommitted-changes prompt (`QAD7021`). No golden yet. |
| `schemas`, `schemas-columns` | New: Schemas with the test system's schemas, and `HLINV.ASSETS`'s columns. No golden yet. |
| `transfer-download-source`, `transfer-download-columns`, `transfer-download-output`, `transfer-download-fixed`, `transfer-download-query` | New: Data Transfer from the system: each page for `HLINV.ASSETS` with a Where clause, chosen columns and a row limit; the fixed-width widths; a query source. No golden yet. |
| `transfer-upload-input`, `transfer-upload-target`, `transfer-upload-mapping`, `transfer-upload-commit` | New: Data Transfer to the system: each page, the mapping grid with a value that does not fit (`QAD7035`), and commit every 10 batches. No golden yet. |
| `transfer-progress`, `transfer-summary`, `transfer-no-commitment-control` | New: the progress dialog, the summary (`QAD7030`), and `SQL7008` as sent with `QAD7034` and Upload Without Commitment Control. No golden yet. |
| `qsa-install`: `wizard-0-welcome`, `wizard-1-licence`, `wizard-2-install-for`, `wizard-3-location`, `wizard-4-options`, `wizard-5-ready`, `wizard-7-finish`, `uninstall`, `repair` | The licence text, the check box control, and no saved passwords in the Welcome, Uninstall and Repair text; `.qdx` among the file types on Options and Ready. |

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
  runs.** With the long query running: Printer Output lists and opens a
  file; a second Run SQL window queries `HLINV.LOCATIONS`. Neither waits for
  the query (QS/400 lets a plain query take turns; see `docs/QAP.md`'s "Long
  statements").
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
  `qsaccess run assets.qdx` (Linux): no Sign On; progress on stderr and in
  the progress window; `QAD7030` on stdout; exit code 0. Cancel in the
  progress window: exit code 4.
- [ ] **`qsaccess run` without one.** Close the program, run it again: the
  Sign On dialog, then the transfer; exit code 0. Cancel the Sign On: exit
  code 2. A request naming a missing table: `QAD7031`, exit code 3.
- [ ] **A sign-off under a window.** With a Run SQL window open, Actions >
  Sign Off in the main window: the window shows the system's `TCP1A58` and
  offers Sign On...; its results stay.
- [ ] **Nothing secret logged.** Search `qsaccess.log` for the passwords
  used and for any 64-hexadecimal-character run: none.

Against a version-3 server (QS/400 at 3134e9ea, if one is at hand; the
automated tests cover version 2 with QS/400's engine at 8306ec41): Run SQL,
Schemas and Data Transfer are dimmed with `QAD5001`; Run Command works on
the shared connection; everything earlier works as before.

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
