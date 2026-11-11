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

## 1.2.0 (2026-11-11)

### QS/400 Access for Desktop

- The release name always follows the version: the README, the documents and
  the scripts no longer name the first release where they mean this one.
  `ci/version.sh names`, run in CI, fails on a release name that is not
  `VERSION`'s, and `ci/version.sh prepare` rewrites the names written out.
- Certificate Management (the Management group and the Tools menu; it was
  listed as Key and Certificate Management and not available): every
  certificate trusted in any system's configuration, one row each, with its
  subject, issuer, validity, SHA-256 fingerprint and the systems that trust
  it. An expired certificate says Expired in red, and one within 30 days of
  expiring says how many days are left. View, Import (PEM or DER, for the
  systems you tick), Export (PEM), Trusted for and Remove. It edits the same
  CA lists as the Security tab of each system's properties, and the two
  stay in step while both are open. The This computer tab says how many
  certificates the operating system's store supplies. Messages `QAD0040` to
  `QAD0044`.
- A system's CA list may reference a DER certificate file as well as a PEM
  one.
- Help. **Help > Help Topics** (F1) opens a Help window in place of
  `QAD0001`: Contents (the topics as a tree) and Search (plain substring
  over titles and text) on the left, the topic on the right, and Back,
  Forward, Home and Print (the topic as a PDF file). There is a topic for
  each window and task, a keyboard reference with the default map, and a
  Messages reference with the cause and recovery of every `QAD` message,
  made from the catalogue. Help is part of the program: no files on disk,
  no browser, no network.
- F1 in any window or dialog opens that window's topic, and so does the Help
  button of the System Properties, Session Properties and file Properties
  dialogs and of Certificate Management. The windows with a Help menu
  (5250 printer session, Printer Output, Messages) have **Help Topics** on
  it. Two windows keep F1 for the system: a 5250 display session, where F1
  is the system's key and Help is **Help > Help Topics**, and the Prompt
  dialog, where F1 and Help show the system's help for the command.
- The message ID in a message's Details opens the message in the Messages
  reference.
- The main window's **Edit > Cut**, **Copy** and **Paste** are dimmed
  instead of reporting `QAD0001`: only 5250 Console and Control Panel still
  do.
- `QAD0050` (a help topic was not found; the build checks every link and
  every topic a window opens, so it should never be seen).
- `QAD1002` and `QAD1010` name the right tab for **Use TLS for all
  connections** (General, not Security).
- Everything saved in Documents is in one folder, **QS400 Access for
  Desktop**: session files in `Sessions`, printer output in `Printer Output`
  and Print Screen's PDFs in `Screens`. Earlier releases used `Quartz
  Access` for the first two and `QS400 Access` for screens. The first start
  moves what those folders hold into the one folder (a rename; a file whose
  name is taken there is left where it was, and nothing is overwritten),
  Recent Sessions and Session Manager's folders follow, and `QAD2012` says
  so.
- Certificate Management's View also shows a certificate's alternative
  names, its public key, how it was signed and whether it is a certificate
  authority's.
- The Security tab of a system's properties lists each certificate by its
  subject, with its Valid to date, instead of a file's path or "Embedded
  PEM". **Add...** takes a DER file as well as a PEM one and refuses a file
  with no certificate (`QAD0043`), **Remove** takes out one certificate, and
  **View...** shows what Certificate Management's View shows.
- The Macro editor's **Close** asks before unsaved changes are lost: Save,
  Don't Save or Cancel.
- A transfer from the system that fails reports its reason as `QAD7036`,
  and says the output file is as it was. It used to report `QAD7031` with
  "row 1 (line 1 of file)", whatever had gone wrong; `QAD7031` is now for
  transfers to the system only, where a row is refused.
- The Prompt dialog's help has **Help on Prompting**, which opens Help's
  topic on prompting; F1 and Help there still show the system's help.
- The XLSX format is listed as "XLSX workbook" in Data Transfer and in Run
  SQL's Export, without another vendor's product name; the clean-room check
  now refuses that name and product IDs of the other vendor's form.
- Releases carry build provenance: the release workflow asks GitHub for an
  artifact attestation of both installers, which `gh attestation verify`
  checks. `SHA256SUMS` is published as before, and the installers are still
  not code-signed. The README and docs/releasing.md say how to check a
  download both ways.

### qsa-net

- `x509::certificates_in` (the certificates in a PEM or DER file's bytes),
  `x509::is_certificate` and `x509::unix_time` (a time as `CertInfo` shows
  it, in seconds since 1970).
- `Trust::from_der`, and `tls::system_certificate_count` (how many
  certificates the operating system's store supplies).
- `x509::details` and `CertDetails`: a certificate's signature algorithm,
  public key, subject alternative names and whether it is a certificate
  authority's.

## 1.1.1 (2026-11-10)

### QS/400 Access for Desktop

- A click on a row of any list selects it: Session Manager, Printer Output,
  Messages, the Integrated File System, Run SQL's results and the rest.
  Double-clicks and the rows' context menus work with it.
- The main window's File menu has Recent Sessions once, as a submenu, with
  Exit last.
- Open menus and drop-down lists have more room between their outline and
  their items.

### qap-client

QAP versions 8 and 9 (drafts), for Code for QS/400; QS/400 pinned at
7f82a43f. QS/400 Access for Desktop does not use them and still offers
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

First release, V1R1M0, for QS/400 over QAP versions 1 to 6.

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
