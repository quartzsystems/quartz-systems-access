// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! Exact decimal text: `DECIMAL`, `NUMERIC` and `DECFLOAT` as the system
//! sends them (`.` as the point; `DECFLOAT` perhaps in E notation, or
//! `Infinity`, `-Infinity`, `NaN`, `sNaN`). Nothing here goes through
//! floating point except [`from_f64`], for a number cell a spreadsheet
//! gave, and [`spreadsheet_exact`], which says when that is safe.

/// The largest number of significant digits a spreadsheet number keeps
/// exactly.
pub const SHEET_DIGITS: usize = 15;

/// A decimal number's text taken apart.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Parts {
    /// Digits before and after the point, as written, and the exponent of
    /// E notation if there is one.
    Finite {
        negative: bool,
        int: String,
        frac: String,
        exp: Option<i32>,
    },
    /// `Infinity` or `-Infinity`.
    Infinity { negative: bool },
    /// `NaN` or `sNaN`, maybe signed.
    NaN { negative: bool, signaling: bool },
}

/// `text` taken apart: an optional sign, digits with at most one `.`, at
/// least one digit, and optional E notation; or a special value. `None`
/// for anything else.
pub fn parse(text: &str) -> Option<Parts> {
    let (negative, body) = match text.as_bytes().first() {
        Some(b'-') => (true, &text[1..]),
        Some(b'+') => (false, &text[1..]),
        _ => (false, text),
    };
    match body {
        "Infinity" => return Some(Parts::Infinity { negative }),
        "NaN" => {
            return Some(Parts::NaN {
                negative,
                signaling: false,
            });
        }
        "sNaN" => {
            return Some(Parts::NaN {
                negative,
                signaling: true,
            });
        }
        _ => {}
    }
    let (mantissa, exp) = match body.find(['e', 'E']) {
        Some(i) => {
            let e = &body[i + 1..];
            let digits = e.strip_prefix(['+', '-']).unwrap_or(e);
            if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            (&body[..i], Some(e.parse::<i32>().ok()?))
        }
        None => (body, None),
    };
    let (int, frac) = match mantissa.split_once('.') {
        Some((i, f)) => (i, f),
        None => (mantissa, ""),
    };
    let digits = |s: &str| s.bytes().all(|b| b.is_ascii_digit());
    if !digits(int) || !digits(frac) || (int.is_empty() && frac.is_empty()) {
        return None;
    }
    Some(Parts::Finite {
        negative,
        int: int.to_owned(),
        frac: frac.to_owned(),
        exp,
    })
}

/// Whether `text` is a decimal number or a `DECFLOAT` special value.
pub fn is_valid(text: &str) -> bool {
    parse(text).is_some()
}

/// Whether `text` is a `DECFLOAT` special value.
pub fn is_special(text: &str) -> bool {
    matches!(
        parse(text),
        Some(Parts::Infinity { .. } | Parts::NaN { .. })
    )
}

/// Whether `text` uses E notation.
pub fn is_e_notation(text: &str) -> bool {
    matches!(parse(text), Some(Parts::Finite { exp: Some(_), .. }))
}

/// The significant digits of a finite number: its digits without leading
/// or trailing zeros (at least 1, for zero). `None` for a special value or
/// text that is not a number.
pub fn significant_digits(text: &str) -> Option<usize> {
    match parse(text)? {
        Parts::Finite { int, frac, .. } => {
            let all = format!("{int}{frac}");
            let s = all.trim_start_matches('0').trim_end_matches('0');
            Some(s.len().max(1))
        }
        _ => None,
    }
}

/// Digits before the point of a plain number, leading zeros left out.
pub fn integer_digits(text: &str) -> Option<usize> {
    match parse(text)? {
        Parts::Finite { int, exp: None, .. } => Some(int.trim_start_matches('0').len()),
        _ => None,
    }
}

/// `text` shown with `scale` digits after the point: a plain number with
/// fewer gets zeros added; one with more keeps them all (nothing is ever
/// rounded). E notation and special values are as sent.
pub fn with_scale(text: &str, scale: u16) -> String {
    match parse(text) {
        Some(Parts::Finite {
            frac, exp: None, ..
        }) if frac.len() < usize::from(scale) => {
            let mut s = text.to_owned();
            if !text.contains('.') {
                s.push('.');
            }
            s.extend(std::iter::repeat_n('0', usize::from(scale) - frac.len()));
            s
        }
        _ => text.to_owned(),
    }
}

/// `text` with `point` for its decimal point.
pub fn with_point(text: &str, point: char) -> String {
    if point == '.' {
        text.to_owned()
    } else {
        text.replacen('.', &point.to_string(), 1)
    }
}

/// A number as typed or read from a file, with `point` as its decimal
/// point, as the exact text the system takes (`.` as the point); surrounding
/// blanks are dropped and a leading `+` kept out. Nothing else changes, so
/// text in is text out. `None` when it is not a number.
pub fn from_input(input: &str, point: char) -> Option<String> {
    let t = input.trim();
    let t = t.strip_prefix('+').unwrap_or(t);
    let t = if point == '.' {
        t.to_owned()
    } else {
        if t.contains('.') {
            return None;
        }
        t.replacen(point, ".", 1)
    };
    is_valid(&t).then_some(t)
}

/// The shortest text that reads back as `f`: plain for exponents from -6
/// to 20, else E notation (`1.5E-30`). For a spreadsheet's number cell.
pub fn from_f64(f: f64) -> String {
    if f.is_nan() {
        return "NaN".into();
    }
    if f.is_infinite() {
        return if f < 0.0 { "-Infinity" } else { "Infinity" }.into();
    }
    if f == 0.0 {
        return if f.is_sign_negative() { "-0" } else { "0" }.into();
    }
    let e = format!("{f:e}");
    let (mantissa, exp) = e.split_once('e').expect("{:e} has an exponent");
    let exp: i32 = exp.parse().expect("an integer exponent");
    if (-6..=20).contains(&exp) {
        format!("{f}")
    } else {
        format!("{mantissa}E{exp}")
    }
}

/// Whether a spreadsheet number cell keeps `text` exactly: a finite value
/// of at most [`SHEET_DIGITS`] significant digits. With `as_written`
/// (`DECFLOAT`, whose text is its value), also only when the cell reads
/// back as the same text ([`from_f64`]).
pub fn spreadsheet_exact(text: &str, as_written: bool) -> Option<f64> {
    if significant_digits(text)? > SHEET_DIGITS {
        return None;
    }
    let f: f64 = text.parse().ok()?;
    if !f.is_finite() {
        return None;
    }
    if as_written && from_f64(f) != text {
        return None;
    }
    Some(f)
}

/// A decimal cell read back from a spreadsheet, for a column of `scale`
/// digits after the point (`None`: `DECFLOAT`, as read).
pub fn from_sheet(f: f64, scale: Option<u16>) -> String {
    let t = from_f64(f);
    match scale {
        Some(s) if !is_e_notation(&t) => with_scale(&t, s),
        _ => t,
    }
}

/// Whether `text` fits `DECIMAL(precision, scale)`: at most
/// `precision - scale` digits before the point. Digits past the scale are
/// the system's to cut.
pub fn fits(text: &str, precision: u32, scale: u16) -> bool {
    match parse(text) {
        Some(Parts::Finite { exp: None, .. }) => {
            let room = precision.saturating_sub(u32::from(scale)) as usize;
            integer_digits(text).is_some_and(|n| n <= room)
        }
        Some(Parts::Finite { .. }) => {
            // E notation for a fixed-point column: the system decides.
            true
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_what_the_system_sends() {
        for t in [
            "0",
            "-12.50",
            "123456789.01",
            ".5",
            "5.",
            "1.5E-30",
            "1e300",
            "Infinity",
            "-Infinity",
            "NaN",
            "sNaN",
            "-sNaN",
        ] {
            assert!(is_valid(t), "{t}");
        }
        for t in [
            "", "-", ".", "1.2.3", "1e", "1e+", "abc", "1,5", "inf", " 1",
        ] {
            assert!(!is_valid(t), "{t:?}");
        }
        assert!(is_special("Infinity") && is_special("sNaN") && !is_special("1"));
        assert!(is_e_notation("1.5E-30") && !is_e_notation("1.5"));
    }

    #[test]
    fn significant_digits_and_spreadsheets() {
        assert_eq!(significant_digits("1200.00"), Some(2));
        assert_eq!(significant_digits("0.001"), Some(1));
        assert_eq!(significant_digits("0"), Some(1));
        assert_eq!(significant_digits("123456789012345.6"), Some(16));
        assert_eq!(significant_digits("NaN"), None);
        assert_eq!(
            spreadsheet_exact("123456789.01", false),
            Some(123_456_789.01)
        );
        assert_eq!(spreadsheet_exact("1234567890123456", false), None);
        assert_eq!(spreadsheet_exact("-9007199254740992", false), None);
        assert_eq!(spreadsheet_exact("Infinity", false), None);
        // DECFLOAT keeps its text: 1.50 would come back 1.5.
        assert_eq!(spreadsheet_exact("1.50", true), None);
        assert_eq!(spreadsheet_exact("1.5E-30", true), Some(1.5e-30));
        assert_eq!(spreadsheet_exact("1234.5678", true), Some(1234.5678));
        assert_eq!(from_sheet(1200.0, Some(2)), "1200.00");
        assert_eq!(from_sheet(1.5e-30, None), "1.5E-30");
        assert_eq!(from_f64(1e300), "1E300");
        assert_eq!(from_f64(-0.1), "-0.1");
        assert_eq!(from_f64(-0.0), "-0");
    }

    #[test]
    fn scale_point_and_input() {
        assert_eq!(with_scale("12.5", 2), "12.50");
        assert_eq!(with_scale("7", 2), "7.00");
        assert_eq!(with_scale("1.23456", 2), "1.23456", "never rounded");
        assert_eq!(with_scale("1.5E-30", 4), "1.5E-30");
        assert_eq!(with_point("-1.25", ','), "-1,25");
        assert_eq!(from_input(" -1,25 ", ','), Some("-1.25".into()));
        assert_eq!(from_input("1.234,5", ','), None);
        assert_eq!(from_input("+3.0", '.'), Some("3.0".into()));
        assert_eq!(from_input("x", '.'), None);
        assert!(fits("123456789.01", 11, 2));
        assert!(!fits("1234567890.01", 11, 2));
        assert!(fits("0000012.345", 7, 3));
        assert!(!fits("NaN", 7, 3));
    }
}
