# Changelog

One section per release, newest first, headed with its version (the one in
VERSION) and date; a release's section is its GitHub release's notes. Work
not yet released goes under Unreleased, which `ci/version.sh prepare X.Y.Z`
turns into X.Y.Z's section (see docs/releasing.md).

Each section has the changes to QS/400 Access for Desktop, then the changes
to the public API of the crates other projects depend on by git (QS/400 pins
them to a commit) and removals of crates that were published here, under
each crate's name. Breaking changes are marked **Breaking**.

## Unreleased

### qap-client

QAP versions 8 and 9 (drafts), for Code for QS/400; QS/400 pinned at
f6cb4aa2. QS/400 Access for Desktop does not use them and still offers
versions 1 to 6.

- `OFFERED_VERSION` (6), what `Options::default()` now offers instead of
  `proto::VERSION` (9 at this pin), and `Target::with_max_version` to offer
  more. An operation the agreed version does not define is refused before
  it is sent with `Error::Unsupported`, which names the version it needs.
- `Requests`, the request paths `Client` and `Attached` share, with
  `Client::request_here` (the connection open now, never a new one).
- `develop` (version 8's `Develop`): the `Develop` trait with
  `get_library_list`, `set_library_list` and `compile` sent as `RunCommand`
  is (the lane), the lists, `describe_object`, `read_member` (a split reply
  joined whole), `write_member` (the whole record list, sent in `pieces` of
  at most one frame each), the member commands, `WriteOptions` and
  `CompileOutcome`. `Compile` has no reply timeout (`runs_long`).
- `jobs` (version 8's `Jobs`): the `Jobs` trait with `list_jobs`,
  `all_jobs`, `job_log`, `hold_job`, `release_job` and `end_job`.
- `Client::list_routines`, `all_routines` and `list_indexes` (`Sql` 12 and
  13).
- `debug` (version 9's `Debug`): the `Debugger` trait, one method per
  operation, `start` handing back a `DebugSession` whose receiver gets
  `DebugEvent::Stopped` and `DebugEvent::DebugEnded` from the connection's
  thread.
- `retryable` also takes the version 8 lists, `DescribeObject`,
  `ReadMember`, `ListJobs` and `JobLog`.

## 1.1.0 (2026-11-04)

### QS/400 Access for Desktop

First release, V1R0M0, for QS/400 over QAP versions 1 to 6.

- 5250 display sessions over Telnet or Telnet TLS (TN5250E), with keyboard
  maps, the keypad, copy and paste, Print Screen, clickable function-key
  hotspots, macros, automatic sign-on by QAP ticket, and `.qsd` session
  files in Session Manager.
- 5250 printer sessions (`.qsp`) writing PDF or printing to a system
  printer, with spooled jobs, a test page and reconnecting.
- System Configurations with Verify Connection, Sign On and Change Password
  over QAP, one shared connection per system, and the password expiry
  warning. Passwords are never saved.
- Run Command with Prompt (F4), Printer Output with Output Queues and the
  spooled file viewer, Run SQL, Schemas, Data Transfer to and from the
  system with `.qdx` transfer requests and `qsaccess run`, the Integrated
  File System window with its transfer queue, editor and Properties, and
  Messages with Send Message, Message Arrived and the Messages-waiting
  indicator.
- Installers for Windows (just me or all users) and Linux (glibc 2.31 or
  later, root or user), with upgrade, repair, uninstall and a silent mode.

### tn5250

- **Breaking:** `negotiate::Request` has a new field, `auto_sign_on`: 5250
  automatic sign-on by a one-time QAP Telnet ticket (`AutoSignOn`, with the
  user and a `Ticket`), sent as the `USER` variable, an empty random-seed
  variable and the ticket as 64 hexadecimal characters in the password
  variable. `None` for no automatic sign-on, as before.
- `negotiate::Ticket`, `AutoSignOn`, `parse_is` and `encode_is_with_user`.

### qsa-net

- **Breaking:** `Transport` has a new method, `poll_reads`: a non-blocking
  socket whose reads wait a moment for data, as QAP's connections now read
  (a socket read timeout on Windows can fail with os error 997).
  `is_timeout` also takes os error 997 on Windows as no data yet.
- `info!`, beside `debug!`, `trace!` and `warn!`.
- **Breaking:** `TlsOptions` has a new field, `tls13_only`: offer TLS 1.3
  alone (QAP) instead of TLS 1.2 and 1.3 (Telnet).

### hostsvr-proto

- **Breaking:** removed. QS/400 Access talks to QS/400 over QAP
  (`docs/qap-protocol-spec.md`) instead of the host-server protocol. The
  crate stays in git history (last at `c8bff49`); nothing should pin it.

### pwsub

- **Breaking:** removed; last at `a5040f5`. 5250 automatic sign-on uses a
  one-time QAP `TelnetTicket` instead of a password substitute, so the DES
  and SHA-1 forms have no use. Its fuzz target and RFC 4777 vector fixtures
  went with it.
- Before that, in this release: removed `change`, `Change` and the
  `unverified-change` feature (the unverified change-password forms), and
  `Secret::from_vec`.

## Earlier (unreleased)

### pwsub

- First version: `Scheme`, `Seeds`, `Secret` (with `from_vec`),
  `substitute`, `telnet`, `user_id`, `Error`.
- `change` and `Change` are behind the `unverified-change` feature, off by
  default: the protected-password forms are not yet checked against a real
  system.

### hostsvr-proto

- First version: `frame` (`Header`, `Param`, `Frame`, `frame_length`),
  `mapper` (`Service`, `MapperRequest`, `MapperReply`), `message`
  (`HostMessage`, `from_frame`, `push_into`), `rc` (`ReturnCode`,
  `Class`), `wire`, `DecodeError`, `reply_id`.
- `wire::SIGNON_DATASTREAM_LEVEL` is 5: password levels 0 to 3, no level 4.
- The sign-on exchanges were never added.
