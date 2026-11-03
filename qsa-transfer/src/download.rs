// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! Downloads: a table or a query's rows, fetched in blocks and written to a
//! file as they come.
//!
//! - The source is a table (with the columns chosen, in their order, and a
//!   `WHERE` clause) or a query (its result's columns, chosen by name).
//! - The rows go to a temporary file beside the output, renamed over it at
//!   the end; a cancel or a failure deletes it, so the output is never left
//!   half written. Appending copies the file there first.
//! - A row limit stops the fetch there; nothing is asked of the system for
//!   it, so any query works.

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

use qap_client::proto::ops::sql::{Column, SqlValue};
use qap_client::{Error, FetchEnd, ServerMessage, SqlConnection};
use qsa_sql::DateStyle;
use serde::{Deserialize, Serialize};

use crate::format::{Existing, FileOptions};
use crate::writer::{self, WriteError};

/// Where a download's rows come from.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Source {
    /// A table, view, alias or file.
    Table {
        schema: String,
        table: String,
        /// A `WHERE` clause without the word, empty for every row.
        #[serde(default, rename = "where", skip_serializing_if = "String::is_empty")]
        where_clause: String,
    },
    /// A query.
    Query { sql: String },
}

impl Default for Source {
    fn default() -> Self {
        Source::Query { sql: String::new() }
    }
}

/// A name as SQL writes it: as it is when an ordinary identifier, else
/// delimited.
pub fn sql_name(name: &str) -> String {
    let ordinary = name
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_uppercase() || matches!(c, '$' | '#' | '@'))
        && name.chars().all(|c| {
            c.is_ascii_uppercase() || c.is_ascii_digit() || matches!(c, '_' | '$' | '#' | '@')
        });
    if ordinary {
        name.to_owned()
    } else {
        format!("\"{}\"", name.replace('"', "\"\""))
    }
}

/// `schema.table` as SQL writes it.
pub fn qualified(schema: &str, table: &str) -> String {
    format!("{}.{}", sql_name(schema), sql_name(table))
}

impl Source {
    /// The statement that reads it, with `columns` (empty for all).
    pub fn statement(&self, columns: &[String]) -> String {
        match self {
            Source::Table {
                schema,
                table,
                where_clause,
            } => {
                let cols = if columns.is_empty() {
                    "*".to_owned()
                } else {
                    columns
                        .iter()
                        .map(|c| sql_name(c))
                        .collect::<Vec<_>>()
                        .join(", ")
                };
                let mut s = format!("SELECT {cols} FROM {}", qualified(schema, table));
                let w = where_clause.trim();
                if !w.is_empty() {
                    s.push_str(" WHERE ");
                    s.push_str(w);
                }
                s
            }
            Source::Query { sql } => sql.trim().trim_end_matches(';').trim().to_owned(),
        }
    }
}

/// A download: what, which columns, how many rows, and where.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Download {
    pub source: Source,
    /// The columns to write, in order, by name; empty for all.
    pub columns: Vec<String>,
    /// At most this many rows; `None` for all.
    pub limit: Option<u64>,
    pub file: PathBuf,
    pub options: FileOptions,
    pub existing: Existing,
}

/// How far a transfer is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Progress {
    pub rows: u64,
    pub elapsed: Duration,
}

/// A transfer that ended.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Summary {
    pub rows: u64,
    pub elapsed: Duration,
}

/// Why a download did not finish. The output file is as it was.
#[derive(Debug, thiserror::Error)]
pub enum Failure {
    /// The system refused the statement or a fetch.
    #[error("{0}")]
    System(Error),
    /// The file could not be written.
    #[error("{0}")]
    File(String),
    /// A column asked for is not in the result.
    #[error("column {0} is not in the result")]
    NoColumn(String),
    /// The output file is there and the request did not say to replace or
    /// append to it.
    #[error("{0} is there already")]
    Exists(PathBuf),
    /// Cancelled after `rows` rows.
    #[error("cancelled")]
    Cancelled {
        rows: u64,
        message: Option<ServerMessage>,
    },
}

impl Failure {
    /// The system's message, when it sent one.
    pub fn server_message(&self) -> Option<&ServerMessage> {
        match self {
            Failure::System(e) => e.server_message(),
            Failure::Cancelled { message, .. } => message.as_ref(),
            _ => None,
        }
    }
}

impl From<WriteError> for Failure {
    fn from(e: WriteError) -> Self {
        Failure::File(e.to_string())
    }
}

/// The temporary file a transfer writes before renaming it to `file`.
pub fn temp_path(file: &Path) -> PathBuf {
    let name = file
        .file_name()
        .map_or_else(|| "transfer".into(), |n| n.to_string_lossy().into_owned());
    file.with_file_name(format!(".{name}.{}.part", std::process::id()))
}

/// Deletes the temporary file when dropped, unless it was renamed.
struct Temp(Option<PathBuf>);

impl Drop for Temp {
    fn drop(&mut self) {
        if let Some(p) = self.0.take() {
            let _ = std::fs::remove_file(p);
        }
    }
}

/// The result's columns `wanted` picks, in its order: their indexes.
pub fn pick(columns: &[Column], wanted: &[String]) -> Result<Vec<usize>, Failure> {
    if wanted.is_empty() {
        return Ok((0..columns.len()).collect());
    }
    wanted
        .iter()
        .map(|w| {
            columns
                .iter()
                .position(|c| c.name.eq_ignore_ascii_case(w))
                .ok_or_else(|| Failure::NoColumn(w.clone()))
        })
        .collect()
}

/// Runs download `d` on `q`. `progress` hears after every block; `cancel`
/// stops it, and a cancel or failure leaves the output as it was.
/// `job_dates` and `job_ccsid` are the job's, for its date format and CCSID
/// 65535 text.
pub fn run(
    q: &mut SqlConnection,
    d: &Download,
    job_dates: DateStyle,
    job_ccsid: u16,
    mut progress: impl FnMut(Progress),
    cancel: &AtomicBool,
) -> Result<Summary, Failure> {
    let start = Instant::now();
    let exists = d.file.exists();
    let append = exists && d.existing == Existing::Append && d.options.format.appends();
    if exists && d.existing == Existing::Ask {
        return Err(Failure::Exists(d.file.clone()));
    }
    // A table's columns are chosen by the statement; a query's here.
    let (sql, choose) = match &d.source {
        Source::Table { .. } => (d.source.statement(&d.columns), Vec::new()),
        Source::Query { .. } => (d.source.statement(&[]), d.columns.clone()),
    };
    let p = q.prepare(&sql).map_err(Failure::System)?;
    let picked = pick(&p.columns, &choose)?;
    let columns: Vec<Column> = picked.iter().map(|&i| p.columns[i].clone()).collect();
    let o = q
        .execute_now(p.statement, Vec::new())
        .map_err(Failure::System)?;
    if o.cancelled {
        let _ = q.release(p.statement);
        return Err(Failure::Cancelled {
            rows: 0,
            message: o.message,
        });
    }
    let Some(cursor) = o.executed.cursor else {
        let _ = q.release(p.statement);
        return Err(Failure::File("the statement is not a query".into()));
    };

    let temp = Temp(Some(temp_path(&d.file)));
    let tp = temp.0.clone().expect("set");
    if append {
        std::fs::copy(&d.file, &tp).map_err(|e| Failure::File(e.to_string()))?;
    }
    let mut sink = writer::open(&tp, &columns, &d.options, job_dates, job_ccsid, append)?;
    let mut rows = 0u64;
    let mut write_error: Option<WriteError> = None;
    let end = q.fetch_into(
        cursor,
        d.limit,
        &mut |block: Vec<Vec<SqlValue>>| {
            for r in block {
                let values: Vec<SqlValue> = picked
                    .iter()
                    .map(|&i| r.get(i).cloned().unwrap_or_default())
                    .collect();
                if let Err(e) = sink.row(&values) {
                    let text = e.to_string();
                    write_error = Some(e);
                    return Err(std::io::Error::other(text));
                }
                rows += 1;
            }
            progress(Progress {
                rows,
                elapsed: start.elapsed(),
            });
            Ok(())
        },
        cancel,
    );
    let end = match end {
        Ok(e) => e,
        Err(Error::Write(_)) if write_error.is_some() => {
            let _ = q.close_cursor(cursor);
            let _ = q.release(p.statement);
            return Err(write_error.expect("checked").into());
        }
        Err(e) => {
            let _ = q.release(p.statement);
            return Err(Failure::System(e));
        }
    };
    if let FetchEnd::Cancelled { message, .. } = end {
        let _ = q.release(p.statement);
        return Err(Failure::Cancelled { rows, message });
    }
    let _ = q.close_cursor(cursor);
    let _ = q.release(p.statement);
    sink.finish()?;
    let mut temp = temp;
    let tp = temp.0.take().expect("set");
    if let Err(e) = std::fs::rename(&tp, &d.file) {
        // Windows will not rename over a file some programs hold open.
        let _ = std::fs::remove_file(&tp);
        return Err(Failure::File(format!("{}: {e}", d.file.display())));
    }
    Ok(Summary {
        rows,
        elapsed: start.elapsed(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn statements_for_a_table_and_a_query() {
        let t = Source::Table {
            schema: "HLINV".into(),
            table: "ASSETS".into(),
            where_clause: "STATUS = 'ACTIVE'".into(),
        };
        assert_eq!(
            t.statement(&["ASSET_TAG".into(), "cost".into()]),
            "SELECT ASSET_TAG, \"cost\" FROM HLINV.ASSETS WHERE STATUS = 'ACTIVE'"
        );
        let all = Source::Table {
            schema: "my lib".into(),
            table: "T".into(),
            where_clause: String::new(),
        };
        assert_eq!(all.statement(&[]), "SELECT * FROM \"my lib\".T");
        let q = Source::Query {
            sql: " SELECT 1 FROM X; ".into(),
        };
        assert_eq!(q.statement(&[]), "SELECT 1 FROM X");
        assert_eq!(sql_name("A\"B"), "\"A\"\"B\"");
        assert!(
            temp_path(Path::new("/o/a.csv"))
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with(".a.csv.")
        );
    }
}
