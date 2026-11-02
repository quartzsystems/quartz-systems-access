# Connecting to QS/400

QS/400 Access for Desktop reaches a QS/400 system on two kinds of port, both
set on the **Ports** tab of the system's properties (System Configurations >
Edit...):

| Port | Default | Used for |
|---|---|---|
| Telnet | 23 | 5250 display and printer sessions (TN5250E, RFC 4777) |
| Telnet (TLS) | 992 | The same, when **Use TLS for all connections** is ticked |
| QAP | 2320 | Everything else: sign-on, password changes, and the functions to come |

## QAP

QAP, the Quartz Access Protocol, is how the program talks to QS/400 for
everything except 5250 sessions. Its specification is
[`qap-protocol-spec.md`](qap-protocol-spec.md).

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
  session's token. Password fields are cleared as soon as they are read.

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
