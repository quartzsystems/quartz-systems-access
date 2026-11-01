// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! Inbound data: what the display sends for a read. Every reply starts
//! with the cursor row and column (1-based) and the AID.
//!
//! - Read MDT Fields: each field with its MDT bit on, behind a Set Buffer
//!   Address order, trailing nulls dropped and other nulls sent as blanks.
//! - Read MDT Fields Alternate: the same, nulls kept.
//! - Read Input Fields: when any field is modified, every input field in
//!   format-table order without addresses, nulls as blanks.

use crate::aid;
use crate::screen::{Read, Screen, byte};
use crate::stream::order;

fn cursor_and_aid(s: &Screen, aid: u8) -> Vec<u8> {
    let (r, c) = s.rc(s.cursor);
    vec![(r + 1) as u8, (c + 1) as u8, aid]
}

/// Whether a key sends field data with this screen's header.
pub fn returns_data(s: &Screen, aid: u8) -> bool {
    if aid::sends_no_data(aid) {
        return false;
    }
    match aid::function_number(aid) {
        Some(n) => s.header.no_data_keys & (1 << (n - 1)) == 0,
        None => true,
    }
}

/// The data of a reply to `read` for the key `aid`.
pub fn read_reply(s: &Screen, read: Read, aid: u8) -> Vec<u8> {
    let mut out = cursor_and_aid(s, aid);
    if !returns_data(s, aid) {
        return out;
    }
    match read {
        Read::MdtFields | Read::MdtAlternate => {
            for f in s.fields.iter().filter(|f| f.ffw.mdt()) {
                let (r, c) = s.rc(f.start);
                out.extend_from_slice(&[order::SBA, (r + 1) as u8, (c + 1) as u8]);
                let data = s.field_data(f);
                let end = data
                    .iter()
                    .rposition(|&b| b != byte::NULL)
                    .map_or(0, |p| p + 1);
                for &b in &data[..end] {
                    out.push(if b == byte::NULL && read == Read::MdtFields {
                        byte::BLANK
                    } else {
                        b
                    });
                }
            }
        }
        Read::InputFields => {
            if s.fields.iter().any(|f| f.ffw.mdt()) {
                for (_, f) in s.input_fields() {
                    out.extend(
                        s.field_data(f).iter().map(
                            |&b| {
                                if b == byte::NULL { byte::BLANK } else { b }
                            },
                        ),
                    );
                }
            }
        }
    }
    out
}

/// The reply to Read Immediate: the input fields as Read Input Fields
/// returns them, with no AID.
pub fn read_immediate(s: &Screen) -> Vec<u8> {
    read_reply(s, Read::InputFields, 0x00)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::screen::{Ffw, Field, attr};

    fn screen() -> Screen {
        let mut s = Screen::default_24x80();
        for (start, len, ffw) in [(161, 5, 0x4000u16), (241, 4, 0x4000), (321, 3, 0x6000)] {
            s.set_byte(start - 1, attr::GREEN_UL);
            s.define_field(Field {
                start,
                len,
                ffw: Ffw(ffw),
                fcw: vec![],
                attr: attr::GREEN_UL,
            });
        }
        s.cursor = 163;
        s
    }

    #[test]
    fn mdt_fields_with_addresses_and_trailing_nulls_dropped() {
        let mut s = screen();
        // Field 1: "A", null, "B", then nulls.
        s.set_byte(161, 0xC1);
        s.set_byte(163, 0xC2);
        s.fields[0].ffw.set_mdt(true);
        let r = read_reply(&s, Read::MdtFields, aid::ENTER);
        assert_eq!(r, [3, 4, 0xF1, 0x11, 3, 2, 0xC1, 0x40, 0xC2]);
        let r = read_reply(&s, Read::MdtAlternate, aid::ENTER);
        assert_eq!(r, [3, 4, 0xF1, 0x11, 3, 2, 0xC1, 0x00, 0xC2]);
    }

    #[test]
    fn input_fields_all_or_nothing() {
        let mut s = screen();
        assert_eq!(read_reply(&s, Read::InputFields, aid::ENTER), [3, 4, 0xF1]);
        s.set_byte(241, 0xF1);
        s.fields[1].ffw.set_mdt(true);
        let r = read_reply(&s, Read::InputFields, aid::ENTER);
        assert_eq!(
            &r[3..],
            &[0x40, 0x40, 0x40, 0x40, 0x40, 0xF1, 0x40, 0x40, 0x40]
        );
    }

    #[test]
    fn keys_without_data() {
        let mut s = screen();
        s.fields[0].ffw.set_mdt(true);
        assert_eq!(read_reply(&s, Read::MdtFields, aid::CLEAR).len(), 3);
        // F3 marked as returning no data in the header.
        s.header.no_data_keys = 1 << 2;
        assert_eq!(
            read_reply(&s, Read::MdtFields, aid::function(3).unwrap()).len(),
            3
        );
        assert!(read_reply(&s, Read::MdtFields, aid::function(4).unwrap()).len() > 3);
    }
}
