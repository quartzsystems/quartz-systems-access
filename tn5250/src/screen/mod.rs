// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! The presentation space of a 5250 display.
//!
//! - The character plane holds the bytes as the data stream wrote them:
//!   EBCDIC characters, nulls, and screen attribute bytes (`0x20`-`0x3F`),
//!   each attribute taking a cell of its own that shows as a blank.
//! - The attribute plane holds the attribute in force at every cell: the
//!   last attribute byte before it, wrapping from the end of the screen.
//! - The format table holds the input fields.
//! - The cursor, the insert-cursor position, the keyboard state, the error
//!   line and the saved screens complete it.
//!
//! Addresses are 0-based (`row * cols + col`); the data stream's 1-based
//! rows and columns are converted at the edges.

pub mod field;

use std::collections::VecDeque;

pub use field::{Adjust, Ffw, Field, Shift};

use crate::codepage::{self, CodePage};
use crate::negotiate::ScreenSize;

/// Screen attribute bytes.
pub mod attr {
    pub const GREEN: u8 = 0x20;
    pub const GREEN_RI: u8 = 0x21;
    pub const WHITE: u8 = 0x22;
    pub const WHITE_RI: u8 = 0x23;
    pub const GREEN_UL: u8 = 0x24;
    pub const GREEN_UL_RI: u8 = 0x25;
    pub const WHITE_UL: u8 = 0x26;
    pub const NONDISPLAY: u8 = 0x27;
    pub const RED: u8 = 0x28;
    pub const RED_RI: u8 = 0x29;
    pub const RED_BLINK: u8 = 0x2A;
    pub const TURQUOISE_CS: u8 = 0x30;
    pub const YELLOW_CS: u8 = 0x34;
    pub const PINK: u8 = 0x38;
    pub const BLUE: u8 = 0x3A;
    pub const BLUE_UL: u8 = 0x3E;

    /// Whether a byte is a screen attribute.
    pub fn is_attr(b: u8) -> bool {
        (0x20..=0x3F).contains(&b)
    }

    /// `x7` and `xF` attributes show nothing.
    pub fn is_nondisplay(a: u8) -> bool {
        a & 0x07 == 0x07
    }
}

/// Special bytes in the character plane.
pub mod byte {
    pub const NULL: u8 = 0x00;
    pub const DUP: u8 = 0x1C;
    pub const FIELD_MARK: u8 = 0x1E;
    pub const BLANK: u8 = 0x40;
}

/// What the host last asked to read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Read {
    /// Read Input Fields.
    InputFields,
    /// Read MDT Fields.
    MdtFields,
    /// Read MDT Fields Alternate.
    MdtAlternate,
}

/// Whether the operator can type.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Keyboard {
    /// Typing allowed.
    Open,
    /// The system has the keyboard (after an AID key, before the reply).
    System,
    /// An operator error: the code is shown until Reset.
    OperatorError(u16),
    /// The host wrote to the error line (Write Error Code): input
    /// inhibited until Reset.
    HostError,
}

impl Keyboard {
    pub fn is_open(self) -> bool {
        self == Keyboard::Open
    }
    /// An error that Reset clears.
    pub fn in_error(self) -> bool {
        matches!(self, Keyboard::OperatorError(_) | Keyboard::HostError)
    }
}

/// Where the cursor goes when the keyboard next opens.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CursorTo {
    /// Where it is (a Move Cursor order put it there).
    Stay,
    /// To [`Screen::home`]: a new format was written.
    Home,
    /// To the Insert Cursor order's position.
    InsertCursor,
}

/// Values from the Start of Header order.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Header {
    pub flags: u8,
    /// Field number the cursor starts at, 0 for none.
    pub resequence: u8,
    /// Function keys that return no data: bit 0 is F1 ... bit 23 is F24.
    pub no_data_keys: u32,
}

/// Everything Save Screen keeps and Restore Screen puts back.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Snapshot {
    pub rows: usize,
    pub cols: usize,
    pub buf: Vec<u8>,
    pub fields: Vec<Field>,
    pub cursor: usize,
    pub insert_cursor: Option<usize>,
    pub error_row: usize,
    pub header: Header,
}

/// How many saved screens are kept.
pub const MAX_SAVED: usize = 16;
/// The most fields a format table holds (the query reply says so).
pub const MAX_FIELDS: usize = 256;

/// One cell as the renderer sees it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cell {
    /// The character; a blank for nulls, attribute cells and nondisplay
    /// cells.
    pub ch: char,
    /// The attribute in force.
    pub attr: u8,
    /// The cell holds an attribute byte.
    pub is_attr: bool,
}

/// The display.
#[derive(Debug, Clone)]
pub struct Screen {
    rows: usize,
    cols: usize,
    /// Whether 27×132 may be used (a 3477-FC).
    wide_ok: bool,
    buf: Vec<u8>,
    attrs: Vec<u8>,
    /// The format table, sorted by start address.
    pub fields: Vec<Field>,
    pub cursor: usize,
    pub insert_cursor: Option<usize>,
    pub keyboard: Keyboard,
    pub pending_read: Option<Read>,
    /// An AID pressed while no read was outstanding, held for the next.
    pub pending_aid: Option<u8>,
    pub error_row: usize,
    /// The error line as it was before an error message covered it.
    pub error_saved: Option<Vec<u8>>,
    pub header: Header,
    pub message_waiting: bool,
    /// Set when the host sounded the alarm; the UI clears it.
    pub alarm: bool,
    /// Insert mode, toggled by the Insert key.
    pub insert_mode: bool,
    /// Where the cursor goes when the keyboard next opens.
    pub cursor_to: CursorTo,
    /// A field-exit-required field was filled: more typing is an error.
    pub exit_pending: bool,
    pub saved: VecDeque<Snapshot>,
    page: &'static dyn CodePage,
    /// Bumped on every change, so a renderer knows to look.
    pub version: u64,
}

impl Screen {
    /// A blank screen for a display of `size`, at 24×80 to begin with as a
    /// 5250 starts.
    pub fn new(size: ScreenSize, page: &'static dyn CodePage) -> Screen {
        let mut s = Screen {
            rows: 24,
            cols: 80,
            wide_ok: size == ScreenSize::S27x132,
            buf: Vec::new(),
            attrs: Vec::new(),
            fields: Vec::new(),
            cursor: 0,
            insert_cursor: None,
            keyboard: Keyboard::System,
            pending_read: None,
            pending_aid: None,
            error_row: 23,
            error_saved: None,
            header: Header::default(),
            message_waiting: false,
            alarm: false,
            insert_mode: false,
            cursor_to: CursorTo::Home,
            exit_pending: false,
            saved: VecDeque::new(),
            page,
            version: 0,
        };
        s.clear(ScreenSize::S24x80);
        s
    }

    /// A 24×80 screen in CCSID 37, for tests.
    pub fn default_24x80() -> Screen {
        Screen::new(ScreenSize::S24x80, codepage::default_page())
    }

    pub fn rows(&self) -> usize {
        self.rows
    }
    pub fn cols(&self) -> usize {
        self.cols
    }
    pub fn size(&self) -> usize {
        self.rows * self.cols
    }
    pub fn wide_ok(&self) -> bool {
        self.wide_ok
    }
    pub fn page(&self) -> &'static dyn CodePage {
        self.page
    }

    /// The 0-based row and column of an address.
    pub fn rc(&self, addr: usize) -> (usize, usize) {
        (addr / self.cols, addr % self.cols)
    }

    /// The address of a 0-based row and column.
    pub fn addr(&self, row: usize, col: usize) -> usize {
        row * self.cols + col
    }

    /// Clear Unit (and Clear Unit Alternate): nulls everywhere, no fields,
    /// cursor home, the given size.
    pub fn clear(&mut self, size: ScreenSize) {
        self.rows = size.rows();
        self.cols = size.cols();
        self.buf = vec![byte::NULL; self.size()];
        self.attrs = vec![attr::GREEN; self.size()];
        self.fields.clear();
        self.cursor = 0;
        self.insert_cursor = None;
        self.error_row = self.rows - 1;
        self.error_saved = None;
        self.header = Header::default();
        self.pending_read = None;
        self.keyboard = Keyboard::System;
        self.touch();
    }

    /// Records a change.
    pub fn touch(&mut self) {
        self.version = self.version.wrapping_add(1);
    }

    /// The byte at an address.
    pub fn byte(&self, addr: usize) -> u8 {
        self.buf.get(addr).copied().unwrap_or(byte::NULL)
    }

    /// The character plane.
    pub fn bytes(&self) -> &[u8] {
        &self.buf
    }

    /// Writes a byte. Attribute bytes change the attribute plane, which is
    /// worked out again by [`Screen::recompute_attrs`].
    pub fn set_byte(&mut self, addr: usize, b: u8) {
        if let Some(c) = self.buf.get_mut(addr) {
            *c = b;
        }
    }

    /// The attribute plane.
    pub fn attrs(&self) -> &[u8] {
        &self.attrs
    }

    /// Works out the attribute in force at every cell.
    pub fn recompute_attrs(&mut self) {
        let mut a = self
            .buf
            .iter()
            .rev()
            .copied()
            .find(|&b| attr::is_attr(b))
            .unwrap_or(attr::GREEN);
        for i in 0..self.buf.len() {
            let b = self.buf[i];
            if attr::is_attr(b) {
                a = b;
            }
            self.attrs[i] = a;
        }
        self.touch();
    }

    /// One cell for display.
    pub fn cell(&self, row: usize, col: usize) -> Cell {
        let i = self.addr(row, col);
        let b = self.byte(i);
        let a = self.attrs.get(i).copied().unwrap_or(attr::GREEN);
        let is_attr = attr::is_attr(b);
        let ch = if is_attr || attr::is_nondisplay(a) {
            ' '
        } else {
            self.char_of(b)
        };
        Cell {
            ch,
            attr: a,
            is_attr,
        }
    }

    /// The character a byte shows as.
    pub fn char_of(&self, b: u8) -> char {
        match b {
            byte::NULL | byte::FIELD_MARK => ' ',
            byte::DUP => '*',
            b if b < 0x40 => ' ',
            b => self.page.to_char(b),
        }
    }

    /// The screen as text rows (attributes, nulls and nondisplay cells as
    /// blanks).
    pub fn text_rows(&self) -> Vec<String> {
        (0..self.rows)
            .map(|r| (0..self.cols).map(|c| self.cell(r, c).ch).collect())
            .collect()
    }

    /// The field holding an address.
    pub fn field_at(&self, addr: usize) -> Option<usize> {
        self.fields.iter().position(|f| f.contains(addr))
    }

    /// Input fields the cursor can stop in, in format-table order.
    pub fn input_fields(&self) -> impl Iterator<Item = (usize, &Field)> {
        self.fields
            .iter()
            .enumerate()
            .filter(|(_, f)| !f.ffw.bypass())
    }

    /// The data bytes of a field.
    pub fn field_data(&self, f: &Field) -> &[u8] {
        let end = f.end().min(self.buf.len());
        &self.buf[f.start.min(end)..end]
    }

    /// Where the cursor goes when the keyboard opens and on Home: the
    /// insert-cursor position, else the field the Start of Header names,
    /// else the first input field, else row 1 column 1.
    pub fn home(&self) -> usize {
        if let Some(ic) = self.insert_cursor {
            return ic;
        }
        let r = self.header.resequence as usize;
        if r > 0
            && let Some(f) = self.fields.get(r - 1)
        {
            return f.start;
        }
        self.input_fields().next().map_or(0, |(_, f)| f.start)
    }

    /// Adds or replaces a field. Fields it overlaps are dropped.
    pub fn define_field(&mut self, f: Field) -> bool {
        if let Some(i) = self.fields.iter().position(|g| g.start == f.start) {
            self.fields[i] = f;
            return true;
        }
        self.fields
            .retain(|g| g.end() <= f.start || g.start >= f.end());
        if self.fields.len() >= MAX_FIELDS {
            return false;
        }
        let at = self.fields.partition_point(|g| g.start < f.start);
        self.fields.insert(at, f);
        true
    }

    /// Takes the state Save Screen keeps.
    pub fn snapshot(&self) -> Snapshot {
        Snapshot {
            rows: self.rows,
            cols: self.cols,
            buf: self.buf.clone(),
            fields: self.fields.clone(),
            cursor: self.cursor,
            insert_cursor: self.insert_cursor,
            error_row: self.error_row,
            header: self.header,
        }
    }

    /// Puts a saved state back.
    pub fn restore(&mut self, s: &Snapshot) {
        self.rows = s.rows;
        self.cols = s.cols;
        self.buf = s.buf.clone();
        self.attrs = vec![attr::GREEN; self.size()];
        self.fields = s.fields.clone();
        self.cursor = s.cursor;
        self.insert_cursor = s.insert_cursor;
        self.error_row = s.error_row;
        self.error_saved = None;
        self.header = s.header;
        self.recompute_attrs();
    }

    /// Keeps a saved screen; the oldest goes past [`MAX_SAVED`].
    pub fn push_saved(&mut self, s: Snapshot) {
        if self.saved.len() >= MAX_SAVED {
            self.saved.pop_front();
        }
        self.saved.push_back(s);
    }
}

impl Snapshot {
    const MAGIC: &'static [u8; 4] = b"QSA1";

    /// The bytes Save Screen sends to the host, which sends them back in
    /// Restore Screen. Self-delimiting: a 16-bit length follows the magic.
    pub fn encode(&self) -> Vec<u8> {
        let mut b = Vec::with_capacity(self.buf.len() + 64);
        b.extend_from_slice(Self::MAGIC);
        b.extend_from_slice(&[0, 0]);
        b.push(self.rows as u8);
        b.push(self.cols as u8);
        b.extend_from_slice(&(self.cursor as u16).to_be_bytes());
        match self.insert_cursor {
            Some(ic) => {
                b.push(1);
                b.extend_from_slice(&(ic as u16).to_be_bytes());
            }
            None => b.extend_from_slice(&[0, 0, 0]),
        }
        b.push(self.error_row as u8);
        b.push(self.header.flags);
        b.push(self.header.resequence);
        b.extend_from_slice(&self.header.no_data_keys.to_be_bytes()[1..]);
        b.extend_from_slice(&self.buf);
        b.extend_from_slice(&(self.fields.len() as u16).to_be_bytes());
        for f in &self.fields {
            b.extend_from_slice(&(f.start as u16).to_be_bytes());
            b.extend_from_slice(&(f.len as u16).to_be_bytes());
            b.extend_from_slice(&f.ffw.0.to_be_bytes());
            b.push(f.attr);
            b.push(f.fcw.len().min(255) as u8);
            for w in f.fcw.iter().take(255) {
                b.extend_from_slice(&w.to_be_bytes());
            }
        }
        let len = (b.len() as u16).to_be_bytes();
        b[4..6].copy_from_slice(&len);
        b
    }

    /// Reads [`Snapshot::encode`]'s bytes; returns the snapshot and how many
    /// bytes it took. `None` if they are not ours or do not add up.
    pub fn decode(data: &[u8]) -> Option<(Snapshot, usize)> {
        if data.len() < 6 || &data[..4] != Self::MAGIC {
            return None;
        }
        let len = u16::from_be_bytes([data[4], data[5]]) as usize;
        let d = data.get(..len)?;
        let mut r = Reader { d, i: 6 };
        let rows = r.u8()? as usize;
        let cols = r.u8()? as usize;
        if !matches!((rows, cols), (24, 80) | (27, 132)) {
            return None;
        }
        let size = rows * cols;
        let cursor = (r.u16()? as usize).min(size - 1);
        let has_ic = r.u8()?;
        let ic = r.u16()? as usize;
        let insert_cursor = (has_ic == 1 && ic < size).then_some(ic);
        let error_row = (r.u8()? as usize).min(rows - 1);
        let flags = r.u8()?;
        let resequence = r.u8()?;
        let keys = r.take(3)?;
        let no_data_keys = u32::from_be_bytes([0, keys[0], keys[1], keys[2]]);
        let buf = r.take(size)?.to_vec();
        let n = r.u16()? as usize;
        let mut fields = Vec::with_capacity(n.min(MAX_FIELDS));
        for _ in 0..n {
            let start = r.u16()? as usize;
            let flen = r.u16()? as usize;
            let ffw = Ffw(r.u16()?);
            let attr = r.u8()?;
            let nfcw = r.u8()? as usize;
            let mut fcw = Vec::with_capacity(nfcw);
            for _ in 0..nfcw {
                fcw.push(r.u16()?);
            }
            if start + flen > size || fields.len() >= MAX_FIELDS {
                return None;
            }
            fields.push(Field {
                start,
                len: flen,
                ffw,
                fcw,
                attr,
            });
        }
        Some((
            Snapshot {
                rows,
                cols,
                buf,
                fields,
                cursor,
                insert_cursor,
                error_row,
                header: Header {
                    flags,
                    resequence,
                    no_data_keys,
                },
            },
            len,
        ))
    }
}

struct Reader<'a> {
    d: &'a [u8],
    i: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let s = self.d.get(self.i..self.i.checked_add(n)?)?;
        self.i += n;
        Some(s)
    }
    fn u8(&mut self) -> Option<u8> {
        self.take(1).map(|b| b[0])
    }
    fn u16(&mut self) -> Option<u16> {
        self.take(2).map(|b| u16::from_be_bytes([b[0], b[1]]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn field(start: usize, len: usize) -> Field {
        Field {
            start,
            len,
            ffw: Ffw(0x4000),
            fcw: vec![],
            attr: attr::GREEN_UL,
        }
    }

    #[test]
    fn attributes_carry_forward_and_wrap() {
        let mut s = Screen::default_24x80();
        s.set_byte(10, attr::WHITE);
        s.set_byte(11, 0xC1);
        s.set_byte(20, attr::RED);
        s.recompute_attrs();
        assert_eq!(s.attrs()[11], attr::WHITE);
        assert_eq!(s.attrs()[19], attr::WHITE);
        assert_eq!(s.attrs()[25], attr::RED);
        // Before the first attribute: the screen's last one, wrapping.
        assert_eq!(s.attrs()[0], attr::RED);
        let c = s.cell(0, 11);
        assert_eq!((c.ch, c.is_attr), ('A', false));
        assert_eq!(s.cell(0, 10).ch, ' ');
        assert!(s.cell(0, 10).is_attr);
    }

    #[test]
    fn nondisplay_cells_show_blank() {
        let mut s = Screen::default_24x80();
        s.set_byte(0, attr::NONDISPLAY);
        s.set_byte(1, 0xC1);
        s.set_byte(2, attr::GREEN);
        s.set_byte(3, 0xC2);
        s.recompute_attrs();
        assert_eq!(&s.text_rows()[0][..4], "   B");
    }

    #[test]
    fn fields_stay_sorted_and_overlaps_go() {
        let mut s = Screen::default_24x80();
        s.define_field(field(100, 10));
        s.define_field(field(10, 5));
        s.define_field(field(50, 5));
        assert_eq!(
            s.fields.iter().map(|f| f.start).collect::<Vec<_>>(),
            [10, 50, 100]
        );
        // Same start: replaced in place.
        let mut f = field(50, 8);
        f.ffw = Ffw(0x6000);
        s.define_field(f);
        assert_eq!(s.fields[1].len, 8);
        assert!(s.fields[1].ffw.bypass());
        // Overlapping 100..110: dropped.
        s.define_field(field(95, 10));
        assert_eq!(
            s.fields.iter().map(|f| f.start).collect::<Vec<_>>(),
            [10, 50, 95]
        );
        assert_eq!(s.field_at(97), Some(2));
        assert_eq!(s.field_at(9), None);
    }

    #[test]
    fn home_prefers_insert_cursor_then_resequence_then_first_input() {
        let mut s = Screen::default_24x80();
        assert_eq!(s.home(), 0);
        let mut bypass = field(10, 5);
        bypass.ffw = Ffw(0x6000);
        s.define_field(bypass);
        s.define_field(field(30, 5));
        s.define_field(field(60, 5));
        assert_eq!(s.home(), 30);
        s.header.resequence = 3;
        assert_eq!(s.home(), 60);
        s.insert_cursor = Some(5);
        assert_eq!(s.home(), 5);
    }

    #[test]
    fn snapshot_round_trips() {
        let mut s = Screen::default_24x80();
        s.set_byte(0, attr::WHITE);
        s.set_byte(1, 0xFF);
        let mut f = field(81, 10);
        f.fcw = vec![0x8002];
        s.define_field(f);
        s.cursor = 81;
        s.insert_cursor = Some(81);
        s.header.no_data_keys = 0x00_0004;
        let snap = s.snapshot();
        let mut bytes = snap.encode();
        bytes.extend_from_slice(&[0x04, 0x40]);
        let (back, used) = Snapshot::decode(&bytes).unwrap();
        assert_eq!(back, snap);
        assert_eq!(&bytes[used..], &[0x04, 0x40]);
        // Truncated or foreign data is refused.
        assert!(Snapshot::decode(&bytes[..used - 1]).is_none());
        assert!(Snapshot::decode(b"XXXX\x00\x10").is_none());
    }

    #[test]
    fn saved_screens_are_bounded() {
        let mut s = Screen::default_24x80();
        for _ in 0..MAX_SAVED + 3 {
            let snap = s.snapshot();
            s.push_saved(snap);
        }
        assert_eq!(s.saved.len(), MAX_SAVED);
    }
}
