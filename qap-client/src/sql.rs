// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! Version 4's `Sql` service: statements, cursors, commitment control and
//! the catalog.
//!
//! - [`SqlConnection`] owns one connection attached to the session
//!   ([`crate::Attached`]): its options (commit level, naming, default
//!   schema), its prepared statements and its open cursors, which belong to
//!   its job. Operations 1 to 8 go on it, one after another in the server's
//!   lane for the job.
//! - The catalog ([`Client::list_schemas`], [`Client::list_tables`],
//!   [`Client::list_columns`], version 8's [`Client::list_routines`] and
//!   [`Client::list_indexes`], and the `all_` helpers that page through
//!   everything) keeps no state and goes on the session's shared
//!   connection.
//! - Rows stream: [`SqlConnection::fetch_into`] fetches [`FETCH_BLOCK`] rows
//!   at a time into a caller's sink, so a whole result is never held here.
//!
//! Statuses:
//!
//! - `SqlError` is [`Error::Sql`], with its `SqlFailure` (SQLSTATE, SQLCODE,
//!   the failing row of a multi-row `Execute`) and the SQL message as sent.
//!   A table the profile may not read is `NotAuthorized` (`SQL0551`) at
//!   `Prepare` and `ListColumns`; `SetOptions` is `InUse` (`SQL0752`) while
//!   changes are uncommitted. Those, and every other refusal, are
//!   [`Error::Status`].
//! - Cancel: a started `Execute` answers `Cancelled` with its `updated`
//!   count and no cursor, a started `Fetch` `Cancelled` with the rows so far
//!   (the cursor is then closed), and a request cancelled before it started
//!   `Cancelled` with an empty body: no rows, nothing changed. Each is an
//!   outcome here ([`ExecuteOutcome`], [`FetchOutcome`]), not an error,
//!   with the SQL message (`SQL0952`).
//! - A lost connection takes the job's statements, cursors and uncommitted
//!   changes with it. Requests that name a statement or a cursor go only on
//!   the connection that made it, never a new one, and fail with
//!   [`Error::Closed`]; [`SqlConnection::lost`] then says so once, for the
//!   window's `QAD7010`. The next `Prepare` attaches a new connection and
//!   sets the options on it again. Nothing is ever run again by itself.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use ipc::Wire;
use qap_proto::ops::sql::{
    Column, Columns, CommitLevel, CursorRef, Execute, Executed, Fetch, Fetched, Index, Indexes,
    ListColumns, ListIndexes, ListRoutines, ListSchemas, ListTables, Prepare, Prepared, Routine,
    RoutineKind, Routines, Schema, Schemas, SetOptions, SqlFailure, SqlValue, StatementRef, Table,
    TableKind, Tables,
};
use qap_proto::{Op, ServerMessage, Status};

use crate::spool::ALL_PAGE;
use crate::{Attached, Client, Error, Pending, Reply, runs_long};

/// Rows per `Fetch` when streaming.
pub const FETCH_BLOCK: u32 = 500;

/// How often a wait looks for a cancel.
const LOOK: Duration = Duration::from_millis(50);

/// A reply of `Prepare`, `Execute`, `Fetch`, `Commit` or `Rollback`:
/// `SqlError`'s body made [`Error::Sql`].
fn sql_refused(mut reply: Reply) -> Error {
    if reply.status == Status::SqlError {
        let message = reply.message.take();
        return match reply.body_as::<SqlFailure>() {
            Ok(Some(failure)) => Error::Sql {
                failure: Box::new(failure),
                message,
            },
            Ok(None) => Error::Sql {
                failure: Box::default(),
                message,
            },
            Err(e) => e,
        };
    }
    reply.body.fill(0);
    Error::Status {
        status: reply.status,
        message: reply.message.take(),
    }
}

/// A reply decoded as `R`, `SqlError` as [`Error::Sql`].
fn sql_decode<R: Wire>(reply: Reply) -> Result<R, Error> {
    if reply.status == Status::Ok {
        reply.decode()
    } else {
        Err(sql_refused(reply))
    }
}

/// How an `Execute` ended.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExecuteOutcome {
    /// Rows changed (so far, when cancelled); a query's open cursor.
    pub executed: Executed,
    /// Cancelled: before it started (nothing changed), or while it ran.
    pub cancelled: bool,
    /// The SQL message, when the server sent one (`SQL0952` for a cancel).
    pub message: Option<ServerMessage>,
}

impl ExecuteOutcome {
    /// An `Execute` reply as its outcome.
    pub fn from_reply(mut reply: Reply) -> Result<ExecuteOutcome, Error> {
        match reply.status {
            Status::Ok | Status::Cancelled => {
                let cancelled = reply.status == Status::Cancelled;
                let message = reply.message.take();
                let mut executed = reply.body_as::<Executed>()?.unwrap_or_default();
                if cancelled {
                    // A cancelled statement leaves no cursor.
                    executed.cursor = None;
                }
                Ok(ExecuteOutcome {
                    executed,
                    cancelled,
                    message,
                })
            }
            _ => Err(sql_refused(reply)),
        }
    }
}

/// How a `Fetch` ended.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FetchOutcome {
    /// The rows, and whether the cursor has no more.
    pub fetched: Fetched,
    /// Cancelled: the rows produced before it, `end` false, the cursor
    /// closed; or none, when cancelled before it started.
    pub cancelled: bool,
    pub message: Option<ServerMessage>,
}

impl FetchOutcome {
    /// A `Fetch` reply as its outcome.
    pub fn from_reply(mut reply: Reply) -> Result<FetchOutcome, Error> {
        match reply.status {
            Status::Ok | Status::Cancelled => {
                let cancelled = reply.status == Status::Cancelled;
                let message = reply.message.take();
                let mut fetched = reply.body_as::<Fetched>()?.unwrap_or_default();
                if cancelled {
                    fetched.end = false;
                }
                Ok(FetchOutcome {
                    fetched,
                    cancelled,
                    message,
                })
            }
            _ => Err(sql_refused(reply)),
        }
    }
}

/// How [`SqlConnection::fetch_into`] ended.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FetchEnd {
    /// The cursor had no more rows; it is still open until closed.
    Done { rows: u64 },
    /// The limit was reached; the cursor is still open.
    Limit { rows: u64 },
    /// Cancelled: the rows the system produced before the cancel went to
    /// the sink too. The cursor is closed. The SQL message, if any.
    Cancelled {
        rows: u64,
        message: Option<ServerMessage>,
    },
}

impl FetchEnd {
    /// Rows given to the sink.
    pub fn rows(&self) -> u64 {
        match self {
            FetchEnd::Done { rows }
            | FetchEnd::Limit { rows }
            | FetchEnd::Cancelled { rows, .. } => *rows,
        }
    }
}

/// What a prepared statement is, for the uncommitted-changes rule.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Kept {
    query: bool,
}

/// One connection of a session for SQL, with its job's statements and
/// cursors.
#[derive(Debug)]
pub struct SqlConnection {
    a: Attached,
    /// The options asked for; set on each new connection.
    options: SetOptions,
    /// The options the connection there is has.
    set: SetOptions,
    /// [`Attached::attaches`] when the statements below were made.
    generation: u64,
    statements: BTreeMap<u32, Kept>,
    cursors: BTreeMap<u32, u32>,
    /// A statement other than a query ran under a commit level since the
    /// last `Commit` or `Rollback`: the server holds the options (`InUse`).
    uncommitted: bool,
    /// The connection was lost since [`SqlConnection::lost`] last said so.
    lost: bool,
}

impl SqlConnection {
    /// SQL on `a`, a connection attached to the session; it starts with no
    /// commitment control, `Sql` naming and no default schema, as the
    /// server's connection does.
    pub fn new(a: Attached) -> SqlConnection {
        let generation = a.attaches();
        SqlConnection {
            a,
            options: SetOptions::default(),
            set: SetOptions::default(),
            generation,
            statements: BTreeMap::new(),
            cursors: BTreeMap::new(),
            uncommitted: false,
            lost: false,
        }
    }

    /// The attached connection: its job, events and the session's end.
    pub fn attached(&self) -> &Attached {
        &self.a
    }

    pub fn attached_mut(&mut self) -> &mut Attached {
        &mut self.a
    }

    /// The options asked for.
    pub fn options(&self) -> &SetOptions {
        &self.options
    }

    /// Whether changes are uncommitted: Commit or Rollback before the
    /// commit level or naming may change, or the connection closes.
    pub fn uncommitted(&self) -> bool {
        self.uncommitted && !self.notice_lost()
    }

    /// Prepared statements open on the connection.
    pub fn statements(&self) -> usize {
        self.statements.len()
    }

    /// Cursors open on the connection.
    pub fn cursors(&self) -> usize {
        self.cursors.len()
    }

    /// True once after the connection was lost with statements, cursors or
    /// changes on it: they are gone, and the next run attaches anew.
    pub fn lost(&mut self) -> bool {
        self.notice_lost();
        self.forget_if_lost();
        std::mem::take(&mut self.lost)
    }

    /// Whether the job the statements were made in is gone.
    fn notice_lost(&self) -> bool {
        self.a.attaches() != self.generation || self.a.is_closed()
    }

    fn forget_if_lost(&mut self) {
        if self.notice_lost()
            && (!self.statements.is_empty() || !self.cursors.is_empty() || self.uncommitted)
        {
            self.lost = true;
            self.statements.clear();
            self.cursors.clear();
            self.uncommitted = false;
        }
    }

    /// A live connection with the options asked for, attaching a new one
    /// after a drop.
    fn ready(&mut self) -> Result<(), Error> {
        self.forget_if_lost();
        self.a.live()?;
        if self.a.attaches() != self.generation {
            self.generation = self.a.attaches();
            self.statements.clear();
            self.cursors.clear();
            self.uncommitted = false;
            self.set = SetOptions::default();
        }
        if self.set != self.options {
            let body = self.options.clone();
            let p = self.a.request_here(Op::SetOptions, &body)?;
            self.wait(p, Op::SetOptions).and_then(Reply::decode::<()>)?;
            self.set = body;
        }
        Ok(())
    }

    /// A pending request's reply: a long one waited for until it ends, a
    /// short one for the reply timeout.
    fn wait(&self, p: Pending, op: Op) -> Result<Reply, Error> {
        if runs_long(op) {
            p.wait_done()
        } else {
            p.wait(self.a.target().options.reply_timeout)
        }
    }

    /// `SetOptions`: the commit level, naming and default schema, from the
    /// next statement. `InUse` (`SQL0752`) while changes are uncommitted;
    /// the options asked for stay as they were then.
    pub fn set_options(&mut self, options: SetOptions) -> Result<(), Error> {
        let before = std::mem::replace(&mut self.options, options);
        match self.ready() {
            Ok(()) => Ok(()),
            Err(e) => {
                self.options = before;
                Err(e)
            }
        }
    }

    /// `Prepare`: a statement in the job, with its result columns (none
    /// when not a query) and its parameters.
    pub fn prepare(&mut self, sql: &str) -> Result<Prepared, Error> {
        self.ready()?;
        let body = Prepare {
            sql: sql.to_owned(),
        };
        let pending = self.a.request_here(Op::Prepare, &body)?;
        let p: Prepared = self.wait(pending, Op::Prepare).and_then(sql_decode)?;
        self.statements.insert(
            p.statement,
            Kept {
                query: !p.columns.is_empty(),
            },
        );
        Ok(p)
    }

    /// Starts `Execute` of `statement` with `rows` of parameters (none to
    /// run it once). Wait on the [`Pending`] (or cancel it) and give the
    /// reply to [`SqlConnection::executed`]. Never sent again after a
    /// drop.
    pub fn execute(&mut self, statement: u32, rows: Vec<Vec<SqlValue>>) -> Result<Pending, Error> {
        let kept = self.statement(statement)?;
        let p = self
            .a
            .request_here(Op::Execute, &Execute { statement, rows })?;
        // From here the job may hold changes, whatever the reply says: a
        // failing row leaves the rows before it changed.
        if !kept.query && self.set.commit != CommitLevel::None {
            self.uncommitted = true;
        }
        Ok(p)
    }

    /// `Execute`'s reply, its cursor kept for `Fetch`.
    pub fn executed(
        &mut self,
        statement: u32,
        reply: Result<Reply, Error>,
    ) -> Result<ExecuteOutcome, Error> {
        let o = self.after(reply).and_then(ExecuteOutcome::from_reply)?;
        if let Some(c) = o.executed.cursor {
            self.cursors.insert(c, statement);
        }
        Ok(o)
    }

    /// `Execute`, waited for until it ends.
    pub fn execute_now(
        &mut self,
        statement: u32,
        rows: Vec<Vec<SqlValue>>,
    ) -> Result<ExecuteOutcome, Error> {
        let p = self.execute(statement, rows)?;
        let r = p.wait_done();
        self.executed(statement, r)
    }

    /// Starts `Fetch` of at most `max_rows` (0: no limit) from `cursor`;
    /// read the reply with [`SqlConnection::fetched`].
    pub fn fetch(&mut self, cursor: u32, max_rows: u32) -> Result<Pending, Error> {
        self.cursor(cursor)?;
        self.a.request_here(Op::Fetch, &Fetch { cursor, max_rows })
    }

    /// `Fetch`'s reply; a cancelled fetch's cursor is closed.
    pub fn fetched(
        &mut self,
        cursor: u32,
        reply: Result<Reply, Error>,
    ) -> Result<FetchOutcome, Error> {
        let o = self.after(reply).and_then(FetchOutcome::from_reply)?;
        if o.cancelled {
            self.cursors.remove(&cursor);
        }
        Ok(o)
    }

    /// Fetches from `cursor` into `sink`, [`FETCH_BLOCK`] rows a request,
    /// until the cursor ends, `limit` rows have gone (`None`: no limit) or
    /// `cancel` is set. A cancel while a block is in flight sends QAP
    /// `Cancel`, and the rows the system produced before it still go to the
    /// sink. A sink that fails ends the fetch with [`Error::Write`], the
    /// cursor left open.
    pub fn fetch_into(
        &mut self,
        cursor: u32,
        limit: Option<u64>,
        sink: &mut dyn FnMut(Vec<Vec<SqlValue>>) -> std::io::Result<()>,
        cancel: &AtomicBool,
    ) -> Result<FetchEnd, Error> {
        let mut rows = 0u64;
        loop {
            let want = match limit {
                Some(l) if rows >= l => return Ok(FetchEnd::Limit { rows }),
                Some(l) => u32::try_from(l - rows).unwrap_or(u32::MAX).min(FETCH_BLOCK),
                None => FETCH_BLOCK,
            };
            if cancel.load(Ordering::SeqCst) {
                return Ok(FetchEnd::Cancelled {
                    rows,
                    message: None,
                });
            }
            let p = self.fetch(cursor, want)?;
            let mut asked = false;
            let reply = loop {
                if !asked && cancel.load(Ordering::SeqCst) {
                    p.cancel();
                    asked = true;
                }
                if let Some(r) = p.wait_for(LOOK) {
                    break r;
                }
            };
            let o = self.fetched(cursor, reply)?;
            let n = o.fetched.rows.len() as u64;
            if n > 0 {
                sink(o.fetched.rows).map_err(Error::Write)?;
            }
            rows += n;
            if o.cancelled {
                return Ok(FetchEnd::Cancelled {
                    rows,
                    message: o.message,
                });
            }
            if o.fetched.end {
                return Ok(FetchEnd::Done { rows });
            }
        }
    }

    /// `CloseCursor`.
    pub fn close_cursor(&mut self, cursor: u32) -> Result<(), Error> {
        self.cursor(cursor)?;
        let p = self
            .a
            .request_here(Op::CloseCursor, &CursorRef { cursor })?;
        let r = self.wait(p, Op::CloseCursor).and_then(Reply::decode::<()>);
        self.cursors.remove(&cursor);
        r
    }

    /// `Release`: the statement is gone, its cursors with it.
    pub fn release(&mut self, statement: u32) -> Result<(), Error> {
        self.statement(statement)?;
        let p = self
            .a
            .request_here(Op::Release, &StatementRef { statement })?;
        let r = self.wait(p, Op::Release).and_then(Reply::decode::<()>);
        self.statements.remove(&statement);
        self.cursors.retain(|_, s| *s != statement);
        r
    }

    /// `Commit`. Open cursors stay open (`WITH HOLD`).
    pub fn commit(&mut self) -> Result<(), Error> {
        self.end_unit(Op::Commit)
    }

    /// `Rollback`. Open cursors stay open (`WITH HOLD`).
    pub fn rollback(&mut self) -> Result<(), Error> {
        self.end_unit(Op::Rollback)
    }

    fn end_unit(&mut self, op: Op) -> Result<(), Error> {
        let p = self.a.request_here(op, &())?;
        let r = self.wait(p, op).and_then(sql_decode::<()>);
        if r.is_ok() {
            self.uncommitted = false;
        }
        r
    }

    /// `ListColumns` on this connection: a table's columns, in order.
    /// `NotAuthorized` (`SQL0551`) for one the profile may not read.
    pub fn list_columns(&mut self, schema: &str, table: &str) -> Result<Vec<Column>, Error> {
        let c: Columns = self.a.call(
            Op::ListColumns,
            &ListColumns {
                schema: schema.to_owned(),
                table: table.to_owned(),
            },
        )?;
        Ok(c.columns)
    }

    /// Closes the connection: the server rolls back what is uncommitted.
    /// It never signs off.
    pub fn close(&mut self) {
        self.a.close();
    }

    fn statement(&mut self, statement: u32) -> Result<Kept, Error> {
        self.forget_if_lost();
        match self.statements.get(&statement) {
            Some(k) if !self.a.is_closed() => Ok(*k),
            _ => Err(self.gone()),
        }
    }

    fn cursor(&mut self, cursor: u32) -> Result<(), Error> {
        self.forget_if_lost();
        if self.cursors.contains_key(&cursor) && !self.a.is_closed() {
            Ok(())
        } else {
            Err(self.gone())
        }
    }

    /// The error for a statement or cursor not on the connection there is.
    fn gone(&self) -> Error {
        match self.a.connection().and_then(crate::Connection::closed) {
            Some(why) => Error::Closed(why),
            None if self.a.ended().is_some() => Error::Status {
                status: Status::SessionExpired,
                message: self.a.ended().and_then(|e| e.message().cloned()),
            },
            None => Error::Status {
                status: Status::NotFound,
                message: None,
            },
        }
    }

    /// A reply; a lost connection noted.
    fn after(&mut self, reply: Result<Reply, Error>) -> Result<Reply, Error> {
        if let Err(Error::Closed(_)) = &reply {
            self.forget_if_lost();
        }
        reply
    }
}

impl Client {
    /// `ListSchemas` (version 4): one page of the schemas matching
    /// `pattern` (SQL `LIKE`), from `start`, at most `max`.
    pub fn list_schemas(&mut self, req: &ListSchemas) -> Result<Schemas, Error> {
        self.call(Op::ListSchemas, req)
    }

    /// `ListTables` (version 4): one page of the tables, views, aliases and
    /// files in a schema; `kinds` empty for all.
    pub fn list_tables(&mut self, req: &ListTables) -> Result<Tables, Error> {
        self.call(Op::ListTables, req)
    }

    /// `ListColumns` (version 4): a table's columns, in order.
    /// `NotAuthorized` (`SQL0551`) for one the profile may not read.
    pub fn list_columns(&mut self, schema: &str, table: &str) -> Result<Vec<Column>, Error> {
        let c: Columns = self.call(
            Op::ListColumns,
            &ListColumns {
                schema: schema.to_owned(),
                table: table.to_owned(),
            },
        )?;
        Ok(c.columns)
    }

    /// Every schema matching `pattern`.
    pub fn all_schemas(&mut self, pattern: &str) -> Result<Vec<Schema>, Error> {
        let mut req = ListSchemas {
            pattern: pattern.to_owned(),
            start: 0,
            max: ALL_PAGE,
        };
        let mut out = Vec::new();
        loop {
            let page = self.list_schemas(&req)?;
            let n = page.schemas.len();
            out.extend(page.schemas);
            if n == 0 || out.len() >= page.total as usize {
                return Ok(out);
            }
            req.start = u32::try_from(out.len()).unwrap_or(u32::MAX);
        }
    }

    /// Every table of `kinds` (empty: all) in `schema` matching `pattern`.
    pub fn all_tables(
        &mut self,
        schema: &str,
        pattern: &str,
        kinds: &[TableKind],
    ) -> Result<Vec<Table>, Error> {
        let mut req = ListTables {
            schema: schema.to_owned(),
            pattern: pattern.to_owned(),
            kinds: kinds.to_vec(),
            start: 0,
            max: ALL_PAGE,
        };
        let mut out = Vec::new();
        loop {
            let page = self.list_tables(&req)?;
            let n = page.tables.len();
            out.extend(page.tables);
            if n == 0 || out.len() >= page.total as usize {
                return Ok(out);
            }
            req.start = u32::try_from(out.len()).unwrap_or(u32::MAX);
        }
    }

    /// `ListRoutines` (version 8): one page of the procedures and functions
    /// in `schema` matching `pattern` (SQL `LIKE`), `kinds` empty for both,
    /// by name and then specific name.
    pub fn list_routines(&mut self, req: &ListRoutines) -> Result<Routines, Error> {
        self.call(Op::ListRoutines, req)
    }

    /// Every routine of `kinds` (empty: both) in `schema` matching
    /// `pattern`.
    pub fn all_routines(
        &mut self,
        schema: &str,
        pattern: &str,
        kinds: &[RoutineKind],
    ) -> Result<Vec<Routine>, Error> {
        let mut req = ListRoutines {
            schema: schema.to_owned(),
            pattern: pattern.to_owned(),
            kinds: kinds.to_vec(),
            start: 0,
            max: ALL_PAGE,
        };
        let mut out = Vec::new();
        loop {
            let page = self.list_routines(&req)?;
            let n = page.routines.len();
            out.extend(page.routines);
            if n == 0 || out.len() >= page.total as usize {
                return Ok(out);
            }
            req.start = u32::try_from(out.len()).unwrap_or(u32::MAX);
        }
    }

    /// `ListIndexes` (version 8): a table's indexes, by name, each with its
    /// key columns in key order. `NotAuthorized` (`SQL0551`) for a table
    /// the profile may not read, as `ListColumns`.
    pub fn list_indexes(&mut self, schema: &str, table: &str) -> Result<Vec<Index>, Error> {
        let i: Indexes = self.call(
            Op::ListIndexes,
            &ListIndexes {
                schema: schema.to_owned(),
                table: table.to_owned(),
            },
        )?;
        Ok(i.indexes)
    }

    /// A connection of its own for SQL (version 4), attached to this
    /// session.
    pub fn sql_connection(&mut self) -> Result<SqlConnection, Error> {
        self.supports(Op::Prepare)?;
        Ok(SqlConnection::new(self.attach()?))
    }
}
