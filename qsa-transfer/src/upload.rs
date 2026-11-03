// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! Uploads: a file's rows into a table, with an `INSERT` prepared once and
//! `Execute` over batches of rows, each request under 1 MiB.
//!
//! - Mapping: each target column takes a source column by name or by
//!   position; unmapped target columns get their defaults.
//! - Append adds the rows; Replace deletes the table's rows first, in the
//!   same transaction when there is commitment control (without it, the
//!   delete cannot be undone).
//! - Commitment ([`Commit`]): one transaction for the whole upload (Read
//!   committed), committed at the end, so a failure keeps nothing; or a
//!   commit every N batches, so a failure keeps the batches committed and
//!   the upload resumes from the first row not committed; or none (a table
//!   that cannot use commitment control, `SQL7008`), so a failure keeps
//!   every row before the failing one and the upload resumes from it.
//! - A failure names the row (counted from 1 among the data rows), its
//!   line in the file, and the system's message. The row in the file is
//!   the batch's first plus `SqlFailure.row`.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use qap_client::proto::ops::sql::{Column, CommitLevel, SetOptions, SqlValue};
use qap_client::{Error, ServerMessage, SqlConnection};
use qsa_sql::DateStyle;
use serde::{Deserialize, Serialize};

use crate::download::{Progress, Summary, qualified, sql_name};
use crate::format::FileOptions;
use crate::reader::{self, Row, Sheet, Value};

/// Append to the table, or replace its rows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    #[default]
    Append,
    Replace,
}

/// How an upload commits.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "commit", rename_all = "kebab-case")]
pub enum Commit {
    /// One transaction (Read committed), committed at the end.
    #[default]
    Once,
    /// A commit after every `batches` batches.
    Every { batches: u32 },
    /// No commitment control: each batch stays as it goes in.
    Without,
}

/// Where a target column's values come from.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum SourceColumn {
    /// The source column of this header name.
    Name(String),
    /// The source column at this position, from 0.
    Position(usize),
}

/// One target column and its source.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Map {
    pub target: String,
    pub from: SourceColumn,
}

/// An upload.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Upload {
    pub file: PathBuf,
    pub options: FileOptions,
    pub schema: String,
    pub table: String,
    pub mode: Mode,
    /// The mapping; empty maps by name with a header row, else by position.
    pub mapping: Vec<Map>,
    pub commit: Commit,
    /// The most bytes of rows a request carries; 0 for [`BATCH_BYTES`].
    pub batch_bytes: usize,
    /// The first data row to send, from 1; `None` for all (a resume).
    pub resume_row: Option<u64>,
}

/// The most an `Execute` request's rows may take, leaving room for the
/// rest of the frame.
pub const BATCH_BYTES: usize = 1024 * 1024 - 64 * 1024;

/// About how many bytes a value takes in a request.
fn size(v: &SqlValue) -> usize {
    match v {
        SqlValue::Null => 1,
        SqlValue::Integer { .. } | SqlValue::Date { .. } | SqlValue::Time { .. } => 9,
        SqlValue::Timestamp { .. } => 13,
        SqlValue::Boolean { .. } => 2,
        SqlValue::Decimal { text } | SqlValue::Float { text } | SqlValue::Text { text } => {
            6 + text.len()
        }
        SqlValue::Binary { data } => 6 + data.len(),
    }
}

/// The mapping in use: every target column with a source index, in the
/// target's order. Unmapped targets are left out (their defaults).
pub fn resolve(
    columns: &[Column],
    header: Option<&[String]>,
    mapping: &[Map],
    width: usize,
) -> Vec<(usize, usize)> {
    let by_name = |name: &str| {
        header.and_then(|h| {
            h.iter()
                .position(|s| s.trim().eq_ignore_ascii_case(name.trim()))
        })
    };
    if mapping.is_empty() {
        return columns
            .iter()
            .enumerate()
            .filter_map(|(t, c)| match header {
                Some(_) => by_name(&c.name).map(|s| (t, s)),
                None => (t < width).then_some((t, t)),
            })
            .collect();
    }
    columns
        .iter()
        .enumerate()
        .filter_map(|(t, c)| {
            let m = mapping
                .iter()
                .find(|m| m.target.eq_ignore_ascii_case(&c.name))?;
            match &m.from {
                SourceColumn::Name(n) => by_name(n).map(|s| (t, s)),
                SourceColumn::Position(p) => (*p < width).then_some((t, *p)),
            }
        })
        .collect()
}

/// The first value of each mapped column that does not fit, from `rows`
/// (a preview): the target column's index and `QAD7035`'s parts.
pub fn type_check(
    columns: &[Column],
    map: &[(usize, usize)],
    rows: &[Row],
    style: &qsa_sql::Style,
) -> Vec<(usize, qsa_sql::Misfit)> {
    let mut out = Vec::new();
    for &(t, s) in map {
        let c = &columns[t];
        if let Some(m) = rows.iter().find_map(|r| {
            let v = r.values.get(s).unwrap_or(&Value::Null);
            qsa_sql::value::import(v.input(), c, style).err()
        }) {
            out.push((t, m));
        }
    }
    out
}

/// Why an upload stopped.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Stop {
    /// The system refused a row (or the delete): its message.
    System,
    /// A value does not fit its column (`QAD7035`).
    Misfit,
    /// The table cannot use commitment control (`SQL7008`, `55019`):
    /// upload without it.
    NoCommitmentControl,
    /// The file could not be read.
    File,
    Cancelled,
}

/// An upload that did not finish.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Failure {
    pub stop: Stop,
    /// The failing data row, from 1, and its line in the file.
    pub row: Option<u64>,
    pub line: Option<usize>,
    /// Rows the table keeps: committed, or in without commitment control.
    pub kept: u64,
    /// The data row to resume from, when rows were kept.
    pub resume: Option<u64>,
    /// Why, as one line.
    pub reason: String,
    pub message: Option<Box<ServerMessage>>,
}

impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.reason)
    }
}

fn reason(e: &Error) -> String {
    match e.server_message() {
        Some(m) => format!("{}: {}", m.id, m.text),
        None => e.to_string(),
    }
}

/// Whether `e` is the database refusing commitment control for the table.
pub fn no_commitment_control(e: &Error) -> bool {
    matches!(e, Error::Sql { failure, .. } if failure.sqlstate == "55019" || failure.sqlcode == -7008)
}

/// Reads the upload's file (all of it, or the first `limit` rows) by the
/// layout of `columns`, the target's.
pub fn read(
    u: &Upload,
    columns: &[Column],
    limit: Option<usize>,
) -> Result<Sheet, reader::ReadError> {
    reader::read(&u.file, &u.options, columns, limit)
}

/// Runs upload `u` on `q`. `progress` hears after every batch, `cancel`
/// stops it (as a failure). `job_dates` and `job_ccsid` read the job's
/// date format and CCSID 65535 text.
#[allow(clippy::too_many_lines)]
pub fn run(
    q: &mut SqlConnection,
    u: &Upload,
    job_dates: DateStyle,
    job_ccsid: u16,
    mut progress: impl FnMut(Progress),
    cancel: &AtomicBool,
) -> Result<Summary, Failure> {
    let start = Instant::now();
    let fail = |stop: Stop, reason: String| Failure {
        stop,
        row: None,
        line: None,
        kept: 0,
        resume: None,
        reason,
        message: None,
    };
    let columns = q.list_columns(&u.schema, &u.table).map_err(|e| Failure {
        message: e.server_message().cloned().map(Box::new),
        ..fail(Stop::System, reason(&e))
    })?;
    let sheet = read(u, &columns, None).map_err(|e| fail(Stop::File, e.to_string()))?;
    let width = sheet.rows.iter().map(|r| r.values.len()).max().unwrap_or(0);
    let map = resolve(&columns, sheet.header.as_deref(), &u.mapping, width);
    if map.is_empty() {
        return Err(fail(
            Stop::File,
            "no source column maps to a column of the table".into(),
        ));
    }
    let style = u.options.style(job_dates, job_ccsid);
    let skip = u.resume_row.unwrap_or(1).max(1) - 1;
    let budget = if u.batch_bytes == 0 {
        BATCH_BYTES
    } else {
        u.batch_bytes
    };

    let level = if u.commit == Commit::Without {
        CommitLevel::None
    } else {
        CommitLevel::ReadCommitted
    };
    q.set_options(SetOptions {
        commit: level,
        ..SetOptions::default()
    })
    .map_err(|e| fail(Stop::System, reason(&e)))?;

    let sys_fail = |e: Error, row: Option<(u64, usize)>, kept: u64, resume: Option<u64>| {
        let stop = if no_commitment_control(&e) {
            Stop::NoCommitmentControl
        } else {
            Stop::System
        };
        Failure {
            stop,
            row: row.map(|r| r.0),
            line: row.map(|r| r.1),
            kept,
            resume,
            reason: reason(&e),
            message: e.server_message().cloned().map(Box::new),
        }
    };
    let under_control = u.commit != Commit::Without;
    let undo = |q: &mut SqlConnection| {
        if under_control {
            let _ = q.rollback();
        }
    };

    if u.mode == Mode::Replace {
        let del = format!("DELETE FROM {}", qualified(&u.schema, &u.table));
        let r = q.prepare(&del).and_then(|p| {
            let o = q.execute_now(p.statement, Vec::new());
            let _ = q.release(p.statement);
            o
        });
        if let Err(e) = r {
            undo(q);
            return Err(sys_fail(e, None, 0, None));
        }
    }
    let names: Vec<String> = map
        .iter()
        .map(|&(t, _)| sql_name(&columns[t].name))
        .collect();
    let marks = vec!["?"; names.len()].join(", ");
    let insert = format!(
        "INSERT INTO {} ({}) VALUES ({marks})",
        qualified(&u.schema, &u.table),
        names.join(", ")
    );
    let p = match q.prepare(&insert) {
        Ok(p) => p,
        Err(e) => {
            undo(q);
            return Err(sys_fail(e, None, 0, None));
        }
    };

    // Data rows are counted from 1 in the whole file, resumed or not.
    let rows: Vec<(u64, &Row)> = sheet
        .rows
        .iter()
        .enumerate()
        .skip(usize::try_from(skip).unwrap_or(usize::MAX))
        .map(|(i, r)| (i as u64 + 1, r))
        .collect();
    let mut done = 0u64;
    // Rows the table keeps for sure, and the first row not kept.
    let mut kept = 0u64;
    let mut batches_since_commit = 0u32;
    let mut i = 0usize;
    while i < rows.len() {
        if cancel.load(Ordering::SeqCst) {
            undo(q);
            return Err(cancelled(u, kept));
        }
        // A batch: as many rows as fit in a request.
        let first = i;
        let mut batch: Vec<Vec<SqlValue>> = Vec::new();
        let mut bytes = 0usize;
        while i < rows.len() {
            let (n, r) = rows[i];
            let mut values = Vec::with_capacity(map.len());
            for &(t, s) in &map {
                let v = r.values.get(s).unwrap_or(&Value::Null);
                match qsa_sql::value::import(v.input(), &columns[t], &style) {
                    Ok(v) => values.push(v),
                    Err(m) => {
                        // What went in before this row is settled first.
                        let (kept_now, resume) = settle_before(q, u, &mut batch, kept, done);
                        return Err(Failure {
                            stop: Stop::Misfit,
                            row: Some(n),
                            line: Some(r.line),
                            kept: kept_now,
                            resume,
                            reason: m.to_string(),
                            message: None,
                        });
                    }
                }
            }
            let s: usize = values.iter().map(size).sum::<usize>() + 8;
            if !batch.is_empty() && bytes + s > budget {
                break;
            }
            bytes += s;
            batch.push(values);
            i += 1;
        }
        let n = batch.len() as u64;
        let pending = match q.execute(p.statement, batch) {
            Ok(x) => x,
            Err(e) => {
                undo(q);
                return Err(sys_fail(e, None, kept, None));
            }
        };
        let mut asked = false;
        let reply = loop {
            if !asked && cancel.load(Ordering::SeqCst) {
                pending.cancel();
                asked = true;
            }
            if let Some(r) = pending.wait_for(Duration::from_millis(50)) {
                break r;
            }
        };
        match q.executed(p.statement, reply) {
            Ok(o) if o.cancelled => {
                undo(q);
                let kept = if under_control {
                    kept
                } else {
                    done + u64::try_from(o.executed.updated).unwrap_or(0)
                };
                return Err(cancelled(u, kept));
            }
            Ok(_) => {
                done += n;
                batches_since_commit += 1;
                match u.commit {
                    Commit::Every { batches } if batches_since_commit >= batches.max(1) => {
                        if let Err(e) = q.commit() {
                            undo(q);
                            return Err(sys_fail(e, None, kept, Some(kept + 1)));
                        }
                        kept = done;
                        batches_since_commit = 0;
                    }
                    Commit::Without => kept = done,
                    _ => {}
                }
                progress(Progress {
                    rows: done,
                    elapsed: start.elapsed(),
                });
            }
            Err(e) => {
                let at = match &e {
                    Error::Sql { failure, .. } => failure.row.map(u64::from),
                    _ => None,
                };
                let failing = at.map(|r| first + usize::try_from(r).unwrap_or(0));
                let row = failing.and_then(|f| rows.get(f)).map(|(n, r)| (*n, r.line));
                undo(q);
                let (kept, resume) = match u.commit {
                    // Nothing kept: run it all again.
                    Commit::Once => (0, None),
                    // The batches committed stay; resume from the first
                    // row not committed.
                    Commit::Every { .. } => (kept, resume_at(u, kept)),
                    // Every row before the failing one stays, those of the
                    // failing request included; resume from the failing row.
                    Commit::Without => {
                        let before = at.unwrap_or(0);
                        let kept = done + before;
                        (kept, row.map(|r| r.0))
                    }
                };
                return Err(sys_fail(e, row, kept, resume));
            }
        }
    }
    if under_control && let Err(e) = q.commit() {
        undo(q);
        let resume = matches!(u.commit, Commit::Every { .. })
            .then(|| resume_at(u, kept))
            .flatten();
        return Err(sys_fail(e, None, kept, resume));
    }
    let _ = q.release(p.statement);
    Ok(Summary {
        rows: done,
        elapsed: start.elapsed(),
    })
}

/// The data row a run starts at, less one: rows before it are counted as
/// done already.
fn rows_start(u: &Upload) -> u64 {
    u.resume_row.unwrap_or(1).max(1) - 1
}

/// The data row to resume from when `kept` rows of this run stay: the
/// first row the table does not have. `None` when it has none of the file
/// (run it all again).
fn resume_at(u: &Upload, kept: u64) -> Option<u64> {
    let have = rows_start(u) + kept;
    (have > 0).then_some(have + 1)
}

/// A cancel, as a failure: what stays, and where to resume.
fn cancelled(u: &Upload, kept: u64) -> Failure {
    let resume = match u.commit {
        Commit::Once => None,
        Commit::Every { .. } | Commit::Without => resume_at(u, kept),
    };
    Failure {
        stop: Stop::Cancelled,
        row: None,
        line: None,
        kept,
        resume,
        reason: "cancelled".into(),
        message: None,
    }
}

/// Before a misfit stops the upload: rows already sent are rolled back
/// under commitment control (the committed batches stay), or stay without
/// it. What the table keeps, and where to resume.
fn settle_before(
    q: &mut SqlConnection,
    u: &Upload,
    batch: &mut Vec<Vec<SqlValue>>,
    kept: u64,
    done: u64,
) -> (u64, Option<u64>) {
    batch.clear();
    match u.commit {
        Commit::Once => {
            let _ = q.rollback();
            (0, None)
        }
        Commit::Every { .. } => {
            let _ = q.rollback();
            (kept, resume_at(u, kept))
        }
        Commit::Without => (done, resume_at(u, done)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn col(name: &str) -> Column {
        Column {
            name: name.into(),
            sql_type: "VARCHAR".into(),
            length: 10,
            nullable: true,
            ..Column::default()
        }
    }

    #[test]
    fn mapping_by_name_position_and_by_hand() {
        let cols = [col("ID"), col("NAME"), col("NOTE")];
        let header = vec!["name".to_owned(), "id".to_owned()];
        assert_eq!(resolve(&cols, Some(&header), &[], 2), [(0, 1), (1, 0)]);
        assert_eq!(resolve(&cols, None, &[], 2), [(0, 0), (1, 1)]);
        let by_hand = [
            Map {
                target: "NOTE".into(),
                from: SourceColumn::Position(0),
            },
            Map {
                target: "ID".into(),
                from: SourceColumn::Name("ID".into()),
            },
        ];
        assert_eq!(resolve(&cols, Some(&header), &by_hand, 2), [(0, 1), (2, 0)]);
    }
}
