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

## Recording a session

1. Start QS/400 Access for Desktop with recording on for the next session:

   ```
   qsaccess --record fixtures/replay/sign-on.qsr
   ```

   or set `QSACCESS_RECORD=fixtures/replay/sign-on.qsr` before starting it.
   Only the next 5250 session opened is recorded.
2. Open 5250 Display for the system and do the session's steps. Type at a
   normal pace; times are recorded but replay ignores them.
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
