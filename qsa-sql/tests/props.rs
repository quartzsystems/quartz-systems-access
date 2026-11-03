// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! Property tests over many generated values (a seeded generator, so a
//! failure names its case and reproduces): decimal text goes in and comes
//! out unchanged through every path a transfer takes it (a CSV field with
//! either decimal point, a spreadsheet number when one is used), and dates,
//! times and timestamps read back as written in every style.

use qap_proto::ops::sql::{Column, SqlValue};
use qsa_sql::datetime::{self, DateStyle, day_number};
use qsa_sql::value::{self, Cell, Input, Style};
use qsa_sql::{BitData, decimal};

/// xorshift64*: small, fast, and the same everywhere.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
    fn digits(&mut self, n: usize) -> String {
        (0..n)
            .map(|_| char::from(b'0' + u8::try_from(self.below(10)).unwrap()))
            .collect()
    }
}

const CASES: u64 = 20_000;

fn col(sql_type: &str, length: u32, scale: u16) -> Column {
    Column {
        name: "C".into(),
        label: "C".into(),
        sql_type: sql_type.into(),
        length,
        scale,
        nullable: true,
        ccsid: 0,
    }
}

/// A `DECIMAL(p,s)` value as the system sends it: its scale's digits.
fn fixed(r: &mut Rng, p: u32, s: u16) -> String {
    let int_len = r.below(u64::from(p - u32::from(s)) + 1) as usize;
    let mut int = r.digits(int_len).trim_start_matches('0').to_owned();
    if int.is_empty() {
        int.push('0');
    }
    let frac = r.digits(usize::from(s));
    let neg = r.below(2) == 0 && (int != "0" || frac.chars().any(|c| c != '0'));
    let sign = if neg { "-" } else { "" };
    if s == 0 {
        format!("{sign}{int}")
    } else {
        format!("{sign}{int}.{frac}")
    }
}

/// A `DECFLOAT` value: plain, E notation, or special.
fn decfloat(r: &mut Rng) -> String {
    match r.below(10) {
        0 => ["Infinity", "-Infinity", "NaN", "sNaN"][r.below(4) as usize].to_owned(),
        1..=4 => {
            let n = r.below(34) as usize + 1;
            let mut m = r.digits(n);
            if n > 1 {
                m.insert(1, '.');
            }
            let e = r.below(12_000) as i64 - 6_000;
            format!("{}{m}E{e}", if r.below(2) == 0 { "-" } else { "" })
        }
        _ => {
            let n = r.below(20) as usize + 1;
            let int = r.digits(n);
            let n = r.below(14) as usize;
            let frac = r.digits(n);
            let sign = if r.below(2) == 0 { "-" } else { "" };
            if frac.is_empty() {
                format!("{sign}{int}")
            } else {
                format!("{sign}{int}.{frac}")
            }
        }
    }
}

/// Through a CSV field and back, with `point`.
fn through_text(v: &SqlValue, c: &Column, point: char) -> SqlValue {
    let style = Style {
        point,
        ..Style::default()
    };
    let field = value::text_field(v, c, &style).unwrap();
    value::import(Input::Text(&field), c, &style).unwrap()
}

/// Through a spreadsheet cell and back.
fn through_sheet(v: &SqlValue, c: &Column) -> SqlValue {
    let s = Style::default();
    match value::sheet_cell(v, c, &s) {
        Cell::Number { value, .. } => value::import(Input::Number(value), c, &s).unwrap(),
        Cell::Text(t) => value::import(Input::Text(&t), c, &s).unwrap(),
        other => panic!("{other:?}"),
    }
}

#[test]
fn decimal_text_in_is_text_out() {
    let mut r = Rng(0x5153_3430_3020_4441);
    for case in 0..CASES {
        let p = r.below(31) as u32 + 1;
        let s = r.below(u64::from(p) + 1) as u16;
        let c = col("DECIMAL", p, s);
        let t = fixed(&mut r, p, s);
        let v = SqlValue::Decimal { text: t.clone() };
        assert!(decimal::is_valid(&t), "case {case}: {t}");
        assert_eq!(
            decimal::from_input(&t, '.').as_deref(),
            Some(t.as_str()),
            "case {case}"
        );
        for point in ['.', ','] {
            assert_eq!(
                through_text(&v, &c, point),
                v,
                "case {case}: {t} with {point}"
            );
        }
        assert_eq!(
            through_sheet(&v, &c),
            v,
            "case {case}: {t} DECIMAL({p},{s})"
        );
        assert_eq!(
            decimal::with_scale(&t, s),
            t,
            "case {case}: already at its scale"
        );
    }
}

#[test]
fn decfloat_text_in_is_text_out() {
    let mut r = Rng(0x4445_4346_4C4F_4154);
    let c = col("DECFLOAT", 34, 0);
    let mut numbers = 0;
    for case in 0..CASES {
        let t = decfloat(&mut r);
        let v = SqlValue::Decimal { text: t.clone() };
        assert!(decimal::is_valid(&t), "case {case}: {t}");
        for point in ['.', ','] {
            assert_eq!(
                through_text(&v, &c, point),
                v,
                "case {case}: {t} with {point}"
            );
        }
        if matches!(
            value::sheet_cell(&v, &c, &Style::default()),
            Cell::Number { .. }
        ) {
            numbers += 1;
        }
        assert_eq!(through_sheet(&v, &c), v, "case {case}: {t}");
        if decimal::is_special(&t) || decimal::significant_digits(&t).is_some_and(|n| n > 15) {
            assert!(
                matches!(value::sheet_cell(&v, &c, &Style::default()), Cell::Text(_)),
                "case {case}: {t} is text"
            );
        }
    }
    assert!(numbers > 0, "some DECFLOAT values are number cells");
}

#[test]
fn integers_in_and_out() {
    let mut r = Rng(0x0049_4E54_4547_4552);
    let c = col("BIGINT", 8, 0);
    for case in 0..CASES {
        let n = r.next() as i64 >> r.below(63);
        let v = SqlValue::Integer { value: n };
        assert_eq!(through_text(&v, &c, '.'), v, "case {case}: {n}");
        assert_eq!(through_sheet(&v, &c), v, "case {case}: {n}");
    }
}

#[test]
fn dates_times_and_timestamps_read_back_in_every_style() {
    let styles = [
        DateStyle::Iso,
        DateStyle::job("*MDY", "/", ":"),
        DateStyle::job("*DMY", "-", "."),
        DateStyle::job("*YMD", ".", ":"),
        DateStyle::job("*JUL", "/", ":"),
    ];
    let first = day_number(1, 1, 1);
    let last = day_number(9999, 12, 31);
    let mut r = Rng(0x5449_4D45_5354_414D);
    for case in 0..CASES {
        let day = first + r.below(u64::try_from(last - first + 1).unwrap()) as i64;
        let date = day * 86_400_000;
        let time = r.below(86_400) as i64 * 1000;
        let scale = r.below(13) as u16;
        let unit = 10u64.pow(12 - u32::from(scale));
        let frac = r.below(1_000_000_000_000) / unit * unit;
        let ms = date + time + (frac / 1_000_000_000) as i64;
        let picos = (frac % 1_000_000_000) as u32;
        for style in styles {
            let d = datetime::date_text(date, style);
            assert_eq!(
                datetime::parse_date(&d, style),
                Some(date),
                "case {case}: {d}"
            );
            let t = datetime::time_text(time, style);
            assert_eq!(
                datetime::parse_time(&t, style),
                Some(time),
                "case {case}: {t}"
            );
            let ts = datetime::timestamp_text(ms, picos, scale, style);
            assert_eq!(
                datetime::parse_timestamp(&ts, style),
                Some((ms, picos)),
                "case {case}: {ts} at scale {scale}"
            );
        }
        // Through a transfer field, ISO by default.
        let c = Column {
            scale,
            ..col("TIMESTAMP", 32, scale)
        };
        let v = SqlValue::Timestamp { ms, picos };
        assert_eq!(through_text(&v, &c, '.'), v, "case {case}");
    }
}

#[test]
fn binary_and_bit_data_in_and_out() {
    let mut r = Rng(0x4249_4E41_5259_2121);
    let bin = col("VARBINARY", 64, 0);
    let bits = Column {
        ccsid: 65535,
        ..col("CHAR", 64, 0)
    };
    for case in 0..CASES / 10 {
        let n = r.below(65) as usize;
        let data: Vec<u8> = (0..n).map(|_| r.next() as u8).collect();
        let v = SqlValue::Binary { data };
        assert_eq!(through_text(&v, &bin, '.'), v, "case {case}");
        assert_eq!(through_text(&v, &bits, '.'), v, "case {case}");
        assert_eq!(through_sheet(&v, &bits), v, "case {case}");
    }
    // As text in CCSID 37, every byte the table maps one to one comes back.
    let s = Style {
        bit_data: BitData::Text { ccsid: 37 },
        ..Style::default()
    };
    let all: Vec<u8> = (0..=255u8).collect();
    let v = SqlValue::Binary { data: all.clone() };
    let t = value::text_field(&v, &bits_256(), &s).unwrap();
    assert_eq!(value::import(Input::Text(&t), &bits_256(), &s), Ok(v));
}

fn bits_256() -> Column {
    Column {
        ccsid: 65535,
        ..col("CHAR", 256, 0)
    }
}
