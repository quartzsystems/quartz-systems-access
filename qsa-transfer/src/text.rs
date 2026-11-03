// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! Text transfer files: delimited (CSV, TSV) and fixed width, and their
//! encodings.
//!
//! - Delimited (RFC 4180, with the options' delimiter and quote): a field
//!   is quoted when it holds the delimiter, the quote, a line end, or
//!   leading or trailing blanks, or is empty; a quote inside is doubled.
//!   NULL is an empty field without quotes, and an empty string `""`, so
//!   the two stay apart both ways. Records end in CRLF; LF alone is read
//!   too, and a line end inside quotes is part of the field.
//! - Fixed width: each field in its width, numbers right-aligned and
//!   everything else left-aligned, blank-filled; with null indicators, a
//!   `Y` (NULL) or `N` after each nullable field. A record is read by its
//!   width in characters, so a line end inside a value stays in it.
//! - Encodings: UTF-8 (with or without a byte order mark) for writing;
//!   UTF-8, UTF-16 (by its byte order mark) and Windows-1252 for reading.

use crate::format::Encoding;

/// One field: `None` is NULL.
pub type Field = Option<String>;

/// A delimited record, without its line end.
pub fn delimited_record(fields: &[Field], delimiter: char, quote: char) -> String {
    let mut out = String::new();
    for (i, f) in fields.iter().enumerate() {
        if i > 0 {
            out.push(delimiter);
        }
        let Some(f) = f else { continue };
        let quoted = f.is_empty()
            || f.contains([delimiter, quote, '\r', '\n'])
            || f.starts_with(char::is_whitespace)
            || f.ends_with(char::is_whitespace);
        if quoted {
            out.push(quote);
            for c in f.chars() {
                if c == quote {
                    out.push(quote);
                }
                out.push(c);
            }
            out.push(quote);
        } else {
            out.push_str(f);
        }
    }
    out
}

/// A delimited record read: its fields and the line it started on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Record {
    pub fields: Vec<Field>,
    /// Its first line in the file, from 1.
    pub line: usize,
}

/// Every record of delimited `text`. An empty unquoted field is NULL;
/// blank lines are skipped.
pub fn delimited_records(text: &str, delimiter: char, quote: char) -> Vec<Record> {
    let mut out = Vec::new();
    let mut fields: Vec<Field> = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut in_quotes = false;
    let mut line = 1usize;
    let mut start = 1usize;
    let mut chars = text.chars().peekable();
    let mut any = false;
    let end_field = |fields: &mut Vec<Field>, field: &mut String, quoted: &mut bool| {
        let f = std::mem::take(field);
        fields.push(if f.is_empty() && !*quoted {
            None
        } else {
            Some(f)
        });
        *quoted = false;
    };
    while let Some(c) = chars.next() {
        if in_quotes {
            if c == quote {
                if chars.peek() == Some(&quote) {
                    chars.next();
                    field.push(quote);
                } else {
                    in_quotes = false;
                }
            } else {
                if c == '\n' {
                    line += 1;
                }
                field.push(c);
            }
            continue;
        }
        match c {
            c if c == quote && field.is_empty() && !quoted => {
                in_quotes = true;
                quoted = true;
                any = true;
            }
            c if c == delimiter => {
                end_field(&mut fields, &mut field, &mut quoted);
                any = true;
            }
            '\r' if chars.peek() == Some(&'\n') => {}
            '\n' | '\r' => {
                if any || !field.is_empty() {
                    end_field(&mut fields, &mut field, &mut quoted);
                    out.push(Record {
                        fields: std::mem::take(&mut fields),
                        line: start,
                    });
                }
                any = false;
                line += 1;
                start = line;
            }
            c => {
                field.push(c);
                any = true;
            }
        }
    }
    if any || !field.is_empty() {
        end_field(&mut fields, &mut field, &mut quoted);
        out.push(Record {
            fields,
            line: start,
        });
    }
    out
}

/// How a fixed-width field is laid out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FixedColumn {
    pub width: usize,
    /// Right-aligned (numbers).
    pub right: bool,
    /// Followed by a null indicator.
    pub indicator: bool,
}

/// A fixed-width record, without its line end. A value longer than its
/// width is an error: nothing is cut.
pub fn fixed_record(fields: &[Field], columns: &[FixedColumn]) -> Result<String, usize> {
    let mut out = String::new();
    for (i, (f, c)) in fields.iter().zip(columns).enumerate() {
        let v = f.as_deref().unwrap_or("");
        let n = v.chars().count();
        if n > c.width {
            return Err(i);
        }
        let pad = " ".repeat(c.width - n);
        if c.right {
            out.push_str(&pad);
            out.push_str(v);
        } else {
            out.push_str(v);
            out.push_str(&pad);
        }
        if c.indicator {
            out.push(if f.is_none() { 'Y' } else { 'N' });
        }
    }
    Ok(out)
}

/// Every record of fixed-width `text`: each read by its width in
/// characters, then an optional line end. Text fields lose their trailing
/// blanks and numbers their leading ones; with an indicator, `Y` is NULL,
/// and without one an all-blank field is NULL.
pub fn fixed_records(text: &str, columns: &[FixedColumn]) -> Vec<Record> {
    let chars: Vec<char> = text.chars().collect();
    let len: usize = columns
        .iter()
        .map(|c| c.width + usize::from(c.indicator))
        .sum();
    let mut out = Vec::new();
    let (mut i, mut line) = (0usize, 1usize);
    while i < chars.len() {
        // Blank lines between records.
        if chars[i] == '\n' || chars[i] == '\r' {
            if chars[i] == '\n' {
                line += 1;
            }
            i += 1;
            continue;
        }
        let end = (i + len).min(chars.len());
        let rec = &chars[i..end];
        let mut fields = Vec::with_capacity(columns.len());
        let mut at = 0;
        for c in columns {
            let w = c.width.min(rec.len().saturating_sub(at));
            let raw: String = rec[at..at + w].iter().collect();
            at += w;
            let null = if c.indicator {
                let ind = rec.get(at).copied();
                at += 1;
                ind == Some('Y')
            } else {
                raw.trim().is_empty()
            };
            fields.push(if null {
                None
            } else if c.right {
                Some(raw.trim_start().to_owned())
            } else {
                Some(raw.trim_end().to_owned())
            });
        }
        let newlines = rec.iter().filter(|&&c| c == '\n').count();
        out.push(Record { fields, line });
        line += newlines;
        i = end;
    }
    out
}

/// `text` as `encoding` writes it: UTF-8, with a byte order mark for
/// [`Encoding::Utf8Bom`] when `bom` (the start of a file).
pub fn encode(text: &str, encoding: Encoding, bom: bool) -> Vec<u8> {
    let mut out = Vec::with_capacity(text.len() + 3);
    if bom && encoding == Encoding::Utf8Bom {
        out.extend_from_slice(b"\xEF\xBB\xBF");
    }
    out.extend_from_slice(text.as_bytes());
    out
}

/// Windows-1252's characters for bytes 0x80 to 0x9F; the rest are
/// Latin-1. Unassigned bytes become U+FFFD.
const CP1252_HIGH: [char; 32] = [
    '\u{20AC}', '\u{FFFD}', '\u{201A}', '\u{0192}', '\u{201E}', '\u{2026}', '\u{2020}', '\u{2021}',
    '\u{02C6}', '\u{2030}', '\u{0160}', '\u{2039}', '\u{0152}', '\u{FFFD}', '\u{017D}', '\u{FFFD}',
    '\u{FFFD}', '\u{2018}', '\u{2019}', '\u{201C}', '\u{201D}', '\u{2022}', '\u{2013}', '\u{2014}',
    '\u{02DC}', '\u{2122}', '\u{0161}', '\u{203A}', '\u{0153}', '\u{FFFD}', '\u{017E}', '\u{0178}',
];

/// Why a file could not be read as text.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DecodeError {
    #[error("the file is not UTF-8 (the first bad byte is at offset {0})")]
    NotUtf8(usize),
    #[error("the file has no UTF-16 byte order mark")]
    NoUtf16Bom,
    #[error("the file's UTF-16 is not valid")]
    BadUtf16,
}

/// A text file's bytes as text in `encoding`. UTF-8's byte order mark is
/// dropped; UTF-16 needs one.
pub fn decode(bytes: &[u8], encoding: Encoding) -> Result<String, DecodeError> {
    match encoding {
        Encoding::Utf8 | Encoding::Utf8Bom => {
            let b = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
            std::str::from_utf8(b)
                .map(str::to_owned)
                .map_err(|e| DecodeError::NotUtf8(e.valid_up_to()))
        }
        Encoding::Utf16 => {
            let (le, body) = match bytes {
                [0xFF, 0xFE, rest @ ..] => (true, rest),
                [0xFE, 0xFF, rest @ ..] => (false, rest),
                _ => return Err(DecodeError::NoUtf16Bom),
            };
            let units: Vec<u16> = body
                .as_chunks::<2>()
                .0
                .iter()
                .map(|&p| {
                    if le {
                        u16::from_le_bytes(p)
                    } else {
                        u16::from_be_bytes(p)
                    }
                })
                .collect();
            String::from_utf16(&units).map_err(|_| DecodeError::BadUtf16)
        }
        Encoding::Windows1252 => Ok(bytes
            .iter()
            .map(|&b| match b {
                0x80..=0x9F => CP1252_HIGH[usize::from(b - 0x80)],
                b => char::from(b),
            })
            .collect()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn f(s: &str) -> Field {
        Some(s.to_owned())
    }

    #[test]
    fn delimited_keeps_null_and_empty_apart() {
        let fields = vec![
            f("plain"),
            None,
            f(""),
            f("a,b"),
            f("say \"hi\""),
            f("two\nlines"),
            f(" lead"),
            f("x"),
        ];
        let rec = delimited_record(&fields, ',', '"');
        assert_eq!(
            rec,
            "plain,,\"\",\"a,b\",\"say \"\"hi\"\"\",\"two\nlines\",\" lead\",x"
        );
        let back = delimited_records(&format!("{rec}\r\n{rec}\r\n"), ',', '"');
        assert_eq!(back.len(), 2);
        assert_eq!(back[0].fields, fields);
        assert_eq!((back[0].line, back[1].line), (1, 3));
        // Tab-separated, and the last field NULL.
        let tsv = delimited_record(&[f("a"), None], '\t', '"');
        assert_eq!(tsv, "a\t");
        assert_eq!(delimited_records(&tsv, '\t', '"')[0].fields, [f("a"), None]);
        // Blank lines are skipped; a lone empty record is one NULL field.
        assert_eq!(delimited_records("\n\nx\n", ',', '"')[0].line, 3);
    }

    #[test]
    fn fixed_width_both_ways() {
        let cols = [
            FixedColumn {
                width: 5,
                right: true,
                indicator: true,
            },
            FixedColumn {
                width: 8,
                right: false,
                indicator: true,
            },
            FixedColumn {
                width: 4,
                right: false,
                indicator: false,
            },
        ];
        let rows = [
            vec![f("12"), f(" lead"), f("ab")],
            vec![None, f(""), f("cd")],
            vec![f("-1.5"), f("two\nln"), f("ef")],
        ];
        let text: String = rows
            .iter()
            .map(|r| fixed_record(r, &cols).unwrap() + "\r\n")
            .collect();
        assert!(text.starts_with("   12N lead   Nab  \r\n"));
        let back = fixed_records(&text, &cols);
        let got: Vec<Vec<Field>> = back.iter().map(|r| r.fields.clone()).collect();
        assert_eq!(got, rows);
        assert_eq!(back.iter().map(|r| r.line).collect::<Vec<_>>(), [1, 2, 3]);
        assert_eq!(fixed_record(&[f("toolong")], &cols[..1]), Err(0));
    }

    #[test]
    fn encodings() {
        assert_eq!(
            encode("é", Encoding::Utf8Bom, true),
            b"\xEF\xBB\xBF\xC3\xA9"
        );
        assert_eq!(encode("é", Encoding::Utf8, true), b"\xC3\xA9");
        assert_eq!(
            decode(b"\xEF\xBB\xBFa\xC3\xA9", Encoding::Utf8).unwrap(),
            "a\u{e9}"
        );
        assert!(matches!(
            decode(b"a\xFF", Encoding::Utf8),
            Err(DecodeError::NotUtf8(1))
        ));
        assert_eq!(
            decode(b"\xFF\xFEa\x00\xE9\x00", Encoding::Utf16).unwrap(),
            "a\u{e9}"
        );
        assert_eq!(decode(b"\xFE\xFF\x00a", Encoding::Utf16).unwrap(), "a");
        assert_eq!(
            decode(b"\x80 \xE9", Encoding::Windows1252).unwrap(),
            "\u{20AC} \u{e9}"
        );
    }
}
