// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! Binary values: `BINARY`, `VARBINARY`, `BLOB`, and character columns of
//! CCSID 65535 (`FOR BIT DATA`), which the system sends as their bytes.
//! They show as hex (`X'C1C2'`), or, for CCSID 65535 character data when
//! asked, as text translated with this program's code-page tables in the
//! job's CCSID.

use tn5250::codepage::{self, CodePage};

/// The bytes as upper-case hex digits: `C1C2`.
pub fn hex(data: &[u8]) -> String {
    use std::fmt::Write;
    let mut s = String::with_capacity(data.len() * 2);
    for b in data {
        let _ = write!(s, "{b:02X}");
    }
    s
}

/// The bytes as an SQL hex literal: `X'C1C2'`.
pub fn hex_literal(data: &[u8]) -> String {
    format!("X'{}'", hex(data))
}

/// Hex digits back to bytes: `C1C2`, `c1c2` or `X'C1C2'`. `None` for an
/// odd number of digits or anything else.
pub fn parse_hex(text: &str) -> Option<Vec<u8>> {
    let t = text.trim();
    let t = match t
        .strip_prefix(['X', 'x'])
        .and_then(|r| r.strip_prefix('\''))
    {
        Some(r) => r.strip_suffix('\'')?,
        None => t,
    };
    if t.len() % 2 != 0 || !t.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    (0..t.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&t[i..i + 2], 16).ok())
        .collect()
}

/// The code page this program has for `ccsid`, if any.
fn page(ccsid: u16) -> Option<&'static dyn CodePage> {
    codepage::by_ccsid(ccsid)
}

/// Whether this program can translate `ccsid`.
pub fn can_translate(ccsid: u16) -> bool {
    page(ccsid).is_some()
}

/// EBCDIC bytes in `ccsid` as text; `None` without a table for it.
pub fn to_text(data: &[u8], ccsid: u16) -> Option<String> {
    page(ccsid).map(|p| p.decode(data))
}

/// Text back to EBCDIC bytes in `ccsid`; `None` without a table, or when a
/// character has no byte there.
pub fn from_text(text: &str, ccsid: u16) -> Option<Vec<u8>> {
    let p = page(ccsid)?;
    text.chars().map(|c| p.to_byte(c)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_both_ways() {
        let b = [0x00, 0x01, 0xFE, 0xFF, 0xC1];
        assert_eq!(hex(&b), "0001FEFFC1");
        assert_eq!(hex_literal(&[0xC1, 0xC2]), "X'C1C2'");
        assert_eq!(hex_literal(&[]), "X''");
        assert_eq!(parse_hex("0001FEFFC1"), Some(b.to_vec()));
        assert_eq!(parse_hex("X'c1c2'"), Some(vec![0xC1, 0xC2]));
        assert_eq!(parse_hex(""), Some(Vec::new()));
        assert_eq!(parse_hex("X''"), Some(Vec::new()));
        assert_eq!(parse_hex("ABC"), None);
        assert_eq!(parse_hex("GG"), None);
    }

    #[test]
    fn ccsid_37_text() {
        assert!(can_translate(37));
        assert_eq!(
            to_text(&[0xC1, 0xC2, 0xC3, 0x40], 37).as_deref(),
            Some("ABC ")
        );
        assert_eq!(from_text("ABC ", 37), Some(vec![0xC1, 0xC2, 0xC3, 0x40]));
        assert_eq!(to_text(&[0xC1], 65535), None);
        assert_eq!(from_text("日", 37), None);
    }
}
