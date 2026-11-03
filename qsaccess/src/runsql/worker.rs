// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! A Run SQL window's connection, on a thread of its own: it takes the
//! window's commands in order and answers with events, so the window never
//! waits on the network and its long query holds up no other window.
//!
//! - Statements run one after another. A query's first block of rows comes
//!   with its columns; the cursor stays open for Fetch More and Fetch All,
//!   and is closed once its last row is in.
//! - Cancel (a shared flag) ends the statement or fetch in flight with QAP
//!   `Cancel`: a cancelled fetch keeps the rows the system sent; a
//!   cancelled run ends there.
//! - Between commands it looks for the session's end (`SessionEnded`).
//! - A lost connection is said once ([`Ev::Lost`], `QAD7010`); the next
//!   run attaches anew. Nothing is run again by itself.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::time::{Duration, Instant};

use qap_client::proto::ops::sql::{Column, SetOptions, SqlValue};
use qap_client::proto::{ServerMessage, Status};
use qap_client::{Ended, Error, FetchEnd, SqlConnection};
use qsa_config::System;
use qsa_msg::{Message, id};

use super::script::{Statement, changes_rows, verb};

/// What the window asks for.
#[derive(Debug)]
pub enum Cmd {
    /// Runs statements in order; at most `first_rows` of each result at
    /// first. With `stop_on_error`, the first failure ends the run.
    Run {
        statements: Vec<Statement>,
        stop_on_error: bool,
        first_rows: u32,
    },
    /// The next block of a result (`all`: every row left).
    Fetch {
        result: usize,
        all: bool,
    },
    /// New options; `InUse` while changes are uncommitted.
    SetOptions(SetOptions),
    Commit,
    Rollback,
    /// Closes the open cursors and releases the statements.
    ClearResults,
    /// Settles uncommitted changes (`Some(true)` commit, `Some(false)` roll
    /// back) and closes the connection.
    Close(Option<bool>),
}

/// What a message line says.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Detail {
    /// The system's message, as sent.
    System(ServerMessage),
    /// This program's.
    Program(Message),
    None,
}

/// One line of the Messages tab.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Line {
    /// The statement's number in its run, from 1; 0 for Commit and the like.
    pub statement: usize,
    /// Its script line.
    pub script_line: usize,
    pub verb: String,
    pub id: String,
    pub sqlstate: String,
    pub sqlcode: Option<i32>,
    pub text: String,
    /// Rows changed (DML) or fetched (a query).
    pub rows: Option<i64>,
    pub elapsed: Duration,
    pub failed: bool,
    pub detail: Detail,
}

/// How a result's rows stand.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Standing {
    /// More rows to fetch; the cursor is open.
    More,
    /// Every row is in.
    Complete,
    /// Not every row came: why (`cancelled`, `the connection was lost`,
    /// `the session ended`).
    Incomplete(String),
}

/// What the worker tells the window.
#[derive(Debug)]
pub enum Ev {
    /// Statement `index` of `of` started.
    Started {
        index: usize,
        of: usize,
        line: usize,
    },
    /// A query's result: its columns.
    Result {
        result: usize,
        columns: Vec<Column>,
    },
    Rows {
        result: usize,
        rows: Vec<Vec<SqlValue>>,
    },
    Standing {
        result: usize,
        standing: Standing,
    },
    Message(Line),
    /// The run (or fetch) ended.
    Done {
        cancelled: bool,
    },
    /// The options now set, or why not (`InUse`: settle first).
    Options(Result<SetOptions, Line>),
    /// Whether changes are uncommitted.
    Uncommitted(bool),
    /// The connection was lost: `QAD7010`.
    Lost,
    /// The session ended.
    Ended(Ended),
    /// The connection was closed; the worker ends.
    Closed,
}

/// The worker's side.
pub struct Worker {
    q: SqlConnection,
    sys: System,
    rx: Receiver<Cmd>,
    tx: Sender<Ev>,
    wake: Box<dyn Fn() + Send>,
    cancel: Arc<AtomicBool>,
    /// Open results: their cursor and statement.
    results: Vec<Option<(u32, u32)>>,
    /// Rows the last fetch gave.
    fetched: u64,
    ended: bool,
}

impl Worker {
    pub fn new(
        q: SqlConnection,
        sys: System,
        rx: Receiver<Cmd>,
        tx: Sender<Ev>,
        cancel: Arc<AtomicBool>,
        wake: Box<dyn Fn() + Send>,
    ) -> Worker {
        Worker {
            q,
            sys,
            rx,
            tx,
            wake,
            cancel,
            results: Vec::new(),
            fetched: 0,
            ended: false,
        }
    }

    fn send(&self, e: Ev) {
        let _ = self.tx.send(e);
        (self.wake)();
    }

    /// Runs until the window closes the connection or goes.
    pub fn run(mut self) {
        loop {
            match self.rx.recv_timeout(Duration::from_millis(250)) {
                Ok(Cmd::Close(settle)) => {
                    self.close(settle);
                    return;
                }
                Ok(cmd) => {
                    self.cancel.store(false, Ordering::SeqCst);
                    self.command(cmd);
                    self.cancel.store(false, Ordering::SeqCst);
                    self.after();
                }
                Err(RecvTimeoutError::Timeout) => self.after(),
                Err(RecvTimeoutError::Disconnected) => {
                    self.q.close();
                    return;
                }
            }
        }
    }

    /// After every command, and when idle: the session's end, a lost
    /// connection, and the uncommitted state.
    fn after(&mut self) {
        if !self.ended
            && let Some(why) = self.q.attached_mut().poll().cloned()
        {
            self.ended = true;
            self.incomplete_all("the session ended");
            self.send(Ev::Ended(why));
            return;
        }
        if !self.ended && self.q.lost() {
            self.incomplete_all("the connection was lost");
            self.send(Ev::Lost);
        }
        let u = self.q.uncommitted();
        self.send(Ev::Uncommitted(u));
    }

    /// Every open result marked incomplete: its cursor is gone.
    fn incomplete_all(&mut self, why: &str) {
        for i in 0..self.results.len() {
            if self.results[i].take().is_some() {
                self.send(Ev::Standing {
                    result: i,
                    standing: Standing::Incomplete(why.to_owned()),
                });
            }
        }
    }

    fn command(&mut self, cmd: Cmd) {
        match cmd {
            Cmd::Run {
                statements,
                stop_on_error,
                first_rows,
            } => self.run_all(&statements, stop_on_error, first_rows),
            Cmd::Fetch { result, all } => {
                let cancelled =
                    self.fetch(result, if all { None } else { Some(super::FETCH_ROWS) });
                self.send(Ev::Done { cancelled });
            }
            Cmd::SetOptions(o) => {
                let r = self
                    .q
                    .set_options(o.clone())
                    .map(|()| o)
                    .map_err(|e| self.line(0, 0, "SET", &e, Instant::now()));
                self.send(Ev::Options(r));
            }
            Cmd::Commit => self.end_unit(true),
            Cmd::Rollback => self.end_unit(false),
            Cmd::ClearResults => {
                for (cursor, statement) in std::mem::take(&mut self.results).into_iter().flatten() {
                    let _ = self.q.close_cursor(cursor);
                    let _ = self.q.release(statement);
                }
            }
            Cmd::Close(_) => {}
        }
    }

    fn end_unit(&mut self, commit: bool) {
        let t = Instant::now();
        let (verb, r) = if commit {
            ("COMMIT", self.q.commit())
        } else {
            ("ROLLBACK", self.q.rollback())
        };
        let line = match r {
            Ok(()) => Line {
                text: if commit {
                    "Changes committed."
                } else {
                    "Changes rolled back."
                }
                .into(),
                ..ok_line(0, 0, verb, t)
            },
            Err(e) => self.line(0, 0, verb, &e, t),
        };
        self.send(Ev::Message(line));
    }

    fn close(&mut self, settle: Option<bool>) {
        match settle {
            Some(true) => {
                let _ = self.q.commit();
            }
            Some(false) => {
                let _ = self.q.rollback();
            }
            None => {}
        }
        self.q.close();
        self.send(Ev::Closed);
    }

    fn run_all(&mut self, statements: &[Statement], stop_on_error: bool, first_rows: u32) {
        let mut cancelled = false;
        for (i, st) in statements.iter().enumerate() {
            if self.cancel.load(Ordering::SeqCst) || self.ended {
                cancelled = true;
                break;
            }
            self.send(Ev::Started {
                index: i + 1,
                of: statements.len(),
                line: st.line,
            });
            let (failed, was_cancelled) = self.one(i + 1, st, first_rows);
            if was_cancelled {
                cancelled = true;
                break;
            }
            if failed && stop_on_error {
                break;
            }
        }
        self.send(Ev::Done { cancelled });
    }

    /// Runs one statement: whether it failed, and whether it was
    /// cancelled.
    fn one(&mut self, n: usize, st: &Statement, first_rows: u32) -> (bool, bool) {
        let t = Instant::now();
        let v = verb(&st.text);
        let p = match self.q.prepare(&st.text) {
            Ok(p) => p,
            Err(e) => {
                let l = self.line(n, st.line, &v, &e, t);
                self.send(Ev::Message(l));
                return (true, false);
            }
        };
        let query = !p.columns.is_empty();
        let pending = match self.q.execute(p.statement, Vec::new()) {
            Ok(x) => x,
            Err(e) => {
                let l = self.line(n, st.line, &v, &e, t);
                self.send(Ev::Message(l));
                return (true, false);
            }
        };
        let mut asked = false;
        let reply = loop {
            if !asked && self.cancel.load(Ordering::SeqCst) {
                pending.cancel();
                asked = true;
            }
            if let Some(r) = pending.wait_for(Duration::from_millis(50)) {
                break r;
            }
        };
        let o = match self.q.executed(p.statement, reply) {
            Ok(o) => o,
            Err(e) => {
                let l = self.line(n, st.line, &v, &e, t);
                self.send(Ev::Message(l));
                let _ = self.q.release(p.statement);
                return (true, false);
            }
        };
        if o.cancelled {
            let mut l = cancelled_line(n, st.line, &v, o.message.clone(), t);
            if changes_rows(&st.text) {
                l.rows = Some(o.executed.updated);
            }
            self.send(Ev::Message(l));
            let _ = self.q.release(p.statement);
            return (false, true);
        }
        match o.executed.cursor {
            Some(cursor) if query => {
                let result = self.results.len();
                self.results.push(Some((cursor, p.statement)));
                self.send(Ev::Result {
                    result,
                    columns: p.columns.clone(),
                });
                // 0 rows first: every row.
                let cancelled = self.fetch(result, (first_rows > 0).then_some(first_rows));
                let rows = self.fetched;
                let mut l = Line {
                    rows: Some(i64::try_from(rows).unwrap_or(i64::MAX)),
                    text: format!("{rows} rows fetched."),
                    ..ok_line(n, st.line, &v, t)
                };
                if let Some(m) = o.message {
                    l = with_message(l, m);
                }
                self.send(Ev::Message(l));
                (false, cancelled)
            }
            _ => {
                let mut l = ok_line(n, st.line, &v, t);
                if changes_rows(&st.text) {
                    let m = qsa_msg::message(id::ROWS_CHANGED, &[&o.executed.updated.to_string()]);
                    l.id = m.id.clone();
                    l.text = m.text.clone();
                    l.rows = Some(o.executed.updated);
                    l.detail = Detail::Program(m);
                } else {
                    l.text = format!("{v} ran.");
                }
                if let Some(m) = o.message {
                    l = with_message(l, m);
                }
                self.send(Ev::Message(l));
                let _ = self.q.release(p.statement);
                (false, false)
            }
        }
    }
}

impl Worker {
    /// Fetches up to `limit` rows (`None`: all) of `result` into events;
    /// whether it was cancelled.
    fn fetch(&mut self, result: usize, limit: Option<u32>) -> bool {
        self.fetched = 0;
        let Some(Some((cursor, statement))) = self.results.get(result).copied() else {
            return false;
        };
        let tx = self.tx.clone();
        let mut count = 0u64;
        let end = {
            let wake = &self.wake;
            self.q.fetch_into(
                cursor,
                limit.map(u64::from),
                &mut |rows| {
                    count += rows.len() as u64;
                    let _ = tx.send(Ev::Rows { result, rows });
                    wake();
                    Ok(())
                },
                &self.cancel,
            )
        };
        self.fetched = count;
        let (standing, cancelled) = match end {
            Ok(FetchEnd::Done { .. }) => {
                let _ = self.q.close_cursor(cursor);
                let _ = self.q.release(statement);
                self.results[result] = None;
                (Standing::Complete, false)
            }
            Ok(FetchEnd::Limit { .. }) => (Standing::More, false),
            Ok(FetchEnd::Cancelled { message, .. }) => {
                let _ = self.q.release(statement);
                self.results[result] = None;
                if let Some(m) = message {
                    self.send(Ev::Message(cancelled_line(
                        0,
                        0,
                        "FETCH",
                        Some(m),
                        Instant::now(),
                    )));
                }
                (Standing::Incomplete("cancelled".into()), true)
            }
            Err(e) => {
                self.results[result] = None;
                let lost = matches!(e, Error::Closed(_));
                if !lost {
                    let l = self.line(0, 0, "FETCH", &e, Instant::now());
                    self.send(Ev::Message(l));
                }
                let why = if lost {
                    "the connection was lost"
                } else {
                    "the fetch failed"
                };
                (Standing::Incomplete(why.into()), false)
            }
        };
        self.send(Ev::Standing { result, standing });
        cancelled
    }

    /// A failure as a message line: the SQL message as sent with its
    /// SQLSTATE and SQLCODE; a refusal with the system's message; anything
    /// else as this program explains it (`QAD7010` for a lost connection).
    fn line(&self, n: usize, script_line: usize, verb: &str, e: &Error, t: Instant) -> Line {
        let mut l = Line {
            failed: true,
            ..ok_line(n, script_line, verb, t)
        };
        match e {
            Error::Sql { failure, message } => {
                l.sqlstate = failure.sqlstate.clone();
                l.sqlcode = Some(failure.sqlcode);
                match message {
                    Some(m) => return with_message(l, m.clone()),
                    None => l.text = format!("SQLSTATE {}", failure.sqlstate),
                }
            }
            Error::Closed(_) => {
                let m = qsa_msg::message(id::STATEMENT_LOST, &[&self.sys.name]);
                l.id = m.id.clone();
                l.text = m.text.clone();
                l.detail = Detail::Program(m);
            }
            e => {
                let x = crate::qap::explain(&self.sys, e);
                match x.server {
                    Some(m) => return with_message(l, m),
                    None => {
                        l.id = x.message.id.clone();
                        l.text = x.message.text.clone();
                        l.detail = Detail::Program(x.message);
                    }
                }
            }
        }
        if e.status() == Some(Status::InUse) {
            l.failed = true;
        }
        l
    }
}

fn ok_line(n: usize, script_line: usize, verb: &str, t: Instant) -> Line {
    Line {
        statement: n,
        script_line,
        verb: verb.to_owned(),
        id: String::new(),
        sqlstate: "00000".into(),
        sqlcode: Some(0),
        text: String::new(),
        rows: None,
        elapsed: t.elapsed(),
        failed: false,
        detail: Detail::None,
    }
}

fn with_message(mut l: Line, m: ServerMessage) -> Line {
    l.id.clone_from(&m.id);
    l.text.clone_from(&m.text);
    l.detail = Detail::System(m);
    l
}

fn cancelled_line(
    n: usize,
    script_line: usize,
    verb: &str,
    m: Option<ServerMessage>,
    t: Instant,
) -> Line {
    let l = Line {
        sqlstate: "57014".into(),
        sqlcode: Some(-952),
        text: "Cancelled.".into(),
        ..ok_line(n, script_line, verb, t)
    };
    match m {
        Some(m) => with_message(l, m),
        None => l,
    }
}
