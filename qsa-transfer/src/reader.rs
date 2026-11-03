// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! Readers of transfer files, for uploads: every row with the line (or
//! sheet row) it came from.
//!
//! - CSV and TSV: an empty unquoted field is NULL, `""` an empty string.
//! - Fixed width: by the layout the upload gives (widths, alignment, null
//!   indicators).
//! - XLSX and ODS (calamine): an empty cell is NULL; text, number and
//!   boolean cells are themselves; a date cell becomes ISO 8601 text.

use std::path::Path;

use calamine::{Data, Reader};
use qap_client::proto::ops::sql::Column;

use crate::format::{FileOptions, Format};
use crate::text::{self, FixedColumn};
use crate::writer::fixed_columns;

/// A value read from a file.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Null,
    Text(String),
    Number(f64),
    Bool(bool),
}

impl Value {
    /// The value as [`qsa_sql::value::import`] takes it.
    pub fn input(&self) -> qsa_sql::Input<'_> {
        match self {
            Value::Null => qsa_sql::Input::Null,
            Value::Text(t) => qsa_sql::Input::Text(t),
            Value::Number(f) => qsa_sql::Input::Number(*f),
            Value::Bool(b) => qsa_sql::Input::Bool(*b),
        }
    }

    /// The value as a preview shows it.
    pub fn shown(&self) -> String {
        match self {
            Value::Null => String::new(),
            Value::Text(t) => t.clone(),
            Value::Number(f) => qsa_sql::decimal::from_f64(*f),
            Value::Bool(b) => if *b { "TRUE" } else { "FALSE" }.to_owned(),
        }
    }
}

/// A row read, and where.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub values: Vec<Value>,
    /// The line (text) or row (sheet) it started on, from 1.
    pub line: usize,
}

/// A file read: its header row, if it has one, and its rows.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Sheet {
    pub header: Option<Vec<String>>,
    pub rows: Vec<Row>,
}

/// Why a file could not be read.
#[derive(Debug, thiserror::Error)]
pub enum ReadError {
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Decode(#[from] text::DecodeError),
    #[error("{0}")]
    Sheet(String),
    #[error("the file has no sheet named {0}")]
    NoSheet(String),
}

/// The sheets of a spreadsheet file, in order.
pub fn sheets(path: &Path) -> Result<Vec<String>, ReadError> {
    let book = calamine::open_workbook_auto(path).map_err(|e| ReadError::Sheet(e.to_string()))?;
    Ok(book.sheet_names())
}

/// An Excel serial date and time as ISO 8601: the date alone when it has
/// no time.
fn serial_to_iso(serial: f64) -> String {
    // Day 0 is 1899-12-30, as both Excel and LibreOffice count.
    let days = serial.floor();
    #[allow(clippy::cast_possible_truncation)]
    let day = days as i64 + qsa_sql::datetime::day_number(1899, 12, 30);
    #[allow(clippy::cast_possible_truncation)]
    let ms = ((serial - days) * 86_400_000.0).round() as i64;
    let date = qsa_sql::datetime::date_text(day * 86_400_000, qsa_sql::DateStyle::Iso);
    if ms == 0 {
        date
    } else {
        let t = qsa_sql::datetime::time_text(ms, qsa_sql::DateStyle::Iso);
        if ms % 1000 == 0 {
            format!("{date}T{t}")
        } else {
            format!("{date}T{t}.{:03}", ms % 1000)
        }
    }
}

fn cell(d: &Data) -> Value {
    match d {
        Data::Empty => Value::Null,
        Data::String(s) => Value::Text(s.clone()),
        // A whole number kept exactly, as text.
        Data::Int(i) => Value::Text(i.to_string()),
        Data::Float(f) => Value::Number(*f),
        Data::Bool(b) => Value::Bool(*b),
        Data::DateTime(dt) => Value::Text(serial_to_iso(dt.as_f64())),
        Data::DateTimeIso(s) | Data::DurationIso(s) => Value::Text(s.clone()),
        Data::Error(e) => Value::Text(e.to_string()),
    }
}

/// Reads `path` as `opts` says, at most `limit` rows (`None` for all).
/// A fixed-width file is read by the layout for `columns` (the target's,
/// in file order).
pub fn read(
    path: &Path,
    opts: &FileOptions,
    columns: &[Column],
    limit: Option<usize>,
) -> Result<Sheet, ReadError> {
    let take = limit.unwrap_or(usize::MAX);
    match opts.format {
        Format::Csv | Format::Tsv => {
            let t = text::decode(&std::fs::read(path)?, opts.encoding)?;
            let mut recs = text::delimited_records(&t, opts.delimiter(), opts.quote).into_iter();
            let header = if opts.header {
                recs.next().map(|r| {
                    r.fields
                        .into_iter()
                        .map(Option::unwrap_or_default)
                        .collect()
                })
            } else {
                None
            };
            let rows = recs
                .take(take)
                .map(|r| Row {
                    values: r
                        .fields
                        .into_iter()
                        .map(|f| f.map_or(Value::Null, Value::Text))
                        .collect(),
                    line: r.line,
                })
                .collect();
            Ok(Sheet { header, rows })
        }
        Format::Fixed => {
            let t = text::decode(&std::fs::read(path)?, opts.encoding)?;
            let layout: Vec<FixedColumn> = fixed_columns(columns, opts);
            let rows = text::fixed_records(&t, &layout)
                .into_iter()
                .take(take)
                .map(|r| Row {
                    values: r
                        .fields
                        .into_iter()
                        .map(|f| f.map_or(Value::Null, Value::Text))
                        .collect(),
                    line: r.line,
                })
                .collect();
            Ok(Sheet { header: None, rows })
        }
        Format::Xlsx | Format::Ods => {
            let mut book =
                calamine::open_workbook_auto(path).map_err(|e| ReadError::Sheet(e.to_string()))?;
            let names = book.sheet_names();
            let name = if opts.sheet.trim().is_empty() {
                names
                    .first()
                    .cloned()
                    .ok_or_else(|| ReadError::NoSheet("Sheet1".into()))?
            } else {
                names
                    .iter()
                    .find(|n| n.eq_ignore_ascii_case(opts.sheet.trim()))
                    .cloned()
                    .ok_or_else(|| ReadError::NoSheet(opts.sheet.clone()))?
            };
            let range = book
                .worksheet_range(&name)
                .map_err(|e| ReadError::Sheet(e.to_string()))?;
            // The range starts at its first used cell: rows and columns
            // before it are empty.
            let (top, left) = range
                .start()
                .map_or((0, 0), |(r, c)| (r as usize, c as usize));
            let mut rows = range.rows().enumerate().map(|(i, r)| {
                let mut values: Vec<Value> = std::iter::repeat_n(Value::Null, left).collect();
                values.extend(r.iter().map(cell));
                Row {
                    values,
                    line: top + i + 1,
                }
            });
            let header = if opts.header {
                rows.next()
                    .map(|r| r.values.iter().map(Value::shown).collect())
            } else {
                None
            };
            Ok(Sheet {
                header,
                rows: rows.take(take).collect(),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serial_dates() {
        assert_eq!(serial_to_iso(46_328.0), "2026-11-02");
        assert_eq!(serial_to_iso(46_328.5), "2026-11-02T12:00:00");
        assert_eq!(serial_to_iso(25_569.0), "1970-01-01");
    }
}
