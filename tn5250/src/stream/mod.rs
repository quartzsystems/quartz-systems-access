// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! The 5250 data stream from the host: commands after `ESC` (`0x04`), the
//! orders inside Write to Display, and their effect on a [`Screen`].
//!
//! [`apply`] takes one record. Whatever the bytes, it never panics: an
//! error stops processing of that record, is logged, and, where the
//! protocol defines one, produces a negative response record.

pub mod query;

use crate::inbound;
use crate::negotiate::ScreenSize;
use crate::record::{Record, flag, opcode};
use crate::screen::{CursorTo, Ffw, Field, Keyboard, Read, Screen, Snapshot, attr, byte};

/// The command introducer.
pub const ESC: u8 = 0x04;

/// Commands.
pub mod cmd {
    pub const CLEAR_UNIT: u8 = 0x40;
    pub const CLEAR_UNIT_ALTERNATE: u8 = 0x20;
    pub const CLEAR_FORMAT_TABLE: u8 = 0x50;
    pub const WRITE_TO_DISPLAY: u8 = 0x11;
    pub const WRITE_ERROR_CODE: u8 = 0x21;
    pub const WRITE_ERROR_CODE_TO_WINDOW: u8 = 0x22;
    pub const READ_INPUT_FIELDS: u8 = 0x42;
    pub const READ_MDT_FIELDS: u8 = 0x52;
    pub const READ_MDT_ALTERNATE: u8 = 0x82;
    pub const READ_SCREEN: u8 = 0x62;
    pub const READ_SCREEN_EXTENDED: u8 = 0x64;
    pub const READ_SCREEN_PRINT: u8 = 0x66;
    pub const READ_SCREEN_PRINT_EXTENDED: u8 = 0x68;
    pub const READ_IMMEDIATE: u8 = 0x72;
    pub const READ_IMMEDIATE_ALTERNATE: u8 = 0x83;
    pub const SAVE_SCREEN: u8 = 0x02;
    pub const SAVE_PARTIAL_SCREEN: u8 = 0x03;
    pub const RESTORE_SCREEN: u8 = 0x12;
    pub const RESTORE_PARTIAL_SCREEN: u8 = 0x13;
    pub const ROLL: u8 = 0x23;
    pub const WRITE_STRUCTURED_FIELD: u8 = 0xF3;
    pub const COPY_TO_PRINTER: u8 = 0x16;
}

/// Orders inside Write to Display and Write Error Code.
pub mod order {
    pub const SOH: u8 = 0x01;
    pub const RA: u8 = 0x02;
    pub const EA: u8 = 0x03;
    pub const TD: u8 = 0x10;
    pub const SBA: u8 = 0x11;
    pub const WEA: u8 = 0x12;
    pub const IC: u8 = 0x13;
    pub const MC: u8 = 0x14;
    pub const WDSF: u8 = 0x15;
    pub const SF: u8 = 0x1D;
}

/// Negative response codes, sent in the data of a record flagged `ERR`.
/// `1003` codes are command errors, `1005` codes parameter errors.
pub mod nr {
    pub const COMMAND_NOT_VALID: u32 = 0x1003_0101;
    pub const CLEAR_UNIT_ALTERNATE_NOT_VALID: u32 = 0x1003_0105;
    pub const STRUCTURED_FIELD_NOT_SUPPORTED: u32 = 0x1003_0111;
    pub const DATA_OUTSIDE_COMMAND: u32 = 0x1005_0101;
    pub const PREMATURE_END: u32 = 0x1005_0110;
    pub const ORDER_NOT_VALID: u32 = 0x1005_0121;
    pub const ADDRESS_NOT_VALID: u32 = 0x1005_0122;
    pub const REPEAT_ADDRESS_BEFORE_CURRENT: u32 = 0x1005_0123;
    pub const FORMAT_TABLE_OVERFLOW: u32 = 0x1005_012A;
    pub const FIELD_PAST_END: u32 = 0x1005_012B;
    pub const BAD_LENGTH: u32 = 0x1005_012C;
    pub const RESTORE_DATA_NOT_VALID: u32 = 0x1005_0130;
}

/// What went wrong in a record.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StreamError {
    /// The negative response code sent for it.
    pub code: u32,
    pub text: String,
}

impl std::fmt::Display for StreamError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} (negative response {:08X})", self.text, self.code)
    }
}

/// What applying a record produced.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Applied {
    /// Records to send back: read replies, the query reply, negative
    /// responses.
    pub replies: Vec<Record>,
    pub errors: Vec<StreamError>,
}

/// A negative response record.
pub fn negative_response(code: u32, opcode: u8) -> Record {
    Record {
        reserved: 0,
        flags: flag::ERR,
        opcode,
        data: code.to_be_bytes().to_vec(),
    }
}

/// Applies one record from the host.
pub fn apply(screen: &mut Screen, rec: &Record) -> Applied {
    let mut p = Parser {
        s: screen,
        d: &rec.data,
        i: 0,
        opcode: rec.opcode,
        out: Applied::default(),
        addr: 0,
    };
    p.run();
    let Parser { s, out, .. } = p;
    s.recompute_attrs();
    for e in &out.errors {
        crate::warn!("data stream: {e}");
    }
    out
}

struct Parser<'a> {
    s: &'a mut Screen,
    d: &'a [u8],
    i: usize,
    opcode: u8,
    out: Applied,
    /// The current buffer address.
    addr: usize,
}

type Step = Result<(), StreamError>;

fn err(code: u32, text: impl Into<String>) -> StreamError {
    StreamError {
        code,
        text: text.into(),
    }
}

impl Parser<'_> {
    fn run(&mut self) {
        let result = self.record();
        if let Err(e) = result {
            self.out
                .replies
                .push(negative_response(e.code, self.opcode));
            self.out.errors.push(e);
        }
    }

    fn record(&mut self) -> Step {
        match self.opcode {
            opcode::CANCEL_INVITE => {
                self.s.pending_read = None;
                if self.s.keyboard.is_open() {
                    self.s.keyboard = Keyboard::System;
                }
                self.out
                    .replies
                    .push(Record::new(opcode::CANCEL_INVITE, Vec::new()));
                return Ok(());
            }
            opcode::MESSAGE_LIGHT_ON => {
                self.s.message_waiting = true;
                return Ok(());
            }
            opcode::MESSAGE_LIGHT_OFF => {
                self.s.message_waiting = false;
                return Ok(());
            }
            opcode::SAVE_SCREEN if self.d.is_empty() => {
                self.save();
                return Ok(());
            }
            opcode::READ_IMMEDIATE if self.d.is_empty() => {
                self.read_immediate();
                return Ok(());
            }
            opcode::READ_SCREEN if self.d.is_empty() => {
                self.read_screen();
                return Ok(());
            }
            opcode::RESTORE_SCREEN if self.d.first() != Some(&ESC) => {
                let (snap, _) = Snapshot::decode(self.d)
                    .ok_or_else(|| err(nr::RESTORE_DATA_NOT_VALID, "restore data not valid"))?;
                self.s.restore(&snap);
                return Ok(());
            }
            _ => {}
        }
        while self.i < self.d.len() {
            if self.d[self.i] != ESC {
                return Err(err(
                    nr::DATA_OUTSIDE_COMMAND,
                    format!(
                        "byte {:02X} at offset {} is not a command",
                        self.d[self.i], self.i
                    ),
                ));
            }
            self.i += 1;
            let c = self.byte("command")?;
            self.command(c)?;
        }
        Ok(())
    }

    fn byte(&mut self, what: &str) -> Result<u8, StreamError> {
        let b = *self
            .d
            .get(self.i)
            .ok_or_else(|| err(nr::PREMATURE_END, format!("record ends inside {what}")))?;
        self.i += 1;
        Ok(b)
    }

    fn u16(&mut self, what: &str) -> Result<u16, StreamError> {
        let hi = self.byte(what)?;
        let lo = self.byte(what)?;
        Ok(u16::from_be_bytes([hi, lo]))
    }

    fn take(&mut self, n: usize, what: &str) -> Result<&[u8], StreamError> {
        let end = self.i.checked_add(n).filter(|&e| e <= self.d.len());
        let end =
            end.ok_or_else(|| err(nr::PREMATURE_END, format!("record ends inside {what}")))?;
        let s = &self.d[self.i..end];
        self.i = end;
        Ok(s)
    }

    /// A 1-based row and column from the stream, as an address.
    fn address(&mut self, what: &str) -> Result<usize, StreamError> {
        let row = self.byte(what)? as usize;
        let col = self.byte(what)? as usize;
        if row == 0 || row > self.s.rows() || col == 0 || col > self.s.cols() {
            return Err(err(
                nr::ADDRESS_NOT_VALID,
                format!("{what}: row {row} column {col} is off the screen"),
            ));
        }
        Ok(self.s.addr(row - 1, col - 1))
    }

    fn command(&mut self, c: u8) -> Step {
        crate::trace!("data stream: command {c:02X}");
        match c {
            cmd::CLEAR_UNIT => {
                self.s.clear(ScreenSize::S24x80);
                self.new_format();
            }
            cmd::CLEAR_UNIT_ALTERNATE => {
                let p = self.byte("Clear Unit Alternate")?;
                match p {
                    0x00 if self.s.wide_ok() => self.s.clear(ScreenSize::S27x132),
                    0x80 => {
                        let size = if self.s.cols() == 132 {
                            ScreenSize::S27x132
                        } else {
                            ScreenSize::S24x80
                        };
                        self.s.clear(size);
                    }
                    _ => {
                        return Err(err(
                            nr::CLEAR_UNIT_ALTERNATE_NOT_VALID,
                            format!("Clear Unit Alternate {p:02X} not supported by this display"),
                        ));
                    }
                }
                self.new_format();
            }
            cmd::CLEAR_FORMAT_TABLE => {
                self.s.fields.clear();
                self.s.insert_cursor = None;
                self.s.pending_read = None;
                if !self.s.keyboard.in_error() {
                    self.s.keyboard = Keyboard::System;
                }
                self.new_format();
            }
            cmd::WRITE_TO_DISPLAY => {
                let cc1 = self.byte("Write to Display")?;
                let cc2 = self.byte("Write to Display")?;
                self.cc1(cc1);
                self.addr = self.s.cursor;
                self.orders()?;
                self.cc2(cc2);
            }
            cmd::WRITE_ERROR_CODE => self.write_error(None)?,
            cmd::WRITE_ERROR_CODE_TO_WINDOW => {
                let first = self.byte("Write Error Code to Window")?;
                let _last = self.byte("Write Error Code to Window")?;
                self.write_error(Some(first))?;
            }
            cmd::READ_INPUT_FIELDS | cmd::READ_MDT_FIELDS | cmd::READ_MDT_ALTERNATE => {
                let cc1 = self.byte("read command")?;
                let cc2 = self.byte("read command")?;
                self.cc1(cc1);
                self.s.pending_read = Some(match c {
                    cmd::READ_INPUT_FIELDS => Read::InputFields,
                    cmd::READ_MDT_FIELDS => Read::MdtFields,
                    _ => Read::MdtAlternate,
                });
                self.cc2(cc2);
                self.unlock();
            }
            cmd::READ_SCREEN
            | cmd::READ_SCREEN_EXTENDED
            | cmd::READ_SCREEN_PRINT
            | cmd::READ_SCREEN_PRINT_EXTENDED => self.read_screen(),
            cmd::READ_IMMEDIATE | cmd::READ_IMMEDIATE_ALTERNATE => self.read_immediate(),
            cmd::SAVE_SCREEN => self.save(),
            cmd::SAVE_PARTIAL_SCREEN => {
                self.take(5, "Save Partial Screen")?;
                self.save();
            }
            cmd::RESTORE_SCREEN | cmd::RESTORE_PARTIAL_SCREEN => {
                let (snap, used) = Snapshot::decode(&self.d[self.i..])
                    .ok_or_else(|| err(nr::RESTORE_DATA_NOT_VALID, "restore data not valid"))?;
                self.i += used;
                self.s.restore(&snap);
            }
            cmd::ROLL => self.roll()?,
            cmd::WRITE_STRUCTURED_FIELD => self.structured_fields()?,
            cmd::COPY_TO_PRINTER => {
                self.byte("Copy to Printer")?;
                crate::debug!("data stream: Copy to Printer ignored");
            }
            other => {
                return Err(err(
                    nr::COMMAND_NOT_VALID,
                    format!("command {other:02X} not valid"),
                ));
            }
        }
        Ok(())
    }

    /// CC1: lock the keyboard, reset MDT bits, null fields.
    fn cc1(&mut self, cc1: u8) {
        let mode = cc1 & 0xE0;
        if mode == 0 {
            return;
        }
        if !self.s.keyboard.in_error() {
            self.s.keyboard = Keyboard::System;
        }
        self.s.pending_aid = None;
        let (null_mdt, null_all, reset_input, reset_all) = match mode {
            0x40 => (false, false, true, false),
            0x60 => (false, false, false, true),
            0x80 => (true, false, false, false),
            0xA0 => (true, false, true, false),
            0xC0 => (false, true, true, false),
            0xE0 => (false, true, false, true),
            _ => (false, false, false, false),
        };
        let nulls: Vec<(usize, usize)> = self
            .s
            .fields
            .iter()
            .filter(|f| !f.ffw.bypass() && (null_all || (null_mdt && f.ffw.mdt())))
            .map(|f| (f.start, f.end()))
            .collect();
        for (a, b) in nulls {
            for i in a..b {
                self.s.set_byte(i, byte::NULL);
            }
        }
        for f in &mut self.s.fields {
            if reset_all || (reset_input && !f.ffw.bypass()) {
                f.ffw.set_mdt(false);
            }
        }
    }

    /// CC2: alarm, message light, unlock.
    fn cc2(&mut self, cc2: u8) {
        if cc2 & 0x04 != 0 {
            self.s.alarm = true;
        }
        if cc2 & 0x02 != 0 {
            self.s.message_waiting = false;
        }
        if cc2 & 0x01 != 0 {
            self.s.message_waiting = true;
        }
        if cc2 & 0x08 != 0 {
            self.unlock();
        }
    }

    /// Opens the keyboard and places the cursor.
    fn unlock(&mut self) {
        if self.s.keyboard.in_error() {
            return;
        }
        self.s.keyboard = Keyboard::Open;
        match (self.s.cursor_to, self.s.insert_cursor) {
            (CursorTo::InsertCursor, Some(ic)) => self.s.cursor = ic,
            (CursorTo::InsertCursor | CursorTo::Home, _) => self.s.cursor = self.s.home(),
            (CursorTo::Stay, _) => {}
        }
        self.s.cursor_to = CursorTo::Stay;
    }

    /// A new format was written: the cursor goes home on unlock, unless an
    /// Insert Cursor order already says where.
    fn new_format(&mut self) {
        if self.s.cursor_to != CursorTo::InsertCursor || self.s.insert_cursor.is_none() {
            self.s.cursor_to = CursorTo::Home;
        }
    }

    fn advance(&mut self) {
        self.addr += 1;
        if self.addr >= self.s.size() {
            self.addr = 0;
        }
    }

    fn put(&mut self, b: u8) {
        self.s.set_byte(self.addr, b);
        self.advance();
    }

    /// Orders and data up to the next command or the end of the record.
    fn orders(&mut self) -> Step {
        while self.i < self.d.len() && self.d[self.i] != ESC {
            let b = self.d[self.i];
            self.i += 1;
            match b {
                order::SOH => self.soh()?,
                order::RA => {
                    let to = self.address("Repeat to Address")?;
                    let ch = self.byte("Repeat to Address")?;
                    if to < self.addr {
                        return Err(err(
                            nr::REPEAT_ADDRESS_BEFORE_CURRENT,
                            "Repeat to Address before the current address",
                        ));
                    }
                    while self.addr <= to {
                        let at = self.addr;
                        self.put(ch);
                        if at == to || self.addr == 0 {
                            break;
                        }
                    }
                }
                order::EA => {
                    let to = self.address("Erase to Address")?;
                    let n = self.byte("Erase to Address")? as usize;
                    if !(2..=5).contains(&n) {
                        return Err(err(nr::BAD_LENGTH, format!("Erase to Address length {n}")));
                    }
                    self.take(n - 1, "Erase to Address")?;
                    if to < self.addr {
                        return Err(err(
                            nr::REPEAT_ADDRESS_BEFORE_CURRENT,
                            "Erase to Address before the current address",
                        ));
                    }
                    while self.addr <= to {
                        let at = self.addr;
                        self.put(byte::NULL);
                        if at == to || self.addr == 0 {
                            break;
                        }
                    }
                }
                order::TD => {
                    let n = self.u16("Transparent Data")? as usize;
                    let data = self.take(n, "Transparent Data")?.to_vec();
                    for b in data {
                        self.put(b);
                    }
                }
                order::SBA => self.addr = self.address("Set Buffer Address")?,
                order::WEA => {
                    let t = self.byte("Write Extended Attribute")?;
                    let v = self.byte("Write Extended Attribute")?;
                    crate::debug!("data stream: extended attribute {t:02X}={v:02X} ignored");
                }
                order::IC => {
                    let a = self.address("Insert Cursor")?;
                    self.s.insert_cursor = Some(a);
                    self.s.cursor_to = CursorTo::InsertCursor;
                }
                order::MC => {
                    let a = self.address("Move Cursor")?;
                    self.s.cursor = a;
                    self.s.cursor_to = CursorTo::Stay;
                }
                order::WDSF => {
                    let n = self.u16("Write to Display Structured Field")? as usize;
                    if n < 2 {
                        return Err(err(nr::BAD_LENGTH, format!("structured field length {n}")));
                    }
                    let body = self.take(n - 2, "Write to Display Structured Field")?;
                    crate::debug!(
                        "data stream: display structured field {} not supported; skipped",
                        crate::log::hex(&body[..body.len().min(2)])
                    );
                }
                order::SF => self.start_field()?,
                byte::NULL | byte::DUP | byte::FIELD_MARK => self.put(b),
                b if b >= 0x20 => self.put(b),
                other => {
                    return Err(err(
                        nr::ORDER_NOT_VALID,
                        format!("order {other:02X} not valid"),
                    ));
                }
            }
        }
        Ok(())
    }

    fn soh(&mut self) -> Step {
        let n = self.byte("Start of Header")? as usize;
        if !(1..=7).contains(&n) {
            return Err(err(nr::BAD_LENGTH, format!("Start of Header length {n}")));
        }
        let mut h = [0u8; 7];
        h[..n].copy_from_slice(self.take(n, "Start of Header")?);
        self.s.header.flags = h[0];
        self.s.header.resequence = h[2];
        let row = h[3] as usize;
        self.s.error_row = if (1..=self.s.rows()).contains(&row) {
            row - 1
        } else {
            self.s.rows() - 1
        };
        // F24..F17, F16..F9, F8..F1.
        self.s.header.no_data_keys =
            (u32::from(h[4]) << 16) | (u32::from(h[5]) << 8) | u32::from(h[6]);
        self.new_format();
        Ok(())
    }

    fn start_field(&mut self) -> Step {
        let mut ffw = None;
        let mut fcw = Vec::new();
        let first = *self
            .d
            .get(self.i)
            .ok_or_else(|| err(nr::PREMATURE_END, "record ends inside Start of Field"))?;
        if first & 0xC0 == 0x40 {
            ffw = Some(Ffw(self.u16("Start of Field")?));
            while let Some(&b) = self.d.get(self.i) {
                if b & 0x80 == 0 {
                    break;
                }
                fcw.push(self.u16("Start of Field")?);
            }
        }
        let a = self.byte("Start of Field")?;
        if !attr::is_attr(a) {
            return Err(err(
                nr::ORDER_NOT_VALID,
                format!("Start of Field attribute {a:02X} not valid"),
            ));
        }
        let len = self.u16("Start of Field")? as usize;
        let at = self.addr;
        let start = at + 1;
        if len == 0 || start + len > self.s.size() {
            return Err(err(
                nr::FIELD_PAST_END,
                format!("field of {len} at {at} runs past the end of the display"),
            ));
        }
        self.put(a);
        if let Some(ffw) = ffw {
            let ok = self.s.define_field(Field {
                start,
                len,
                ffw,
                fcw,
                attr: a,
            });
            if !ok {
                return Err(err(nr::FORMAT_TABLE_OVERFLOW, "format table is full"));
            }
        }
        Ok(())
    }

    fn write_error(&mut self, first_col: Option<u8>) -> Step {
        let row = self.s.error_row;
        let cols = self.s.cols();
        if self.s.error_saved.is_none() {
            let line = self.s.bytes()[row * cols..(row + 1) * cols].to_vec();
            self.s.error_saved = Some(line);
        }
        let col = first_col
            .map(|c| (c as usize).saturating_sub(1).min(cols - 1))
            .unwrap_or(0);
        self.addr = row * cols + col;
        self.orders()?;
        self.s.keyboard = Keyboard::HostError;
        // The cursor goes to the field in error at once.
        if self.s.cursor_to == CursorTo::InsertCursor
            && let Some(ic) = self.s.insert_cursor
        {
            self.s.cursor = ic;
            self.s.cursor_to = CursorTo::Stay;
        }
        Ok(())
    }

    fn roll(&mut self) -> Step {
        let ctl = self.byte("Roll")?;
        let top = self.byte("Roll")? as usize;
        let bottom = self.byte("Roll")? as usize;
        let rows = self.s.rows();
        if top == 0 || bottom > rows || top > bottom {
            return Err(err(
                nr::ADDRESS_NOT_VALID,
                format!("Roll rows {top} to {bottom}"),
            ));
        }
        let n = (ctl & 0x1F) as usize;
        let down = ctl & 0x80 != 0;
        let cols = self.s.cols();
        let (t, b) = (top - 1, bottom - 1);
        let mut lines: Vec<Vec<u8>> = (t..=b)
            .map(|r| self.s.bytes()[r * cols..(r + 1) * cols].to_vec())
            .collect();
        let n = n.min(lines.len());
        if down {
            lines.rotate_right(n);
            lines[..n].iter_mut().for_each(|l| l.fill(byte::NULL));
        } else {
            lines.rotate_left(n);
            let k = lines.len();
            lines[k - n..].iter_mut().for_each(|l| l.fill(byte::NULL));
        }
        for (k, line) in lines.into_iter().enumerate() {
            for (c, &v) in line.iter().enumerate() {
                self.s.set_byte((t + k) * cols + c, v);
            }
        }
        Ok(())
    }

    fn structured_fields(&mut self) -> Step {
        while self.i < self.d.len() && self.d[self.i] != ESC {
            let n = self.u16("Write Structured Field")? as usize;
            if n < 5 {
                return Err(err(nr::BAD_LENGTH, format!("structured field length {n}")));
            }
            let body = self.take(n - 2, "Write Structured Field")?.to_vec();
            match (body[0], body[1]) {
                (0xD9, 0x70) => {
                    crate::debug!("data stream: 5250 Query; answering");
                    self.out.replies.push(query::reply(self.s));
                }
                (class, kind) => {
                    return Err(err(
                        nr::STRUCTURED_FIELD_NOT_SUPPORTED,
                        format!("structured field {class:02X} {kind:02X} not supported"),
                    ));
                }
            }
        }
        Ok(())
    }

    fn save(&mut self) {
        let snap = self.s.snapshot();
        let mut data = vec![ESC, cmd::RESTORE_SCREEN];
        data.extend(snap.encode());
        self.s.push_saved(snap);
        self.out
            .replies
            .push(Record::new(opcode::SAVE_SCREEN, data));
    }

    fn read_screen(&mut self) {
        self.out
            .replies
            .push(Record::new(opcode::READ_SCREEN, self.s.bytes().to_vec()));
    }

    fn read_immediate(&mut self) {
        let data = inbound::read_immediate(self.s);
        self.out.replies.push(Record::new(opcode::PUT_GET, data));
    }
}

#[cfg(test)]
mod tests;
