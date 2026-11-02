# Connecting to QS/400

QS/400 Access for Desktop reaches a QS/400 system on two kinds of port, both
set on the **Ports** tab of the system's properties (System Configurations >
Edit...):

| Port | Default | Used for |
|---|---|---|
| Telnet | 23 | 5250 display and printer sessions (TN5250E, RFC 4777) |
| Telnet (TLS) | 992 | The same, when **Use TLS for all connections** is ticked |
| QAP | 2320 | Everything else: sign-on, password changes, and the functions to come |

## Phases and QAP versions

Each desktop phase after the first four needs QS/400 to ship a QAP version
first, so the phases follow the versions:

| Phase | QAP version | What |
|---|---|---|
| 5 | 2 | Run Command (`Command`), the password expiry warning (`PasswordStatus`), 5250 automatic sign-on by ticket (`TelnetTicket`) |
| 6 | 3 | Printer Output (`Spool`) |
| 7 | 4 | Run SQL, Schemas and Data Transfer (`Sql`) |
| 8 | 5 | Integrated File System (`Ifs`) |
| 9 | 6 | Messages (`Messages`) |
| 10 | none | 5250 Console and Control Panel (the service console, not QAP) |

## QAP

QAP, the Quartz Access Protocol, is how the program talks to QS/400 for
everything except 5250 sessions. Its specification is
[`qap-protocol-spec.md`](qap-protocol-spec.md), a read-only copy of QS/400's
`docs/QAP.md` at the commit the workspace pins; its first line names that
commit. It is never edited here: a change goes to QS/400 first, and the copy
is refreshed in the same commit as the re-pin (`ci/spec-mirror.sh` checks
both).

- **Versions.** The program offers QAP versions 1 to 3, and the server
  picks the highest both speak. Version 2 adds Run Command (the `Command`
  service: run, check and describe CL commands), the password expiry
  warning (`PasswordStatus`) and 5250 automatic sign-on by ticket
  (`TelnetTicket`). On a system whose QAP server speaks only version 1,
  those are disabled, with `QAD5001` saying which version they need and
  which the system has; nothing of version 2 is ever sent to it, and
  sign-on and everything else work as before. Version 3 adds Printer
  Output (the `Spool` service), likewise disabled with `QAD5001` on a
  system that speaks only version 1 or 2.
- **Requests side by side.** One connection carries every request of a
  system's session. The server runs commands one at a time, in the order
  they came, and everything else beside them, so a reply can come back
  before one asked earlier: each is matched to its request by ID. Short
  requests time out; a command that runs and a spooled file being read run
  until they end or are cancelled, and nothing cancels them by itself.
- **TLS 1.3 only.** QAP has no plain mode and no older TLS version. A
  connection that cannot agree on TLS 1.3 fails with `QAD4054`.
- **The server's certificate.** The QAP server presents the certificate
  assigned to application ID `QS400_ACCESS_SERVER` in the system's
  certificate manager, and refuses to start until one is assigned. A
  certificate this computer does not trust is shown in **Certificate Not
  Trusted** (`QAD1011`), as for 5250 sessions over TLS; **Trust for this
  system** adds it to the system's CA list (the **Security** tab).
- **Starting the server.** On the system, `STRTCPSVR SERVER(*QAP)` starts it
  and `ENDTCPSVR SERVER(*QAP)` ends it. `CHGQAPA` sets whether it starts with
  TCP/IP (`AUTOSTART`), its port (`PORT`, 2320 unless changed) and how long a
  session may wait without a connection (`IDLETIMO`). A server that is not
  started, or not on the port configured here, gives `QAD4053`.

## Signing on

Select the system in the task pane, then **Actions > Sign On...**. The status
bar shows *system: Signed on as PROFILE* once signed on.

- The system applies the same checks as its 5250 sign-on display: the limit
  of sign-on attempts (`QMAXSIGN`) and what happens when it is reached,
  disabled profiles, profiles with password `*NONE`, and expired passwords.
  Each attempt is audited as a display sign-on would be.
- When the system refuses, its own message is shown as it sent it, with its
  message ID (for example `CPF1107`); **Details...** shows its second-level
  text. The status bar and the log carry `QAD4021` with the same text.
- An expired password (`QAD4022`) offers **Change Password...**, which changes
  it and signs on in one step.
- Passwords are never saved and never written to the log, and neither is the
  session's token. Password fields are cleared as soon as they are read. The
  Connection tab of a system configuration keeps only a **Default user ID**,
  which the Sign On dialog starts with.
- One sign-on per system per run. The first task that needs a system signs
  on (or **Actions > Sign On...** does), and every task after it shares that
  QAP session and its connection, each request with its own ID on it. A
  dropped connection resumes the session quietly. QS/400 answers a
  connection's requests in the order they arrive, so while one task's
  command runs (a `DLYJOB`, say), another task's request on the same system
  waits for it to end. The program asks for a sign-on again only when the session is
  gone: `SessionExpired` or `TCP1A53` on resume, or `SessionEnded` from the
  system.
- Releases before systems file version 4 could save passwords in the
  platform credential store. The first start after the upgrade deletes them
  and logs how many: on Windows from Credential Manager; on Linux with
  `secret-tool` (libsecret) when it is installed. Without it the log says so,
  and any left can be removed with the desktop's password manager (service
  *Quartz Systems QS/400 Access*).

## Run Command

**Run Command** in the task pane's General group opens a window for the
selected system, signing on first if this program has no session on it.
It needs QAP version 2; on an older system the window says so (`QAD5001`)
and only Close works.

- **Command line.** Type a CL command, in the system-output font. Up and
  Down recall the last 50 commands run on that system (kept in
  `settings.toml`); a command that gives a password parameter
  (`PASSWORD(...)`, `NEWPWD(...)` and the like) is never kept, and the log
  shows such values as `*`. Enter runs the command.
- **Run** runs it in the session's connection job, `QAPCNN` in `QUSRWRK`,
  as the signed-on profile with its job description's library list. The
  results show the command, then every message it sent, oldest first, as
  the system sent them; the job line shows the job's name, user (the job's
  own, `QUSER`) and number. Each run adds to the results until **Clear**.
  Select a message's ID for its second-level text.
- **How it ended**, on the status line: `QAD5010` completed, `QAD5011`
  ended with an error (the escape message is the last one listed),
  `QAD5012` cancelled, `QAD5013` not found. Not authorized shows the
  system's escape message itself. A command that can only run
  interactively ends with an error and the system's escape message.
- **Cancel** ends the command while it runs (a program or `DLYJOB` ends
  where it is; a command the system carries out in one piece finishes
  first). QS/400 answers a cancelled request without the command's
  messages, so none are listed for it.
- **Check** checks the command without running it and lists the check's
  messages; `QAD5020` when it has errors.
- **Prompt** (F4) builds the command from its definition, below.

### Prompt

Prompt (F4, or the Prompt button) asks the system for the command's
definition (`DescribeCommand`, with any `CHGCMDDFT` defaults applied, as F4
on a 5250 display shows them) and shows it as a form titled with the
command's text and name. Definitions are kept for the rest of the session.
Values already typed on the command line are filled in.

- One row per parameter: its prompt text, keyword and a value field as long
  as the value may be, with the default filled in. Special values and the
  values allowed are in an editable list. Required parameters are in bold.
- A qualified name's parts (name, then library) are indented rows under the
  parameter; a parameter made of elements shows them as a group of rows. A
  parameter that takes a list shows one value and a **+** button for
  another, up to the most it takes.
- **Additional Parameters** (F10) shows the parameters the system keeps
  out of the first view. Constants are never shown.
- **?** on a row, or F1 in its field, shows that value's help; **Help** shows
  the command's. QS/400 ships no help text for commands yet, so these show
  the prompt text and the values allowed, with a note that no help is
  available.
- **Show Command String** writes the command the form makes: only values
  that differ from their defaults, in the definition's order; a qualified
  name as `LIB/NAME`, without the library when it is the default `*LIBL`;
  elements in parentheses; list values separated by blanks; text quoted
  where CL needs it.
- **OK** checks the command first (`CheckCommand`). Errors are listed in the
  dialog with `QAD5020` and it stays open; a command without errors goes to
  the command line, ready to run. **Cancel** keeps nothing.
- The parameters scroll inside the dialog; it never grows past the screen.

## 5250 automatic sign-on

A display session can sign on by itself, without a password: **Sign on
automatically** in Session Properties (`auto_sign_on` in the `.qsd` file's
`[session]`, off unless set). It needs Telnet over TLS and QAP version 2;
otherwise the check box is disabled and says why (`QAD5030`, or `QAD5001`
with the version the system has).

- At each connect the session gets the system's QAP session (signing on
  first if this program has none) and asks it for a one-time ticket
  (`TelnetTicket`): 256 random bits for the signed-on profile, good for one
  sign-on within 60 seconds. A new one is asked for at every connect.
- The ticket goes where RFC 4777 puts the password: the session sends the
  user variable, the random-seed variable present and empty, and the
  password variable carrying the ticket as 64 hexadecimal characters. A
  password is never sent this way.
- The ticket is wiped once sent. It is never logged (the negotiation trace
  shows only that one was sent), and never recorded: recordings hold 5250
  records, not the Telnet negotiation.
- When the system will not sign the profile on automatically, the ticket is
  refused before the connection (`TCP1A55`, shown as the system sent it) and
  the session shows the sign-on display. The system decides with system
  value `QRMTSIGN`: `*VERIFY` or `*SAMEPRF` allow it; systems installed
  before QS/400 8306ec41 keep `*FRCSIGNON`, which turns it off.
- When the system refuses the ticket at the Telnet server (used, expired,
  not over TLS), it shows its sign-on display with no message; the session
  says so with `QAD4040`.
- Macros and Session Manager work as before: a macro that waits for the
  sign-on display waits in vain on a session that signs on by itself, so
  record such macros with automatic sign-on off, or from the first menu.

## Password expiry warning

After each sign-on, and each time a session is resumed on a new connection,
the program asks the system when the signed-on profile's password expires
(`PasswordStatus`, QAP version 2). When that is within the system's warning
period (`QPWDEXPWRN`), it shows `QAD4031` with **Change Password...**, once per
session. The expiry is the profile's last password change plus its interval
(`PWDEXPITV`, or `QPWDEXPITV` when that says `*SYSVAL`), counted on the
system's clock. A password that never expires, or a system on QAP version 1,
shows nothing.

## Changing a password

**Actions > Change Password...** changes the password of the signed-on profile
(its user ID cannot be changed in the dialog). Before signing on it changes
the password of the user ID entered and signs on. The new password is typed
twice; when the two differ nothing is sent (`QAD4027`). The system's password
rules decide whether the new password is accepted, and its message is shown as
sent. On a system with a password validation program, a refusal from that
program comes back as `CPF22C6`, shown as it is.

## When the connection or the session ends

- **A quiet close.** The program sends a keep-alive after a minute without
  traffic. If the system still closes the connection (it closes one after five
  minutes without traffic), the program reconnects and resumes the session
  without asking. A session waits for a new connection for the system's idle
  timeout (`CHGQAPA IDLETIMO`); after that, resuming fails and the program asks
  you to sign on again (`QAD4026`).
- **An operator ends the session.** The program shows `QAD4024` with the
  system's message and offers **Sign On...**.
- **The QAP server ends.** The program shows `QAD4025` with the system's
  message. Every session on the system ends with the server; nothing
  reconnects by itself. **Reconnect** connects again and asks you to sign on.
- **Actions > Sign Off** ends the session (`QAD4028`). Closing the program
  without signing off leaves the session on the system until its idle
  timeout.

## Verify Connection

**System Configurations > Verify...** checks the selected system's connection
and shows each check as **OK**, **Failed** or **Skipped**, with a detail.
Double-click a failed check for its message. The checks, in order:

| Check | Fails with |
|---|---|
| Name lookup: the system's name (or its fixed IP address) resolves | `QAD1003` |
| Telnet port: a TCP connection to the Telnet port | `QAD1002`, `QAD1004` |
| Telnet TLS port: a TLS handshake and certificate trust, when the system uses TLS for 5250 sessions; skipped otherwise | `QAD1010`, `QAD1011` |
| QAP port: a TCP connection to the QAP port | `QAD4053`, which says the server may not be started (`STRTCPSVR SERVER(*QAP)`) or may refuse to start until a certificate is assigned to `QS400_ACCESS_SERVER` |
| QAP TLS 1.3 and certificate: the handshake and certificate trust | `QAD4054`, which says the server accepts TLS 1.3 only; `QAD1011` |
| QAP Hello: the protocol version, system name and release | `QAD4055`, `QAD4060` |
| Test sign-on (optional): signs on with the user ID and password entered, then signs off at once; skipped when no user ID is entered | `QAD4021` with the system's message, `QAD4022` |

A check that cannot run because an earlier one failed is skipped. At the end
the status line shows `QAD4050` (everything passed), `QAD4051` (only the test
sign-on failed) or `QAD4052` (a required check failed). When a certificate was
not trusted, **Certificate Not Trusted** is offered, and the checks run again
if you trust it.

The test sign-on is a real sign-on. A failed one counts toward the system's
`QMAXSIGN` limit, which can disable the profile, and is audited like any
sign-on; the dialog says so beside the fields.
