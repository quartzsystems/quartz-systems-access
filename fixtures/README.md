# Fixtures

Recorded 5250 sessions and the screens they must end on. `cargo test -p tn5250
--test replay` replays every recording in `replay/` without a network and
compares the final screen with its golden; CI runs it on Windows and Linux.

- `replay/NAME.qsr`: the recording, JSON lines (format in
  `tn5250/src/replay.rs`).
- `replay/NAME.golden`: the screen it must end on: characters, attributes,
  fields, cursor and keyboard state.
- `console/`: captures of the QS/400 service console, for the terminal
  widget's pixel test (see `console/README.md`).

## Fixtures still to record

The replay test lists these and skips each until its `.qsr` exists:

| Name | Session |
|---|---|
| `sign-on` | Sign on at the sign-on display; stop at the first menu. |
| `sign-on-wrkactjob-f3` | Sign on, type `WRKACTJOB`, Enter, page down once, F3. |
| `screen-27x132` | Start the session at 27×132, sign on, run a command whose panel uses the wide screen (for example `WRKACTJOB`). |

`qs400-signon-24x80` and `qs400-signon-27x132` are already recorded: QS/400's
sign-on display at each size (with Enter pressed on an empty sign-on at 24×80).

The macros in `../macros/` are run against these recordings by
`qsaccess/tests/macro_replay.rs` (a macro named like a recording plays against
it and must reach its golden). `sign-on-wrkactjob-f3.qsm` waits for its
recording.

TODO(Cody): QS/400 is moving its start-up response record to the RFC 4777
layout (reserved field `9000`). The recordings above have the older layout,
which is still read but now logged as a warning. Once QS/400 ships the fix,
re-record `qs400-signon-24x80` and `qs400-signon-27x132` (and any QS/400
fixture recorded before then), regenerate their goldens, and check the log
has no "not in the RFC 4777 layout" line.

`switch-24x80-27x132` is not recorded: it is a synthetic stream made by
`tn5250/tests/replay.rs` (see "Switching screen sizes" there), for the screen
switching between 24×80 and 27×132 in both directions.

## Recording a session

1. Start QS/400 Access for Desktop with recording on for the next session:

   ```
   qsaccess --record fixtures/replay/sign-on.qsr
   ```

   or set `QSACCESS_RECORD=fixtures/replay/sign-on.qsr` before starting it.
   Only the next 5250 session opened is recorded.
2. Open 5250 Display for the system and do the session's steps. Type at a
   normal pace; times are recorded but replay ignores them. Use the keyboard
   only: paste and clicking to move the cursor are not recorded, so a
   session that uses them does not replay.
3. Close the session window. The recording is complete.
4. Write the golden from the replay, then look at it before committing:

   ```
   QSA_UPDATE_GOLDEN=1 cargo test -p tn5250 --test replay
   git diff fixtures/replay/
   ```

   Check that the `chars` grid shows the screen you ended on, and that it
   holds nothing you would not publish (user names on `WRKACTJOB` are fine
   for test profiles; use a test profile, not your own).

Passwords never reach the file: anything typed into a nondisplay field is
written as `X`, and the matching bytes in what was sent are rewritten too. The
length of the password is still visible, so use a test profile's password.

### The systems

- **QS/400 at 10.42.70.150**, port 23 (no TLS listener on 992 yet). Record all
  three fixtures here first. Use a test profile such as `QSATEST`.
- **Your reference system** (the other midrange system). Record the same
  three under `replay/reference/` with the same
  names when the system is up, so the two can be compared; add them to
  `FIXTURES` in `tn5250/tests/replay.rs`.
- **pub400.com.** By hand only, and lightly: a sign-on and one command at
  most, never scripted or repeated in a loop. It is a shared public system.
  Recordings from it carry your pub400 user name; check before committing.

### Without the UI

`cargo run -p tn5250 --example probe -- HOST 23 [--132] --record FILE`
records the sign-on display alone (`--enter` presses Enter on it once). It
cannot sign on. It is how the two `qs400-signon-*` fixtures were made.

## Printer sessions

A printer session's recording (`"kind":"printer"` in its first line) holds the
print records the system sent and the client's answers. `cargo test -p
qsa-print --test pdf_goldens` replays every printer recording in `replay/`
through the printer protocol, checks that the answers match, and checks each
job (`NAME-jobN`) against its goldens in `qsa-print/tests/goldens/`: the text
(`.txt`), the page model (`.pages`) and every page of the PDF (`-N.png`, with
the pinned `pdftoppm`; see `TESTING.md`). Nothing is scrubbed: print data holds
no password. Look at what a spooled file shows before committing it.

`synthetic-printer-test-page.qsr` is synthetic: the built-in test page as a
system would send it, written by
`cargo run -p qsa-print --example synthetic_printer_recording`. It stands in
until recordings from a real system are added.

### Printer fixtures still to record

| Name | Spooled file |
|---|---|
| `printer-joblog` | A job log (`DSPJOBLOG OUTPUT(*PRINT)`), several pages. |
| `printer-wrkactjob` | `WRKACTJOB OUTPUT(*PRINT)`: 132 columns. |
| `printer-qs400-joblog` | The same job log from QS/400, when its Telnet server takes printer sessions. |

### Recording a printer session

On the system (a test profile with authority to the device and the output
queue is enough):

1. Create the printer device, or let the Telnet server create it: virtual
   devices are created on first use when automatic configuration of virtual
   devices is on. To create it yourself:

   ```
   CRTDEVPRT DEVD(QSAPRT01) DEVCLS(*VRT) TYPE(3812) MODEL(1) +
             FONT(011) TRANSFORM(*NO) MSGQ(QSYSOPR) +
             TEXT('QS/400 Access for Desktop test printer')
   ```

   Host print transform must stay off: the client prints SCS only, and
   discards transformed data (`QAD3020`).
2. On the desktop, start the session with recording on, then connect it:

   ```
   qsaccess --record fixtures/replay/printer-joblog.qsr
   ```

   then 5250 Printer for the system, device `QSAPRT01`, and Start. Wait for
   *Ready* (`QAD3001`). Without the UI, the same is
   `cargo run -p tn5250 --example printer_probe -- HOST 23 --device QSAPRT01
   --seconds 120 --jobs target/jobs --record fixtures/replay/printer-joblog.qsr`.
3. Start the writer for the device, if the system did not start it:

   ```
   STRPRTWTR DEV(QSAPRT01)
   ```

4. Print the spooled file to that device's output queue, for example:

   ```
   DSPJOBLOG OUTPUT(*PRINT)
   CHGSPLFA FILE(QPJOBLOG) SPLNBR(*LAST) OUTQ(QSAPRT01)
   ```

   or `WRKACTJOB OUTPUT(*PRINT)` with `OVRPRTF FILE(QPDSPAJB) OUTQ(QSAPRT01)`
   first. Wait until the job appears in Received output, then close the
   session window: the recording is complete.
5. For the line-for-line check, save the system's own text of the same
   spooled file beside the recording as `printer-joblog-job1.system.txt`:

   ```
   CPYSPLF FILE(QPJOBLOG) TOFILE(QTEMP/SPLTXT) SPLNBR(*LAST) CTLCHAR(*NONE)
   CPYTOSTMF FROMMBR('/QSYS.LIB/QTEMP.LIB/SPLTXT.FILE/SPLTXT.MBR') +
             TOSTMF('/home/QSATEST/printer-joblog-job1.system.txt') +
             STMFCCSID(1208)
   ```

   and copy it down (FTP or the integrated file system share).
6. Write the goldens with the pinned poppler (the CI Linux image), and look
   at every page image before committing:

   ```
   docker build -t qsa-ci -f ci/linux-ci.Dockerfile ci
   docker run --rm -v "$PWD:/src" -w /src -e QSA_BLESS=1 qsa-ci \
     cargo test -p qsa-print --test pdf_goldens
   git status qsa-print/tests/goldens fixtures/replay
   ```

If a system's Telnet server refuses printer sessions, keep the probe's
output (the debug log shows every negotiation step) with the fixture notes
instead of a recording.

## Password substitute vectors

`pwsub/tests/vectors.rs` checks every `pwsub/*.vec` here (format in that
file). Three are the RFC 4777 examples (`rfc4777-*.vec`). The others are
exchanges recorded against real systems with a throwaway test profile whose
password is in the file: they show that a real system accepted what
`pwsub` computed. Each starts with a comment saying it is test-only.

### Recording vectors (Cody, against the reference system)

The profile exists only while you record; its password is committed, so it
must never be used for anything else.

1. Signed on as a security officer, look at the password level, since the
   vectors are for that level only:

   ```
   DSPSYSVAL SYSVAL(QPWDLVL)
   ```

2. Create the profile. It can sign on to the host servers and do nothing
   else useful:

   ```
   CRTUSRPRF USRPRF(QSAVEC) PASSWORD(VEC4TEST) USRCLS(*USER) +
             INLMNU(*SIGNOFF) LMTCPB(*YES) PWDEXPITV(*NOMAX) +
             TEXT('QS/400 Access test vectors - delete after use')
   ```

   At level 2 or 3, use a mixed-case passphrase instead, for example
   `PASSWORD('Vec 4 test, mixed Case')`, so the vector covers case and
   spaces.
3. Record a sign-on, then a password change (the tool signs on with the
   new password afterwards to prove it took):

   ```
   cargo run -p hostsvr --example capture_vectors -- HOST --user QSAVEC
   cargo run -p hostsvr --example capture_vectors -- HOST --user QSAVEC --change
   ```

   The tool asks for the passwords, runs the sign-on server's exchange of
   attributes and retrieve sign-on information (and the change-password
   request with `--change`), and writes
   `fixtures/pwsub/capture-<host>-level<N>-{signon,change}.vec` only when
   the system answered with return code 0. Nothing is written for a
   failed exchange; the return code is printed instead.
4. Check `cargo test -p pwsub --test vectors` passes, read the files, and
   commit them.
5. Delete the profile:

   ```
   DLTUSRPRF USRPRF(QSAVEC) OWNOBJOPT(*DLT)
   ```

These are the only files in the repository that hold a seed, a substitute
or a password together; everything the client records or logs otherwise is
scrubbed.
