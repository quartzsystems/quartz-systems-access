// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! Values for showing and for transfer files, and back.
//!
//! - [`display`]: a grid cell. Decimals with the column's scale, dates and
//!   times in the [`Style`]'s format (the job's, for Run SQL), timestamps
//!   to the column's precision, `TRUE` or `FALSE`, binary data as
//!   `X'C1C2'` (CCSID 65535 character data as text when asked). `None` is
//!   NULL.
//! - [`text_field`]: CSV, TSV and fixed width: the exact text. Decimals as
//!   sent (with the transfer's decimal point), dates in ISO 8601 unless the
//!   transfer says otherwise, `true` or `false`, binary data as hex digits.
//! - [`sheet_cell`]: XLSX and ODS: a number cell only for a value a
//!   spreadsheet keeps exactly (at most 15 significant digits; a `DECFLOAT`
//!   only when it reads back as the same text), otherwise text. Special
//!   values and long E notation are always text. Booleans are boolean
//!   cells; dates, times and binary data text.
//! - [`import`]: a field or cell back into the value for a target column,
//!   or a [`Misfit`] (`QAD7035`) saying why it does not fit.

use qap_proto::ops::sql::{Column, SqlValue};

use crate::binary;
use crate::datetime::{self, DateStyle};
use crate::decimal;

/// How CCSID 65535 character data is shown and written.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BitData {
    /// As hex.
    #[default]
    Hex,
    /// As text, translated from this CCSID (the job's).
    Text { ccsid: u16 },
}

/// How values are written.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Style {
    pub dates: DateStyle,
    /// The decimal point for decimals and floats.
    pub point: char,
    pub bit_data: BitData,
}

impl Default for Style {
    fn default() -> Self {
        Style {
            dates: DateStyle::Iso,
            point: '.',
            bit_data: BitData::Hex,
        }
    }
}

/// What a column holds, from its SQL type.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Integer {
        min: i64,
        max: i64,
    },
    Decimal {
        precision: u32,
        scale: u16,
    },
    DecFloat,
    Float,
    /// Character or graphic data; `bits` for CCSID 65535.
    Character {
        fixed: bool,
        bits: bool,
    },
    Binary,
    Date,
    Time,
    Timestamp,
    Boolean,
    /// A type this program does not know: taken as text.
    Other,
}

/// The base of an SQL type name: `DECIMAL(11,2)` is `DECIMAL`.
fn base(sql_type: &str) -> String {
    let t = sql_type.trim().to_ascii_uppercase();
    let t = t.split('(').next().unwrap_or("").trim().to_owned();
    t.split(" FOR ").next().unwrap_or("").trim().to_owned()
}

/// What `c` holds.
pub fn kind(c: &Column) -> Kind {
    let bits = c.ccsid == 65535 || c.sql_type.to_ascii_uppercase().contains("FOR BIT DATA");
    match base(&c.sql_type).as_str() {
        "SMALLINT" => Kind::Integer {
            min: i64::from(i16::MIN),
            max: i64::from(i16::MAX),
        },
        "INTEGER" | "INT" => Kind::Integer {
            min: i64::from(i32::MIN),
            max: i64::from(i32::MAX),
        },
        "BIGINT" => Kind::Integer {
            min: i64::MIN,
            max: i64::MAX,
        },
        "DECIMAL" | "NUMERIC" | "DEC" => Kind::Decimal {
            precision: c.length,
            scale: c.scale,
        },
        "DECFLOAT" => Kind::DecFloat,
        "REAL" | "DOUBLE" | "FLOAT" | "DOUBLE PRECISION" => Kind::Float,
        "CHAR" | "CHARACTER" | "GRAPHIC" | "NCHAR" => Kind::Character { fixed: true, bits },
        "VARCHAR" | "CHARACTER VARYING" | "VARGRAPHIC" | "CLOB" | "DBCLOB" | "NVARCHAR"
        | "NCLOB" => Kind::Character { fixed: false, bits },
        "BINARY" | "VARBINARY" | "BLOB" => Kind::Binary,
        "DATE" => Kind::Date,
        "TIME" => Kind::Time,
        "TIMESTAMP" | "TIMESTMP" => Kind::Timestamp,
        "BOOLEAN" => Kind::Boolean,
        _ => Kind::Other,
    }
}

/// The column's type as a person writes it: `DECIMAL(11,2)`,
/// `VARCHAR(40)`, `TIMESTAMP(12)`, `CHAR(8) FOR BIT DATA`.
pub fn type_name(c: &Column) -> String {
    let b = base(&c.sql_type);
    match kind(c) {
        Kind::Decimal { precision, scale } => format!("{b}({precision},{scale})"),
        Kind::Character { bits, .. } => {
            let n = if c.length > 0 {
                format!("{b}({})", c.length)
            } else {
                b
            };
            if bits && !n.contains("FOR BIT DATA") {
                format!("{n} FOR BIT DATA")
            } else {
                n
            }
        }
        Kind::Binary | Kind::DecFloat if c.length > 0 => format!("{b}({})", c.length),
        Kind::Timestamp => format!("{b}({})", c.scale),
        _ => b,
    }
}

/// Whether CCSID 65535 character data of `c` is translated by `style`.
fn translated(c: &Column, style: &Style) -> Option<u16> {
    match (kind(c), style.bit_data) {
        (Kind::Character { bits: true, .. }, BitData::Text { ccsid })
            if binary::can_translate(ccsid) =>
        {
            Some(ccsid)
        }
        _ => None,
    }
}

/// A value as a grid shows it; `None` for NULL.
pub fn display(v: &SqlValue, c: &Column, style: &Style) -> Option<String> {
    Some(match v {
        SqlValue::Null => return None,
        SqlValue::Integer { value } => value.to_string(),
        SqlValue::Decimal { text } => {
            let t = match kind(c) {
                Kind::Decimal { scale, .. } => decimal::with_scale(text, scale),
                _ => text.clone(),
            };
            decimal::with_point(&t, style.point)
        }
        SqlValue::Float { text } => decimal::with_point(text, style.point),
        SqlValue::Text { text } => text.clone(),
        SqlValue::Binary { data } => match translated(c, style) {
            Some(ccsid) => binary::to_text(data, ccsid).unwrap_or_default(),
            None => binary::hex_literal(data),
        },
        SqlValue::Date { ms } => datetime::date_text(*ms, style.dates),
        SqlValue::Time { ms } => datetime::time_text(*ms, style.dates),
        SqlValue::Timestamp { ms, picos } => {
            datetime::timestamp_text(*ms, *picos, c.scale, style.dates)
        }
        SqlValue::Boolean { value } => if *value { "TRUE" } else { "FALSE" }.to_owned(),
    })
}

/// A value as CSV, TSV and fixed width write it: the exact text; `None`
/// for NULL.
pub fn text_field(v: &SqlValue, c: &Column, style: &Style) -> Option<String> {
    Some(match v {
        SqlValue::Null => return None,
        SqlValue::Integer { value } => value.to_string(),
        SqlValue::Decimal { text } | SqlValue::Float { text } => {
            decimal::with_point(text, style.point)
        }
        SqlValue::Text { text } => text.clone(),
        SqlValue::Binary { data } => match translated(c, style) {
            Some(ccsid) => binary::to_text(data, ccsid).unwrap_or_default(),
            None => binary::hex(data),
        },
        SqlValue::Date { ms } => datetime::date_text(*ms, style.dates),
        SqlValue::Time { ms } => datetime::time_text(*ms, style.dates),
        SqlValue::Timestamp { ms, picos } => {
            datetime::timestamp_text(*ms, *picos, c.scale, style.dates)
        }
        SqlValue::Boolean { value } => if *value { "true" } else { "false" }.to_owned(),
    })
}

/// A spreadsheet cell.
#[derive(Clone, Debug, PartialEq)]
pub enum Cell {
    /// No value: NULL.
    Empty,
    Text(String),
    /// A number the cell keeps exactly; `text` is the exact value.
    Number {
        value: f64,
        text: String,
    },
    Bool(bool),
}

/// A value as XLSX and ODS write it.
pub fn sheet_cell(v: &SqlValue, c: &Column, style: &Style) -> Cell {
    let as_text = || text_field(v, c, style).map_or(Cell::Empty, Cell::Text);
    match v {
        SqlValue::Null => Cell::Empty,
        SqlValue::Boolean { value } => Cell::Bool(*value),
        SqlValue::Integer { value } => {
            let text = value.to_string();
            match decimal::spreadsheet_exact(&text, false) {
                Some(f) => Cell::Number { value: f, text },
                None => Cell::Text(text),
            }
        }
        SqlValue::Decimal { text } => {
            let as_written = !matches!(kind(c), Kind::Decimal { .. });
            match decimal::spreadsheet_exact(text, as_written) {
                Some(f) => Cell::Number {
                    value: f,
                    text: text.clone(),
                },
                None => as_text(),
            }
        }
        SqlValue::Float { text } => match text.parse::<f64>() {
            Ok(f) if f.is_finite() => Cell::Number {
                value: f,
                text: text.clone(),
            },
            _ => as_text(),
        },
        _ => as_text(),
    }
}

/// A field or cell read from a transfer file.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Input<'a> {
    /// No value: an empty unquoted CSV field, an empty cell. NULL.
    Null,
    Text(&'a str),
    Number(f64),
    Bool(bool),
}

/// Why a value does not fit its column: `QAD7035`'s column, value and
/// type.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Misfit {
    pub column: String,
    pub value: String,
    pub sql_type: String,
}

impl std::fmt::Display for Misfit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Column {}: value {} does not fit {}",
            self.column, self.value, self.sql_type
        )
    }
}

impl std::error::Error for Misfit {}

/// A boolean as text: `true`, `false`, `1`, `0`, any case.
fn boolean(t: &str) -> Option<bool> {
    match t.trim().to_ascii_lowercase().as_str() {
        "true" | "1" => Some(true),
        "false" | "0" => Some(false),
        _ => None,
    }
}

/// `input` as the value for column `c`, read with `style` (the dates and
/// decimal point the transfer uses, and how CCSID 65535 data was written).
pub fn import(input: Input<'_>, c: &Column, style: &Style) -> Result<SqlValue, Misfit> {
    let shown = match input {
        Input::Null => "NULL".to_owned(),
        Input::Text(t) => t.to_owned(),
        Input::Number(f) => decimal::from_f64(f),
        Input::Bool(b) => b.to_string(),
    };
    let misfit = || Misfit {
        column: c.name.clone(),
        value: shown.clone(),
        sql_type: type_name(c),
    };
    if input == Input::Null {
        return if c.nullable {
            Ok(SqlValue::Null)
        } else {
            Err(misfit())
        };
    }
    let number_text = |scale: Option<u16>| -> Option<String> {
        match input {
            Input::Text(t) => decimal::from_input(t, style.point),
            Input::Number(f) => Some(decimal::from_sheet(f, scale)),
            Input::Bool(b) => Some(if b { "1" } else { "0" }.to_owned()),
            Input::Null => None,
        }
    };
    let text = || match input {
        Input::Text(t) => t.to_owned(),
        Input::Number(f) => decimal::from_f64(f),
        Input::Bool(b) => b.to_string(),
        Input::Null => String::new(),
    };
    Ok(match kind(c) {
        Kind::Integer { min, max } => {
            let n: i64 = match input {
                Input::Text(t) => t
                    .trim()
                    .strip_prefix('+')
                    .unwrap_or(t.trim())
                    .parse()
                    .map_err(|_| misfit())?,
                // A number cell holds at most 15 significant digits exactly
                // (sheet_cell writes no more): its shortest text is the
                // integer, whatever its size.
                Input::Number(f) if f.fract() == 0.0 => {
                    let t = decimal::from_f64(f);
                    if decimal::is_e_notation(&t) {
                        return Err(misfit());
                    }
                    t.parse().map_err(|_| misfit())?
                }
                Input::Bool(b) => i64::from(b),
                _ => return Err(misfit()),
            };
            if !(min..=max).contains(&n) {
                return Err(misfit());
            }
            SqlValue::Integer { value: n }
        }
        Kind::Decimal { precision, scale } => {
            let t = number_text(Some(scale)).ok_or_else(misfit)?;
            if decimal::is_special(&t) || !decimal::fits(&t, precision, scale) {
                return Err(misfit());
            }
            SqlValue::Decimal { text: t }
        }
        Kind::DecFloat => SqlValue::Decimal {
            text: number_text(None).ok_or_else(misfit)?,
        },
        Kind::Float => {
            let t = number_text(None).ok_or_else(misfit)?;
            if t.parse::<f64>().is_err() {
                return Err(misfit());
            }
            SqlValue::Float { text: t }
        }
        Kind::Character { bits: true, .. } => {
            let t = text();
            let data = match style.bit_data {
                BitData::Text { ccsid } if binary::can_translate(ccsid) => {
                    binary::from_text(&t, ccsid)
                }
                _ => binary::parse_hex(&t),
            }
            .ok_or_else(misfit)?;
            if c.length > 0 && data.len() > c.length as usize {
                return Err(misfit());
            }
            SqlValue::Binary { data }
        }
        Kind::Character { fixed, .. } => {
            let t = text();
            let n = if fixed {
                t.trim_end_matches(' ').chars().count()
            } else {
                t.chars().count()
            };
            if c.length > 0 && n > c.length as usize {
                return Err(misfit());
            }
            SqlValue::Text { text: t }
        }
        Kind::Binary => {
            let data = binary::parse_hex(&text()).ok_or_else(misfit)?;
            if c.length > 0 && data.len() > c.length as usize {
                return Err(misfit());
            }
            SqlValue::Binary { data }
        }
        Kind::Date => SqlValue::Date {
            ms: datetime::parse_date(&text(), style.dates).ok_or_else(misfit)?,
        },
        Kind::Time => SqlValue::Time {
            ms: datetime::parse_time(&text(), style.dates).ok_or_else(misfit)?,
        },
        Kind::Timestamp => {
            let (ms, picos) = datetime::parse_timestamp(&text(), style.dates).ok_or_else(misfit)?;
            SqlValue::Timestamp { ms, picos }
        }
        Kind::Boolean => SqlValue::Boolean {
            value: match input {
                Input::Bool(b) => b,
                Input::Number(f) => f != 0.0,
                Input::Text(t) => boolean(t).ok_or_else(misfit)?,
                Input::Null => return Err(misfit()),
            },
        },
        Kind::Other => SqlValue::Text { text: text() },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn col(name: &str, sql_type: &str, length: u32, scale: u16, ccsid: u16) -> Column {
        Column {
            name: name.into(),
            label: name.into(),
            sql_type: sql_type.into(),
            length,
            scale,
            nullable: true,
            ccsid,
        }
    }

    #[test]
    fn display_per_decision() {
        let s = Style {
            dates: DateStyle::job("*MDY", "/", ":"),
            ..Style::default()
        };
        let dec = col("COST", "DECIMAL", 11, 2, 0);
        assert_eq!(
            display(
                &SqlValue::Decimal {
                    text: "1200".into()
                },
                &dec,
                &s
            )
            .unwrap(),
            "1200.00"
        );
        assert_eq!(display(&SqlValue::Null, &dec, &s), None);
        let bits = col("C_BITS", "CHAR", 8, 0, 65535);
        let v = SqlValue::Binary {
            data: vec![0xC1, 0xC2, 0xC3, 0x40],
        };
        assert_eq!(display(&v, &bits, &s).unwrap(), "X'C1C2C340'");
        let t = Style {
            bit_data: BitData::Text { ccsid: 37 },
            ..s
        };
        assert_eq!(display(&v, &bits, &t).unwrap(), "ABC ");
        // Binary columns stay hex whatever the option.
        let blob = col("C_BLOB", "BLOB", 65536, 0, 65535);
        assert_eq!(display(&v, &blob, &t).unwrap(), "X'C1C2C340'");
        let b = col("B", "BOOLEAN", 0, 0, 0);
        assert_eq!(
            display(&SqlValue::Boolean { value: true }, &b, &s).unwrap(),
            "TRUE"
        );
        assert_eq!(
            text_field(&SqlValue::Boolean { value: false }, &b, &s).unwrap(),
            "false"
        );
        let d = col("D", "DATE", 0, 0, 0);
        assert_eq!(
            display(&SqlValue::Date { ms: 0 }, &d, &s).unwrap(),
            "01/01/1970"
        );
        assert_eq!(
            text_field(&SqlValue::Date { ms: 0 }, &d, &Style::default()).unwrap(),
            "1970-01-01"
        );
        assert_eq!(type_name(&dec), "DECIMAL(11,2)");
        assert_eq!(type_name(&bits), "CHAR(8) FOR BIT DATA");
        assert_eq!(
            type_name(&col("T", "TIMESTAMP", 32, 12, 0)),
            "TIMESTAMP(12)"
        );
    }

    #[test]
    fn sheet_cells() {
        let s = Style::default();
        let dec = col("COST", "DECIMAL", 11, 2, 0);
        let df = col("DF", "DECFLOAT", 34, 0, 0);
        let big = col("BIG", "BIGINT", 0, 0, 0);
        assert_eq!(
            sheet_cell(
                &SqlValue::Decimal {
                    text: "123456789.01".into()
                },
                &dec,
                &s
            ),
            Cell::Number {
                value: 123_456_789.01,
                text: "123456789.01".into()
            }
        );
        assert_eq!(
            sheet_cell(
                &SqlValue::Integer {
                    value: -9_007_199_254_740_992
                },
                &big,
                &s
            ),
            Cell::Text("-9007199254740992".into())
        );
        for t in [
            "Infinity",
            "-Infinity",
            "NaN",
            "sNaN",
            "1.50",
            "1.234567890123456789E-30",
        ] {
            assert_eq!(
                sheet_cell(&SqlValue::Decimal { text: t.into() }, &df, &s),
                Cell::Text(t.into()),
                "{t}"
            );
        }
        assert!(matches!(
            sheet_cell(
                &SqlValue::Decimal {
                    text: "1.5E-30".into()
                },
                &df,
                &s
            ),
            Cell::Number { .. }
        ));
        assert_eq!(sheet_cell(&SqlValue::Null, &dec, &s), Cell::Empty);
        assert_eq!(
            sheet_cell(&SqlValue::Boolean { value: true }, &dec, &s),
            Cell::Bool(true)
        );
    }

    #[test]
    fn import_and_misfits() {
        let s = Style::default();
        let dec = col("COST", "DECIMAL", 9, 2, 0);
        assert_eq!(
            import(Input::Text("123.45"), &dec, &s),
            Ok(SqlValue::Decimal {
                text: "123.45".into()
            })
        );
        assert_eq!(
            import(Input::Number(1200.0), &dec, &s),
            Ok(SqlValue::Decimal {
                text: "1200.00".into()
            })
        );
        let e = import(Input::Text("123456789.123"), &dec, &s).unwrap_err();
        assert_eq!(
            e.to_string(),
            "Column COST: value 123456789.123 does not fit DECIMAL(9,2)"
        );
        let small = col("S", "SMALLINT", 0, 0, 0);
        assert!(import(Input::Text("40000"), &small, &s).is_err());
        assert_eq!(
            import(Input::Number(7.0), &small, &s),
            Ok(SqlValue::Integer { value: 7 })
        );
        let vc = col("V", "VARCHAR", 3, 0, 1208);
        assert!(import(Input::Text("abcd"), &vc, &s).is_err());
        assert_eq!(
            import(Input::Text(""), &vc, &s),
            Ok(SqlValue::Text {
                text: String::new()
            })
        );
        assert_eq!(import(Input::Null, &vc, &s), Ok(SqlValue::Null));
        let mut nn = vc.clone();
        nn.nullable = false;
        assert!(import(Input::Null, &nn, &s).is_err());
        let bits = col("C_BITS", "CHAR", 8, 0, 65535);
        assert_eq!(
            import(Input::Text("C1C2"), &bits, &s),
            Ok(SqlValue::Binary {
                data: vec![0xC1, 0xC2]
            })
        );
        let t = Style {
            bit_data: BitData::Text { ccsid: 37 },
            ..s
        };
        assert_eq!(
            import(Input::Text("AB"), &bits, &t),
            Ok(SqlValue::Binary {
                data: vec![0xC1, 0xC2]
            })
        );
        let b = col("B", "BOOLEAN", 0, 0, 0);
        assert_eq!(
            import(Input::Text("TRUE"), &b, &s),
            Ok(SqlValue::Boolean { value: true })
        );
        assert_eq!(
            import(Input::Bool(false), &b, &s),
            Ok(SqlValue::Boolean { value: false })
        );
        assert!(import(Input::Text("maybe"), &b, &s).is_err());
        let comma = Style { point: ',', ..s };
        assert_eq!(
            import(Input::Text("-1,25"), &col("F", "DOUBLE", 0, 0, 0), &comma),
            Ok(SqlValue::Float {
                text: "-1.25".into()
            })
        );
    }
}
