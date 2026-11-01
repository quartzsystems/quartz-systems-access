// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! The keyboard: typing into input fields, the editing keys, and the
//! attention keys that send a record to the host.
//!
//! A key that the field or the keyboard state does not allow puts the
//! keyboard into an operator error with a four-digit code, shown until
//! Reset.

use crate::aid;
use crate::inbound;
use crate::record::{Record, flag, opcode};
use crate::screen::{Adjust, Field, Keyboard, Screen, Shift, byte};

/// Operator error codes.
pub mod err {
    /// The cursor is not in an input field.
    pub const PROTECTED: u16 = 5;
    /// A mandatory-entry field has no data.
    pub const MANDATORY_ENTER: u16 = 7;
    /// The field takes letters only.
    pub const ALPHA_ONLY: u16 = 8;
    /// The field takes numeric characters only.
    pub const NUMERIC_ONLY: u16 = 9;
    /// The field takes the digits 0 to 9 only.
    pub const DIGITS_ONLY: u16 = 10;
    /// Nothing can be typed in the sign position.
    pub const SIGN_POSITION: u16 = 11;
    /// No room to insert: the field's last position is in use.
    pub const NO_ROOM: u16 = 12;
    /// A mandatory-fill field is only partly filled.
    pub const MANDATORY_FILL: u16 = 14;
    /// Field− is not allowed in this field.
    pub const FIELD_MINUS: u16 = 16;
    /// The field must be left with Field Exit, Field+ or Field−.
    pub const EXIT_REQUIRED: u16 = 18;
    /// Dup is not allowed in this field.
    pub const DUP: u16 = 19;
    /// The character cannot be sent in this code page.
    pub const CHARACTER: u16 = 27;
}

/// The text shown for an operator error code.
pub fn error_text(code: u16) -> &'static str {
    match code {
        err::PROTECTED => "Cursor is in a protected area of the display.",
        err::MANDATORY_ENTER => "Mandatory-entry field: data must be typed.",
        err::ALPHA_ONLY => "Field takes letters only.",
        err::NUMERIC_ONLY => "Field takes numeric characters only.",
        err::DIGITS_ONLY => "Field takes the digits 0 to 9 only.",
        err::SIGN_POSITION => "Nothing can be typed in the sign position.",
        err::NO_ROOM => "No room to insert data.",
        err::MANDATORY_FILL => "Mandatory-fill field: every position must be filled.",
        err::FIELD_MINUS => "Field- is not allowed in this field.",
        err::EXIT_REQUIRED => "Leave this field with Field Exit, Field+ or Field-.",
        err::DUP => "Dup is not allowed in this field.",
        err::CHARACTER => "Character cannot be sent in this code page.",
        _ => "Key not valid.",
    }
}

/// A 5250 key, after keyboard mapping.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Key {
    Char(char),
    // Attention keys.
    Enter,
    /// F1 to F24.
    F(u8),
    PageUp,
    PageDown,
    Clear,
    Help,
    Print,
    RecordBackspace,
    // Editing.
    FieldExit,
    FieldPlus,
    FieldMinus,
    Dup,
    EraseInput,
    EraseEof,
    Insert,
    Delete,
    Backspace,
    Home,
    NewLine,
    Tab,
    BackTab,
    Left,
    Right,
    Up,
    Down,
    // Signals.
    Reset,
    SysReq,
    Attention,
}

/// What a key did.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Outcome {
    /// Records to send.
    pub send: Vec<Record>,
    /// The operator error it raised.
    pub error: Option<u16>,
}

impl Outcome {
    fn send(r: Record) -> Outcome {
        Outcome {
            send: vec![r],
            error: None,
        }
    }
}

/// Handles one key.
pub fn key(s: &mut Screen, k: Key) -> Outcome {
    // The cursor parked on a filled field-exit-required field's last
    // position: Field Exit keeps that character.
    let parked = s.exit_pending;
    if !matches!(k, Key::Char(_) | Key::Reset) {
        s.exit_pending = false;
    }
    let out = match k {
        Key::Reset => {
            reset(s);
            Outcome::default()
        }
        Key::SysReq => signal(s, flag::SRQ),
        Key::Attention => signal(s, flag::ATN),
        Key::Help if s.keyboard.in_error() => help_in_error(s),
        _ if !s.keyboard.is_open() => Outcome::default(),
        Key::Char(c) => type_char(s, c),
        Key::Enter => attention(s, aid::ENTER),
        Key::F(n) => match aid::function(n) {
            Some(a) => attention(s, a),
            None => Outcome::default(),
        },
        Key::PageUp => attention(s, aid::ROLL_DOWN),
        Key::PageDown => attention(s, aid::ROLL_UP),
        Key::Clear => attention(s, aid::CLEAR),
        Key::Help => attention(s, aid::HELP),
        Key::Print => attention(s, aid::PRINT),
        Key::RecordBackspace => attention(s, aid::RECORD_BACKSPACE),
        Key::FieldExit => field_exit(s, Exit::Plain, parked),
        Key::FieldPlus => field_exit(s, Exit::Plus, parked),
        Key::FieldMinus => field_exit(s, Exit::Minus, parked),
        Key::Dup => dup(s),
        Key::EraseInput => {
            erase_input(s);
            Outcome::default()
        }
        Key::EraseEof => erase_eof(s),
        Key::Insert => {
            s.insert_mode = !s.insert_mode;
            Outcome::default()
        }
        Key::Delete => delete(s),
        Key::Backspace => {
            backspace(s);
            Outcome::default()
        }
        Key::Home => {
            s.cursor = s.home();
            Outcome::default()
        }
        Key::NewLine => {
            new_line(s);
            Outcome::default()
        }
        Key::Tab => tab(s),
        Key::BackTab => {
            back_tab(s);
            Outcome::default()
        }
        Key::Left => step(s, -1),
        Key::Right => step(s, 1),
        Key::Up => step(s, -(s.cols() as isize)),
        Key::Down => step(s, s.cols() as isize),
    };
    s.touch();
    out
}

/// Types text: lines go to the next line's input field, protected areas
/// are skipped, and typing stops at the end of the screen's input fields
/// or at the first error.
pub fn paste(s: &mut Screen, text: &str) -> Outcome {
    let mut out = Outcome::default();
    let mut last = None;
    for (n, line) in text.lines().enumerate() {
        if n > 0 {
            // The next line starts below the last character typed.
            if let Some(l) = last {
                s.cursor = l;
            }
            let before = s.cursor;
            new_line(s);
            if s.cursor <= before {
                break;
            }
        }
        // Set when the line filled the screen's last input field: the rest
        // of the line is dropped rather than wrapped to the top.
        let mut full = false;
        for c in line.chars() {
            if !s.keyboard.is_open() || full {
                break;
            }
            if editable(s).is_none() {
                match next_field_after(s, s.cursor) {
                    Some(i) if s.fields[i].start > s.cursor => s.cursor = s.fields[i].start,
                    _ => return out,
                }
            }
            let before = s.cursor;
            last = Some(before);
            let o = type_char(s, c);
            out.send.extend(o.send);
            if o.error.is_some() {
                out.error = o.error;
                return out;
            }
            if !out.send.is_empty() {
                return out;
            }
            if s.cursor < before {
                s.cursor = before;
                full = true;
            }
        }
    }
    s.touch();
    out
}

fn operator_error(s: &mut Screen, code: u16) -> Outcome {
    s.keyboard = Keyboard::OperatorError(code);
    s.alarm = true;
    Outcome {
        send: Vec::new(),
        error: Some(code),
    }
}

/// The input field under the cursor, by index.
fn editable(s: &Screen) -> Option<usize> {
    s.field_at(s.cursor).filter(|&i| !s.fields[i].ffw.bypass())
}

/// One past the last position that takes data: a signed-numeric field's
/// last position holds the sign.
fn data_end(f: &Field) -> usize {
    if f.ffw.shift() == Shift::SignedNumeric && f.len > 1 {
        f.end() - 1
    } else {
        f.end()
    }
}

/// The next input field after `addr`, wrapping to the first.
fn next_field_after(s: &Screen, addr: usize) -> Option<usize> {
    let first = s.input_fields().next()?.0;
    Some(
        s.input_fields()
            .find(|(_, f)| f.start > addr)
            .map_or(first, |(i, _)| i),
    )
}

/// Field Advance from the field `from` (or from the cursor): a cursor
/// progression word names the next field, else the next in order.
fn advance_from(s: &mut Screen, from: Option<usize>) {
    if let Some(i) = from
        && let Some(n) = s.fields[i].progression()
        && let Some(f) = s.fields.get(n - 1)
        && !f.ffw.bypass()
    {
        s.cursor = f.start;
        return;
    }
    let at = from.map_or(s.cursor, |i| s.fields[i].start);
    if let Some(i) = next_field_after(s, at) {
        s.cursor = s.fields[i].start;
    }
}

fn allowed(shift: Shift, c: char) -> Result<(), u16> {
    let ok = match shift {
        Shift::AlphaShift | Shift::NumericShift | Shift::Katakana => true,
        Shift::AlphaOnly => c.is_alphabetic() || ",.- ".contains(c),
        Shift::NumericOnly => c.is_ascii_digit() || "+,.- ".contains(c),
        Shift::DigitsOnly | Shift::SignedNumeric => c.is_ascii_digit(),
        Shift::Io => false,
    };
    if ok {
        return Ok(());
    }
    Err(match shift {
        Shift::AlphaOnly => err::ALPHA_ONLY,
        Shift::NumericOnly => err::NUMERIC_ONLY,
        Shift::DigitsOnly | Shift::SignedNumeric => err::DIGITS_ONLY,
        _ => err::PROTECTED,
    })
}

fn type_char(s: &mut Screen, c: char) -> Outcome {
    let exit_pending = std::mem::take(&mut s.exit_pending);
    let Some(i) = editable(s) else {
        return operator_error(s, err::PROTECTED);
    };
    let f = s.fields[i].clone();
    let end = data_end(&f);
    if exit_pending {
        s.exit_pending = true;
        return operator_error(s, err::EXIT_REQUIRED);
    }
    if s.cursor >= end {
        return operator_error(s, err::SIGN_POSITION);
    }
    if let Err(code) = allowed(f.ffw.shift(), c) {
        return operator_error(s, code);
    }
    let c = if f.ffw.monocase() {
        c.to_uppercase().next().unwrap_or(c)
    } else {
        c
    };
    let Some(b) = s.page().to_byte(c) else {
        return operator_error(s, err::CHARACTER);
    };
    if s.insert_mode {
        let last = s.byte(end - 1);
        if last != byte::NULL && last != byte::BLANK {
            return operator_error(s, err::NO_ROOM);
        }
        for a in (s.cursor + 1..end).rev() {
            let prev = s.byte(a - 1);
            s.set_byte(a, prev);
        }
    }
    s.set_byte(s.cursor, b);
    s.fields[i].ffw.set_mdt(true);
    s.cursor += 1;
    if s.cursor >= end {
        if f.ffw.exit_required() {
            // Stays on the last position; more typing is an error until
            // the field is left with Field Exit, Field+ or Field−.
            s.cursor = end - 1;
            s.exit_pending = true;
            return Outcome::default();
        }
        if f.ffw.auto_enter() {
            return attention(s, aid::ENTER);
        }
        advance_from(s, Some(i));
    }
    Outcome::default()
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Exit {
    Plain,
    Plus,
    Minus,
}

fn field_exit(s: &mut Screen, how: Exit, parked: bool) -> Outcome {
    let Some(i) = editable(s) else {
        return operator_error(s, err::PROTECTED);
    };
    let f = s.fields[i].clone();
    let shift = f.ffw.shift();
    let numeric = matches!(shift, Shift::NumericOnly | Shift::SignedNumeric);
    if how == Exit::Minus && !numeric {
        return operator_error(s, err::FIELD_MINUS);
    }
    let end = data_end(&f);
    let from = if parked { end } else { s.cursor.min(end) };
    for a in from..end {
        s.set_byte(a, byte::NULL);
    }
    let data: Vec<u8> = (f.start..end).map(|a| s.byte(a)).collect();
    let filled = data.iter().filter(|&&b| b != byte::NULL).count();
    if f.ffw.adjust() == Adjust::MandatoryFill && filled > 0 && filled < data.len() {
        return operator_error(s, err::MANDATORY_FILL);
    }
    let fill = match f.ffw.adjust() {
        Adjust::RightZero => Some(0xF0),
        Adjust::RightBlank => Some(byte::BLANK),
        _ => None,
    };
    if let Some(fill) = fill {
        let kept: Vec<u8> = data.iter().copied().filter(|&b| b != byte::NULL).collect();
        let pad = data.len() - kept.len();
        for (k, b) in std::iter::repeat_n(fill, pad).chain(kept).enumerate() {
            s.set_byte(f.start + k, b);
        }
    }
    match (shift, how) {
        (Shift::SignedNumeric, Exit::Minus) => s.set_byte(f.end() - 1, 0x60),
        (Shift::SignedNumeric, _) => s.set_byte(f.end() - 1, byte::BLANK),
        (_, Exit::Minus) => {
            // The low-order digit's zone becomes D: negative.
            let last = end - 1;
            let b = s.byte(last);
            if (0xF0..=0xF9).contains(&b) {
                s.set_byte(last, 0xD0 | (b & 0x0F));
            }
        }
        _ => {}
    }
    s.fields[i].ffw.set_mdt(true);
    if f.ffw.auto_enter() {
        return attention(s, aid::ENTER);
    }
    advance_from(s, Some(i));
    Outcome::default()
}

fn dup(s: &mut Screen) -> Outcome {
    let Some(i) = editable(s) else {
        return operator_error(s, err::PROTECTED);
    };
    if !s.fields[i].ffw.dup_enabled() {
        return operator_error(s, err::DUP);
    }
    let end = data_end(&s.fields[i]);
    for a in s.cursor..end {
        s.set_byte(a, byte::DUP);
    }
    s.fields[i].ffw.set_mdt(true);
    advance_from(s, Some(i));
    Outcome::default()
}

fn erase_input(s: &mut Screen) {
    let spans: Vec<(usize, usize)> = s.input_fields().map(|(_, f)| (f.start, f.end())).collect();
    for (a, b) in spans {
        for i in a..b {
            s.set_byte(i, byte::NULL);
        }
    }
    for f in s.fields.iter_mut().filter(|f| !f.ffw.bypass()) {
        f.ffw.set_mdt(true);
    }
    s.cursor = s.home();
}

fn erase_eof(s: &mut Screen) -> Outcome {
    let Some(i) = editable(s) else {
        return operator_error(s, err::PROTECTED);
    };
    let end = data_end(&s.fields[i]);
    for a in s.cursor..end {
        s.set_byte(a, byte::NULL);
    }
    s.fields[i].ffw.set_mdt(true);
    Outcome::default()
}

fn delete(s: &mut Screen) -> Outcome {
    let Some(i) = editable(s) else {
        return operator_error(s, err::PROTECTED);
    };
    let end = data_end(&s.fields[i]);
    if s.cursor >= end {
        return operator_error(s, err::SIGN_POSITION);
    }
    for a in s.cursor..end - 1 {
        let next = s.byte(a + 1);
        s.set_byte(a, next);
    }
    s.set_byte(end - 1, byte::NULL);
    s.fields[i].ffw.set_mdt(true);
    Outcome::default()
}

/// Cursor left, staying in input fields: from a field's first position to
/// the last position of the previous input field.
fn backspace(s: &mut Screen) {
    match editable(s) {
        Some(i) if s.cursor > s.fields[i].start => s.cursor -= 1,
        Some(i) => {
            let start = s.fields[i].start;
            let prev = s
                .input_fields()
                .filter(|(_, f)| f.start < start)
                .last()
                .or_else(|| s.input_fields().last())
                .map(|(_, f)| data_end(f) - 1);
            if let Some(p) = prev {
                s.cursor = p;
            }
        }
        None => {
            let _ = step(s, -1);
        }
    }
}

fn step(s: &mut Screen, by: isize) -> Outcome {
    let n = s.size() as isize;
    s.cursor = (s.cursor as isize + by).rem_euclid(n) as usize;
    Outcome::default()
}

fn new_line(s: &mut Screen) {
    let row_start = (s.cursor / s.cols() + 1) * s.cols();
    let next = s
        .input_fields()
        .find(|(_, f)| f.start >= row_start)
        .or_else(|| s.input_fields().next())
        .map(|(_, f)| f.start);
    if let Some(a) = next {
        s.cursor = a;
    }
}

fn tab(s: &mut Screen) -> Outcome {
    if let Some(i) = editable(s) {
        let f = &s.fields[i];
        let data: Vec<u8> = (f.start..data_end(f)).map(|a| s.byte(a)).collect();
        let filled = data.iter().filter(|&&b| b != byte::NULL).count();
        if f.ffw.adjust() == Adjust::MandatoryFill && filled > 0 && filled < data.len() {
            return operator_error(s, err::MANDATORY_FILL);
        }
        advance_from(s, Some(i));
    } else {
        advance_from(s, None);
    }
    Outcome::default()
}

fn back_tab(s: &mut Screen) {
    if let Some(i) = editable(s)
        && s.cursor > s.fields[i].start
    {
        s.cursor = s.fields[i].start;
        return;
    }
    let at = s.cursor;
    let prev = s
        .input_fields()
        .filter(|(_, f)| f.start < at && !f.contains(at))
        .last()
        .or_else(|| s.input_fields().last())
        .map(|(_, f)| f.start);
    if let Some(a) = prev {
        s.cursor = a;
    }
}

fn reset(s: &mut Screen) {
    if s.keyboard == Keyboard::HostError
        && let Some(line) = s.error_saved.take()
    {
        let start = s.error_row * s.cols();
        for (k, b) in line.into_iter().enumerate() {
            s.set_byte(start + k, b);
        }
        s.recompute_attrs();
    }
    if s.keyboard.in_error() {
        s.keyboard = Keyboard::Open;
    }
    s.insert_mode = false;
}

/// System Request and Attention: a record with the header flag alone.
fn signal(s: &mut Screen, f: u16) -> Outcome {
    s.keyboard = Keyboard::System;
    s.insert_mode = false;
    s.pending_aid = None;
    Outcome::send(Record {
        reserved: 0,
        flags: f,
        opcode: opcode::NO_OP,
        data: Vec::new(),
    })
}

/// Help during an operator error: the HLP flag with the error code.
fn help_in_error(s: &mut Screen) -> Outcome {
    let code = match s.keyboard {
        Keyboard::OperatorError(c) => c,
        _ => 0,
    };
    let bcd = ((code / 1000 % 10) << 12)
        | ((code / 100 % 10) << 8)
        | ((code / 10 % 10) << 4)
        | (code % 10);
    Outcome::send(Record {
        reserved: 0,
        flags: flag::HLP,
        opcode: opcode::NO_OP,
        data: bcd.to_be_bytes().to_vec(),
    })
}

/// An attention key: checks mandatory-entry fields, then sends the read
/// reply, or holds the key until the host reads.
fn attention(s: &mut Screen, a: u8) -> Outcome {
    if inbound::returns_data(s, a) {
        let missing = s
            .input_fields()
            .find(|(_, f)| f.ffw.mandatory_enter() && !f.ffw.mdt())
            .map(|(_, f)| f.start);
        if let Some(at) = missing {
            s.cursor = at;
            return operator_error(s, err::MANDATORY_ENTER);
        }
    }
    s.keyboard = Keyboard::System;
    s.insert_mode = false;
    match s.pending_read.take() {
        Some(read) => Outcome::send(Record::new(
            opcode::PUT_GET,
            inbound::read_reply(s, read, a),
        )),
        None => {
            s.pending_aid = Some(a);
            Outcome::default()
        }
    }
}

#[cfg(test)]
mod tests;
