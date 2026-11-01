// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! EBCDIC translation, single-byte only. Everything else in the crate uses
//! the [`CodePage`] trait, so the tables here can be swapped for another
//! implementation without touching the callers.
//!
//! The tables come from `codepages/cpNNN.txt`, compiled by `build.rs`.

include!(concat!(env!("OUT_DIR"), "/codepages.rs"));

/// A single-byte EBCDIC code page.
pub trait CodePage: Send + Sync + std::fmt::Debug {
    /// The coded character set identifier.
    fn ccsid(&self) -> u16;
    /// The character for a byte.
    fn to_char(&self, byte: u8) -> char;
    /// The byte for a character, if the code page has it.
    fn to_byte(&self, c: char) -> Option<u8>;

    /// Bytes to text.
    fn decode(&self, bytes: &[u8]) -> String {
        bytes.iter().map(|&b| self.to_char(b)).collect()
    }

    /// Text to bytes; characters the code page lacks are skipped.
    fn encode(&self, text: &str) -> Vec<u8> {
        text.chars().filter_map(|c| self.to_byte(c)).collect()
    }
}

/// A code page from the compiled tables.
#[derive(Debug)]
pub struct Table {
    ccsid: u16,
    to: &'static [char; 256],
    from: &'static [(char, u8); 256],
}

impl CodePage for Table {
    fn ccsid(&self) -> u16 {
        self.ccsid
    }
    fn to_char(&self, byte: u8) -> char {
        self.to[byte as usize]
    }
    fn to_byte(&self, c: char) -> Option<u8> {
        self.from
            .binary_search_by_key(&c, |&(ch, _)| ch)
            .ok()
            .map(|i| self.from[i].1)
    }
}

static PAGES: std::sync::OnceLock<Vec<Table>> = std::sync::OnceLock::new();

fn pages() -> &'static [Table] {
    PAGES.get_or_init(|| {
        TABLES
            .iter()
            .map(|&(ccsid, to, from)| Table { ccsid, to, from })
            .collect()
    })
}

/// CCSIDs that can be chosen for a session.
pub fn supported() -> impl Iterator<Item = u16> {
    TABLES.iter().map(|t| t.0)
}

/// The code page for a CCSID.
pub fn by_ccsid(ccsid: u16) -> Option<&'static dyn CodePage> {
    pages()
        .iter()
        .find(|t| t.ccsid == ccsid)
        .map(|t| t as &dyn CodePage)
}

/// The default: CCSID 37.
pub fn default_page() -> &'static dyn CodePage {
    by_ccsid(37).expect("CCSID 37 is built in")
}

/// What NEW-ENVIRON says about a CCSID (RFC 2877): the code page, the
/// character set, and the keyboard type the server should assume.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Environ {
    pub codepage: u16,
    pub charset: u16,
    pub kbdtype: &'static str,
}

/// [`Environ`] for a CCSID. Euro code pages use character set 695, the
/// others 697.
pub fn environ(ccsid: u16) -> Environ {
    let (charset, kbdtype) = match ccsid {
        273 => (697, "AGB"),
        285 => (697, "UKB"),
        297 => (697, "FAB"),
        500 => (697, "BLB"),
        1140 => (695, "USB"),
        _ => (697, "USB"),
    };
    Environ {
        codepage: ccsid,
        charset,
        kbdtype,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_table_round_trips_all_256_bytes() {
        let mut n = 0;
        for ccsid in supported() {
            let p = by_ccsid(ccsid).unwrap();
            assert_eq!(p.ccsid(), ccsid);
            for b in 0..=255u8 {
                let c = p.to_char(b);
                assert_eq!(
                    p.to_byte(c),
                    Some(b),
                    "CCSID {ccsid} byte {b:02X} via {c:?}"
                );
            }
            n += 1;
        }
        assert_eq!(n, 6);
    }

    #[test]
    fn characteristic_positions() {
        let at = |ccsid, b| by_ccsid(ccsid).unwrap().to_char(b);
        // Common to all: space, digits, letters.
        for ccsid in supported() {
            assert_eq!(at(ccsid, 0x40), ' ');
            assert_eq!(at(ccsid, 0xC1), 'A');
            assert_eq!(at(ccsid, 0x81), 'a');
            assert_eq!(at(ccsid, 0xF0), '0');
        }
        assert_eq!(at(37, 0x4A), '¢');
        assert_eq!(at(37, 0xBA), '[');
        assert_eq!(at(37, 0x9F), '¤');
        assert_eq!(at(1140, 0x9F), '€');
        assert_eq!(at(285, 0x5B), '£');
        assert_eq!(at(285, 0x4A), '$');
        assert_eq!(at(273, 0x4A), 'Ä');
        assert_eq!(at(297, 0x7B), '£');
        assert_eq!(at(500, 0x4A), '[');
    }

    #[test]
    fn text_conversion() {
        let p = default_page();
        assert_eq!(
            p.encode("QPADEV0001"),
            [0xD8, 0xD7, 0xC1, 0xC4, 0xC5, 0xE5, 0xF0, 0xF0, 0xF0, 0xF1]
        );
        assert_eq!(p.decode(&[0xC9, 0xF9, 0xF0, 0xF2]), "I902");
        assert_eq!(p.encode("a€b"), [0x81, 0x82], "37 has no euro");
        assert!(by_ccsid(1208).is_none());
    }

    #[test]
    fn environment_values() {
        assert_eq!(
            environ(37),
            Environ {
                codepage: 37,
                charset: 697,
                kbdtype: "USB"
            }
        );
        assert_eq!(environ(1140).charset, 695);
        assert_eq!(environ(285).kbdtype, "UKB");
    }
}
