<!-- Read-only copy of quartz-systems-400 docs/QAP.md at commit 7f82a43fc5edfb8bfd7a84753d6e77e23704fffb. Do not edit: change the spec in QS/400 first, then re-pin (ci/spec-mirror.sh). -->
# QAP: Quartz Access Protocol, version 1 (draft)

QAP is how QS/400 Access talks to QS/400 for everything except display and printer sessions, which stay on the system's Telnet server. It is an original Quartz design.

This draft settles the decisions below. Anything it leaves open is listed under Open questions; the first agent to reach one stops and asks Cody.

## Decisions

| Topic | Decision |
| --- | --- |
| Transport | TCP, one port, default **2320**, configurable on both ends |
| Security | TLS 1.3 only, no plaintext mode. Server certificate from the QS/400 certificate manager, application ID `QS400_ACCESS_SERVER`. The client validates it with its existing untrusted-certificate trust flow |
| Framing and encoding | Wire, from the QS/400 repository's `ipc` crate, the same framing and serialization QSI uses. Types below are Wire types; enums use `wire_enum!`. If Wire cannot express something below, stop and ask; do not invent a second encoding |
| Where the code lives | `qap-proto` (envelope and message types) lives in the QS/400 repository beside `ipc`. QS/400 Access pins both by commit |
| Multiplexing | One connection carries many concurrent requests. Every request has a service ID and a request ID; replies carry the request ID, in the order operations finish, not the order requests came. No port mapper and no per-service ports. Lanes: `Command` requests and `Sql` operations 1 to 8 (version 4) run one at a time, in arrival order, in the connection job, because they share its job state (library list, `QTEMP`, job attributes, commitment definition, statements and cursors). Every other operation, `Sql` operations 9 to 11 (the catalog lists, which keep no state) and every `Ifs` operation (version 5) included, runs concurrently, up to the outstanding limit, as the signed-on profile, without waiting for the lane; so do the watches' events (version 6). `Ping` and `Cancel` are never queued behind anything |
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

**Status values (u16).** `Ok` 0 · `Error` 1 (the message says what) · `VersionUnsupported` 2 · `NotSignedOn` 3 · `SignOnFailed` 4 · `PasswordExpired` 5 · `NotAuthorized` 6 · `NotFound` 7 · `InvalidRequest` 8 · `Busy` 9 · `Cancelled` 10 · `SessionExpired` 11 · `ServerEnding` 12. Sign-on failures carry the same message the sign-on display would show for the same case, and nothing more.

**Limits.** A frame is at most 1 MiB; larger results are sent as several reply frames with `More` set on all but the last. At most 64 outstanding requests per connection; beyond that the server replies `Busy`. The client sends `Ping` after 60 s without traffic; the server closes a connection after 300 s without frames. Closing a connection does not end the session: it can be resumed until its idle timeout.

**Events (server to client, `request_id` 0).** `Connection` op 100 `ServerEnding` (status `ServerEnding`, message says why; the server closes after sending it) · `SignOn` op 100 `SessionEnded` (status `SessionExpired`, or `Ok` when ended by an operator or, from version 4, by `SignOff` on another of the session's connections, with a message).

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
- `SignOn` applies the same checks as the sign-on display: `QMAXSIGN` and its action, disabled profiles, password expiry, `*NONE` passwords refused, and the same audit journal entries. An expired password gets `PasswordExpired` and no token; the client then sends `ChangePassword`, which applies the system password rules and on success returns a `SignOnReply`.
- `ChangePassword` is also allowed in a signed-on session for that session's own profile.
- The token is 256 random bits, stored server-side only. `Resume` re-attaches a new connection to the session until the idle timeout; after it, `SessionExpired`. `SignOff` ends the session and invalidates the token.
- Anything other than the operations marked "yes" before sign-on gets `NotSignedOn`. Before sign-on the connection job runs as `QUSER`; after it, as the signed-on profile.
- The client never logs or stores passwords or tokens, and clears them from memory after use.

## Later phases (spec now, build later)

The client agent inventories which system functions QS/400 Access uses, and adds each as a QAP service with its requests and replies to this document before building it. Expected, in this order: remote command (run CL, return messages and the job), spooled files (list, read as SCS, text or PDF, hold, release, delete, move), SQL, IFS files, message queues. Optional: a one-time `TelnetTicket` from a signed-on session that the Telnet server accepts for automatic sign-on, which would remove the need for password substitution on Telnet.

### Inventory

What QS/400 Access needs of the system besides display sessions, from its task pane, README and messages, and the service each becomes:

| Function in QS/400 Access | Service |
| --- | --- |
| Run Command task | `Command` (3) |
| Printer Output task: spooled files and output queues | `Spool` (4) |
| Run SQL, Schemas and Data Transfer tasks (Data Transfer downloads with a query and uploads with an `INSERT` run over batches of rows) | `Sql` (5) |
| Integrated File System task | `Ifs` (6) |
| Message queues: the user's own and `QSYSOPR`, inquiry replies | `Messages` (7) |
| Password expiry warning (`QAD4031`), which the phase 0 `SignOnReply` cannot give | `SignOn` op 6 `PasswordStatus` |
| Automatic sign-on of a display session without a password on Telnet (the optional ticket) | `SignOn` op 5 `TelnetTicket` |

Not in QAP: display and printer sessions (Telnet), the printer session's message queue (named in the Telnet session's startup), the Console and Control Panel tasks (the service console), and Key and Certificate Management (this computer's trust store).

### Rules for these additions

- Service values append from 3 and are never renumbered. Operations number from 1 within each service; events from 100. `SignOn` continues from 5.
- Every operation below is not allowed before sign-on: before it, each gets `NotSignedOn`.
- Bodies use only Wire types: structs, enums with explicit tags (one byte unless a wider tag is stated), lists, options, bytes, fixed-size byte arrays, integers, UTF-8 strings. Times are i64 milliseconds since 1970-01-01 UTC. A flag is a u8, 1 for yes and 0 for no; any other value gets `InvalidRequest`.
- A request is one frame, so at most 1 MiB. Operations that send more (`WriteFile`, `Execute` with many rows) are repeated by the client over pieces; nothing splits a request. Replies over 1 MiB are split with `More` as in phase 0.
- Lists that can be long take `start` (0-based) and `max` (0 for no limit) and answer `total`, the number of entries matching, beside the entries returned.
- Every operation can also get `Error` (the message says what), `InvalidRequest` (a body that does not decode or a value out of range), `Busy`, and `Cancelled` when cancelled. The tables' Statuses column lists the others. For example, `ReleaseSpooledFile` of a file that is not held gets `Error` with `CPF3337`.
- Each service, operation, status and event value is introduced by a protocol version (see Compatibility of new values). The services are planned for versions 2 to 6 in the order below; the `SignOn` additions with version 2. A version number is fixed only when QS/400 ships it.

**New status values (u16), appended after `ServerEnding` 12.** `Exists` 13 (the name is taken) · `InUse` 14 (another job holds the object) · `SqlError` 15 (an SQL statement failed; the body is `SqlFailure`, the message the SQL message).

**Common types.**

- `JobId` = { `name` String, `user` String, `number` String (six digits) }.
- `ObjectName` = { `library` String, `name` String }. `library` may be `*LIBL` or `*CURLIB` where a library list search applies.
- `EndHow` enum: `Controlled` 0 (`*CNTRLD`), `Immediate` 1 (`*IMMED`). Defined by version 7 for `Manage`; a common type from version 8, which `Jobs` uses too. Its values and their numbers are version 7's.

**Common types (version 8).**

- `MemberName` = { `file` ObjectName (a source physical file; `library` may be `*LIBL` or `*CURLIB`), `member` String }.
- `SourceRecord` = { `seq` u32 (the sequence number in hundredths: `100` is `0001.00`, at most `999999`), `date` u32 (`yymmdd`, 0 for none), `data` String (UTF-8, trailing blanks removed) }.
- `Where` enum: `Member` 0 { `member` MemberName }, `Path` 1 { `path` String }, `None` 2.
- `Diagnostic` = { `where` Where (the source the message points into: the member compiled, a `/COPY` member, a stream file, or nowhere), `line` u32 (from 1; 0 unknown), `column` u16 (from 1; 0 unknown), `end_line` u32 (0: `line`), `end_column` u16 (0 unknown), `severity` u8, `id` String (`RNF7030`), `text` String (first level), `help` String (second level) }. The Rust field of `where` is `r#where`; `rpg/compdiag` has the same type, its field `at`.

### Service 3 `Command`: run CL commands (version 2)

Commands run in the connection job, as the signed-on profile, with its job description's library list. A command that only runs interactively ends with `Error` and its escape message.

| Service | Op | Request body | Reply body | Allowed before sign-on |
| --- | --- | --- | --- | --- |
| Command | 1 `RunCommand` | `command` String | `CommandReply`: `job` JobId, `messages` list of `ServerMessage` (every message the command sent to its caller, oldest first, second level included) | no |
| Command | 2 `CheckCommand` | `command` String | `messages` list of `ServerMessage` (the syntax check's messages; empty when the command is valid) | no |
| Command | 3 `DescribeCommand` | `command` ObjectName (library may be `*LIBL`) | `CommandDefinition` | no |

| Op | Statuses |
| --- | --- |
| `RunCommand` | `Ok`; `Error` when the command ended with an escape message (the message is that one, and the body is still `CommandReply`); `NotFound` (no such command); `NotAuthorized` (to the command or what it names); `Cancelled` (the command was ended; the body is still `CommandReply`, the message the one the end caused) |
| `CheckCommand` | `Ok`; `Error` when the check found errors (the body is still the messages); `NotFound` |
| `DescribeCommand` | `Ok`; `NotFound`; `NotAuthorized` |

`CommandDefinition` = { `name` ObjectName (as resolved), `text` String, `help` String, `nodes` list of `ParmNode` }. The definition is flattened, so Wire needs no recursive type; nodes come in the order the prompter shows them, and `parent` links elements and qualifiers to their owner:

`ParmNode` = { `index` u16, `parent` Option of u16, `role` enum (`Parameter` 0, `Element` 1, `Qualifier` 2), `keyword` String (empty for elements and qualifiers), `prompt` String, `help` String, `kind` enum (`Name` 0, `Character` 1, `Decimal` 2, `Integer` 3, `Logical` 4, `Date` 5, `Time` 6, `Generic` 7, `Path` 8, `Command` 9), `length` u32, `decimals` u16, `required` flag, `min_values` u16, `max_values` u16 (more than 1 = a list), `default` Option of String, `special_values` list of String, `values` list of String (when only listed values are valid), `range` Option of { `low` String, `high` String }, `show` enum (`Always` 0, `Additional` 1, `Never` 2) }.

### Service 4 `Spool`: spooled files and output queues (version 3)

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
| `ListSpooledFiles`, `ListOutputQueues` | `Ok`; `NotFound` (the queue or library, or for `ListSpooledFiles` an unknown user profile, `CPF2204`); `NotAuthorized` |
| `ReadSpooledFile` | `Ok`; `NotFound`; `NotAuthorized`; `InUse` (still `Open` or `Writing`); `Error` (the format cannot be made from this file's device type, for example `Scs` from `Afp`) |
| `HoldSpooledFile`, `ReleaseSpooledFile`, `DeleteSpooledFile`, `MoveSpooledFile` | `Ok`; `NotFound`; `NotAuthorized`; `InUse` (printing, for delete and move) |

**Reading pages.**

- `ReadSpooledFile` with `first_page` and `last_page` returns exactly those pages, and `pages` is how many the whole file has. A `first_page` of 0 or past the last page gets `InvalidRequest`. A `last_page` of 0, or past the last page, reads to the end. A file with no pages answers `first_page` 1 with no data.
- For `Scs`, the data is the data stream from the start of `first_page` to the end of `last_page`, as it was spooled.
  - A range starting after page 1 is preceded by the state that applies at that point: the page format, pitch, line density and font settings in effect at `first_page`. That state is sent as SCS controls, so the range renders exactly as it would inside the whole file.
  - The ranges of consecutive reads join, byte for byte after their leading state, into the whole file.
- Clients read large files in page batches.
- The `Text` and `Pdf` formats are optional per device type. A file whose format cannot be made gets `Error`, with a message saying so.

**Spool watches (version 6).**

- `SpoolTarget` enum: `Queue` 0 { `outq` ObjectName }, `User` 1 { `profile` String (`*CURRENT` or a profile) }.
- `SpoolChange` enum: `Added` 0, `Changed` 1 (status, queue or attributes), `Removed` 2.

| Service | Op | Request body | Reply body | Allowed before sign-on |
| --- | --- | --- | --- | --- |
| Spool | 8 `WatchSpool` | `target` SpoolTarget | — | no |
| Spool | 9 `UnwatchSpool` | `target` SpoolTarget | — | no |

| Op | Statuses |
| --- | --- |
| `WatchSpool` | `Ok`; `NotFound` (the queue, or an unknown profile, `CPF2204`); `NotAuthorized` |
| `UnwatchSpool` | `Ok`; `NotFound` (that target is not watched on this connection) |

**Event (`request_id` 0).** `Spool` op 100 `SpooledFileChanged` (status `Ok`; body `target` SpoolTarget, `change` SpoolChange, `file` Option of SpooledFile (absent for `Removed`), `id` SpooledFileId), for each change to the files a watched target lists.

- A target's files are the ones `ListSpooledFiles` lists for it, with the same authority: a `Queue` target as `ListSpooledFiles` with that `outq` and user `*ALL`, a `User` target as `ListSpooledFiles` with that user.
- `file` is the file as `ListSpooledFiles` would list it after the change.
- Moving a file between two watched queues sends `Removed` for one and `Added` for the other. For a `User` target the move is `Changed`.
- Watches are passive and end with their connection (and a `SignOff` on it), as for messages. A watch reports the changes after its `Ok`.
- A `profile` of `*ALL` is `InvalidRequest`.

### Service 5 `Sql`: SQL, the catalog and data transfer (version 4)

Statements run in the connection job through the system's database engine, as the signed-on profile, with the same authority checks as any SQL interface. Statements and cursors belong to the connection and end with it; changes not committed are rolled back when it closes. Data Transfer downloads with `Prepare`, `Execute` and `Fetch`, and uploads with an `INSERT` prepared once and `Execute` repeated over batches of rows, each request under 1 MiB.

- `CommitLevel` enum: `None` 0, `ReadUncommitted` 1, `ReadCommitted` 2, `RepeatableRead` 3, `Serializable` 4.
- `Naming` enum: `Sql` 0 (`schema.table`), `System` 1 (`library/file`).
- `SqlValue` enum: `Null` 0, `Integer` 1 { `value` i64 }, `Decimal` 2 { `text` String (exact, `.` as the point) }, `Float` 3 { `text` String (shortest text that reads back as the same value) }, `Text` 4 { `text` String (character and graphic types, converted to UTF-8) }, `Binary` 5 { `data` bytes }, `Date` 6 { `ms` i64 (midnight UTC of the date) }, `Time` 7 { `ms` i64 (since midnight) }, `Timestamp` 8 { `ms` i64 (as if UTC), `picos` u32 (picoseconds past `ms`, 0 to 999 999 999) }, `Boolean` 9 { `value` flag }. `picos` carries the full fraction of `TIMESTAMP(12)`; the column's `scale` is the timestamp's precision (0 to 12), and digits past it are zero.
- Character and graphic columns with CCSID 65535 are sent as `Binary`, the bytes as stored, and their `Column`'s `ccsid` is 65535. Parameters for them are sent as `Binary` too.
- `DECFLOAT` is sent as `Decimal` whose `text` may use E notation (`1.5E-30`), or is `Infinity`, `-Infinity`, `NaN` or `sNaN`.
- Every other type is sent as above: integer types as `Integer`, `DECIMAL` and `NUMERIC` as `Decimal`, `REAL` and `DOUBLE` as `Float`, other character and graphic types as `Text`, `BINARY`, `VARBINARY` and `BLOB` as `Binary`.
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

**Rules.**

- Operations 1 to 8 run in the connection's lane, in arrival order, with `Command` requests (see Multiplexing). Operations 9 to 11 read the catalog with the user's authority, keep no state and run concurrently, also while a statement runs in the lane.
- A reply larger than 1 MiB (`Fetch` rows, a catalog list) is split with `More`. `Fetch` returns at most `max_rows` rows (0 for no limit), fewer at the end of the result, with `end` 1 when the cursor has no more rows.
- Cancel:
  - a started `Execute` ends with `Cancelled`, its own body (`updated` the rows changed before the cancel, `cursor` none) and the SQL message for the cancel;
  - a started `Fetch` ends with `Cancelled`, the rows produced before the cancel, `end` 0, and the cursor closed;
  - a statement cancelled before it started gets `Cancelled` and an empty body, as any request.
  - Changes a cancelled statement made under commitment control stay uncommitted until `Commit` or `Rollback`.
- Commitment control:
  - `SetOptions` is per connection; a connection starts with `None`, `Sql` naming and no default schema.
  - Uncommitted changes are rolled back when the connection closes.
  - A commit level other than `None` against a table that cannot use commitment control (for example one that is not journaled) gets the database's own `SqlError`. The client decides what to do.
- A statement or type the database does not support gets the database's own `SqlError`, as it reports it.
- An `Execute` over several parameter rows stops at the first row that fails: `SqlError`, with `SqlFailure.row` the 0-based index of that row. The rows before it stay changed (uncommitted under commitment control).

### Service 6 `Ifs`: Integrated File System files (version 5)

Paths are absolute and UTF-8 (`/home/BOB/a.txt`, `/QSYS.LIB/QGPL.LIB`). Every `Ifs` operation runs concurrently (see Multiplexing), as the signed-on profile, never in the lane.

- `EntryKind` enum: `File` 0, `Directory` 1, `Symlink` 2, `Other` 3.
- `Entry` = { `name` String, `kind` EntryKind, `size` u64, `modified` i64, `owner` String, `ccsid` u16, `target` Option of String (a symbolic link's) }.
- `WriteMode` enum: `Create` 0 (the file must not exist), `Replace` 1 (created or emptied first), `Update` 2 (at `offset` in an existing file), `Append` 3.

| Service | Op | Request body | Reply body | Allowed before sign-on |
| --- | --- | --- | --- | --- |
| Ifs | 1 `ListDirectory` | `path` String, `pattern` String (`*` and `?` wildcards), `start` u32, `max` u32 | `total` u32, `entries` list of `Entry` | no |
| Ifs | 2 `GetInfo` | `path` String | `Entry` | no |
| Ifs | 3 `ReadFile` | `path` String, `offset` u64, `length` u64 (0 for to the end), `as_ccsid` Option of u16 | `size` u64 (the file's size now), `next_offset` u64, `data` bytes (split with `More` when large) | no |
| Ifs | 4 `WriteFile` | `path` String, `mode` WriteMode, `offset` u64, `ccsid` u16 (for a file created; 0 for 1208), `data` bytes, `data_ccsid` Option of u16 | `size` u64 | no |
| Ifs | 5 `CreateDirectory` | `path` String, `parents` flag | — | no |
| Ifs | 6 `Delete` | `path` String (a file, a symbolic link or an empty directory) | — | no |
| Ifs | 7 `Rename` | `from` String, `to` String, `replace` flag | — | no |
| Ifs | 8 `Copy` | `from` String, `to` String, `replace` flag | — | no |
| Ifs | 9 `Stat` | `path` String | `entry` Entry, `change` u64 (version 6) | no |

| Op | Statuses |
| --- | --- |
| `ListDirectory`, `GetInfo`, `Stat`, `ReadFile` | `Ok`; `NotFound`; `NotAuthorized`; `InUse` (`ReadFile` of a file locked for writing); `Error` (`ReadFile` with an `as_ccsid` the file's CCSID cannot be converted to) |
| `WriteFile` | `Ok`; `NotFound` (the directory, or the file for `Update`); `NotAuthorized`; `Exists` (`Create`); `InUse`; `Error` (a `data_ccsid` that cannot be converted to the file's CCSID) |
| `CreateDirectory`, `Rename` | `Ok`; `NotFound`; `NotAuthorized`; `Exists` (`Rename` without `replace`) |
| `Delete` | `Ok`; `NotFound`; `NotAuthorized`; `InUse`; `Error` (a directory that is not empty) |
| `Copy` | `Ok`; `NotFound`; `NotAuthorized`; `Exists` (without `replace`); `InUse` |

**Reading with conversion.**

- With `as_ccsid`, the server converts the file's data from the file's CCSID to `as_ccsid` (for example 1208 for UTF-8). `length` counts the file's bytes, not the converted ones.
- A reply holds only whole characters. `next_offset` is the file offset just past the last byte converted; it may be less than `offset + length` when the range ends inside a character. The client continues from `next_offset`, never from `offset + length`. Without `as_ccsid`, `next_offset` is `offset` plus the bytes returned.
- In a mixed single- and double-byte file, the server converts from `offset` in the shift state the file has at that offset, so reads that each start at the previous reply's `next_offset` join into exactly the whole file's conversion. When `as_ccsid` is itself a mixed CCSID, each reply's data starts and ends in single-byte state.
- An `as_ccsid` equal to the file's CCSID returns the data as stored. A file whose CCSID cannot be converted to `as_ccsid` (for example 65535, binary) gets `Error` with the message.

**Writing with conversion.**

- With `data_ccsid`, `data` is in that CCSID and the server converts it to the file's CCSID: the `ccsid` given for a file created, or the existing file's. Without it, `data` is written as it is.
- Conversion is per request; the client splits text only on whole characters.
- For a mixed single- and double-byte target, each request's converted output ends in single-byte state. When a converted write lands at the end of the file, the file ends in the shift-in an earlier write left and the new output starts with a shift-out, the two are dropped, so text written in pieces makes the same file as one write.
- `size` in the reply is the file's size in its own CCSID.

**Stat (version 6).** `Stat` answers what `GetInfo` answers, and `change`: a counter of the object's changes.

- `change` increases on every change to the object's data or attributes: data written, truncation, rename, owner, primary group, authority and CCSID. For a directory, an entry added, removed or renamed in it changes its data.
- It never decreases, and a value is never reused for the same object. Two changes within one millisecond give two values, though `modified` may not differ.
- A client that keeps a copy of a file compares `change` to know whether the file changed since; `modified` alone cannot tell it.
- `Stat`, as `GetInfo`, does not follow a symbolic link: it describes the link.

**Copy.** `Copy` copies a file, not a directory, on the system, keeping its CCSID and data as stored, without sending the data to the client. `Exists` answers a `to` that exists when `replace` is 0; with `replace` 1 the copy replaces it.

**Paths and authority.** Every operation applies the same authority checks as the system's own file-system commands for the same action: listing and reading as displaying a link, `WriteFile` as writing a stream file, `CreateDirectory` as creating a directory, `Delete` as removing a link or a directory, `Rename` as renaming or moving, `Copy` as copying. Paths under `/QSYS.LIB` behave as those commands allow; anything not allowed there gets `Error` with the system's message.

### Service 7 `Messages`: message queues (version 6)

- `MessageType` enum: `Informational` 0, `Inquiry` 1, `Reply` 2, `Completion` 3, `Diagnostic` 4, `Escape` 5, `Notify` 6, `SenderCopy` 7 (the sender's copy of an inquiry, on the queue its reply goes to).
- `QueuedMessage` = { `key` 4 bytes, `id` String (empty for an impromptu message), `kind` MessageType, `severity` u8, `text` String, `help` String, `sent` i64, `from_user` String, `from_job` JobId, `needs_reply` flag, `reply` Option of String, `answers` Option of 4 bytes }.
  - `reply` is an inquiry's or a sender's copy's reply, once it has one.
  - `answers` is a `Reply`'s: the key of the message it answers on the same queue, an inquiry or a sender's copy. Other types have none.
- `Queue` enum: `Own` 0 (the signed-on profile's), `SystemOperator` 1 (`QSYSOPR`), `Named` 2 { `queue` ObjectName }.
- `Recipient` enum: `User` 0 { `profile` String }, `Queue` 1 { `queue` Queue }.

| Service | Op | Request body | Reply body | Allowed before sign-on |
| --- | --- | --- | --- | --- |
| Messages | 1 `ListMessages` | `queue` Queue, `newest_first` flag, `start` u32, `max` u32 | `total` u32, `messages` list of `QueuedMessage` | no |
| Messages | 2 `ReplyMessage` | `queue` Queue, `key` 4 bytes, `reply` String | — | no |
| Messages | 3 `RemoveMessages` | `queue` Queue, `keys` list of 4 bytes | — | no |
| Messages | 4 `SendMessage` | `to` Recipient, `text` String, `inquiry` flag | `key` Option of 4 bytes (an inquiry's, to find its reply: see Inquiries and their replies) | no |
| Messages | 5 `WatchQueue` | `queue` Queue | — | no |
| Messages | 6 `UnwatchQueue` | `queue` Queue | — | no |

| Op | Statuses |
| --- | --- |
| `ListMessages`, `SendMessage` | `Ok`; `NotFound` (the queue or profile); `NotAuthorized` |
| `ReplyMessage` | `Ok`; `NotFound` (queue or key); `NotAuthorized`; `InvalidRequest` (not an inquiry, or already answered); `Error` (the reply is not valid for the message) |
| `RemoveMessages` | `Ok`; `NotFound` (the queue, or a key not on it: then nothing is removed); `NotAuthorized` |
| `WatchQueue` | `Ok`; `NotFound`; `NotAuthorized` |
| `UnwatchQueue` | `Ok`; `NotFound` (not watched on this connection) |

**Event (`request_id` 0).** `Messages` op 100 `MessageArrived` (status `Ok`; body `queue` Queue, `message` QueuedMessage), for each message that arrives on a queue watched on the connection, in arrival order per queue.

**Rules.**

- Each operation does what the message commands do, with their authority checks: `ListMessages` and `WatchQueue` as displaying the queue's messages, `ReplyMessage` as sending a reply, `RemoveMessages` as removing a message, `SendMessage` as sending a message to the queue. `QSYSOPR` and named queues are checked as any queue is.
- Text and second-level text are in the connection job's language.
- Keys:
  - A message's key is unique within its queue for the life of the message, and never reused while the message exists.
  - Messages are kept in arrival order. `ListMessages` with `newest_first` 1 lists strictly newest first, so a client can stop at a key it already has; with 0, oldest first.
- Watches:
  - `WatchQueue` is passive. It never allocates the queue, never changes its delivery mode, and never conflicts with a job that holds the queue in break mode.
  - A watch reports the messages that arrive after its `Ok`.
  - A watch ends with its connection, and with a `SignOff` on it. A `Resume` on a new connection does not restore watches: clients watch again and catch up with `ListMessages`.
- `ReplyMessage` checks the reply against the message's valid replies, as the reply command does; a reply it does not take is `Error` with the system's message. An empty reply is the message's default reply.
- Inquiries and their replies:
  - An inquiry sent with `SendMessage` gets its reply on the sender's own queue (`Own`).
  - When the inquiry goes to another queue, a `SenderCopy` of it is put on the sender's own queue, and `key` in the reply is the copy's key. When the inquiry goes to the sender's own queue, `key` is the inquiry's.
  - When the inquiry is answered, a `Reply` arrives on the sender's own queue whose `answers` is that `key`, and the copy's `reply` is set. A client matches a reply to its inquiry by `answers` = the `key` `SendMessage` returned.
  - The queue the inquiry went to keeps it, with its `reply` set and `needs_reply` 0, followed by a `Reply` of its own whose `answers` is the inquiry's key there.
- A `Recipient` `User` is the profile's message queue, as the profile names it; an unknown profile is `NotFound` (`CPF2204`).

### Service 8 `Manage`: managing the system (version 7, draft)

**Draft until QS/400 ships it.** Version 7 is the management API of Phase 12 Section 7: one protocol surface for QS/MC and for scripts, so neither drives the system through screens. Nothing in versions 1 to 6 changes; a peer that negotiates 6 or less never sees `Manage`.

Every operation runs as the signed-on profile, concurrently on the connection's secondary threads (never in the lane), and does what the command it stands for does: the same authority checks, the same messages, the same audit journal entries. There is no other way in: no operation is allowed before sign-on, and no operation skips the command's checks because the request came over QAP.

- `SubsystemStatus` enum: `Active` 0, `Inactive` 1, `Ending` 2, `Starting` 3.
- `Subsystem` = { `sbsd` ObjectName, `status` SubsystemStatus, `jobs` u32 (active jobs), `monitor` Option of JobId (while it runs), `text` String }.
- `EndHow`: see Common types (from version 8 a common type; unchanged).
- `ValueKind` enum: `Character` 0, `Decimal` 1, `Integer` 2, `List` 3.
- `SystemValue` = { `name` String, `kind` ValueKind, `value` String (as `CHGSYSVAL VALUE()` takes it, without apostrophes; a list's entries separated by one blank), `text` String, `category` String (the catalog group: `SEC`, `DATTIM`, `SYSCTL`, `LIBL`...) }.
- `PtfStatus` enum: `NotApplied` 0, `TemporarilyApplied` 1, `PermanentlyApplied` 2, `TemporarilyRemoved` 3, `PermanentlyRemoved` 4, `Superseded` 5, `Damaged` 6, `SaveFileOnly` 7 (only its save file or cover letter is here).
- `IplAction` enum: `None` 0, `Apply` 1, `Remove` 2 (at the next IPL).
- `Ptf` = { `product` String, `id` String, `release` String, `status` PtfStatus, `ipl_action` IplAction, `delayed` flag, `text` String }.
- `UnitStatus` enum: `Active` 0, `Suspended` 1, `Resuming` 2, `Failed` 3, `NonConfigured` 4. `Protection` enum: `Unprotected` 0, `Mirrored` 1, `Parity` 2.
- `DiskUnit` = { `unit` u16 (0: not configured), `asp` u16 (0: none), `resource` String (`DD001`), `type_model` String, `serial` String, `capacity` u64 (bytes), `used` u64, `status` UnitStatus, `protection` Protection }. `Asp` = { `asp` u16, `capacity` u64, `used` u64, `threshold` u8 (percent) }.
- `ResourceType` enum, the `WRKHDWRSC TYPE` values: `All` 0 (`*AHW`), `Communication` 1 (`*CMN`), `LocalWorkStation` 2 (`*LWS`), `Processor` 3 (`*PRC`), `Storage` 4 (`*STG`), `Coupled` 5 (`*CSA`), `Cryptographic` 6 (`*CRP`). `ResourceStatus` enum: `Operational` 0, `NotDetected` 1.
- `HardwareResource` = { `name` String (`CMN01`), `depth` u8 (levels below the top of the list), `type_model` String (`290A-001`, or the type alone), `status` ResourceStatus, `text` String, `location` String, `serial` String, `parent` String (empty at the top) }.
- `IplMode` enum: `Normal` 0, `Manual` 1. `PreviousEnd` enum: `Normal` 0, `Abnormal` 1. `IplSource` enum: `Panel` 0 (`*PANEL`), `A` 1, `B` 2.

| Service | Op | Request body | Reply body | Allowed before sign-on |
| --- | --- | --- | --- | --- |
| Manage | 1 `ListSubsystems` | `library` String (`*ALL`, `*LIBL` or a library), `start` u32, `max` u32 | `total` u32, `subsystems` list of `Subsystem` (active first, then by name) | no |
| Manage | 2 `StartSubsystem` | `sbsd` ObjectName | — | no |
| Manage | 3 `EndSubsystem` | `sbsd` Option of ObjectName (none: `*ALL`), `how` EndHow, `delay` Option of u32 (seconds; none: `*NOLIMIT`) | — | no |
| Manage | 4 `GetSystemValues` | `names` list of String (empty: every value) | `values` list of `SystemValue` (in the order asked; by name for all) | no |
| Manage | 5 `SetSystemValue` | `name` String, `value` String | `SystemValue` (as it now is) | no |
| Manage | 6 `ListPtfs` | `product` String (`*ALL` or a product), `start` u32, `max` u32 | `total` u32, `ptfs` list of `Ptf` (by product, then identifier) | no |
| Manage | 7 `ListDiskUnits` | — | `units` list of `DiskUnit` (configured by unit number, then the others), `asps` list of `Asp` | no |
| Manage | 8 `ListHardwareResources` | `kind` ResourceType | `resources` list of `HardwareResource` (in tree order, as the list shows them) | no |
| Manage | 9 `IplStatus` | — | `ipl_time` i64, `ipl_type` String, `mode` IplMode, `slot` String, `next_slot` Option of String, `last_src` String (eight characters), `src_text` String, `previous_end` PreviousEnd, `restricted` flag | no |
| Manage | 10 `PowerDown` | `how` EndHow, `delay` Option of u32, `restart` flag, `ipl_source` IplSource | — | no |

| Op | Stands for | Statuses |
| --- | --- | --- |
| `ListSubsystems` | `WRKSBS` | `Ok`; `NotFound` (the library); `NotAuthorized` |
| `StartSubsystem` | `STRSBS` | `Ok`; `NotFound` (`CPF9801`); `NotAuthorized` (`CPF222E`: `*JOBCTL`); `Error` (`CPF1010`: already active) |
| `EndSubsystem` | `ENDSBS` | `Ok`; `NotFound`; `NotAuthorized` (`*JOBCTL`); `Error` (`CPF1054`: not active; `CPF1034`: the controlling subsystem alone) |
| `GetSystemValues` | `RTVSYSVAL`, `DSPSYSVAL` | `Ok`; `NotFound` (`CPF1842`, naming the first unknown value; nothing else is returned) |
| `SetSystemValue` | `CHGSYSVAL` | `Ok`; `NotFound` (`CPF1842`); `NotAuthorized` (`CPF222E`; `CPF18B4` when DST locks security values); `Error` (`TCP1A5F`: a value it does not take) |
| `ListPtfs` | `DSPPTF` | `Ok`; `NotAuthorized` |
| `ListDiskUnits` | `WRKDSKSTS`, DST's configuration display | `Ok`; `NotAuthorized` |
| `ListHardwareResources` | `WRKHDWRSC` | `Ok`; `NotAuthorized` |
| `IplStatus` | `DSPSYSSTS`, the control panel | `Ok` |
| `PowerDown` | `PWRDWNSYS` | `Ok`; `NotAuthorized` (`*JOBCTL`); `Error` (a power down already started) |

**Authentication and authority.**

- The profile signed on with `SignOn`, `Resume` or `Attach` is the only identity; there are no keys or tokens of another kind.
- Each operation applies its command's checks in the connection job, as that profile, with its groups and the authority it adopts: `STRSBS`, `ENDSBS` and `PWRDWNSYS` need `*JOBCTL`; `CHGSYSVAL` needs `*ALLOBJ`, and for security values `*ALLOBJ` and `*SECADM`, for the auditing values `*AUDIT`, and DST's "allow system value security changes" for either (`sec::sysval`). Reads need what their displays need.
- A refusal for a missing special authority is audited as the command's is (`AF` type `K`).

**Exit point.** Every `Manage` request passes the exit point `QS400_QAP_MGMT`, format `QAPX0100`, before it runs, as the host servers' requests pass theirs:

- registered at the first IPL with the shipped points (`WRKREGINF`); no deregistration, change allowed, at most one program, as the host servers' points;
- the program gets two parameters, the return code (CHAR(1), `1` allows, anything else refuses) and the request: user CHAR(10), server CHAR(10) `*QAPMGMT`, format CHAR(8) `QAPX0100`, operation BINARY(4) (the `Manage` op code), operation name CHAR(20) (`EndSubsystem`), target CHAR(64) (`LIB/SBSD`, `*ALL`, a system value's name, a product, a `*CMN`-style type; blank when none), detail length BINARY(4), detail CHAR(*) (`*CNTRLD 600`, the new value, `*IMMED *NOLIMIT RESTART(*YES)`); big-endian, CCSID 37;
- a refusal is `NotAuthorized` with `TCP1A5E` naming the program, audited `GR` (`F`, the function refused), and the operation does nothing; a program that cannot be called or ends in an exception refuses, as for the host servers. A point with no program allows everything.

**Audit journal.** Beside the `AF` and `GR` entries above, the entries are the commands' own: `SV` for a system value changed (old and new value, as `CHGSYSVAL` writes it), and the job entries the subsystem and power commands write with `*JOBDTA` and `*SYSMGT` auditing. QAP adds no entry type and no entry of its own.

**Rules.**

- Lists that can be long (`ListSubsystems`, `ListPtfs`) take `start` and `max` and answer `total`, as every QAP list does. The others are short and come whole.
- `EndSubsystem` with `sbsd` none is `ENDSBS *ALL`: the system is in restricted state when it answers, and `IplStatus` says so. The connection's own job ends with its subsystem (`QUSRWRK`): the client gets the reply, then `ServerEnding`.
- `PowerDown` answers `Ok` once the power down has started, then every connection gets `ServerEnding` as the subsystems end. With `restart` the system comes back with an IPL; a client reconnects and reads `IplStatus` to see it complete.
- `SetSystemValue`'s reply is the value as the system keeps it; a value that takes effect at the next IPL is the new value, as `DSPSYSVAL` shows it.
- `ListHardwareResources` lists what `WRKHDWRSC TYPE()` lists, in its order, the guest agent's channel among the `*CMN` resources when the system runs under a hypervisor that offers one.
- No event: a client that follows subsystems or IPL state asks again; message queue watches (version 6) carry the operator's messages.

### Service 9 `Develop`: libraries, objects, source members and compiles (version 8, draft)

**Draft until QS/400 ships it.** Version 8 is what Code for QS/400, a VS Code extension, needs beside versions 1 to 7: the library list, objects, source members with their sequence numbers and dates, and compiles that return their diagnostics. Nothing in versions 1 to 7 changes; a peer that negotiates 7 or less never sees `Develop`, and its requests for it get `InvalidRequest`.

Every operation runs as the signed-on profile and does what the command named beside it does, with that command's authority checks and messages. Names are system names; the server folds them to upper case, as the command line folds a name not in quotes. A generic `pattern` is a name, a name ending in `*` (`ASSET*`), or `*ALL`; empty is `*ALL`.

- `LibraryKind` enum: `Production` 0 (`TYPE(*PROD)`), `Test` 1 (`TYPE(*TEST)`).
- `LibraryList` = { `current` Option of String (none: no current library), `system` list of String (the system part and then the product libraries, in search order), `user` list of String (in search order) }.
- `Library` = { `name` String, `kind` LibraryKind, `text` String }.
- `ObjectEntry` = { `library` String, `name` String, `type` String (`*FILE`), `attribute` String (`PF`, `RPGLE`; empty when none), `text` String, `size` u64 (bytes), `changed` i64, `owner` String }.
- `Attribute` = { `name` String, `value` String }.
- `MemberEntry` = { `name` String, `srctype` String, `text` String, `records` u32, `created` i64, `changed` i64 }.

| Service | Op | Request body | Reply body | Lane | Stands for |
| --- | --- | --- | --- | --- | --- |
| Develop | 1 `GetLibraryList` | — | `LibraryList` | lane | `DSPLIBL` |
| Develop | 2 `SetLibraryList` | `current` Option of String, `user` list of String | `LibraryList` (as it now is) | lane | `CHGLIBL` |
| Develop | 3 `ListLibraries` | `pattern` String, `start` u32, `max` u32 | `total` u32, `libraries` list of `Library` (by name) | concurrent | `WRKLIB` |
| Develop | 4 `ListObjects` | `library` String (`*LIBL`, `*CURLIB`, `*USRLIBL`, `*ALL` or a library), `pattern` String, `types` list of String (empty: every type), `attribute` Option of String, `start` u32, `max` u32 | `total` u32, `objects` list of `ObjectEntry` (by library in search order, then name, then type) | concurrent | `WRKOBJ` |
| Develop | 5 `DescribeObject` | `object` ObjectName, `type` String | `attributes` list of `Attribute` (in `DSPOBJD DETAIL(*FULL)` order) | concurrent | `DSPOBJD` |
| Develop | 6 `ListMembers` | `file` ObjectName, `pattern` String, `start` u32, `max` u32 | `total` u32, `members` list of `MemberEntry` (by name) | concurrent | `WRKMBRPDM`, `DSPFD TYPE(*MBRLIST)` |
| Develop | 7 `ReadMember` | `member` MemberName, `lock` flag | `srctype` String, `text` String, `width` u16 (the record's data length), `ccsid` u16 (the file's), `records` list of `SourceRecord` (split with `More` when large) | concurrent | `STRSEU` |
| Develop | 8 `WriteMember` | `member` MemberName, `records` list of `SourceRecord`, `renumber` flag, `srctype` Option of String, `text` Option of String, `first` flag, `last` flag | `records` u32 | concurrent | `STRSEU`'s save |
| Develop | 9 `UnlockMember` | `member` MemberName | — | concurrent | `DLCOBJ` |
| Develop | 10 `CreateMember` | `member` MemberName, `srctype` String, `text` String | — | concurrent | `ADDPFM` |
| Develop | 11 `CopyMember` | `from` MemberName, `to` MemberName, `replace` flag | — | concurrent | `CPYSRCF` |
| Develop | 12 `RenameMember` | `member` MemberName, `new_name` String | — | concurrent | `RNMM` |
| Develop | 13 `DeleteMember` | `member` MemberName | — | concurrent | `RMVM` |
| Develop | 14 `ChangeMember` | `member` MemberName, `srctype` Option of String, `text` Option of String | — | concurrent | `CHGPFM` |
| Develop | 15 `Compile` | `command` String | `job` JobId, `messages` list of `ServerMessage`, `diagnostics` list of `Diagnostic` | lane | the command |

| Op | Statuses |
| --- | --- |
| `GetLibraryList` | `Ok` |
| `SetLibraryList` | `Ok`; `NotFound` (a library, `CPF2110`); `NotAuthorized` (`CPF2182`); `Error` (`CHGLIBL`'s escape message, for example a library named twice) |
| `ListLibraries`, `ListObjects`, `ListMembers` | `Ok`; `NotFound` (the library or file); `NotAuthorized` |
| `DescribeObject` | `Ok`; `NotFound`; `NotAuthorized` |
| `ReadMember` | `Ok`; `NotFound` (the file or member); `NotAuthorized`; `InUse` (`lock` 1 and another job holds the member; `lock` 0 and another job holds it `*EXCL`); `Error` (not a source file, or data the server cannot convert) |
| `WriteMember` | `Ok`; `NotFound`; `NotAuthorized`; `InUse` (another job holds the member); `Error` (data that does not convert to the file's CCSID); `InvalidRequest` (pieces out of order, sequence numbers not ascending or out of range, a record longer than the file's data length) |
| `UnlockMember` | `Ok`; `NotFound` (the member is not locked by this connection) |
| `CreateMember` | `Ok`; `NotFound` (the file); `NotAuthorized`; `Exists` |
| `CopyMember`, `RenameMember` | `Ok`; `NotFound`; `NotAuthorized`; `Exists` (`to` or `new_name` taken; for `CopyMember` only without `replace`); `InUse` |
| `DeleteMember`, `ChangeMember` | `Ok`; `NotFound`; `NotAuthorized`; `InUse` |
| `Compile` | as `RunCommand`; `Error` when the compile failed (the message is the command's escape message, and the body is still filled) |

**Rules.**

- Lanes: `GetLibraryList`, `SetLibraryList` and `Compile` run in the connection's lane, in arrival order with `Command` requests and `Sql` 1 to 8, since they use the job's library list. Every other `Develop` operation runs concurrently, as the signed-on profile.
- The library list is the connection job's. `SetLibraryList` changes it as `CHGLIBL LIBL(..) CURLIB(..)` does: `user` empty is `LIBL(*NONE)`, `current` none is `CURLIB(*CRTDFT)` (no current library), so a list `GetLibraryList` answered can be set again as it is. `Command.RunCommand`, `Compile` and the `*LIBL` and `*CURLIB` searches of every other operation see it.
- Lists show what the commands show the profile: libraries and objects it has some authority to, as `WRKLIB` and `WRKOBJ` list them. `ListObjects` `*ALL` is every library the profile can see; `*USRLIBL` the user part of the library list; `types` and `attribute` filter (`attribute` none: any).
- `DescribeObject`'s `name`s are the server's labels (`Library`, `Object`, `Type`, `Attribute`, `Owner`, `Text`, `Created`, `Changed`, `Size`, ...), its `value`s the text `DSPOBJD` shows; a client shows them as they come.
- `ReadMember`:
  - `data` is the record's data converted from the file's CCSID to UTF-8, trailing blanks removed; `width` is the data length in the file's bytes. A file whose CCSID cannot be converted (65535) is `Error` with `TCP1A5A`.
  - With `lock` 1 the member is held `*SHRNUP` for the connection, as `STRSEU` holds a member it edits, until `UnlockMember`, a `WriteMember` of it by the same connection, or the connection's end. Other jobs read it meanwhile; none changes it. A member another job holds (any lock but `*SHRRD`) is `InUse` with `CPF1002`.
- `WriteMember` replaces the member's records:
  - A member over 1 MiB is sent in pieces: `first` 1 on the first piece, `last` 1 on the last (one request is both). Each piece is sent after the reply to the one before. The server keeps the pieces for the connection and writes the member, whole and at once, when the last arrives, so no other job ever reads a member half written, and the comparison below is with the member as it was. `renumber`, `srctype` and `text` are the last piece's. A piece with `first` 0 when no write of that member is in progress on the connection is `InvalidRequest`; a `first` 1 discards a write of the same member in progress. Pieces not written are discarded when the connection ends. The reply's `records` is the records received so far on a piece that is not the last, and the member's records on the last.
  - Without `renumber`, `seq` must be ascending, each from 1 to 999999; with `renumber` 1 the records are numbered from `0001.00` in steps of `0001.00` (at most 9999 records).
  - Dates: a record whose data equals the data of the member's record with the same `seq` (its `seq` before renumbering) keeps the `date` sent; every other record, changed or new, is dated today (the job's date). A client that reads a member and writes the same records back leaves it as it was.
  - `data` is converted to the file's CCSID; a character with no encoding there is `Error` with `TCP1A5A`, and nothing is written. Data longer than the record's data length once converted is `InvalidRequest`.
  - The member must exist (`CreateMember` adds one). Another job's lock on it (any but `*SHRRD`) is `InUse` with `CPF1002`. A lock this connection holds ends with the write.
  - `srctype` and `text` none leave the member's as they are.
- `UnlockMember` ends this connection's lock; a member it does not hold is `NotFound` with `TCP1A61`.
- `CreateMember` is `ADDPFM`; `CopyMember` is `CPYSRCF` of one member, sequence numbers and dates kept, `MBROPT(*REPLACE)` with `replace` 1; `RenameMember` is `RNMM`; `DeleteMember` is `RMVM`; `ChangeMember` is `CHGPFM`, `srctype` and `text` none leaving each as it is. Each answers with its command's messages; a name taken is `Exists`.
- `Compile` runs `command` as `RunCommand` does, in the connection job, and answers its `CommandReply` with the diagnostics the compiler left on the job for that command (`rpg/compdiag`): `CRTBNDRPG`, `CRTRPGMOD`, `CRTSQLRPGI`, `CRTBNDCL`, `CRTCLMOD`, `CRTPF`, `CRTLF`, `CRTDSPF`, `CRTPRTF`, `CRTCMD`, `RUNSQLSTM`, `CRTPGM`, `CRTSRVPGM`. A command that is not a compile answers its messages and no diagnostics. `Error` when the compile failed, the body still filled; `Cancelled` as `RunCommand`.

### Service 10 `Jobs`: jobs and job logs (version 8, draft)

**Draft until QS/400 ships it**, with `Develop`. A peer of version 7 or less never sees `Jobs`.

- `JobState` enum: `Active` 0, `OnJobQueue` 1, `OnOutputQueue` 2 (ended, its spooled files kept).
- `JobEntry` = { `id` JobId, `state` JobState, `status` String (as `WRKUSRJOB` shows it: `*ACTIVE`, `*JOBQ`, `*OUTQ`), `type` String (`BATCH`, `INTER`, `AUTO`, `PJ`, `SBS`, `SYS`, `RDR`, `WTR`), `subsystem` String (empty when not active), `function` String (as `WRKACTJOB` shows it, `PGM-QCMD`; empty when none), `entered` i64 (when the job entered the system) }.
- `JobLogEntry` = { `message` ServerMessage, `kind` MessageType (service 7's), `sent` i64, `from_program` String, `to_program` String }.

| Service | Op | Request body | Reply body | Stands for |
| --- | --- | --- | --- | --- |
| Jobs | 1 `ListJobs` | `user` String (`*CURRENT`, `*ALL`, a profile), `status` Option of JobState, `start` u32, `max` u32 | `total` u32, `jobs` list of `JobEntry` (active first, then on job queues, then on output queues; each by when it entered) | `WRKUSRJOB` |
| Jobs | 2 `JobLog` | `job` JobId, `start` u32, `max` u32 | `total` u32, `entries` list of `JobLogEntry` (oldest first) | `DSPJOBLOG` |
| Jobs | 3 `HoldJob` | `job` JobId | — | `HLDJOB` |
| Jobs | 4 `ReleaseJob` | `job` JobId | — | `RLSJOB` |
| Jobs | 5 `EndJob` | `job` JobId, `how` EndHow, `delay` Option of u32 (seconds; none: the job's default) | — | `ENDJOB` |

| Op | Statuses |
| --- | --- |
| `ListJobs` | `Ok`; `NotFound` (an unknown profile, `CPF2204`); `NotAuthorized` (another profile's jobs, or `*ALL`, without `*JOBCTL`) |
| `JobLog` | `Ok`; `NotFound` (no such job, or an ended job without a spooled job log); `NotAuthorized` |
| `HoldJob`, `ReleaseJob`, `EndJob` | `Ok`; `NotFound` (`CPF1321`); `NotAuthorized`; `Error` (the command's escape message, for example a job already held) |

**Rules.** Every operation runs concurrently as the signed-on profile, with its command's checks: the profile's own jobs need nothing, another user's need `*JOBCTL`, as the commands require. `JobLog` of an active job is its job log now; of an ended job, its spooled job log (`QPJOBLOG`) if one exists, else `NotFound`. `EndJob` of the connection's own job is `InvalidRequest`: a client signs off instead.

### `Sql` additions (version 8, draft)

- `RoutineKind` enum: `Procedure` 0, `Function` 1.
- `Routine` = { `schema` String, `name` String, `specific` String (the specific name), `kind` RoutineKind, `text` String }.
- `Index` = { `schema` String, `name` String, `unique` flag, `columns` list of String (the key columns, in key order) }.

| Service | Op | Request body | Reply body | Allowed before sign-on |
| --- | --- | --- | --- | --- |
| Sql | 12 `ListRoutines` | `schema` String, `pattern` String (SQL `LIKE`), `kinds` list of RoutineKind (empty for both), `start` u32, `max` u32 | `total` u32, `routines` list of `Routine` (by name, then specific name) | no |
| Sql | 13 `ListIndexes` | `schema` String, `table` String | `indexes` list of `Index` (by name) | no |

Statuses as `ListSchemas` to `ListColumns`: `Ok`; `NotFound`; `NotAuthorized`. Both read the catalog (`QSYS2.SYSROUTINES`, `SYSINDEXES`, `SYSKEYS`) with the user's authority, keep no state and run concurrently, as operations 9 to 11 do.

### Service 11 `Debug`: the source debugger (version 9, draft)

**Draft until QS/400 ships it.** Version 9 is the debugger Code for QS/400 needs: the system's own source debugger (`STRDBG`), its stops reported over QAP instead of on a display. Nothing in versions 1 to 8 changes; a peer that negotiates 8 or less never sees `Debug`, its bodies or its events.

A connection has one debug session at a time. The session debugs either the connection job itself (`Own`: the programs `Command.RunCommand` calls while the session is active) or another job (`Job`), attached as `STRSRVJOB` attaches it, or attached by a service entry point when it calls the program the entry names. Every operation runs concurrently on the connection's secondary threads, never in the lane, as the signed-on profile; the events are sent from the concurrent lane, as the watches' are.

- `DebugTarget` enum: `Own` 0, `Job` 1 { `job` JobId }.
- `View` enum: `Statement` 0, `Source` 1, `Listing` 2.
- `DebugModule` = { `name` String, `views` list of View }.
- `LineBreakpoint` = { `line` u32 (the `*SOURCE` view's, 1-based), `condition` Option of String (`variable op value`, `op` one of `=`, `<>`, `<`, `<=`, `>`, `>=`) }.
- `BreakpointSet` = { `line` u32 (the line it landed on), `stmt` u32 }.
- `ResumeHow` enum: `Go` 0, `StepOver` 1, `StepInto` 2, `StepOut` 3.
- `Frame` = { `program` ObjectName, `module` String, `procedure` String, `stmt` u32, `line` u32 (0 when not known), `view` View (the view `line` is of) }.
- `Variable` = { `name` String, `type` String, `value` String (as `EVAL` shows it; empty for a structure or array with parts), `expandable` flag }.
- `StopReason` enum: `Breakpoint` 0, `Step` 1, `ServiceEntry` 2, `Exception` 3 { `message` ServerMessage }, `ProgramEnd` 4.

| Service | Op | Request body | Reply body | Allowed before sign-on |
| --- | --- | --- | --- | --- |
| Debug | 1 `Start` | `target` DebugTarget | `session` u32 | no |
| Debug | 2 `End` | — | — | no |
| Debug | 3 `AddProgram` | `program` ObjectName, `type` String (`*PGM`, `*SRVPGM`) | `modules` list of `DebugModule` | no |
| Debug | 4 `SetBreakpoints` | `program` ObjectName, `module` String (empty: the first), `lines` list of `LineBreakpoint` | `set` list of `BreakpointSet`, one per line asked, in order | no |
| Debug | 5 `Resume` | `how` ResumeHow | — | no |
| Debug | 6 `Stack` | — | `frames` list of `Frame`, the newest first | no |
| Debug | 7 `Variables` | `frame` u32 (0 the newest), `parent` Option of String (a structure or array to expand) | `variables` list of `Variable` | no |
| Debug | 8 `Eval` | `frame` u32, `expression` String (`NAME`, `NAME:x`, `NAME:c`) | `value` String | no |
| Debug | 9 `SetServiceEntry` | `program` ObjectName, `user` String (`*CURRENT` or a profile) | — | no |
| Debug | 10 `RemoveServiceEntry` | `program` ObjectName | — | no |

| Op | Statuses |
| --- | --- |
| `Start` | `Ok`; `InUse` (a session is active on the connection, `TCP1A72`; the job is serviced already, `CPF3503`); `NotFound` (no such job, `CPF3520`); `NotAuthorized` (another user's job without `*SERVICE`, `CPF3502`); `Error` (the connection's own job named, `CPF3501`) |
| `End` | `Ok`; `Error` (no session, `TCP1A71`) |
| `AddProgram` | `Ok`; `NotFound` (`CPF9801`); `NotAuthorized`; `Error` (compiled `DBGVIEW(*NONE)`: `CPF9C15`, the message `STRDBG` gives on the primary oracle, `tools/conformance/oracle/debug/primary`) |
| `SetBreakpoints` | `Ok`; `NotFound` (the program was not added, or no such module, `CPF9801`); `Error` (no statement starts on or after a line, `CPD9CCB`; a condition that does not read, `CPF9C59`); nothing changes when one line fails |
| `Resume`, `Stack` | `Ok`; `Error` (no job is stopped, `CPD9CCC`) |
| `Variables`, `Eval` | `Ok`; `Error` (no job is stopped, `CPD9CCC`; `Eval` of a name that is not a variable of the frame, `CPF7E12`); `InvalidRequest` (no such frame, `TCP1A77`) |
| `SetServiceEntry` | `Ok`; `NotFound` (`CPF9801`); `NotAuthorized` (another user's jobs without `*SERVICE`, `TCP1A74`) |
| `RemoveServiceEntry` | `Ok`; `NotFound` (not set by this session, `TCP1A73`) |

Every operation but `Start` answers `Error` with `TCP1A71` when no session is active on the connection.

**Events (`request_id` 0, status `Ok`).** `Debug` op 100 `Stopped` (body `job` JobId, `reason` StopReason, `frame` Frame: the newest frame, as `Stack` gives it), each time a debugged job stops; it waits for `Resume`. `Debug` op 101 `DebugEnded` (body `message` Option of ServerMessage), when the session ends other than by `End`: the debugged job ended (`TCP1A75`), or the connection signed off (`TCP1A76`).

**Authority.**

- `Start` with `Job` checks as `STRSRVJOB`: a job of the signed-on profile needs nothing; another user's needs `*SERVICE`.
- `SetServiceEntry` for a `user` other than the signed-on profile needs `*SERVICE`.
- `AddProgram` finds the program as `STRDBG` does, in the connection job's library list for `*LIBL`.

**Rules.**

- Line numbers are the `*SOURCE` view's. A breakpoint lands on the first statement that starts on or after its line; `BreakpointSet` says where. A module without a source view takes statement numbers as lines and answers `Statement` frames.
- `SetBreakpoints` replaces the module's breakpoints; an empty `lines` removes them. Breakpoints may change while a job is stopped.
- A breakpoint with a `condition` stops only when the condition holds at the statement, the variable's value read as `EVAL` shows it (compared as numbers when both sides are numbers).
- `Own`: programs added go under debug in the connection job and the breakpoints reach it from its next call. A `RunCommand` whose program stops gets no reply while it is stopped; meanwhile `Stack`, `Variables`, `Eval` and `Resume` answer, and its reply comes when the command ends. A `Cancel` of the command while it is stopped ends it as any cancel does.
- `Job`: the job takes the session's programs and breakpoints at its next program call or break poll, as a job serviced by `STRSRVJOB` takes them. A stop holds the job exactly as `STRDBG` from the console holds it: a display session waits at its keyboard-locked display until `Resume`.
- `SetServiceEntry`: the next call of the program (from a command line, a menu, a program's call) by a job running as `user` attaches that job to this session, puts the program under debug there and stops it at the program's first statement with reason `ServiceEntry`. The entry fires while the session debugs no other job; while a job is attached, other jobs' calls run on. An attached job stays attached until the session ends, and the session's breakpoints apply to it.
- `StepOver` stops at the next statement of the stopped procedure or its callers, `StepInto` at the next statement anywhere, `StepOut` at the next statement of a caller.
- `Variables` without `parent` lists a frame's own variables; with it, the parts of a structure (`DS.SUB`) or array (`ARR(1)`) whose `expandable` was 1.
- `End`, a sign-off and the connection's close end the session: a stopped job goes on, an attached job is let go, the service entry points are removed.

`ServerMessage`s in bodies are in the job's language, as every message is.

### `SignOn` additions (version 2)

| Service | Op | Request body | Reply body | Allowed before sign-on |
| --- | --- | --- | --- | --- |
| SignOn | 5 `TelnetTicket` | — | `ticket` 32 bytes, `expires_secs` u32 | no |
| SignOn | 6 `PasswordStatus` | — | `expires` Option of i64 (none when the password does not expire), `warning_days` u16 (the system's warning period) | no |

| Op | Statuses |
| --- | --- |
| `TelnetTicket` | `Ok`; `NotAuthorized` (the profile may not sign on automatically) |
| `PasswordStatus` | `Ok` |

A ticket is 256 random bits for the session's own profile, kept by the server, good for one automatic sign-on of a display session within `expires_secs`, and only over Telnet with TLS. The client sends it, as 64 hexadecimal characters, where automatic sign-on otherwise carries the password; the client sends the Telnet environment's user variable, and the password variable carrying the ticket as 64 hexadecimal characters with an empty random-seed variable, and only over Telnet TLS. The client treats a ticket as it treats a token: never logged or stored, wiped after use.

### `SignOn` addition (version 4)

| Service | Op | Request body | Reply body | Allowed before sign-on |
| --- | --- | --- | --- | --- |
| SignOn | 7 `Attach` | `token` 32 bytes | `SignOnReply` (same token) | yes (as the first request after `Hello`) |

| Op | Statuses |
| --- | --- |
| `Attach` | `Ok`; `SessionExpired` (no such session, or it timed out); `Busy` (the session has 16 connections); `InvalidRequest` (not the first request after `Hello`) |

- `Attach` adds this connection to an existing session without taking the session from its other connections. `Resume` still moves the session's resumable connection, the one that signed on or last resumed, to the new one; attached connections stay.
- Each attached connection has its own connection job, running as the session's profile, with its own job state: library list, `QTEMP`, commitment definition, statements and cursors.
- A session has at most 16 connections, attached or not; beyond that `Attach` gets `Busy`.
- The session's idle timer runs only while no connection is attached to it.
- `SignOff` on any connection ends the session. That connection gets its reply and stays open, not signed on, as in version 1; every other connection of the session gets `SessionEnded` (status `Ok`, `TCP1A58`) and closes.

### `Connection` addition (version 4)

| Service | Op | Request body | Reply body | Allowed before sign-on |
| --- | --- | --- | --- | --- |
| Connection | 3 `JobInfo` | — | `job` JobId (the connection job as the system names it), `user` String (the profile it runs as now: the signed-on one, else the job's user) | yes |

`JobInfo` is answered at once, never queued, as `Ping` is. A client that will `Attach` sends `Attach` first: it must be the first request after `Hello`.

### Compatibility of new values

A Wire enum fails to decode on a tag it does not know, so an older client would reject a whole envelope carrying a newer `Status`, `Service` or `Kind` value, or a newer value of any enum in a body, before it could even read the `request_id`. Rule: **a value is sent only to a peer that negotiated a version that defines it.**

- Every enum value, operation and event added after version 1 is defined from one protocol version. `Hello` fixes the version for the connection.
- The server sends no `Kind`, `Service` or `Status` value, no event and no enum value in a body that the connection's version does not define. Where a newer status would apply it sends `Error` with the same message; where a newer value of a body enum would apply, the operation's definition names the older value to send instead.
- `Hello`'s `services` lists only the services the chosen version defines.
- A client sends only services, operations and values its negotiated version defines. A server that gets one its version does not define replies `InvalidRequest`; it cannot decode a newer `Service` or `Kind` at all, so the client must never send one.
- A frame that does not decode is still a protocol error that ends the connection, on either side. Within a version, optional features are announced with `Hello`'s `capabilities` strings, never with new enum values.

This keeps Wire and the envelope as they are; the cost is that the server encodes per connection version.

### Version 2 (fixed)

Version 2 is: `Command` (service 3) operations 1 `RunCommand`, 2 `CheckCommand` and 3 `DescribeCommand`; `SignOn` operations 5 `TelnetTicket` and 6 `PasswordStatus`; and the new status values those operations use, which are none: every status they answer exists in version 1. `Exists`, `InUse` and `SqlError` belong to the versions whose operations first use them.

### Version 3 (fixed)

Version 3 is: `Spool` (service 4) operations 1 `ListSpooledFiles` to 7 `ListOutputQueues`, as above, with the reading rules above; and the status `InUse` (14), which version 3 introduces. A version 1 or 2 peer never receives `InUse`, the `Spool` service or any `Spool` body. The server accepts versions 1 to 3 and picks the highest both sides support.

### Version 4 (fixed)

Version 4 is: `Sql` (service 5) operations 1 `SetOptions` to 11 `ListColumns`, with the types and rules above; `SignOn` operation 7 `Attach`; `Connection` operation 3 `JobInfo`; and the status `SqlError` (15), which version 4 introduces. `Sql` does not use `Exists`: a name that is taken is the database's `SqlError` (for example SQLSTATE `42710`), so `Exists` (13) stays for the version that first uses it. A version 1 to 3 peer never receives `SqlError`, the `Sql` service or any `Sql` body; where `SqlError` would apply it gets `Error` with the same message. The server accepts versions 1 to 4 and picks the highest both sides support.

### Version 5 (fixed)

Version 5 is: `Ifs` (service 6) operations 1 `ListDirectory` to 8 `Copy`, with the types and rules above, `ReadFile`'s `as_ccsid` and `next_offset` and `WriteFile`'s `data_ccsid` included; and the status `Exists` (13), which version 5 introduces. `InUse` is version 3's. A version 1 to 4 peer never receives `Exists`, the `Ifs` service or any `Ifs` body; where `Exists` would apply it gets `Error` with the same message. The server accepts versions 1 to 5 and picks the highest both sides support.

### Version 6 (fixed)

Version 6 is:

- `Messages` (service 7) operations 1 `ListMessages` to 6 `UnwatchQueue` and the event `MessageArrived` (100), with the types and rules above, `SenderCopy` and `answers` included;
- `Spool` operations 8 `WatchSpool` and 9 `UnwatchSpool` and the event `SpooledFileChanged` (100);
- `Ifs` operation 9 `Stat`.

It introduces no status: every status these operations answer exists in an earlier version. Events are sent from the concurrent lane, as every operation outside the lane runs. A version 1 to 5 peer never receives the `Messages` service, a watch's event or any of these bodies. The server accepts versions 1 to 6 and picks the highest both sides support.

### Version 7 (draft)

Version 7 is `Manage` (service 8) operations 1 `ListSubsystems` to 10 `PowerDown`, with the types and rules above, and the exit point `QS400_QAP_MGMT`. It introduces no status and no event: every status these operations answer exists in an earlier version. A version 1 to 6 peer never receives the `Manage` service or any of its bodies, and its requests for them get `InvalidRequest`. The server accepts versions 1 to 7 and picks the highest both sides support. Until QS/400 ships version 7 its operations and bodies may still change; versions 1 to 6 may not.

### Version 8 (draft)

Version 8 is:

- `Develop` (service 9) operations 1 `GetLibraryList` to 15 `Compile`, with the types and rules above;
- `Jobs` (service 10) operations 1 `ListJobs` to 5 `EndJob`;
- `Sql` operations 12 `ListRoutines` and 13 `ListIndexes`;
- the common types `MemberName`, `SourceRecord`, `Where` and `Diagnostic`, and `EndHow` as a common type (its values unchanged).

It introduces no status and no event: every status these operations answer exists in an earlier version. A version 1 to 7 peer never receives `Develop`, `Jobs`, `Sql` 12 or 13 or any of their bodies, and its requests for them get `InvalidRequest`; `Hello` lists services 9 and 10 only at version 8 and above. The server accepts versions 1 to 8 and picks the highest both sides support. Until QS/400 ships version 8 its operations and bodies may still change; versions 1 to 6 may not.

**Why `WriteMember` pieces carry `first` and `last` rather than an offset.** A member is records, not bytes, and it is written whole: the server compares each record with the member's to keep or set its date, renumbers the whole list, and replaces the records in one step, so a reader never sees half of a write. With an offset, each piece would be a write of its own and the member would stand half old and half new between pieces (and after a client that stops); with the pair, the server holds the pieces and writes once, at `last`. A client that stops before `last` changes nothing.

### Version 9 (draft)

Version 9 is `Debug` (service 11) operations 1 `Start` to 10 `RemoveServiceEntry` and the events `Stopped` (100) and `DebugEnded` (101), with the types and rules above. It introduces no status: every status these operations answer exists in an earlier version. A version 1 to 8 peer never receives the `Debug` service, its events or any of its bodies, and its requests for them get `InvalidRequest`. The server accepts versions 1 to 9 and picks the highest both sides support. Until QS/400 ships version 9 its operations and bodies may still change.

## Open questions

Settled; see QS/400's implementation below.

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
- `Cancel` gets no reply of its own; a cancel naming nothing
  outstanding is ignored. The server reads only its `request_id`.
- A request cancelled before it started ends with `Cancelled` and an
  empty body.
- A request cancelled once started ends with `Cancelled` and the
  operation's own body and message, as far as it got: `RunCommand`'s
  is its `CommandReply`, every message included, the one the cancel
  caused too.
- A first request other than `Hello` gets `InvalidRequest` and the
  connection closes. `SignOn` or `Resume` on a signed-on connection
  gets `InvalidRequest`; the connection and session stay.
- A connection idle for 300 s is closed without an event. Idle means
  no frame either way and no request outstanding: a request still
  running (a long command) keeps the connection.
- Every close is logged by the connection job (`net/qapd`, target
  `qapd`, level `info`): the connection, the job, the client's
  address, the profile, the reason and the requests outstanding.
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

Version 2, settled with Cody on 2026-11-02:

- The "Later phases" section above came from QS/400 Access's copy of
  this spec (its commit `0774f1e`), verbatim but for: the
  compatibility rule adopted, `DescribeCommand` added, the ticket's
  wire form fixed, and version 2 fixed.
- Version negotiation: the server accepts 1 and 2. A version-1
  connection never receives a version-2 value; where a newer status
  would apply it gets `Error` with the same message. `Hello`'s
  `services` lists only what the chosen version defines, and a
  request for an operation the version does not define gets
  `InvalidRequest`.
- `QRMTSIGN` decides automatic sign-on, for tickets and the Telnet
  password substitute alike: `*VERIFY` and `*SAMEPRF` allow it,
  `*FRCSIGNON` shows the sign-on display, `*REJECT` refuses the
  connection. It ships as `*VERIFY`. `TelnetTicket` answers
  `NotAuthorized` when `QRMTSIGN` would not sign the profile on
  automatically, or the profile cannot sign on (disabled, `*NONE`
  password, password expired).
- A ticket is checked only on a TLS connection, with the seed variable
  present and empty; it is used up by its first use, accepted or not.
  At its use, as at its issue, `QRMTSIGN` and the profile are checked
  again: a ticket for a profile since disabled, or whose password has
  expired, signs nothing on.
  A plain-text password through automatic sign-on is never accepted.
  Issue and use are audited (`PS` entries `P` and `V`); ticket values
  are never logged.
- `DescribeCommand`'s `help` strings are empty: QS/400 ships no help
  panel groups for commands yet, and the display prompter shows none
  either. Elements and qualifiers have the prompt text their
  definitions give.
- The lanes: `Command` requests run on the connection job's own
  thread. Every other operation runs on a secondary thread of the same
  job, made when none is free. Each secondary thread has its own link
  to the storage service (`hello_thread`). The link shares the job's
  security session, so a sign-on on any thread swaps the profile for
  all of them, and the job's runtime follows before its next command.
  `Hello` and `Ping` are answered where requests are routed.
- `RunCommand` and `CheckCommand` run in the connection job through
  the same analyzer as Command Entry. `Cancel` of a running command
  ends its request as System Request option 2 does (`CPF1907`), and
  the reply is `Cancelled` with the command's `CommandReply`.

Version 3, as built on 2026-11-02 (decisions made in the build, for Cody to review):

- The server accepts versions 1 to 3. `Spool` runs on the connection
  job's secondary threads, as the signed-on profile, through the
  storage service (`stgsvc/qap/spool.rs`). Each operation calls the
  same functions as the spooled-file command it stands for, with that
  command's authority checks and `SF` audit entries:
  - `ListSpooledFiles` lists as `WRKSPLF` does, or as `WRKOUTQ` does
    for one queue;
  - `ReadSpooledFile` reads as `DSPSPLF` (`SF` `R`);
  - `HoldSpooledFile` holds as `HLDSPLF` (`H`);
  - `ReleaseSpooledFile` releases as `RLSSPLF` (`L`);
  - `DeleteSpooledFile` deletes as `DLTSPLF` (`D`);
  - `MoveSpooledFile` moves as `CHGSPLFA OUTQ` (`A`);
  - `ListOutputQueues` lists as `WRKOUTQ`.
- Status values from QS/400's: `RDY` `Ready`, `HLD` `Held`, `OPN`
  `Open`, `CLO` `Closed`, `WTR` `Printing`, `MSGW` `Message`, `PND`
  and `DFR` `Pending`, `SAV` `Saved`. QS/400 has no separate state
  for a file being written, so it never sends `Writing`. Device types
  come from `DEVTYPE`: `*SCS` is `Scs`; `*AFPDS`, `*AFPDSLINE` and
  `*IPDS` are `Afp`; `*USERASCII` is `Ascii`; `*LINE` is `Line`.
- The page index is computed from the data stream at each read; the
  `*SPLF` object keeps none and its layout is unchanged. Pages start
  where the SCS reader behind `DSPSPLF` starts them: at a form feed, a
  line past the page length, or a move up the page.
  - A range's leading state is the last format command of each kind
    before the page (`SHF`, `SVF`, `SLD`, the `X'2BD1'` and `X'2BD2'`
    commands by their first byte, any other by class), then an
    absolute move to the line and column before the page's first
    command.
  - `pages` in the reply is how many pages that reader finds.
- Only SCS files are read. `Text` and `Pdf` are the views `DSPSPLF`
  and Navigator use (`spool::view::text`, `spool::pdf::render`), made
  from the whole file's pages, so a range reads as those pages do in
  the whole. Any format of a file of another device type is `Error`
  with `TCP1A56`. A file still open is `InUse` with `TCP1A57`. Pages
  out of range are `InvalidRequest` with `CPD0084`.
- `HoldSpooledFile` `immediately` 0 is `HLDSPLF OPTION(*PAGEEND)`, 1 is
  `*IMMED`. A writer hands a file to its device whole, so the two
  hold alike: the file prints no more until released.
- `MoveSpooledFile` `first` 1 puts the file at the top of the queue,
  also on the queue it is already on. A printing file is not moved
  (`InUse`), by `CHGSPLFA OUTQ` either: that command now refuses it as
  `DLTSPLF` refuses deleting one (`CPF3330`).
- `ListOutputQueues` `*LIBL` is the connection job's library list;
  libraries in it that do not exist are passed over.

Version 4, `Attach`, as built on 2026-11-02 (decisions made in the build, for Cody to review):

- `Attach` must be the second request the connection routes, `Hello`
  being the first. After any other request, a `Ping` or a failed
  `Attach` included, it is `InvalidRequest` and the connection stays.
  On a version 1 to 3 connection it is `InvalidRequest` too.
- The session table (`qapd::sessions`) keeps one resumable job per
  session (the one that signed on or last resumed) and a list of
  attached jobs. `Resume` replaces the resumable job only; attached
  jobs stay. The idle timer starts when the last connection of either
  kind leaves. Dead jobs are swept from both, as before.
- The limit of 16 counts both kinds. `Attach` beyond it is `Busy` with
  `TCP1A59`. A `Resume` while the resumable connection is gone and 16
  others are attached would make 17, so it is `Busy` with `TCP1A59`
  too.
- An attached job swaps to the profile as `Resume` does (`become_user`,
  then the library list of the profile's job description). The rest of
  its job state is its own (rpc 749 `qap_attach`).
- When one job ends a session, the table records the session's other
  jobs as ended, and how. Each connection job asks at its end poll (rpc
  739 `qap_ended`, every 500 ms beside the `ENDJOB` check) and ends with
  `Ending::Session`: `SessionEnded` with status `Ok`, then close
  (`Closed::SessionEnded`), with no second sign-off or detach. A
  connection of any version gets this event, since all its values are
  version 1's.
- `SignOff` on any connection: that connection gets `Ok` and stays
  open, not signed on. The others get `TCP1A58`.
- `ENDJOB` of one connection job of a session with several: as before,
  that job sends `SessionEnded` with `TCP1A54` and ends the session
  (`qap_signoff` with `operator` set). The session's other connections
  then get `SessionEnded` with `TCP1A54` too, and close. `ENDTCPSVR`
  is unchanged: every connection gets `ServerEnding`.
- `TCP1A58` and `TCP1A59` are in `QTCPMSG`.

Version 4 Sql, as built on 2026-11-02 (decisions made in the build, for Cody to review):

- The lane: `Command` requests and `Sql` 1 to 8 (`Op::in_lane`) go to
  the connection job's own thread in arrival order; `ListSchemas`,
  `ListTables` and `ListColumns` run on the secondary threads. The
  `Sql` rules live in `net/qapd/src/sql.rs` (`SqlStop`: `SqlError`
  with `SqlFailure` and the SQL message, a `Refusal` for `NotFound`,
  `NotAuthorized` and `InUse`, or `Cancelled` with the body so far).
- Cancel: an `Execute` or `Fetch` the host finished after the client
  cancelled still ends `Cancelled`: the cursor it opened or read is
  closed, `cursor` none or `end` 0, with `SQL0952` (added to
  `QSQLMSG`). The connection job asks the engine to end a running
  statement with the file `/run/qs400/sqlcancel/<job number>`
  (`stg::sql_cancel_path`); the engine looks for it every 64 rows it
  reads (`SqlContext::interrupt`, `SQL0952`, SQLSTATE `57014`). The
  lane also looks between parameter rows and between fetch batches of
  100 rows.
- Statements run through the job's embedded SQL (`sql_run`, the
  job's own session in the storage service), so as the signed-on
  profile with the store's authority checks. An authority failure is
  now the engine's `SQL0551` (SQLSTATE `42501`) rather than
  `SQL0901`; QAP answers it `NotAuthorized` with that message.
  Every other failure is `SqlError` with the engine's SQLSTATE,
  SQLCODE and its own text as the message text.
- Statements and cursors are the job's embedded scope `QAP:LANE`; its
  commitment definition is `*SQLQAP`, started at the first statement
  under a commit level. `ReadUncommitted` is `*CHG`, `ReadCommitted`
  `*CS`, `RepeatableRead` and `Serializable` `*ALL` (the engine has no
  `*RR`). Cursors are opened `WITH HOLD`: they stay open across
  `Commit` and `Rollback`. `Commit` and `Rollback` without commitment
  control answer `Ok`.
- `SetOptions` is `InUse` (`SQL0752`, added) once a statement other
  than a query ran under a commit level since the last `Commit` or
  `Rollback`. Otherwise the new level holds from the next statement:
  the engine's commitment definition takes each statement's level
  between units of work (as `SET TRANSACTION ISOLATION LEVEL`, which
  the engine now has and refuses with `SQL0428`, SQLSTATE `25001`,
  once the unit changed something). The default schema is `SET
  SCHEMA` (none: `SET SCHEMA DEFAULT`); under `System` naming as
  under `Sql`, a default schema qualifies unqualified table, view,
  index and alias names in place of the library list, as the
  `DFTRDBCOL` parameter and `SET SCHEMA` do.
- When the connection closes, and when the job runs as another
  profile (sign-off, resume), the lane rolls back, ends the scope
  `QAP:` (its cursors and statements go) and resets the options. The
  job's end rolls back any commitment definition left as well.
- Values by the engine's `DESCRIBE` (which now also gives each
  entry's CCSID): integers `Integer`; `DECIMAL` and `NUMERIC` the
  engine's exact text; `DECFLOAT` (SQLTYPE 996, `length` its 16 or 34
  digits) `Decimal` with the engine's scientific text (`1.5E-30`,
  `-12.50`) or `Infinity`, `-Infinity`, `NaN`, `sNaN`; a `DECFLOAT`
  keeps its coefficient and exponent from insert to fetch (`1.50`,
  `1E+2` apart from `100`, `0E+3`, `-0`; `DECIMAL.md` §9), and so do
  `UPDATE`, column-to-column assignment, `CAST` to `DECFLOAT`, unary
  minus and arithmetic's own exponents, while `=` compares by value
  (`1.5 = 1.50`). A number literal with an exponent is a `DOUBLE`, but
  into or beside a `DECFLOAT` it is read from its text, so `1E+2` stays
  `1E+2` there; a `DOUBLE` value becomes a `DECFLOAT` of its shortest
  digits, a whole one with exponent 0; `REAL` and
  `DOUBLE` the shorter of the plain and E notation that reads back the
  same; `CHAR`, `VARCHAR`, `GRAPHIC`, `VARGRAPHIC`, `CLOB` (408) and
  `DBCLOB` (412) `Text`, with CCSID 37 (graphic and `DBCLOB` 1200) when
  the field has none; a DDS `H` field, `FOR BIT DATA` or CCSID 65535
  `Binary`, its CP037 bytes; `BINARY` (912), `VARBINARY` (908) and
  `BLOB` (404) `Binary`, CCSID 65535; `BOOLEAN` (2436) `Boolean`;
  dates, times and timestamps as milliseconds, a timestamp with the
  picoseconds past them and `scale` its precision, 0 to 12 (SQLLEN
  19, or 20 plus the precision). Parameters come back the same way: a
  `Decimal` for a `DECFLOAT` column is read as `DECFLOAT(34)` (special
  values too), `Binary` is bytes, `Boolean` a boolean, a `Timestamp`
  keeps every picosecond for the column to cut to its precision. A
  parameter the engine cannot take (a special value for a column that
  is not `DECFLOAT`, digits past 63) is `SqlError` `22018`
  (`SQL0302`) for its row.
- Catalog lists read `QSYS2.SYSSCHEMAS` and `SYSTABLES` (indexes left
  out; `T` table, `V` view, `A` alias, `P` physical file, `L`
  logical file) on the thread's own link, match the `LIKE` pattern and
  cut the page in the job, sorted by name. `ListColumns` prepares
  `SELECT *` from the table and answers its `DESCRIBE`, as `Prepare`
  does.
- Column names and labels are the engine's `DESCRIBE`: `name` the
  column's SQL name (the field's `ALIAS`, which `CREATE TABLE` keeps
  for a name longer than its system name; else the system name),
  `label` its heading (`LABEL ON` or DDS `COLHDG`) with each of its
  three parts filled to 20 characters, as a descriptor area's label; a column
  without a heading, or an expression, has its name as `label`. A
  column renamed with `AS` keeps its heading.
- The storage service is one thread, but no statement holds up
  another job: each query and each `INSERT`, `UPDATE` or `DELETE` runs
  on a stack of its own and takes turns with every other request
  there, and with the other statements in progress in any order
  (`STORAGE_SERVICE.md` "Long statements"). It yields between batches
  of every scan (a correlated subquery's too), between rows of the
  steps after them (filters, `EXISTS`, joins, grouping and aggregates,
  the select list, the sort's keys) and between rows a change writes,
  at most every 5 ms; meanwhile the other jobs' requests and this
  connection's catalog lists are answered. A query under
  `RepeatableRead` or `Serializable` (`*ALL`, whether `SetOptions` or
  `SET TRANSACTION` chose it) takes turns too: each row it reads is
  read-locked until `Commit` or `Rollback`, so its reads repeat; other
  jobs read those rows, but a change to one waits until then, up to
  the table's record wait (`WAITRCD`, 60 s for a table SQL made) and
  then fails with `SQL0913`; meanwhile everything else goes on, the
  holder's `Commit` or `Rollback` included. Rows other jobs add meanwhile may be read (both levels are
  `*ALL`; no table lock). `CREATE INDEX`, `ALTER TABLE` and `ROLLBACK`
  take turns too, holding the locks the rules give meanwhile: an index
  being built keeps its table from change (it is read) and is itself
  out of reach, an altered table is the statement's alone, a
  rolled-back unit keeps its rows locked; other tables and jobs go on.
  An `UPDATE` or `DELETE` whose condition has a subquery takes turns
  too, and so does `MERGE`, each decided as of its start: it finds its
  rows (and a merge's matches and actions) first, its subqueries'
  answers kept, and never sees its own changes. An index is loaded in
  steps too (its keys sorted in runs, its tree written a few pages at a
  time), and an altered table is rebuilt beside itself in parts and
  switched in one short step at the end, so a crash leaves it as it
  was. Still running to their end, each short: `CREATE TABLE`, `CREATE
  VIEW`, `LABEL ON`, `COMMIT`, `DROP`. The long commands `RunCommand`
  sends take turns the same way: `RUNSQL` and `RUNSQLSTM` (inside each
  statement and between them), `CPYF`, `CPYFRMIMPF`, `CPYTOIMPF`,
  `CRTLF` (its path built in steps) and `RGZPFM` (its reading; it
  writes the reorganized member in one step), each holding the locks
  its rules give throughout. Others still run to their end
  (`STORAGE_SERVICE.md` lists them and why). Step 90 runs an aggregate
  over `EXISTS` and a slow select on two connections and the third's
  command, catalog lists and query beside them, then the aggregate
  under `RepeatableRead` with the third's requests beside it and its
  change of a row read refused, then a subquery `UPDATE`, a `CREATE
  INDEX` and a `CPYF` over a large table with the third's requests
  beside each.
- Engine limits the build met, now lifted as the database rules have them:
  `SMALLINT`, `INTEGER` and `BIGINT` columns made by SQL take their
  whole 16, 32 and 64-bit range (a field flag, `DB_FORMAT.md`; a DDS
  `B` field keeps its digits), `SYSCOLUMNS` giving `LENGTH` 2, 4, 8 and
  `NUMERIC_PRECISION` 5, 10, 19. `REAL` and `DOUBLE` are IEEE binary
  values (`1E300`, `-2.5E-308`), a literal with an exponent a `DOUBLE`,
  a float beside a decimal a `DOUBLE` and beside a `DECFLOAT` a
  `DECFLOAT`; their wire tags are 9 and 10 (`embed::to_wire`), and a
  float's value reaches the client as it is. `CREATE ALIAS` makes the
  `*FILE` alias (attribute `DDMF`, naming a table and a member if one
  is given; `DB_FORMAT.md` `AliasAttr`), which queries and data changes
  read through, `SYSTABLES` lists as `A` and `DROP ALIAS` (or `DLTF`)
  deletes. `GRAPHIC` and `VARGRAPHIC` of CCSID 1200 or 13488 hold
  UTF-16, so any Unicode literal (`G'..'`, `N'..'` or a plain string)
  goes in. `VARBINARY` keys compare without padding (`X'01'` before
  `X'0100'`). LOB space an `UPDATE` or `DELETE` leaves is freed at
  once in a table that is not journaled and by `RGZPFM` in one that is
  (a journal's before images and `RMVJRNCHG` still need it); `CPYF`,
  `CPYTOIMPF` and `CPYFRMIMPF` copy LOB columns. `CURRENT
  TIMESTAMP(p)` has its precision; `SYSCOLUMNS.DATA_TYPE` spells a
  timestamp `TIMESTMP`; a predicate used as a value (`K = 1` in a select
  list) is a `BOOLEAN` (SQLTYPE 2436).

The test system's database (`qapd::testing::sql`, for QS/400's tests
and QS/400 Access's):

- A small statement runner over tables in memory, not QS/400's engine:
  its tables carry `DECFLOAT`, binary types, `BOOLEAN` and
  `TIMESTAMP(12)`, which the client must meet. It runs `SELECT` of
  `*` or columns with `WHERE col = x AND ...`, `INSERT ... VALUES`,
  `UPDATE` and `DELETE` over one table; anything else is the syntax
  error `SQL0104`, as a database that does not know the statement
  reports it.
- A `DECFLOAT` is assigned and compared with the engine's own code
  (`qdecimal`'s `DecFloat`): a `Decimal` or literal read as
  `DECFLOAT(34)` from its text and rounded to the column's 16 or 34
  digits, its exponent kept and its text the engine's (`sNaN` for a
  signaling NaN); `=` by value. So a value reads back the same here as
  from QS/400.
- Its fixtures and the cases they give are in the module's
  documentation: `HLINV.ASSETS` with Navigator's twelve assets in
  service, `HLTEST.ALLTYPES`, journaled and unjournaled tables
  (`SQL7008`, `55019`), slow tables for cancels, a table over 1 MiB,
  keys for a multi-row failure (`SQL0803`), `HLPAY.SALARIES` (only
  `ANN`), and enough schemas and tables for several pages.
- A table the profile may not use is `NotAuthorized` with `SQL0551`
  on `Prepare` and `ListColumns`, the status the operations' table
  gives, as the real job answers the engine's `SQL0551`. The catalog
  still lists it.
- Ends from outside, for the client's tests: `System::end(Server)` is
  `ENDTCPSVR` as the real system has it. Every open connection sends
  `ServerEnding` and closes, and the listener is gone: a
  `testing::Listener` (`System::listen`, one port, a job per
  connection, as `QAPD` listens) closes its port, so connects are
  refused, and `try_serve_loopback_as` answers `ConnectionRefused`
  (`serve_loopback` panics). `System::restart()` is `STRTCPSVR`: the
  listeners open the same ports again. A connection opened before an
  end still ends with it when the restart comes before its next poll.
  The real `QAPD` stops its accept loop, closing the port, before it
  ends its connections (`servers` test `listen_stop`, QEMU step 89).
  `System::end(Operator)` is one job's, as `ENDJOB` is. `System::drop_connection(job)` and `drop_all()` close a
  connection's socket on the server's side mid-request, as a network
  drop would: no reply and no event. `System::crash` still injects a
  panic. `System::wait_running(job, timeout)` tells a test when a
  command runs, so a cancel needs no sleep.
- Simplifications: no locks (other connections see changes before
  they are committed); a rollback puts back each table the
  transaction changed as it was at its first change; unqualified
  names are in the default schema, else the profile's schema (`Sql`
  naming) or `QGPL` then `HLINV` (`System` naming).

Version 5, as built on 2026-11-02 (decisions made in the build, for Cody to review):

- The server accepts versions 1 to 5. `Ifs` runs on the connection
  job's secondary threads, never in the lane, through the IFS requests
  every job already makes of the storage service
  (`stg::remote::RemoteIfs` on the thread's link). A thread's link
  shares the job's handle and security session, so its calls are the
  job's own: the signed-on profile, the service's one `Ifs`, its
  authority checks and its `QAUDJRN` entries. No new rpc was needed.
- The service's rules live in `net/qapd/src/files/`, over an
  `IfsClient`; the job and the test system both answer through it.
  Each operation makes the calls of the command it stands for:
  - `ListDirectory` and `GetInfo` read as `WRKLNK` (`*R` on the
    directory, `*X` on the path); names match as `WRKLNK`'s patterns
    do (without case except under `/QOpenSys`) and come in its order
    (by name without case, then with it). An empty `pattern` is `*`.
    `GetInfo` does not follow a symbolic link; `ReadFile` does.
    `kind` is by the mode: regular `File`, directory `Directory`
    (`/QSYS.LIB` libraries and files too), link `Symlink`, anything
    else `Other`. An entry whose attributes cannot be read is listed
    by name, `Other`.
  - `CreateDirectory` is `MD` with its defaults (`*INDIR`). A name
    taken is `Exists`. With `parents` 1 each missing directory above
    is made the same way, and a directory already there is `Ok` (as
    `mkdir -p`); a component that is not a directory is `Error`.
  - `Delete` is `RD` for a directory (empty only: `Error`, `CPFA0AC`)
    and `RMVLNK` for anything else, a link and not what it names. As
    those commands, it removes a file another job has open; `InUse`
    is a file checked out to another user.
  - `Rename` is `MOV`: a rename, or a copy and delete across file
    systems; `replace` 1 replaces `to`.
  - `Copy` is `CPY SUBTREE(*NODIR)` with `OWNER(*NEW)` and no
    `DTAFMT`: a directory is `Error` (`CPFA0DA`).
- A failure is the command's message for the `errno`: the `CPFA0xx`
  escape with its substitution data as the command lays it out (a
  path as `*CCHAR *VARY 4`). The status is by the message: `CPFA0A9`
  `NotFound`, `CPFA09C` `NotAuthorized`, `CPFA0A0` `Exists`, `CPFA09E`
  `InUse`, any other `Error`. For `Copy` that is the message `CPY`
  gives as its diagnostic, not its `CPFA097` escape. `ThreadHost`
  gained `with_ifs` and `message_data` (a message from data already
  laid out).
- `InUse`: another job's open whose share mode refuses this one, or a
  byte-range lock of another job over the range read or written
  (asked with `F_GETLK` once the file is open).
- `ReadFile` opens the file for reading only, with the default share
  mode. `as_ccsid` 0 is `InvalidRequest`. A file and an `as_ccsid`
  that differ and cannot be converted (either 65535, or no table) is
  `Error` with `TCP1A5A` (added to `QTCPMSG`). When the range reaches
  the end of the file and a partial character is left, it is
  converted as one substitution and `next_offset` is the file's
  size, so a client reading on from `next_offset` always ends; a
  `length` shorter than one character can make no progress inside
  the file, which the client avoids. The memo of mixed files keeps 32
  marks per connection (path, offset, the file's change time, the
  state); a read from anywhere else scans from the start. The body is
  built as it is read, so a large reply is copied once into frames.
- `WriteFile` opens as the FTP server writes a stream file: a new file
  tagged with `ccsid` (0: 1208), its data authorities from its
  directory (`O_INHERITMODE`). `Replace` and `Append` make the file
  when missing, `Update` does not; an existing file keeps its CCSID.
  `data_ccsid` 0 is `InvalidRequest`; a request's data ends in whole
  characters, so a partial one at its end is one substitution. The
  shift-in/shift-out join applies to `Append`, and to `Update` at the
  file's end. It reads the file's last byte, so such a write opens the
  file for reading too; a profile without `*R` gets the data written
  as converted, not joined.
- The test system's file system (`qapd::testing::files`) is the `ifs`
  crate over a `MemBackend` with `MemQsys`, its fixtures in the
  module's documentation; each connection job is one owner of
  descriptors and locks. `Delete` of a file another job holds open
  succeeds, as `RMVLNK` does; `InUse` (`CPFA09E`) comes from a file
  checked out to another user. `CHECKED_OUT` is such a file in `BOB`'s
  home (checked out to `ANN`), refusing `Delete`, `Rename` and
  `WriteFile` but not reads; `TREE` is a folder with two levels of
  subfolders whose file `TREE_HELD`, deep inside, is checked out to
  `ANN`, so a client's depth-first recursive delete removes what comes
  before it and stops there with the system's message.

Version 6, as built on 2026-11-03 (decisions made in the build, for Cody to review):

- The server accepts versions 1 to 6. `Messages` runs on the connection
  job's secondary threads, never in the lane, through rpcs 760 to 763
  (`stgsvc/qap/messages.rs`) over the functions the message commands
  use (`stgsvc/msgs.rs`), as the signed-on profile: `ListMessages` and
  `WatchQueue` check as `DSPMSG` (`msgq.receive`), `ReplyMessage` as
  `SNDRPYMSG` (`msgq.reply`), `RemoveMessages` as `RMVMSG`
  (`msgq.remove`), `SendMessage` as `SNDMSG` (`msgq.send`). A queue not
  there is `NotFound` with `CPF2403`, a key `CPF2410`; a refused queue
  `NotAuthorized` with `CPF9802` naming it (`CPF9820` for its library);
  not an inquiry waiting `InvalidRequest` with `CPF2422`.
- `Own` is the queue the profile's `MSGQ` names (`*USRPRF`: its own in
  `QUSRSYS`). A profile's own queue is made at its first sign-on, as
  before; a `User` recipient who never signed on has none yet and is
  `NotFound` (`CPF2403`).
- The job gives each message the client's form: an identifier's text
  and second-level text are its message file's description with the
  message's data, looked up in the job, so in its language; an
  impromptu message's are as sent.
- Keys: a queue hands out the key after the last, never 0 and never
  one a message on it has, so keys go up as messages arrive (a wrap
  past 2^32 included). Lists are in that order, counted back from the
  queue's next key; a reply, kept just after its inquiry on the queue,
  lists where it arrived.
- Inquiries and replies (`stgsvc/msgs.rs`, so the commands do the
  same): a reply is checked against the valid replies of the message's
  description (`MsgDesc::reply_ok`; an impromptu inquiry takes any),
  `CPF2420` when refused (added to `QCPFMSG`); `SNDRPYMSG` and the
  replies typed on `DSPMSG` are now checked the same way. An inquiry
  whose reply goes to another queue leaves a sender's copy there: a
  new `*MSGQ` message type, `*COPY` (8), its `ReplyTo` the queue the
  inquiry went to and its `ReplyKey` the inquiry's key there
  (`WORKMGMT.md`, `MessageEntry`). Answering the inquiry answers the
  copy too, as the system, whatever the replier may do with the
  sender's queue. `SNDMSG MSGTYPE(*INQ)` leaves a copy on its
  `RPYMSGQ` (by default `QSYSOPR`) as well. If the copy cannot be put,
  the inquiry is taken back and the send fails.
- `RemoveMessages` removes exactly the keys given, all or none; an
  inquiry removed before its reply gets none, as `RMVMSG` removes it.
- `SendMessage` with empty text is `InvalidRequest` (`CPD0071`). Its
  messages name `QAPCNN` as the sending program. A message or reply sent
  through QAP names the signed-on profile as its sender's user, and so
  as `from_user` and `from_job`'s user: a queue entry keeps one user
  for its sender, and the connection job's own (`QUSER`) would say
  nothing of who sent it.
- Watches (`net/qapd/src/watches.rs`): a connection's first watch makes
  one more secondary thread, which looks at every watch each 250 ms
  (`Config::watch_poll`) through the same calls as `ListMessages` and
  `ListSpooledFiles`, with the profile's authority, and sends the
  events. Nothing is allocated and no delivery mode changes, so a job
  holding the queue in break mode keeps it.
  - Messages: a look lists newest first, in pages of 32, until a key
    the watch knows; what came before it is sent, oldest first. When
    the keys known outgrow the queue by 256 (messages removed), the
    next look lists it whole and keeps only what is there.
  - Spool: a look lists the target's files and compares them with the
    last: `Removed`, `Changed` (any field of `SpooledFile` differs),
    then `Added`, each in the order the files were made.
  - What changes and changes back between two looks is not seen: a
    message sent and removed, a file added and deleted, within one
    look's interval sends nothing; several changes to a file between
    two looks send one `Changed`.
  - A watch is of the `Queue` or `SpoolTarget` value given: `Own` and a
    `Named` queue that is the same queue are two watches. A watch whose
    queue or target can no longer be listed (deleted, authority
    revoked) stays and sends nothing until it can be again.
  - `UnwatchQueue` and `UnwatchSpool` of what is not watched are
    `NotFound` with `TCP1A5B`; a host without message queues answers
    `Error` with `TCP1A5C` (both added to `QTCPMSG`).
  - `SignOff` ends the connection's watches with its profile (the
    spec's rule says a watch ends with its connection; a watch carried
    across a sign-off would list as nobody).
- The test system (`qapd::testing::messages`): message queues under the
  same rules, its fixtures in the module's documentation (`QSYSOPR` and
  `BOB`'s own with every type, `CPA0701` taking `C`, `D`, `I`, `R`
  with default `C`, `CPA5305` taking `Y` or `N`, 149 messages on
  `QSYSOPR`, `QGPL/SECRETQ` only `ANN` may use, `QGPL/TEAMQ` held in
  break mode by job 4242). `System::inject_message` puts a message on a
  queue as another job sends it; `System::spool_add`, `spool_change`
  and `spool_remove` change spooled files from outside. Watches in
  tests look every 20 ms (`Config::watch_poll`).
- QEMU step 93 runs the `Messages` service and a spool watch through the
  real connection jobs as `QAPTEST` and `QAPTEST2`, and answers a batch
  job's `CEE9901` on `QSYSOPR` (a job with `INQMSGRPY(*RQD)` whose
  program fails unmonitored waits in `MSGW` for it): `X` is refused
  with `CPF2420`, `C` lets the job go on.
- `Stat` (`net/qapd/src/files/entries.rs`) answers `GetInfo`'s entry,
  with its checks and messages, and the object's change counter
  (`IFS.md`, "Change counter"). In root, `/QOpenSys` and user-defined
  file systems it is the metadata record's (tag 21), from one counter
  of the volume handed out in reserved blocks whose top is kept on the
  volume: data writes, truncation, a rename (and both directories),
  owner, group, authority, CCSID and the other attributes raise it,
  entries made, removed or renamed raise their directory's, and an
  object without a record or a counter gets one at its first change of
  any kind. Under `/QSYS.LIB` it is the store object's change stamp,
  the ID of the last transaction that changed it (`STORE_FORMAT.md`
  §8): every change to the object's header, attributes, data or records
  raises it, a member's records raising the member's. Under `/QOPT` a
  read-only volume's files have a constant value from its descriptor,
  a writable volume's the volume's counter, kept in its label and
  raised by every change in it; devices and the virtual directories
  answer 1. No value repeats for an object.
- The test system's `Files::change_twice(path)` changes a file twice
  with its `modified` held, for the client's test of two changes in one
  millisecond.

Version 7, as built on 2026-11-04 (draft; decisions made in the build, for Cody to review):

- `qap-proto` speaks 1 to 7 (`VERSION` 7, `tools/fmtcheck`'s lock bumped
  with it); every value of versions 1 to 6 keeps its number and meaning
  (`qap-proto` test `version7::versions_1_to_6_unchanged`, `fmtcheck`).
  `Manage` is service 8; its ten operations are 1 to 10 and none is in
  the lane or allowed before sign-on.
- The service's rules live in `net/qapd/src/manage.rs`: the body
  decoded (`InvalidRequest` when it does not), the exit request built
  and given to `ThreadHost::exit_allows`, then the host's method; lists
  are paged there from the host's whole list. A host without management
  answers every operation `Error` with `TCP1A5D`; `TCP1A5E` (an exit
  program refused) and `TCP1A5F` (a value a system value does not take)
  are in `QTCPMSG` beside it.
- `QS400_QAP_MGMT` (`QAPX0100`) is a shipped exit point of the
  registration facility (`net/regfac/src/shipped.rs`), with the host
  servers' flags and two parameters, so `regfac::call_exit` calls its
  program with `ExitRequest::host("QAPX0100", ..)` and `GR` entries come
  from the registry as for the host servers. `*QAP`'s row in the servers
  table names it.
- The test system (`qapd::testing::manage`): subsystems, system values,
  PTFs, disks, hardware and the IPL under the commands' rules, its
  fixtures in the module's documentation; profiles `OPER` (`*JOBCTL`)
  and `SECOFR` (every special authority); `Manage::exit_refuses` is the
  exit program, `exit_calls` what it was given. `PowerDown` answers, then
  ends the server 50 ms later, so the client sees `ServerEnding`.
- The connection job (`work/qsjob/src/work/qap/manage.rs`) answers
  every `Manage` operation on a secondary thread's link, as the
  signed-on profile. `StartSubsystem`, `EndSubsystem`,
  `SetSystemValue` and `PowerDown` are first analyzed as the command
  strings `STRSBS SBSD()`, `ENDSBS SBS() OPTION() DELAY()`,
  `CHGSYSVAL SYSVAL() VALUE('')` and `PWRDWNSYS OPTION() DELAY()
  RESTART() IPLSRC()` (the `*CMD` found and its authority checked as
  for a typed line). `ENDSBS` and `CHGSYSVAL` then run their
  processing programs in the service (`run_cmd`), with their checks
  and `SV` entries; `STRSBS` asks the service's check (`sbs_check`,
  `*JOBCTL`) and starts the monitor as the job's `STRSBS` does;
  `PowerDown` goes to the service's request 773, which requires
  `*JOBCTL` there (so a refusal is audited `AF K`) and hands the power
  down to `slic` as `PWRDWNSYS` from a job does.
- `SetSystemValue` checks the name (`CPF1842`) and the value against
  the catalog's type, allowed values and range before the command
  (`TCP1A5F`), and answers the value read back; a value the command
  did not take is `TCP1A5F` too.
- A read first finds its display's command with the authority to use
  it: `WRKSBS`, `DSPSYSVAL`, `DSPPTF`, `WRKDSKSTS`, `WRKHDWRSC`; the
  job looks it up through the analyzer's own source (`RemoteCmds`, the
  service's `cmd_lookup`: the library list, then `*OBJOPR` and `*USE`
  to the `*CMD`, `cmd.run`), so a request has exactly the profile's
  authority, adopted authority included, and a command not found is
  `NotFound` `CPD0030`, never a pass. The service checks again for the
  reads it answers itself (`ListPtfs` `DSPPTF`, `ListDiskUnits`
  `WRKDSKSTS`) rather than trust the job. `DSPPTF` is shipped `*PUBLIC
  *EXCLUDE`, so `ListPtfs` is `NotAuthorized` for a user the security
  officer has not authorized. `IplStatus` needs nothing. Commands,
  programs and objects named in `RunCommand` are the analyzer's and the
  store guard's checks as at a 5250 command line: a command, a program
  or a library `*PUBLIC *EXCLUDE` refuses the profile (QEMU step 123;
  `lic/osobjects/tests/exclude.rs`). In earlier builds the first IPL's
  re-owning had widened the excluded commands to `*USE`
  (`SECURITY.md` §1, Shipped profiles).
- The lists: `ListSubsystems` from `sbs_list` and the `*SBSD` objects
  of the library (`*ALL` every library, `*LIBL` the job's list; an
  unknown library is `NotFound` `CPF2110`), a monitor's job named as
  its subsystem under `QSYS`; `GetSystemValues` from `DSPSYSVAL`'s
  data, `value` its elements joined by one blank, `category` the
  group without `*`, `kind` the catalog's type (no value is
  `Integer`); `ListHardwareResources` from the hardware table in
  `WRKHDWRSC TYPE()`'s order, `parent` the row above at one level
  less. `ListPtfs`, `ListDiskUnits` and `IplStatus` come from the
  storage service's requests 770 to 772 (`stgsvc/qap/manage.rs`): the
  PTF database (`text` the status's text, as no PTF carries one;
  not-loaded, ordered and cover-letter-only PTFs are `SaveFileOnly`),
  the disk configuration as DST shows it without a service tools
  sign-on (units numbered from 1 as DST shows them; an ASP's capacity counts a mirrored pair once, its
  threshold DST's), the LIC record's last IPL (type, mode, slot, its
  final SRC) with `QABNORMSW`, the next slot a LIC PTF arms, and
  `restricted` when no subsystem is active; `ipl_time` is the system
  job `SCPF`'s start, as `SystemInfo` has it.
- `QS400_QAP_MGMT`'s programs are listed through the registry
  (`RemoteRegistry`); when one is registered the thread builds a
  runtime job of its own over its link to call it, so a program runs
  in the connection job beside the lane, never in it.
- QEMU step 123 runs the service end to end as a user without special
  authority and as `QSECOFR`.

Version 8, as built on 2026-11-07 (draft; decisions made in the build, for Cody to review):

- `qap-proto` speaks 1 to 8 (`VERSION` 8; `tools/fmtcheck`'s `qap` lock
  bumped with it, `develop.rs` and `jobs.rs` registered as the draft
  format `qap-v8`). The server accepts 1 to 8. Version 9 (`Debug`)
  appends `Service::Debug` = 11 with `since` 9, its operations in `Op`
  with `since` 9, and raises `VERSION` to 9; nothing else changes, and
  `Hello` then lists service 11 only at 9. `EndHow` is
  `qap_proto::types::EndHow`, re-exported from `ops::manage`.
- `qap_proto::types::{MemberName, Where, Diagnostic}` convert to and from
  `rpg/compdiag`'s (`From`), `MemberName`'s library and file becoming
  the `ObjectName`. The Rust fields `where` and `type` are `r#where` and
  `r#type`.
- The rules shared by every host live in `net/qapd/src/develop.rs` and
  `jobs.rs`: decoding, paging, `WriteMember`'s pieces (kept per
  connection in `Shared`, by member, until `last`), the sequence-number
  checks, the date rule (`develop::dated`) and `EndJob` of the
  connection's own job (`InvalidRequest`). A host without `Develop` or
  `Jobs` answers `Error` with `TCP1A60`; `TCP1A61` (`UnlockMember` of a
  member not held) and `TCP1A62` (a record too long) are in `QTCPMSG`
  beside it; a CCSID that cannot hold the data is version 5's `TCP1A5A`,
  a member another job holds `CPF1002`.
- The connection job (`work/qsjob/src/work/qap/develop.rs`, `jobs.rs`):
  `GetLibraryList` reads the job's attributes, `SetLibraryList` runs
  `CHGLIBL LIBL() CURLIB()` in the job's runtime, so the next
  `RunCommand` and `Compile` see it. `ListLibraries` lists the `*LIB`
  objects of `QSYS` with their `TYPE`; `ListObjects` the library's
  objects with their sizes (`obj_sizes`); `DescribeObject` the object
  and `DSPOBJD DETAIL(*FULL)`'s fields under readable labels;
  `ListMembers` the file's members. `CreateMember`, `CopyMember`,
  `RenameMember`, `DeleteMember` and `ChangeMember` are `ADDPFM`,
  `CPYSRCF`, `RNMM`, `RMVM` and `CHGPFM` analyzed as typed lines and run
  in the service; `HoldJob`, `ReleaseJob` and `EndJob` likewise
  `HLDJOB`, `RLSJOB` and `ENDJOB`. `CPYSRCF` now keeps each record's
  sequence number and date (`SRCOPT(*SAME)`, as the original command does; `*SEQNBR`
  renumbers from `SRCSEQ`, `*DATE` clears the dates) and takes
  `MBROPT(*ADD)`; it used to renumber and redate every record.
- `ReadMember`, `WriteMember` and `UnlockMember` are the service's
  requests 780 to 782 (`stgsvc/qap/develop.rs`), as the job's user under
  the store's guard, over `db::source` (`records`, `read_member` and the
  new `write_records`). The lock is the store's `*SHRNUP`, held by the
  connection job, so the job's end releases it. A write takes the member
  `*EXCL` for the moment it writes, as SEU's save does, and ends the
  connection's `*SHRNUP`. Today's date is the UTC date, as SEU's.
- `ListJobs` reads what `WRKUSRJOB` reads (the active job table, the job
  queues, the jobs with spooled files); its `type` is `INTER`, `BATCH`,
  `AUTO` or the `WRKACTJOB` code. `JobLog` of an active job is the
  service's copy (`joblog_of`); of an ended one its last spooled
  `QPJOBLOG`, read back with `wm::joblog::parse_printed`. A request is
  `Informational` (service 7 has no request type).
- `ListRoutines` and `ListIndexes` read `QSYS2.SYSROUTINES`,
  `SYSINDEXES` and `SYSKEYS` on the thread's link as 9 to 11 do; an
  index is `unique` when `IS_UNIQUE` is `U` or `V`, its keys in
  `ORDINAL_POSITION` order. `ListIndexes` checks the table as
  `ListColumns` does.
- `Compile` runs `work/qsjob/src/work/qap/compile.rs`'s `compile`: the job's last
  compile cleared, the command run as `RunCommand` runs it, then the
  diagnostics the compiler left on the job (`mi::Job::last_compile`),
  converted from `compdiag`'s. A failed compile is `Error` with its
  escape message and the body filled. The test system compiles
  `CRTBNDRPG`, `CRTRPGMOD` and `CRTSQLRPGI` of its members, knowing one
  error (`n = 'x';`, `RNF7416`; member `HLINV/QRPGLESRC BAD42` has it on
  line 42). QEMU step 161 compiles `tools/conformance/qap/COMPILE42.rpgle`.
- The test system (`qapd::testing::develop` and `jobs`): its fixtures
  are in the modules' documentation. QS/400 ships no `CMPPFM`, so QEMU
  step 160 compares the members record by record.

Version 9, as built on 2026-11-07 (draft; decisions made in the build, for Cody to review):

- `qap-proto` speaks 1 to 9 (`VERSION` 9). `Debug` is service 11; its
  operations are 1 to 10 (`Op::DebugStart`, `DebugEnd`, `AddProgram`,
  `SetBreakpoints`, `DebugResume`, `Stack`, `Variables`, `Eval`,
  `SetServiceEntry`, `RemoveServiceEntry`), none in the lane or allowed
  before sign-on; its events 100 and 101 are defined from version 9
  (`ops::event::since`), so `Shared::send` drops them for an older peer.
  `tools/fmtcheck` holds `ops/debug.rs` as the draft format `qap-debug`.
- One session per connection, not per signed-on session: an attached
  connection has a job of its own, and `Own` debugs the job of the
  connection that started it. `Start` on a connection with a session is
  `InUse` (`TCP1A72`). Messages `TCP1A70` to `TCP1A77` are in `QTCPMSG`.
- The rules live in `net/qapd/src/debug.rs` over the host's `DebugHost`
  (`ThreadHost::debugger`; a host without one answers `Error` with
  `TCP1A70`). The events are the host's news, taken by the watches'
  thread at each look (`Config::watch_poll`); a session started makes
  that thread as a watch does. `End` sends no event; a sign-off ends the
  session with `DebugEnded` (`TCP1A76`) before its reply; a connection
  closing ends it quietly.
- The connection job (`work/qsjob/src/work/qap/debug/`) keeps the
  session for the process. `Own`: the programs added are put under
  debug (`Job::strdbg`) and the breakpoints given to the core
  (`mi::debug`) by the job's own thread at its next `CALL` and break
  poll; its stop hook (installed before the display's, so `STRDBG` at a
  display is unchanged) tells the client and waits, answering the
  secondary threads' `Stack`, `Variables` and `Eval` (`Job::dbg_frames`,
  `dbg_variables`, `eval_in`, added to the core) until `Resume`, `End`,
  a `Cancel` of the command (`CPF1907`, as any cancel) or the job's end.
- `Job`: `Start` is `STRSRVJOB`'s request (`srvjob_start`: the profile's
  own jobs, or `*SERVICE`, or `*ALLOBJ` as `STRSRVJOB` allows it in
  QS/400), and every order goes through its mailboxes (requests 384 to
  387), with four new orders for QAP (`Frames`, `Vars`, `EvalAt`,
  `StepOut`) and their answers. The serviced job takes them as it takes
  `STRSRVJOB`'s, at a program call, a break poll, and while stopped, so a
  5250 job holds at its locked display exactly as under `STRSRVJOB`. The
  servicer learns the job ended when `srvjob_serving` finds it gone
  (`DebugEnded`, `TCP1A75`).
- Service entry points (requests 388 to 391, `stgsvc/dbg.rs`): the
  storage service keeps owner, program (`LIB/NAME`, found in the owner's
  library list) and user. Every job's dynamic calls (`Job::call_program`,
  so `CALL` at a command line, a menu option and a program's call)
  check a list of the programs with entries for the job's user, read
  again at most once a second (`devtools/dbg/entry.rs`); a name on it
  asks the service, which attaches the job to the owner as `STRSRVJOB`
  would when the owner is active and services no job and nobody
  services the caller. The job then puts the program under debug and
  stops at its first statement; the owner's next look sees the
  attachment, puts the entry's programs on the session and sends the
  breakpoints, and reports the stop as `ServiceEntry`.
- Lines: `AddProgram` reads the modules as Display Module Source does
  (`dbg_program`, which now also gives the statements each module's
  debug code has); a breakpoint lands on the first line at or after the
  one asked whose statement the code has. RPG numbers statements by
  source line, so an RPG module's lines and statements are the same.
  `DBGVIEW(*SOURCE)` and `*COPY` give `Statement` and `Source`, `*LIST`
  `Statement` and `Listing`, `*ALL` all three, `*STMT` `Statement`.
- Conditions are those of `ADDBKP`'s and Display Module Source's
  breakpoints (`dbg::cond`), evaluated at the stop: in the job for `Own`,
  with an `EvalAt` order for `Job`.
- Not sent in this build: `StopReason::Exception` and `ProgramEnd`. An
  exception the program does not handle goes to the job's own handling
  (an inquiry, a function check), as under `STRDBG` in QS/400 today; a
  step past a program's end runs on without a stop.
- The test system (`qapd::testing::debug`): `HLINV/ASSETS` with its
  source and variables, `HLINV/NODEBUG`, display sessions' jobs
  (`System::interactive_job`, `call_in`, `end_job`); `SECOFR` now has
  `*SERVICE` too.
- QEMU steps 162 (`Own`, a version 8 connection's gate) and 163 (a
  service entry point stopping a 5250 session's job) run it end to end
  as `QAPTEST`.

## Frozen at V1R1M0

Frozen: the envelope and versions 1 to 6 as specified above, the `qap-proto` crate's types for them, and the rules under "Compatibility of new values". A new operation, service, status or event is a new protocol version; nothing in versions 1 to 6 changes meaning. Draft: anything added after version 6 (the management API of Phase 12 Section 7 is specified in its own section and is draft until QS/400 ships it); the "Later phases" inventory describes what was planned and is history, not specification.

Changes require a format version bump and a migration: a reader of the old version kept beside the new one, or a conversion run at IPL or on restore, and a row in the release notes. `tools/fmtcheck` holds the frozen definitions (`tools/fmtcheck/formats.toml`) and their hashes (`formats.lock`); `cargo test -p fmtcheck` and `build test` fail when a definition changes and its version does not.
