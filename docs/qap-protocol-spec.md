<!-- Read-only copy of quartz-systems-400 docs/QAP.md at commit dd1f61a4e93022bd53a4e6cd95b1badcd605bd22. Do not edit: change the spec in QS/400 first, then re-pin (ci/spec-mirror.sh). -->
# QAP: Quartz Access Protocol, version 1 (draft)

QAP is how QS/400 Access talks to QS/400 for everything except 5250 and printer sessions, which stay on TN5250E (RFC 4777). It is an original Quartz design: it takes nothing from any other access protocol or from any other protocol's sources.

This draft settles the decisions below. Anything it leaves open is listed under Open questions; the first agent to reach one stops and asks Cody.

## Decisions

| Topic | Decision |
| --- | --- |
| Transport | TCP, one port, default **2320**, configurable on both ends |
| Security | TLS 1.3 only, no plaintext mode. Server certificate from the QS/400 certificate manager, application ID `QS400_ACCESS_SERVER`. The client validates it with its existing untrusted-certificate trust flow |
| Framing and encoding | Wire, from the QS/400 repository's `ipc` crate, the same framing and serialization QSI uses. Types below are Wire types; enums use `wire_enum!`. If Wire cannot express something below, stop and ask; do not invent a second encoding |
| Where the code lives | `qap-proto` (envelope and message types) lives in the QS/400 repository beside `ipc`. QS/400 Access pins both by commit |
| Multiplexing | One connection carries many concurrent requests. Every request has a service ID and a request ID; replies carry the request ID, in the order operations finish, not the order requests came. No port mapper and no per-service ports. Lanes: `Command` requests and `Sql` operations 1 to 8 (version 4) run one at a time, in arrival order, in the connection job, because they share its job state (library list, `QTEMP`, job attributes, commitment definition, statements and cursors). Every other operation, `Sql` operations 9 to 11 (the catalog lists, which keep no state) included, runs concurrently, up to the outstanding limit, as the signed-on profile, without waiting for the lane. `Ping` and `Cancel` are never queued behind anything |
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
- `SignOn` applies the same checks as a 5250 sign-on: `QMAXSIGN` and its action, disabled profiles, password expiry, `*NONE` passwords refused, and the same audit journal entries. An expired password gets `PasswordExpired` and no token; the client then sends `ChangePassword`, which applies the system password rules and on success returns a `SignOnReply`.
- `ChangePassword` is also allowed in a signed-on session for that session's own profile.
- The token is 256 random bits, stored server-side only. `Resume` re-attaches a new connection to the session until the idle timeout; after it, `SessionExpired`. `SignOff` ends the session and invalidates the token.
- Anything other than the operations marked "yes" before sign-on gets `NotSignedOn`. Before sign-on the connection job runs as `QUSER`; after it, as the signed-on profile.
- The client never logs or stores passwords or tokens, and clears them from memory after use.

## Later phases (spec now, build later)

The client agent inventories which system functions QS/400 Access uses, and adds each as a QAP service with its requests and replies to this document before building it. Expected, in this order: remote command (run CL, return messages and the job), spooled files (list, read as SCS, text or PDF, hold, release, delete, move), SQL, IFS files, message queues. Optional: a one-time `TelnetTicket` from a signed-on session that the Telnet server accepts for 5250 auto sign-on, which would remove the need for password substitution on Telnet.

### Inventory

What QS/400 Access needs of the system besides 5250, from its task pane, README and messages, and the service each becomes:

| Function in QS/400 Access | Service |
| --- | --- |
| Run Command task | `Command` (3) |
| Printer Output task: spooled files and output queues | `Spool` (4) |
| Run SQL, Schemas and Data Transfer tasks (Data Transfer downloads with a query and uploads with an `INSERT` run over batches of rows) | `Sql` (5) |
| Integrated File System task | `Ifs` (6) |
| Message queues: the user's own and `QSYSOPR`, inquiry replies | `Messages` (7) |
| Password expiry warning (`QAD4031`), which the phase 0 `SignOnReply` cannot give | `SignOn` op 6 `PasswordStatus` |
| 5250 automatic sign-on without a password on Telnet (the optional ticket) | `SignOn` op 5 `TelnetTicket` |

Not in QAP: 5250 display and printer sessions (Telnet), the printer session's message queue (named in the TN5250E startup), the 5250 Console and Control Panel tasks (the service console), and Key and Certificate Management (this computer's trust store).

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

**Spool events (planned for a later version).** Spool change events are planned for a later version: a watch on an output queue, like `WatchQueue`, with an event when a file arrives, changes status or leaves. Version 3 has none; version 3 clients poll with `ListSpooledFiles`.

### `SignOn` additions (version 2)

| Service | Op | Request body | Reply body | Allowed before sign-on |
| --- | --- | --- | --- | --- |
| SignOn | 5 `TelnetTicket` | — | `ticket` 32 bytes, `expires_secs` u32 | no |
| SignOn | 6 `PasswordStatus` | — | `expires` Option of i64 (none when the password does not expire), `warning_days` u16 (the system's warning period) | no |

| Op | Statuses |
| --- | --- |
| `TelnetTicket` | `Ok`; `NotAuthorized` (the profile may not sign on automatically) |
| `PasswordStatus` | `Ok` |

A ticket is 256 random bits for the session's own profile, kept by the server, good for one 5250 automatic sign-on within `expires_secs`, and only over Telnet with TLS. The client sends it, as 64 hexadecimal characters, where automatic sign-on otherwise carries the password; the client sends the RFC 4777 user variable, and the password variable carrying the ticket as 64 hexadecimal characters with an empty random-seed variable, and only over Telnet TLS. The client treats a ticket as it treats a token: never logged or stored, wiped after use.

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
- `QRMTSIGN` decides automatic sign-on, for tickets and the RFC 4777
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
  panel groups for commands yet, and the 5250 prompter shows none
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
  index and alias names in place of the library list, as IBM i's
  `DFTRDBCOL` and `SET SCHEMA` do.
- When the connection closes, and when the job runs as another
  profile (sign-off, resume), the lane rolls back, ends the scope
  `QAP:` (its cursors and statements go) and resets the options. The
  job's end rolls back any commitment definition left as well.
- Values by the engine's `DESCRIBE` (which now also gives each
  entry's CCSID): integers `Integer`; `DECIMAL` and `NUMERIC` the
  engine's exact text; `DECFLOAT` (SQLTYPE 996, `length` its 16 or 34
  digits) `Decimal` with the engine's scientific text (`1.5E-30`,
  `-12.50`) or `Infinity`, `-Infinity`, `NaN`, `sNaN`; `REAL` and
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
  three parts filled to 20 characters, as an SQLDA's label; a column
  without a heading, or an expression, has its name as `label`. A
  column renamed with `AS` keeps its heading.
- The storage service is one thread, but a query (`Execute` of a
  `SELECT`, opened as a cursor) takes turns with every other request
  there: between batches of its scan it lets the other jobs' requests
  and this connection's catalog lists in (`STORAGE_SERVICE.md` "Long
  statements"). Still waiting on it: statements that change data
  (`UPDATE` of many rows), queries under `RepeatableRead` and
  `Serializable` (`*ALL`), the sort or join of a query once its rows
  are read, and a query met while another runs, which goes first (the
  earlier window waits for it).
- Engine limits the build met, now lifted as IBM i has them:
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
  list) is a `BOOLEAN` (SQLTYPE 2436), as on IBM i 7.5.

The test system's database (`qapd::testing::sql`, for QS/400's tests
and QS/400 Access's):

- A small statement runner over tables in memory, not QS/400's engine:
  its tables carry `DECFLOAT`, binary types, `BOOLEAN` and
  `TIMESTAMP(12)`, which the client must meet. It runs `SELECT` of
  `*` or columns with `WHERE col = x AND ...`, `INSERT ... VALUES`,
  `UPDATE` and `DELETE` over one table; anything else is the syntax
  error `SQL0104`, as a database that does not know the statement
  reports it.
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
- Ends from outside, for the client's tests: `System::end(Server)`
  reaches every connection, open or opened later (each sends
  `ServerEnding`); `System::end(Operator)` is one job's, as `ENDJOB`
  is. `System::drop_connection(job)` and `drop_all()` close a
  connection's socket on the server's side mid-request, as a network
  drop would: no reply and no event. `System::crash` still injects a
  panic. `System::wait_running(job, timeout)` tells a test when a
  command runs, so a cancel needs no sleep.
- Simplifications: no locks (other connections see changes before
  they are committed); a rollback puts back each table the
  transaction changed as it was at its first change; unqualified
  names are in the default schema, else the profile's schema (`Sql`
  naming) or `QGPL` then `HLINV` (`System` naming).
