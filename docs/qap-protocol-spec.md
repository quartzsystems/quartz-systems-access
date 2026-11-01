# QAP: Quartz Access Protocol, version 1 (draft)

QAP is how QS/400 Access talks to QS/400 for everything except 5250 and printer sessions, which stay on TN5250E (RFC 4777). It replaces the plan to clone the host-server protocol. It is an original Quartz design: nothing in it is taken from the vendor's host-server protocols, JTOpen or any vendor-authored source.

This draft settles the decisions below. Anything it leaves open is listed under Open questions; the first agent to reach one stops and asks Cody.

## Decisions

| Topic | Decision |
| --- | --- |
| Transport | TCP, one port, default **2320**, configurable on both ends |
| Security | TLS 1.3 only, no plaintext mode. Server certificate from the QS/400 certificate manager, application ID `QS400_ACCESS_SERVER`. The client validates it with its existing untrusted-certificate trust flow |
| Framing and encoding | Wire, the same framing and serialization QSI uses. If Wire cannot express something below, stop and ask; do not invent a second encoding |
| Multiplexing | One connection carries many concurrent requests. Every request has a service ID and a request ID; replies carry the request ID. No port mapper and no per-service ports |
| Versioning | Client hello sends the range of protocol versions it supports; server picks one or refuses. Version 1 is this document |
| Server jobs | Listener job `QAPD` in subsystem `QSYSWRK`; one job per connection in `QUSRWRK`, running as `QUSER` until sign-on and as the signed-on user profile after it |
| Server control | `STRTCPSVR SERVER(*QAP)`, `ENDTCPSVR SERVER(*QAP)`, autostart attribute; shown on Navigator's Servers page |

## Phase 0 (this round): connection, sign-on, server information

**Handshake.** After TLS, the client sends `Hello`: supported version range, client product name and version, requested capabilities. The server replies `HelloReply`: chosen version, system name, release (`V1R0M0`), the services and capabilities it offers, the sign-on methods it accepts, server time and time zone, and the system CCSID. A version mismatch gets an error reply, then the server closes.

**Sign-on (`signon` service).**

- `SignOn{user, password}` — sent inside TLS. The server applies the same rules as a 5250 sign-on: `QMAXSIGN` and its action, disabled profiles, password expiry, `*NONE` passwords refused. Failed attempts write the audit journal entry a failed 5250 sign-on writes.
- Success returns a **session token** (256 random bits, opaque, stored server-side only), its idle timeout, and the user's attributes the client needs: user profile name, text, CCSID, date and time format, home directory, and special authorities.
- An expired password returns status `PasswordExpired` and no token. The client must then send `ChangePassword{user, old, new}`, which applies the system's password rules and, on success, signs on and returns a token as above.
- `ChangePassword` is also available to a signed-on session.
- `Resume{token}` re-attaches a new connection to an existing session within its idle timeout. `SignOff` ends the session and invalidates the token.
- Nothing before a successful sign-on may run as any profile other than `QUSER`, and nothing other than `Hello`, `SignOn`, `ChangePassword` and `Ping` is accepted before it.

**Server information (`system` service).** `Ping` (allowed before sign-on; returns server time), `SystemInfo` (system name, release, serial, model, IPL time, installed licensed programs relevant to the client).

**Errors.** Every reply carries a status. A failure also carries a QS/400 message ID, its substitution data, and the first- and second-level text in the user's language, so the client can show the server's message without carrying message files. The client shows its own `QAD` messages only for client-side failures (DNS, TCP, TLS, version).

## Later phases (spec now, build later)

The client agent inventories which host-server functions QS/400 Access was going to use, and adds each as a QAP service with its requests and replies to this document before building it. Expected, in this order: remote command (run CL, return messages and the job), spooled files (list, read as SCS, text or PDF, hold, release, delete, move), SQL, IFS files, message queues. Optional: a one-time `TelnetTicket` from a signed-on session that the Telnet server accepts for 5250 auto sign-on, which would remove the need for password substitution on Telnet.

## Open questions

1. The `QAPD` job and `*QAP` server names, and port 2320. Confirm or change before the server ships.
2. Whether QS/400 Access must still reach the vendor's own systems for anything other than 5250 and printing. This spec assumes no.
