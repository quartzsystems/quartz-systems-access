// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! Shared by the targets: reading the fuzz input, and what must hold after
//! every step.

use tn5250::codepage;
use tn5250::negotiate::ScreenSize;
use tn5250::record::Record;
use tn5250::screen::Screen;

/// Reads input bytes in order; past the end everything is zero.
pub struct Input<'a> {
    pub d: &'a [u8],
    pub i: usize,
}

impl<'a> Input<'a> {
    pub fn new(d: &'a [u8]) -> Input<'a> {
        Input { d, i: 0 }
    }
    pub fn done(&self) -> bool {
        self.i >= self.d.len()
    }
    pub fn u8(&mut self) -> u8 {
        let b = self.d.get(self.i).copied().unwrap_or(0);
        self.i += 1;
        b
    }
    pub fn u16(&mut self) -> u16 {
        u16::from_be_bytes([self.u8(), self.u8()])
    }
    pub fn take(&mut self, n: usize) -> &'a [u8] {
        let start = self.i.min(self.d.len());
        let end = start.saturating_add(n).min(self.d.len());
        self.i = end;
        &self.d[start..end]
    }
    /// A record: opcode, 16-bit length, data (cut short at the end).
    pub fn record(&mut self) -> Record {
        let op = self.u8() % 13;
        let n = (self.u16() as usize) % 8192;
        Record::new(op, self.take(n).to_vec())
    }
}

/// The display a flags byte asks for: bit 0 wide, bits 1-3 the CCSID.
pub fn setup(flags: u8) -> (ScreenSize, u16) {
    let size = if flags & 1 == 1 {
        ScreenSize::S27x132
    } else {
        ScreenSize::S24x80
    };
    let ccsids: Vec<u16> = codepage::supported().collect();
    (size, ccsids[(flags as usize >> 1) % ccsids.len()])
}

/// What must hold after any step.
pub fn check(s: &Screen) {
    let size = s.rows() * s.cols();
    assert!(matches!((s.rows(), s.cols()), (24, 80) | (27, 132)));
    assert_eq!(s.bytes().len(), size);
    assert_eq!(s.attrs().len(), size);
    assert!(s.cursor < size);
    assert!(s.insert_cursor.is_none_or(|ic| ic < size));
    assert!(s.error_row < s.rows());
    assert!(s.fields.len() <= tn5250::screen::MAX_FIELDS);
    for w in s.fields.windows(2) {
        assert!(
            w[0].end() <= w[1].start,
            "fields overlap or are out of order"
        );
    }
    for f in &s.fields {
        assert!(f.len > 0 && f.end() <= size);
    }
    assert_eq!(s.text_rows().len(), s.rows());
}
