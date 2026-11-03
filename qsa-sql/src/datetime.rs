// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! Dates, times and timestamps as the system sends them (milliseconds,
//! and a timestamp's picoseconds past them), shown in the job's date format
//! and separators or in ISO 8601, and read back.
//!
//! - A date is milliseconds at midnight UTC; a time, milliseconds since
//!   midnight; a timestamp, milliseconds as if UTC plus `picos` past them,
//!   so `TIMESTAMP(12)` keeps every digit. A timestamp shows as many
//!   fractional digits as its column's scale (0 to 12).
//! - The job's formats (`SignOnReply`'s `date_format`, `date_separator`,
//!   `time_separator`): `*MDY`, `*DMY`, `*YMD` and `*JUL`, and `*ISO`,
//!   `*USA`, `*EUR` and `*JIS`. Years always have four digits, so every
//!   date from 0001 to 9999 shows and reads back.
//! - ISO 8601: `2026-11-02`, `09:14:03`, `2026-11-02T09:14:03.123456`.

/// Milliseconds in a day.
const DAY_MS: i64 = 86_400_000;

/// The order of a date's parts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DateOrder {
    Mdy,
    Dmy,
    Ymd,
    /// Year and day of the year.
    Jul,
}

/// How dates and times are written.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DateStyle {
    /// ISO 8601.
    #[default]
    Iso,
    /// The job's format and separators.
    Job {
        order: DateOrder,
        date_sep: char,
        time_sep: char,
    },
}

impl DateStyle {
    /// The job's style from `SignOnReply`'s `date_format` (`*MDY`, ...),
    /// `date_separator` and `time_separator`. `*ISO` and `*JIS` are ISO's
    /// order with the job's separators, `*USA` month first with `/` and
    /// `:`, `*EUR` day first with `.` and `.`; anything not known is ISO.
    pub fn job(date_format: &str, date_separator: &str, time_separator: &str) -> DateStyle {
        let sep = |s: &str, d: char| s.chars().find(|c| !c.is_whitespace()).unwrap_or(d);
        let (order, ds, ts) = match date_format.trim().to_ascii_uppercase().as_str() {
            "*MDY" => (
                DateOrder::Mdy,
                sep(date_separator, '/'),
                sep(time_separator, ':'),
            ),
            "*DMY" => (
                DateOrder::Dmy,
                sep(date_separator, '/'),
                sep(time_separator, ':'),
            ),
            "*YMD" => (
                DateOrder::Ymd,
                sep(date_separator, '/'),
                sep(time_separator, ':'),
            ),
            "*JUL" => (
                DateOrder::Jul,
                sep(date_separator, '/'),
                sep(time_separator, ':'),
            ),
            "*USA" => (DateOrder::Mdy, '/', ':'),
            "*EUR" => (DateOrder::Dmy, '.', '.'),
            "*ISO" | "*JIS" => (
                DateOrder::Ymd,
                sep(date_separator, '-'),
                sep(time_separator, ':'),
            ),
            _ => return DateStyle::Iso,
        };
        DateStyle::Job {
            order,
            date_sep: ds,
            time_sep: ts,
        }
    }
}

/// The civil date of a day number (days since 1970-01-01).
pub fn civil(days: i64) -> (i64, u32, u32) {
    // Howard Hinnant's algorithm, for the proleptic Gregorian calendar.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (
        y,
        u32::try_from(m).unwrap_or(1),
        u32::try_from(d).unwrap_or(1),
    )
}

/// The day number (days since 1970-01-01) of a civil date.
pub fn day_number(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let m = i64::from(m);
    let mp = if m > 2 { m - 3 } else { m + 9 };
    let doy = (153 * mp + 2) / 5 + i64::from(d) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn leap(y: i64) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

fn days_in_month(y: i64, m: u32) -> u32 {
    match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap(y) => 29,
        2 => 28,
        _ => 0,
    }
}

fn valid(y: i64, m: u32, d: u32) -> bool {
    (1..=9999).contains(&y) && (1..=12).contains(&m) && d >= 1 && d <= days_in_month(y, m)
}

/// A date's text in `style`.
pub fn date_text(ms: i64, style: DateStyle) -> String {
    let (y, m, d) = civil(ms.div_euclid(DAY_MS));
    match style {
        DateStyle::Iso => format!("{y:04}-{m:02}-{d:02}"),
        DateStyle::Job {
            order, date_sep: s, ..
        } => match order {
            DateOrder::Mdy => format!("{m:02}{s}{d:02}{s}{y:04}"),
            DateOrder::Dmy => format!("{d:02}{s}{m:02}{s}{y:04}"),
            DateOrder::Ymd => format!("{y:04}{s}{m:02}{s}{d:02}"),
            DateOrder::Jul => {
                let doy = ms.div_euclid(DAY_MS) - day_number(y, 1, 1) + 1;
                format!("{y:04}{s}{doy:03}")
            }
        },
    }
}

/// A time's text in `style`.
pub fn time_text(ms: i64, style: DateStyle) -> String {
    let secs = ms.rem_euclid(DAY_MS) / 1000;
    // 24:00:00 is the end of the day.
    let secs = if ms == DAY_MS { 86_400 } else { secs };
    let (h, mi, s) = (secs / 3600, secs / 60 % 60, secs % 60);
    let t = match style {
        DateStyle::Iso => ':',
        DateStyle::Job { time_sep, .. } => time_sep,
    };
    format!("{h:02}{t}{mi:02}{t}{s:02}")
}

/// The fraction of a second of `ms` and `picos`, as 12 digits.
fn fraction(ms: i64, picos: u32) -> String {
    let p = u64::try_from(ms.rem_euclid(1000)).unwrap_or(0) * 1_000_000_000 + u64::from(picos);
    format!("{p:012}")
}

/// A timestamp's text in `style`, with `scale` fractional digits (0 to
/// 12): `2026-11-02T09:14:03.123456` in ISO, the job's date and time with a
/// blank between them otherwise.
pub fn timestamp_text(ms: i64, picos: u32, scale: u16, style: DateStyle) -> String {
    let day = ms.div_euclid(DAY_MS) * DAY_MS;
    let mut t = format!(
        "{}{}{}",
        date_text(day, style),
        if style == DateStyle::Iso { 'T' } else { ' ' },
        time_text(ms - day, style)
    );
    let scale = usize::from(scale.min(12));
    if scale > 0 {
        t.push('.');
        t.push_str(&fraction(ms, picos)[..scale]);
    }
    t
}

/// Splits `text` at any of `seps` into its numeric parts.
fn numbers(text: &str, seps: &[char]) -> Option<Vec<String>> {
    let parts: Vec<String> = text
        .split(|c| seps.contains(&c))
        .map(str::to_owned)
        .collect();
    parts
        .iter()
        .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
        .then_some(parts)
}

/// A date's text in `style` (ISO always read too) as its milliseconds.
pub fn parse_date(text: &str, style: DateStyle) -> Option<i64> {
    let t = text.trim();
    let iso = || -> Option<i64> {
        let p = numbers(t, &['-'])?;
        if p.len() != 3 || p[0].len() != 4 {
            return None;
        }
        ymd(p[0].parse().ok()?, p[1].parse().ok()?, p[2].parse().ok()?)
    };
    if let Some(ms) = iso() {
        return Some(ms);
    }
    let DateStyle::Job {
        order, date_sep, ..
    } = style
    else {
        return None;
    };
    let p = numbers(t, &[date_sep])?;
    match (order, p.len()) {
        (DateOrder::Mdy, 3) => ymd(p[2].parse().ok()?, p[0].parse().ok()?, p[1].parse().ok()?),
        (DateOrder::Dmy, 3) => ymd(p[2].parse().ok()?, p[1].parse().ok()?, p[0].parse().ok()?),
        (DateOrder::Ymd, 3) => ymd(p[0].parse().ok()?, p[1].parse().ok()?, p[2].parse().ok()?),
        (DateOrder::Jul, 2) => {
            let y: i64 = p[0].parse().ok()?;
            let doy: i64 = p[1].parse().ok()?;
            let len = if leap(y) { 366 } else { 365 };
            ((1..=9999).contains(&y) && (1..=len).contains(&doy))
                .then(|| (day_number(y, 1, 1) + doy - 1) * DAY_MS)
        }
        _ => None,
    }
}

fn ymd(y: i64, m: u32, d: u32) -> Option<i64> {
    valid(y, m, d).then(|| day_number(y, m, d) * DAY_MS)
}

/// A time's text (`HH:MM:SS`, `.` or the job's separator also taken) as
/// milliseconds since midnight.
pub fn parse_time(text: &str, style: DateStyle) -> Option<i64> {
    let mut seps = vec![':', '.'];
    if let DateStyle::Job { time_sep, .. } = style {
        seps.push(time_sep);
    }
    let p = numbers(text.trim(), &seps)?;
    if p.len() != 3 || p.iter().any(|x| x.len() != 2) {
        return None;
    }
    let (h, m, s): (i64, i64, i64) = (p[0].parse().ok()?, p[1].parse().ok()?, p[2].parse().ok()?);
    let ok = (h < 24 && m < 60 && s < 60) || (h == 24 && m == 0 && s == 0);
    ok.then_some((h * 3600 + m * 60 + s) * 1000)
}

/// A timestamp's text as milliseconds and picoseconds past them: ISO
/// (`T`, a blank or `-` between date and time; `:` or `.` in the time) or
/// the job's date and time with a blank between; up to 12 fractional
/// digits.
pub fn parse_timestamp(text: &str, style: DateStyle) -> Option<(i64, u32)> {
    let t = text.trim();
    let (main, frac) = match t.rsplit_once('.') {
        // The fraction is the last `.` part when it follows the seconds.
        Some((m, f))
            if !f.is_empty()
                && f.len() <= 12
                && f.bytes().all(|b| b.is_ascii_digit())
                && split_date_time(m, style).is_some() =>
        {
            (m, f)
        }
        _ => (t, ""),
    };
    let (date, time) = split_date_time(main, style)?;
    let day = parse_date(date, style)?;
    let tm = parse_time(time, style)?;
    let mut digits = frac.to_owned();
    while digits.len() < 12 {
        digits.push('0');
    }
    let p: u64 = digits.parse().ok()?;
    let ms = day + tm + i64::try_from(p / 1_000_000_000).ok()?;
    Some((ms, u32::try_from(p % 1_000_000_000).ok()?))
}

/// A timestamp's date and time parts.
fn split_date_time(text: &str, style: DateStyle) -> Option<(&str, &str)> {
    for sep in ['T', ' '] {
        if let Some((d, t)) = text.split_once(sep)
            && parse_date(d, style).is_some()
            && parse_time(t, style).is_some()
        {
            return Some((d, t));
        }
    }
    // The system's own form: `2026-11-02-09.14.03`.
    if text.len() >= 11 && text.is_char_boundary(10) && text.as_bytes()[10] == b'-' {
        let (d, t) = (&text[..10], &text[11..]);
        if parse_date(d, style).is_some() && parse_time(t, style).is_some() {
            return Some((d, t));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mdy() -> DateStyle {
        DateStyle::job("*MDY", "/", ":")
    }

    #[test]
    fn dates_in_every_order() {
        let ms = day_number(2026, 11, 2) * DAY_MS;
        assert_eq!(date_text(ms, DateStyle::Iso), "2026-11-02");
        assert_eq!(date_text(ms, mdy()), "11/02/2026");
        assert_eq!(
            date_text(ms, DateStyle::job("*DMY", "-", ":")),
            "02-11-2026"
        );
        assert_eq!(
            date_text(ms, DateStyle::job("*YMD", ".", ":")),
            "2026.11.02"
        );
        assert_eq!(date_text(ms, DateStyle::job("*JUL", "/", ":")), "2026/306");
        assert_eq!(date_text(ms, DateStyle::job("*EUR", "", "")), "02.11.2026");
        assert_eq!(date_text(0, DateStyle::Iso), "1970-01-01");
        let first = day_number(1, 1, 1) * DAY_MS;
        assert_eq!(date_text(first, mdy()), "01/01/0001");
        for style in [DateStyle::Iso, mdy(), DateStyle::job("*JUL", "/", ":")] {
            for (y, m, d) in [(1, 1, 1), (2000, 2, 29), (9999, 12, 31), (1969, 12, 31)] {
                let ms = day_number(y, m, d) * DAY_MS;
                assert_eq!(
                    parse_date(&date_text(ms, style), style),
                    Some(ms),
                    "{style:?} {y}"
                );
            }
        }
        assert_eq!(parse_date("2026-02-30", DateStyle::Iso), None);
        assert_eq!(parse_date("13/01/2026", mdy()), None);
        assert_eq!(DateStyle::job("*XYZ", "/", ":"), DateStyle::Iso);
    }

    #[test]
    fn times_and_timestamps() {
        assert_eq!(time_text(34_445_000, DateStyle::Iso), "09:34:05");
        assert_eq!(
            time_text(34_445_000, DateStyle::job("*MDY", "/", ".")),
            "09.34.05"
        );
        assert_eq!(parse_time("09.34.05", DateStyle::Iso), Some(34_445_000));
        assert_eq!(parse_time("24:00:00", DateStyle::Iso), Some(DAY_MS));
        assert_eq!(parse_time("25:00:00", DateStyle::Iso), None);

        // 2026-11-02-09.14.03.123456789012: ms .123, picos 456 789 012.
        let ms = day_number(2026, 11, 2) * DAY_MS + 33_243_123;
        let picos = 456_789_012;
        assert_eq!(
            timestamp_text(ms, picos, 12, DateStyle::Iso),
            "2026-11-02T09:14:03.123456789012"
        );
        assert_eq!(
            timestamp_text(ms, picos, 6, DateStyle::Iso),
            "2026-11-02T09:14:03.123456"
        );
        assert_eq!(
            timestamp_text(ms, picos, 0, DateStyle::Iso),
            "2026-11-02T09:14:03"
        );
        assert_eq!(
            timestamp_text(ms, picos, 3, mdy()),
            "11/02/2026 09:14:03.123"
        );
        for text in [
            "2026-11-02T09:14:03.123456789012",
            "2026-11-02 09:14:03.123456789012",
            "2026-11-02-09.14.03.123456789012",
        ] {
            assert_eq!(
                parse_timestamp(text, DateStyle::Iso),
                Some((ms, picos)),
                "{text}"
            );
        }
        assert_eq!(
            parse_timestamp("11/02/2026 09:14:03.123456789012", mdy()),
            Some((ms, picos))
        );
        assert_eq!(
            parse_timestamp("1970-01-01T00:00:00.000000000001", DateStyle::Iso),
            Some((0, 1))
        );
        assert_eq!(
            parse_timestamp("2026-11-02T09:14:03", DateStyle::Iso),
            Some((ms - 123, 0))
        );
        assert_eq!(
            parse_timestamp("2026-11-02T09:14:03.1234567890123", DateStyle::Iso),
            None
        );
        // Before 1970.
        let early = day_number(1, 1, 1) * DAY_MS;
        assert_eq!(
            parse_timestamp(
                &timestamp_text(early + 5, 7, 12, DateStyle::Iso),
                DateStyle::Iso
            ),
            Some((early + 5, 7))
        );
    }
}
