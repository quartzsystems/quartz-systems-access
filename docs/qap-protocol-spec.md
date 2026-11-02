# QAP: Quartz Access Protocol, version 1 (draft)

QAP is how QS/400 Access talks to QS/400 for everything except 5250 and printer sessions, which stay on TN5250E (RFC 4777). It replaces the plan to clone the host-server protocol. It is an original Quartz design: nothing in it is taken from the vendor's host-server protocols, JTOpen or any vendor-authored source.

This draft settles the decisions below. Anything it leaves open is listed under Open questions; the first agent to reach one stops and asks Cody.

## Decisions

| Topic | Decision |
| --- | --- |
| Transport | TCP, one port, default **2320**, configurable on both ends |
| Security | TLS 1.3 only, no plaintext mode. Server certificate from the QS/400 certificate manager, application ID `QS400_ACCESS_SERVER`. The client validates it with its existing untrusted-certificate trust flow |
| Framing and encoding | Wire, from the QS/400 repository's `ipc` crate, the same framing and serialization QSI uses. Types below are Wire types; enums use `wire_enum!`. If Wire cannot express something below, stop and ask; do not invent a second encoding |
| Where the code lives | `qap-proto` (envelope and message types) lives in the QS/400 repository beside `ipc`. QS/400 Access pins both by commit |
| Multiplexing | One connection carries many concurrent requests. Every request has a service ID and a request ID; replies carry the request ID. No port mapper and no per-service ports |
| Versioning | Client hello sends the range of protocol versions it supports; server picks one or refuses. Version 1 is this document |
| Server jobs | Listener job `QAPD` in subsystem `QSYSWRK`; one job per connection in `QUSRWRK`, running as `QUSER` until sign-on and as the signed-on user profile after it |
| Server control | `STRTCPSVR SERVER(*QAP)`, `ENDTCPSVR SERVER(*QAP)`, autostart attribute; shown on Navigator's Servers page |

## Envelope (all versions)

Every frame is one Wire-encoded `Envelope`. The envelope layout never changes between protocol versions; versions change message bodies and add operations.

| Field | Type | Meaning |
| --- | --- | --- |
| `kind` | enum `Kind` | `Request` = 0, `Reply` = 1, `Event` = 2, `Cancel` = 3 |
| `request_id` | u32 | Chosen by the client, non-zero, unique among its outstanding requests on the connection; reusable after the final reply. Events use 0. `Cancel` names the request to cancel |
| `service` | enum `Service` | `Connection` = 0, `SignOn` = 1, `System` = 2. Later phases append values; never renumber |
| `op` | u16 | Operation within the service (tables below) |
| `flags` | u16 bit set | bit 0 `More`: further reply frames follow for this request. All other bits zero in version 1 |
| `status` | enum `Status` | Replies and events only; `Ok` on requests |
| `message` | Option of `ServerMessage` | Present when the reply or event carries a QS/400 message |
| `body` | bytes | The Wire-encoded body for this service and operation; empty when the operation has none |

`ServerMessage` = { `id`: String (seven characters, for example `CPF2204`), `severity`: u8, `data`: bytes (substitution data), `text`: String (first level), `help`: String (second level: cause and recovery) }, in the user's language once signed on, the system's before.

**Status values (u16).** `Ok` 0 · `Error` 1 (the message says what) · `VersionUnsupported` 2 · `NotSignedOn` 3 · `SignOnFailed` 4 · `PasswordExpired` 5 · `NotAuthorized` 6 · `NotFound` 7 · `InvalidRequest` 8 · `Busy` 9 · `Cancelled` 10 · `SessionExpired` 11 · `ServerEnding` 12. Sign-on failures carry the same message a 5250 sign-on would show for the same case, and nothing more.

**Limits.** A frame is at most 1 MiB; larger results are sent as several reply frames with `More` set on all but the last. At most 64 outstanding requests per connection; beyond that the server replies `Busy`. The client sends `Ping` after 60 s without traffic; the server closes a connection after 300 s without frames. Closing a connection does not end the session: it can be resumed until its idle timeout.

**Events (server to client, `request_id` 0).** `Connection` op 100 `ServerEnding` (status `ServerEnding`, message says why; the server closes after sending it) · `SignOn` op 100 `SessionEnded` (status `SessionExpired`, or `Ok` when ended by an operator, with a message).

## Phase 0 (this round): connection, sign-on, server information

**Operations.** Strings are UTF-8. Times are i64 milliseconds since 1970-01-01 UTC. Bodies not listed as a reply are empty.

| Service | Op | Request body | Reply body | Allowed before sign-on |
| --- | --- | --- | --- | --- |
| Connection | 1 `Hello` | `min_version` u16, `max_version` u16, `client_name` String, `client_version` String, `capabilities` list of String | `version` u16, `system_name` String, `release` String, `services` list of `Service`, `capabilities` list of String, `signon_methods` list of enum (`Password` = 0), `server_time` i64, `time_zone` String, `system_ccsid` u16 | yes; must be the first request, `request_id` 1 |
| Connection | 2 `Ping` | — | `server_time` i64 | yes |
| SignOn | 1 `SignOn` | `user` String, `password` String | `SignOnReply` | yes |
| SignOn | 2 `ChangePassword` | `user` String, `old_password` String, `new_password` String | `SignOnReply` | yes |
| SignOn | 3 `Resume` | `token` 32 bytes | `SignOnReply` (same token, timeout restarted) | yes |
| SignOn | 4 `SignOff` | — | — | no |
| System | 1 `SystemInfo` | — | `system_name` String, `release` String, `serial` String, `model` String, `ipl_time` i64, `products` list of { `id` String, `option` String, `release` String } | no |

`SignOnReply` = { `token` 32 bytes, `idle_timeout_secs` u32, `profile` String, `text` String, `ccsid` u16, `date_format` String, `date_separator` String, `time_separator` String, `home_directory` String, `special_authorities` list of String }.

**Rules.**

- A `Hello` that is not the first request, or a second `Hello`, gets `InvalidRequest` and the connection closes. No version in the client's range gets `VersionUnsupported`, then the connection closes.
- `SignOn` applies the same checks as a 5250 sign-on: `QMAXSIGN` and its action, disabled profiles, password expiry, `*NONE` passwords refused, and the same audit journal entries. An expired password gets `PasswordExpired` and no token; the client then sends `ChangePassword`, which applies the system password rules and on success returns a `SignOnReply`.
- `ChangePassword` is also allowed in a signed-on session for that session's own profile.
- The token is 256 random bits, stored server-side only. `Resume` re-attaches a new connection to the session until the idle timeout; after it, `SessionExpired`. `SignOff` ends the session and invalidates the token.
- Anything other than the operations marked "yes" before sign-on gets `NotSignedOn`. Before sign-on the connection job runs as `QUSER`; after it, as the signed-on profile.
- The client never logs or stores passwords or tokens, and clears them from memory after use.

## Later phases (spec now, build later)

The client agent inventories which host-server functions QS/400 Access was going to use, and adds each as a QAP service with its requests and replies to this document before building it. Expected, in this order: remote command (run CL, return messages and the job), spooled files (list, read as SCS, text or PDF, hold, release, delete, move), SQL, IFS files, message queues. Optional: a one-time `TelnetTicket` from a signed-on session that the Telnet server accepts for 5250 auto sign-on, which would remove the need for password substitution on Telnet.

### Inventory

What QS/400 Access was going to reach through the host servers, from its task pane, README and messages, and the service each becomes:

| Function in QS/400 Access | Service |
| --- | --- |
| Run Command task | `Command` (3) |
| Printer Output task: spooled files and output queues | `Spool` (4) |
| Run SQL, Schemas and Data Transfer tasks (Data Transfer downloads with a query and uploads with an `INSERT` run over batches of rows) | `Sql` (5) |
| Integrated File System task | `Ifs` (6) |
| Message queues: the user's own and `QSYSOPR`, inquiry replies | `Messages` (7) |
| Password expiry warning (`QAD4031`), which the phase 0 `SignOnReply` cannot give | `SignOn` op 6 `PasswordStatus` |
| 5250 automatic sign-on without a password on Telnet (the optional ticket) | `SignOn` op 5 `TelnetTicket` |

Not host-server functions, so not in QAP: 5250 display and printer sessions (Telnet), the printer session's message queue (named in the TN5250E startup), the 5250 Console and Control Panel tasks (the service console), and Key and Certificate Management (this computer's trust store).

### Rules for these additions

- Service values append from 3 and are never renumbered. Operations number from 1 within each service; events from 100. `SignOn` continues from 5.
- Every operation below is not allowed before sign-on: before it, each gets `NotSignedOn`.
- Bodies use only Wire types: structs, enums with explicit tags (one byte unless a wider tag is stated), lists, options, bytes, fixed-size byte arrays, integers, UTF-8 strings. Times are i64 milliseconds since 1970-01-01 UTC. A flag is a u8, 1 for yes and 0 for no; any other value gets `InvalidRequest`.
- A request is one frame, so at most 1 MiB. Operations that send more (`WriteFile`, `Execute` with many rows) are repeated by the client over pieces; nothing splits a request. Replies over 1 MiB are split with `More` as in phase 0.
- Lists that can be long take `start` (0-based) and `max` (0 for no limit) and answer `total`, the number of entries matching, beside the entries returned.
- Every operation can also get `Error` (the message says what), `InvalidRequest` (a body that does not decode or a value out of range), `Busy`, and `Cancelled` when cancelled. The tables' Statuses column lists the others.
- Each service, operation, status and event value is introduced by a protocol version (see Compatibility of new values). The services are planned for versions 2 to 6 in the order below; the `SignOn` additions with version 2. A version number is fixed only when QS/400 ships it.

**New status values (u16), appended after `ServerEnding` 12.** `Exists` 13 (the name is taken) · `InUse` 14 (another job holds the object) · `SqlError` 15 (an SQL statement failed; the body is `SqlFailure`, the message the SQL message).

**Common types.**

- `JobId` = { `name` String, `user` String, `number` String (six digits) }.
- `ObjectName` = { `library` String, `name` String }. `library` may be `*LIBL` or `*CURLIB` where a library list search applies.

### Service 3 `Command`: run CL commands (planned version 2)

Commands run in the connection job, as the signed-on profile, with its job description's library list. A command that only runs interactively ends with `Error` and its escape message.

| Service | Op | Request body | Reply body | Allowed before sign-on |
| --- | --- | --- | --- | --- |
| Command | 1 `RunCommand` | `command` String | `CommandReply`: `job` JobId, `messages` list of `ServerMessage` (every message the command sent to its caller, oldest first, second level included) | no |
| Command | 2 `CheckCommand` | `command` String | `messages` list of `ServerMessage` (the syntax check's messages; empty when the command is valid) | no |

| Op | Statuses |
| --- | --- |
| `RunCommand` | `Ok`; `Error` when the command ended with an escape message (the message is that one, and the body is still `CommandReply`); `NotFound` (no such command); `NotAuthorized` (to the command or what it names); `Cancelled` (the command was ended) |
| `CheckCommand` | `Ok`; `Error` when the check found errors (the body is still the messages); `NotFound` |

### Service 4 `Spool`: spooled files and output queues (planned version 3)

- `SpooledFileId` = { `job` JobId, `file` String, `number` u32 }.
- `SpoolStatus` enum: `Ready` 0, `Held` 1, `Open` 2, `Closed` 3, `Writing` 4, `Printing` 5, `Message` 6 (waiting for a reply), `Pending` 7, `Saved` 8.
- `DeviceType` enum: `Scs` 0, `Afp` 1, `Ascii` 2, `Line` 3.
- `SpooledFile` = { `id` SpooledFileId, `status` SpoolStatus, `outq` ObjectName, `user` String, `user_data` String, `form_type` String, `device_type` DeviceType, `pages` u32, `copies` u16, `size` u64 (bytes), `created` i64 }.
- `ReadFormat` enum: `Scs` 0 (the data stream as spooled), `Text` 1 (UTF-8, one line per printed line, form feed between pages), `Pdf` 2.

| Service | Op | Request body | Reply body | Allowed before sign-on |
| --- | --- | --- | --- | --- |
| Spool | 1 `ListSpooledFiles` | `user` String (`*CURRENT`, `*ALL` or a profile), `outq` Option of ObjectName, `status` Option of SpoolStatus, `form_type` Option of String, `user_data` Option of String, `start` u32, `max` u32 | `total` u32, `files` list of `SpooledFile` | no |
| Spool | 2 `ReadSpooledFile` | `id` SpooledFileId, `format` ReadFormat, `first_page` u32 (1-based), `last_page` u32 (0 for the last) | `format` ReadFormat, `pages` u32, `data` bytes (split with `More` when large) | no |
| Spool | 3 `HoldSpooledFile` | `id` SpooledFileId, `immediately` flag (0: at the end of the page) | — | no |
| Spool | 4 `ReleaseSpooledFile` | `id` SpooledFileId | — | no |
| Spool | 5 `DeleteSpooledFile` | `id` SpooledFileId | — | no |
| Spool | 6 `MoveSpooledFile` | `id` SpooledFileId, `outq` ObjectName, `first` flag (1: to the top of the queue) | — | no |
| Spool | 7 `ListOutputQueues` | `library` String (`*ALL`, `*LIBL` or a library), `start` u32, `max` u32 | `total` u32, `queues` list of { `outq` ObjectName, `text` String, `files` u32, `writer` String (empty when none), `held` flag } | no |

| Op | Statuses |
| --- | --- |
| `ListSpooledFiles`, `ListOutputQueues` | `Ok`; `NotFound` (the queue or library); `NotAuthorized` |
| `ReadSpooledFile` | `Ok`; `NotFound`; `NotAuthorized`; `InUse` (still `Open` or `Writing`); `Error` (the format cannot be made from this file's device type, for example `Scs` from `Afp`) |
| `HoldSpooledFile`, `ReleaseSpooledFile`, `DeleteSpooledFile`, `MoveSpooledFile` | `Ok`; `NotFound`; `NotAuthorized`; `InUse` (printing, for delete and move) |

### Service 5 `Sql`: SQL, the catalog and data transfer (planned version 4)

Statements and cursors belong to the connection and end with it; changes not committed are rolled back when it closes. Data Transfer downloads with `Prepare`, `Execute` and `Fetch`, and uploads with an `INSERT` prepared once and `Execute` repeated over batches of rows, each request under 1 MiB.

- `CommitLevel` enum: `None` 0, `ReadUncommitted` 1, `ReadCommitted` 2, `RepeatableRead` 3, `Serializable` 4.
- `Naming` enum: `Sql` 0 (`schema.table`), `System` 1 (`library/file`).
- `SqlValue` enum: `Null` 0, `Integer` 1 { `value` i64 }, `Decimal` 2 { `text` String (exact, `.` as the point) }, `Float` 3 { `text` String (shortest text that reads back as the same value) }, `Text` 4 { `text` String (character and graphic types, converted to UTF-8) }, `Binary` 5 { `data` bytes }, `Date` 6 { `ms` i64 (midnight UTC of the date) }, `Time` 7 { `ms` i64 (since midnight) }, `Timestamp` 8 { `ms` i64 (as if UTC), `micros` u16 (microseconds past `ms`, 0 to 999) }, `Boolean` 9 { `value` flag }.
- `Column` = { `name` String, `label` String, `sql_type` String (for example `DECIMAL`), `length` u32, `scale` u16, `nullable` flag, `ccsid` u16 }.
- `SqlFailure` = { `sqlstate` String (five characters), `sqlcode` i32, `row` Option of u32 (the failing parameter row of an `Execute`) }.
- `TableKind` enum: `Table` 0, `View` 1, `Alias` 2, `PhysicalFile` 3, `LogicalFile` 4.

| Service | Op | Request body | Reply body | Allowed before sign-on |
| --- | --- | --- | --- | --- |
| Sql | 1 `SetOptions` | `commit` CommitLevel, `naming` Naming, `default_schema` Option of String | — | no |
| Sql | 2 `Prepare` | `sql` String | `statement` u32, `columns` list of `Column` (empty when not a query), `parameters` list of `Column` | no |
| Sql | 3 `Execute` | `statement` u32, `rows` list of list of `SqlValue` (one parameter set per row; empty to run once without parameters) | `updated` i64 (rows changed), `cursor` Option of u32 (a query's open cursor) | no |
| Sql | 4 `Fetch` | `cursor` u32, `max_rows` u32 | `rows` list of list of `SqlValue`, `end` flag | no |
| Sql | 5 `CloseCursor` | `cursor` u32 | — | no |
| Sql | 6 `Release` | `statement` u32 | — | no |
| Sql | 7 `Commit` | — | — | no |
| Sql | 8 `Rollback` | — | — | no |
| Sql | 9 `ListSchemas` | `pattern` String (SQL `LIKE`), `start` u32, `max` u32 | `total` u32, `schemas` list of { `name` String, `text` String } | no |
| Sql | 10 `ListTables` | `schema` String, `pattern` String, `kinds` list of `TableKind` (empty for all), `start` u32, `max` u32 | `total` u32, `tables` list of { `schema` String, `name` String, `kind` TableKind, `text` String } | no |
| Sql | 11 `ListColumns` | `schema` String, `table` String | `columns` list of `Column` | no |

| Op | Statuses |
| --- | --- |
| `Prepare`, `Execute`, `Fetch`, `Commit`, `Rollback` | `Ok`; `SqlError` (body `SqlFailure`); `NotFound` (statement or cursor not open); `NotAuthorized`; `Cancelled` (a long `Execute` or `Fetch`; the statement is ended) |
| `SetOptions` | `Ok`; `InUse` (a transaction is open: commit or roll back first) |
| `CloseCursor`, `Release` | `Ok`; `NotFound` |
| `ListSchemas`, `ListTables`, `ListColumns` | `Ok`; `NotFound`; `NotAuthorized` |

### Service 6 `Ifs`: Integrated File System files (planned version 5)

Paths are absolute and UTF-8 (`/home/BOB/a.txt`, `/QSYS.LIB/QGPL.LIB`).

- `EntryKind` enum: `File` 0, `Directory` 1, `Symlink` 2, `Other` 3.
- `Entry` = { `name` String, `kind` EntryKind, `size` u64, `modified` i64, `owner` String, `ccsid` u16, `target` Option of String (a symbolic link's) }.
- `WriteMode` enum: `Create` 0 (the file must not exist), `Replace` 1 (created or emptied first), `Update` 2 (at `offset` in an existing file), `Append` 3.

| Service | Op | Request body | Reply body | Allowed before sign-on |
| --- | --- | --- | --- | --- |
| Ifs | 1 `ListDirectory` | `path` String, `pattern` String (`*` and `?` wildcards), `start` u32, `max` u32 | `total` u32, `entries` list of `Entry` | no |
| Ifs | 2 `GetInfo` | `path` String | `Entry` | no |
| Ifs | 3 `ReadFile` | `path` String, `offset` u64, `length` u64 (0 for to the end) | `size` u64 (the file's size now), `data` bytes (split with `More` when large) | no |
| Ifs | 4 `WriteFile` | `path` String, `mode` WriteMode, `offset` u64, `ccsid` u16 (for a file created; 0 for 1208), `data` bytes | `size` u64 | no |
| Ifs | 5 `CreateDirectory` | `path` String, `parents` flag | — | no |
| Ifs | 6 `Delete` | `path` String (a file, a symbolic link or an empty directory) | — | no |
| Ifs | 7 `Rename` | `from` String, `to` String, `replace` flag | — | no |

| Op | Statuses |
| --- | --- |
| `ListDirectory`, `GetInfo`, `ReadFile` | `Ok`; `NotFound`; `NotAuthorized`; `InUse` (`ReadFile` of a file locked for writing) |
| `WriteFile` | `Ok`; `NotFound` (the directory, or the file for `Update`); `NotAuthorized`; `Exists` (`Create`); `InUse` |
| `CreateDirectory`, `Rename` | `Ok`; `NotFound`; `NotAuthorized`; `Exists` (`Rename` without `replace`) |
| `Delete` | `Ok`; `NotFound`; `NotAuthorized`; `InUse`; `Error` (a directory that is not empty) |

### Service 7 `Messages`: message queues (planned version 6)

- `MessageType` enum: `Informational` 0, `Inquiry` 1, `Reply` 2, `Completion` 3, `Diagnostic` 4, `Escape` 5, `Notify` 6.
- `QueuedMessage` = { `key` 4 bytes, `id` String (empty for an impromptu message), `kind` MessageType, `severity` u8, `text` String, `help` String, `sent` i64, `from_user` String, `from_job` JobId, `needs_reply` flag, `reply` Option of String }.
- `Queue` enum: `Own` 0 (the signed-on profile's), `SystemOperator` 1 (`QSYSOPR`), `Named` 2 { `queue` ObjectName }.
- `Recipient` enum: `User` 0 { `profile` String }, `Queue` 1 { `queue` Queue }.

| Service | Op | Request body | Reply body | Allowed before sign-on |
| --- | --- | --- | --- | --- |
| Messages | 1 `ListMessages` | `queue` Queue, `newest_first` flag, `start` u32, `max` u32 | `total` u32, `messages` list of `QueuedMessage` | no |
| Messages | 2 `ReplyMessage` | `queue` Queue, `key` 4 bytes, `reply` String | — | no |
| Messages | 3 `RemoveMessages` | `queue` Queue, `keys` list of 4 bytes | — | no |
| Messages | 4 `SendMessage` | `to` Recipient, `text` String, `inquiry` flag | `key` Option of 4 bytes (an inquiry's, to find its reply) | no |
| Messages | 5 `WatchQueue` | `queue` Queue | — | no |
| Messages | 6 `UnwatchQueue` | `queue` Queue | — | no |

| Op | Statuses |
| --- | --- |
| `ListMessages`, `SendMessage` | `Ok`; `NotFound` (the queue or profile); `NotAuthorized` |
| `ReplyMessage` | `Ok`; `NotFound` (queue or key); `NotAuthorized`; `InvalidRequest` (not an inquiry, or already answered); `Error` (the reply is not valid for the message) |
| `RemoveMessages` | `Ok`; `NotFound`; `NotAuthorized` |
| `WatchQueue` | `Ok`; `NotFound`; `NotAuthorized`; `InUse` (another job holds the queue in break mode) |
| `UnwatchQueue` | `Ok`; `NotFound` (not watched on this connection) |

**Event (`request_id` 0).** `Messages` op 100 `MessageArrived` (status `Ok`; body `queue` Queue, `message` QueuedMessage), for each queue watched on the connection. A watch ends with its connection.

### `SignOn` additions (planned version 2)

| Service | Op | Request body | Reply body | Allowed before sign-on |
| --- | --- | --- | --- | --- |
| SignOn | 5 `TelnetTicket` | — | `ticket` 32 bytes, `expires_secs` u32 | no |
| SignOn | 6 `PasswordStatus` | — | `expires` Option of i64 (none when the password does not expire), `warning_days` u16 (the system's warning period) | no |

| Op | Statuses |
| --- | --- |
| `TelnetTicket` | `Ok`; `NotAuthorized` (the profile may not sign on automatically) |
| `PasswordStatus` | `Ok` |

A ticket is 256 random bits for the session's own profile, kept by the server, good for one 5250 automatic sign-on within `expires_secs`, and only over Telnet with TLS. The client sends it, as 64 hexadecimal characters, where automatic sign-on otherwise carries the password; how the Telnet server tells a ticket from a password is the Telnet server's to settle. The client treats a ticket as it treats a token: never logged or stored, wiped after use.

### Compatibility of new values (proposed)

A Wire enum fails to decode on a tag it does not know, so an older client would reject a whole envelope carrying a newer `Status`, `Service` or `Kind` value, or a newer value of any enum in a body, before it could even read the `request_id`. Proposed rule: **a value is sent only to a peer that negotiated a version that defines it.**

- Every enum value, operation and event added after version 1 is defined from one protocol version. `Hello` fixes the version for the connection.
- The server sends no `Kind`, `Service` or `Status` value, no event and no enum value in a body that the connection's version does not define. Where a newer status would apply it sends `Error` with the same message; where a newer value of a body enum would apply, the operation's definition names the older value to send instead.
- `Hello`'s `services` lists only the services the chosen version defines.
- A client sends only services, operations and values its negotiated version defines. A server that gets one its version does not define replies `InvalidRequest`; it cannot decode a newer `Service` or `Kind` at all, so the client must never send one.
- A frame that does not decode is still a protocol error that ends the connection, on either side. Within a version, optional features are announced with `Hello`'s `capabilities` strings, never with new enum values.

This keeps Wire and the envelope as they are; the cost is that the server encodes per connection version.

## Open questions

1. The `QAPD` job and `*QAP` server names, port 2320 and the limits above. Confirm or change before the server ships.
2. Whether QS/400 Access must still reach the vendor's real systems for anything other than 5250 and printing. This spec assumes no.

## QS/400's implementation

Settled with Cody on 2026-11-01, in answer to the open questions and
to points the draft above leaves open:

- Port 2320, `*QAP`, listener `QAPD` in `QSYSWRK` as `QUSER`;
  connection jobs `QAPCNN` in `QUSRWRK`, one per connection, as `QUSER`
  until sign-on. `CHGQAPA AUTOSTART(*NO) PORT(2320) IDLETIMO(3600)` is
  the shipped state.
- Enums use `wire_enum!`. `Status` is a `u16` on the wire
  (`wire_enum!(Status: u16 { .. })`, added to `ipc` for it); `Kind`,
  `Service` and the sign-on method are one byte.
- The 1 MiB limit is on the Wire-encoded `Envelope`, not counting the
  four-byte length prefix.
- A split reply cuts the encoded body: the client joins the bodies of
  its frames in order and decodes once. Every frame carries the
  status; the message rides on the last.
- `Cancel` gets no reply of its own. A request still outstanding ends
  with `Cancelled` and an empty body; a cancel naming nothing
  outstanding is ignored. The server reads only its `request_id`.
- A first request other than `Hello` gets `InvalidRequest` and the
  connection closes. `SignOn` or `Resume` on a signed-on connection
  gets `InvalidRequest`; the connection and session stay.
- A connection idle for 300 s is closed without an event.
- Sessions: `idle_timeout_secs` is `CHGQAPA IDLETIMO`. The timer runs
  only while no connection is attached; an attached session never
  expires. A session whose connection job ended without detaching
  counts as detached from when that is noticed. `Resume` moves a
  session to the new connection. An operator ending a connection job
  (`ENDJOB`) sends `SessionEnded` with status `Ok` and `TCP1A54`, and
  ends the session. `ENDTCPSVR` ends every session.
- `ServerMessage` text is looked up in the connection job, so in the
  language of whoever the job runs as: the system's before sign-on,
  the profile's after it. QS/400 ships one language of message text
  today, so every path yields it.

Code: `work/qap-proto` (the types), `net/qapd` (connections, sessions,
the `Host` trait), `work/qsjob/src/work/qap/` (the jobs),
`lic/slic/src/stgsvc/qap.rs` (sign-on, swap, tokens, in the storage
service), `net/servers` (`*QAP`, `CHGQAPA`). Messages `TCP1A50` to
`TCP1A54` are in `QTCPMSG`.
