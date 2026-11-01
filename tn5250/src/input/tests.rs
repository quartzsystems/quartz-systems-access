// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

use super::*;
use crate::screen::{Ffw, Read, attr};

/// FFW for a plain input field.
const PLAIN: u16 = 0x4000;

/// A screen with input fields of `(row, col, len, ffw)` (1-based row and
/// column of the first data position), the keyboard open and a Read MDT
/// Fields outstanding.
fn screen(fields: &[(usize, usize, usize, u16)]) -> Screen {
    let mut s = Screen::default_24x80();
    for &(r, c, len, ffw) in fields {
        let start = s.addr(r - 1, c - 1);
        s.set_byte(start - 1, attr::GREEN_UL);
        s.define_field(Field {
            start,
            len,
            ffw: Ffw(ffw),
            fcw: vec![],
            attr: attr::GREEN_UL,
        });
    }
    s.recompute_attrs();
    s.keyboard = Keyboard::Open;
    s.pending_read = Some(Read::MdtFields);
    s.cursor = s.home();
    s
}

fn typed(s: &mut Screen, text: &str) -> Vec<Outcome> {
    text.chars().map(|c| key(s, Key::Char(c))).collect()
}

fn text(s: &Screen, i: usize) -> String {
    s.field_data(&s.fields[i])
        .iter()
        .map(|&b| s.char_of(b))
        .collect()
}

#[test]
fn typing_fills_the_field_and_sets_mdt() {
    let mut s = screen(&[(6, 53, 10, PLAIN), (7, 53, 10, PLAIN)]);
    typed(&mut s, "QSECOFR");
    assert_eq!(text(&s, 0), "QSECOFR   ");
    assert!(s.fields[0].ffw.mdt() && !s.fields[1].ffw.mdt());
    assert_eq!(s.cursor, s.addr(5, 59));
}

#[test]
fn a_full_field_advances_to_the_next() {
    let mut s = screen(&[(6, 53, 3, PLAIN), (7, 53, 3, PLAIN)]);
    typed(&mut s, "ABCD");
    assert_eq!(text(&s, 0), "ABC");
    assert_eq!(text(&s, 1), "D  ");
}

#[test]
fn cursor_progression_word_chooses_the_next_field() {
    let mut s = screen(&[(2, 2, 2, PLAIN), (3, 2, 2, PLAIN), (4, 2, 2, PLAIN)]);
    s.fields[0].fcw = vec![0x8003];
    typed(&mut s, "AB");
    assert_eq!(s.cursor, s.fields[2].start);
}

#[test]
fn typing_outside_a_field_is_an_error_until_reset() {
    let mut s = screen(&[(6, 53, 10, PLAIN)]);
    s.cursor = 0;
    let o = key(&mut s, Key::Char('A'));
    assert_eq!(o.error, Some(err::PROTECTED));
    assert_eq!(s.keyboard, Keyboard::OperatorError(5));
    // Locked: further keys do nothing.
    s.cursor = s.fields[0].start;
    assert_eq!(key(&mut s, Key::Char('A')), Outcome::default());
    assert_eq!(text(&s, 0).trim(), "");
    key(&mut s, Key::Reset);
    assert!(s.keyboard.is_open());
    typed(&mut s, "A");
    assert_eq!(text(&s, 0).trim(), "A");
}

#[test]
fn bypass_fields_cannot_be_typed_into_or_tabbed_to() {
    let mut s = screen(&[(2, 2, 5, 0x6000), (3, 2, 5, PLAIN)]);
    assert_eq!(s.cursor, s.fields[1].start, "home skips the bypass field");
    s.cursor = s.fields[0].start;
    assert_eq!(key(&mut s, Key::Char('A')).error, Some(err::PROTECTED));
    key(&mut s, Key::Reset);
    key(&mut s, Key::Tab);
    assert_eq!(s.cursor, s.fields[1].start);
    key(&mut s, Key::Tab);
    assert_eq!(s.cursor, s.fields[1].start, "wraps to the only input field");
}

#[test]
fn auto_enter_sends_when_the_field_fills() {
    let mut s = screen(&[(5, 10, 2, PLAIN | Ffw::AUTO_ENTER)]);
    let o = typed(&mut s, "12");
    let sent = &o[1].send;
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].data[2], aid::ENTER);
    assert_eq!(&sent[0].data[3..], &[0x11, 5, 10, 0xF1, 0xF2]);
    assert_eq!(s.keyboard, Keyboard::System);
}

#[test]
fn mandatory_enter_blocks_enter_until_typed() {
    let mut s = screen(&[(5, 10, 4, PLAIN), (6, 10, 4, PLAIN | Ffw::MANDATORY_ENTER)]);
    let o = key(&mut s, Key::Enter);
    assert_eq!(o.error, Some(err::MANDATORY_ENTER));
    assert_eq!(s.cursor, s.fields[1].start);
    key(&mut s, Key::Reset);
    typed(&mut s, "X");
    assert_eq!(key(&mut s, Key::Enter).send.len(), 1);
    // A key that sends no data is not checked.
    let mut s = screen(&[(6, 10, 4, PLAIN | Ffw::MANDATORY_ENTER)]);
    assert_eq!(key(&mut s, Key::Clear).send.len(), 1);
}

#[test]
fn mandatory_fill_on_exit() {
    let mut s = screen(&[(5, 10, 4, PLAIN | 0x0007), (6, 10, 4, PLAIN)]);
    typed(&mut s, "AB");
    assert_eq!(key(&mut s, Key::FieldExit).error, Some(err::MANDATORY_FILL));
    key(&mut s, Key::Reset);
    assert_eq!(key(&mut s, Key::Tab).error, Some(err::MANDATORY_FILL));
    key(&mut s, Key::Reset);
    s.cursor = s.fields[0].start + 2;
    typed(&mut s, "CD");
    assert_eq!(s.cursor, s.fields[1].start);
}

#[test]
fn numeric_only_digits_only_and_alpha_only() {
    let mut s = screen(&[(5, 10, 6, 0x4300)]);
    typed(&mut s, "-1,5.");
    assert_eq!(text(&s, 0), "-1,5. ");
    assert_eq!(key(&mut s, Key::Char('A')).error, Some(err::NUMERIC_ONLY));

    let mut s = screen(&[(5, 10, 6, 0x4500)]);
    typed(&mut s, "42");
    assert_eq!(key(&mut s, Key::Char('-')).error, Some(err::DIGITS_ONLY));

    let mut s = screen(&[(5, 10, 6, 0x4100)]);
    typed(&mut s, "Ab-");
    assert_eq!(key(&mut s, Key::Char('1')).error, Some(err::ALPHA_ONLY));
}

#[test]
fn signed_numeric_keeps_the_sign_position() {
    let mut s = screen(&[(5, 10, 5, 0x4700), (6, 10, 3, PLAIN)]);
    typed(&mut s, "12");
    key(&mut s, Key::FieldMinus);
    // Four digit positions, right-adjusted? No adjust set: as typed.
    assert_eq!(text(&s, 0), "12  -");
    assert_eq!(s.cursor, s.fields[1].start);
    // Typing four digits moves on before the sign position.
    let mut s = screen(&[(5, 10, 5, 0x4700), (6, 10, 3, PLAIN)]);
    typed(&mut s, "1234");
    assert_eq!(s.cursor, s.fields[1].start);
    s.cursor = s.fields[0].start + 4;
    assert_eq!(key(&mut s, Key::Char('5')).error, Some(err::SIGN_POSITION));
    // Field+ leaves the sign blank.
    let mut s = screen(&[(5, 10, 5, 0x4700)]);
    typed(&mut s, "7");
    key(&mut s, Key::FieldPlus);
    assert_eq!(text(&s, 0), "7    ");
}

#[test]
fn field_minus_in_numeric_only_marks_the_last_digit() {
    let mut s = screen(&[(5, 10, 4, 0x4300 | 0x0005)]);
    typed(&mut s, "12");
    key(&mut s, Key::FieldMinus);
    // Right adjusted with zeros, then the low digit's zone made D.
    assert_eq!(s.field_data(&s.fields[0]), &[0xF0, 0xF0, 0xF1, 0xD2]);
    let mut s = screen(&[(5, 10, 4, PLAIN)]);
    assert_eq!(key(&mut s, Key::FieldMinus).error, Some(err::FIELD_MINUS));
}

#[test]
fn right_adjust_blank_and_zero_fill() {
    let mut s = screen(&[(5, 10, 5, 0x4006), (6, 10, 5, 0x4005)]);
    typed(&mut s, "AB");
    key(&mut s, Key::FieldExit);
    assert_eq!(text(&s, 0), "   AB");
    typed(&mut s, "7");
    key(&mut s, Key::FieldExit);
    assert_eq!(text(&s, 1), "00007");
}

#[test]
fn field_exit_clears_to_the_end_and_advances() {
    let mut s = screen(&[(5, 10, 6, PLAIN), (6, 10, 3, PLAIN)]);
    typed(&mut s, "ABCDEF");
    s.cursor = s.fields[0].start + 2;
    key(&mut s, Key::FieldExit);
    assert_eq!(text(&s, 0), "AB    ");
    assert_eq!(s.cursor, s.fields[1].start);
    s.cursor = 0;
    assert_eq!(key(&mut s, Key::FieldExit).error, Some(err::PROTECTED));
}

#[test]
fn field_exit_required() {
    let mut s = screen(&[(5, 10, 3, PLAIN | Ffw::EXIT_REQUIRED), (6, 10, 3, PLAIN)]);
    typed(&mut s, "ABC");
    assert_eq!(
        s.cursor,
        s.fields[0].start + 2,
        "stays on the last position"
    );
    assert_eq!(key(&mut s, Key::Char('D')).error, Some(err::EXIT_REQUIRED));
    key(&mut s, Key::Reset);
    key(&mut s, Key::FieldExit);
    assert_eq!(text(&s, 0), "ABC");
    assert_eq!(s.cursor, s.fields[1].start);
}

#[test]
fn monocase_upper_cases() {
    let mut s = screen(&[(5, 10, 5, PLAIN | Ffw::MONOCASE)]);
    typed(&mut s, "wrk");
    assert_eq!(text(&s, 0), "WRK  ");
}

#[test]
fn insert_mode_shifts_right_and_needs_room() {
    let mut s = screen(&[(5, 10, 4, PLAIN)]);
    typed(&mut s, "AC");
    s.cursor = s.fields[0].start + 1;
    key(&mut s, Key::Insert);
    typed(&mut s, "B");
    assert_eq!(text(&s, 0), "ABC ");
    typed(&mut s, "X");
    assert_eq!(text(&s, 0), "ABXC");
    assert_eq!(key(&mut s, Key::Char('Y')).error, Some(err::NO_ROOM));
}

#[test]
fn delete_erase_eof_and_erase_input() {
    let mut s = screen(&[(5, 10, 5, PLAIN), (6, 10, 5, PLAIN)]);
    typed(&mut s, "ABCDE");
    typed(&mut s, "XY");
    s.cursor = s.fields[0].start + 1;
    key(&mut s, Key::Delete);
    assert_eq!(text(&s, 0), "ACDE ");
    key(&mut s, Key::EraseEof);
    assert_eq!(text(&s, 0), "A    ");
    key(&mut s, Key::EraseInput);
    assert_eq!(text(&s, 0), "     ");
    assert_eq!(text(&s, 1), "     ");
    assert_eq!(s.cursor, s.fields[0].start);
    assert!(
        s.fields.iter().all(|f| f.ffw.mdt()),
        "so the host sees them empty"
    );
}

#[test]
fn dup_fills_and_needs_dup_enable() {
    let mut s = screen(&[(5, 10, 4, PLAIN | Ffw::DUP), (6, 10, 4, PLAIN)]);
    typed(&mut s, "A");
    key(&mut s, Key::Dup);
    assert_eq!(s.field_data(&s.fields[0]), &[0xC1, 0x1C, 0x1C, 0x1C]);
    assert_eq!(text(&s, 0), "A***");
    assert_eq!(key(&mut s, Key::Dup).error, Some(err::DUP));
}

#[test]
fn movement_keys() {
    let mut s = screen(&[(5, 10, 4, PLAIN), (5, 20, 4, PLAIN), (8, 2, 4, PLAIN)]);
    key(&mut s, Key::Tab);
    assert_eq!(s.cursor, s.fields[1].start);
    key(&mut s, Key::Right);
    key(&mut s, Key::BackTab);
    assert_eq!(s.cursor, s.fields[1].start, "back to this field's start");
    key(&mut s, Key::BackTab);
    assert_eq!(s.cursor, s.fields[0].start);
    key(&mut s, Key::BackTab);
    assert_eq!(s.cursor, s.fields[2].start, "wraps backwards");
    key(&mut s, Key::Home);
    assert_eq!(s.cursor, s.fields[0].start);
    key(&mut s, Key::NewLine);
    assert_eq!(s.cursor, s.fields[2].start);
    key(&mut s, Key::NewLine);
    assert_eq!(s.cursor, s.fields[0].start, "wraps to the first field");
    key(&mut s, Key::Up);
    assert_eq!(s.rc(s.cursor), (3, 9));
    s.cursor = 0;
    key(&mut s, Key::Left);
    assert_eq!(s.cursor, s.size() - 1);
    key(&mut s, Key::Down);
    assert_eq!(s.rc(s.cursor), (0, 79));
}

#[test]
fn backspace_moves_left_within_and_between_fields() {
    let mut s = screen(&[(5, 10, 4, PLAIN), (6, 10, 4, PLAIN)]);
    s.cursor = s.fields[1].start + 1;
    key(&mut s, Key::Backspace);
    assert_eq!(s.cursor, s.fields[1].start);
    key(&mut s, Key::Backspace);
    assert_eq!(s.cursor, s.fields[0].start + 3);
}

#[test]
fn attention_keys_send_their_aid() {
    for (k, a) in [
        (Key::Enter, aid::ENTER),
        (Key::F(3), 0x33),
        (Key::F(24), 0xBC),
        (Key::PageUp, aid::ROLL_DOWN),
        (Key::PageDown, aid::ROLL_UP),
        (Key::Clear, aid::CLEAR),
        (Key::Help, aid::HELP),
        (Key::Print, aid::PRINT),
        (Key::RecordBackspace, aid::RECORD_BACKSPACE),
    ] {
        let mut s = screen(&[(6, 53, 10, PLAIN)]);
        let o = key(&mut s, k);
        assert_eq!(o.send.len(), 1, "{k:?}");
        let r = &o.send[0];
        assert_eq!((r.opcode, r.data[2]), (opcode::PUT_GET, a), "{k:?}");
        assert_eq!(&r.data[..2], &[6, 53]);
        assert_eq!(s.keyboard, Keyboard::System);
        assert!(s.pending_read.is_none());
        // Locked now: another key does nothing.
        assert!(key(&mut s, Key::Enter).send.is_empty());
    }
}

#[test]
fn an_attention_key_before_the_read_is_held() {
    let mut s = screen(&[(6, 53, 10, PLAIN)]);
    s.pending_read = None;
    typed(&mut s, "A");
    assert!(key(&mut s, Key::Enter).send.is_empty());
    assert_eq!(s.pending_aid, Some(aid::ENTER));
}

#[test]
fn system_request_and_attention_are_header_flags() {
    let mut s = screen(&[]);
    let o = key(&mut s, Key::SysReq);
    assert_eq!(
        o.send[0].encode(),
        [0, 10, 0x12, 0xA0, 0, 0, 4, 0x04, 0x00, 0]
    );
    let o = key(&mut s, Key::Attention);
    assert_eq!(
        o.send[0].encode(),
        [0, 10, 0x12, 0xA0, 0, 0, 4, 0x40, 0x00, 0]
    );
}

#[test]
fn help_in_an_error_sends_the_code() {
    let mut s = screen(&[(6, 53, 10, 0x4500)]);
    key(&mut s, Key::Char('x'));
    let o = key(&mut s, Key::Help);
    assert_eq!(o.send[0].flags, flag::HLP);
    assert_eq!(o.send[0].data, [0x00, 0x10]);
}

#[test]
fn reset_restores_the_error_line_after_a_host_error() {
    let mut s = screen(&[(6, 53, 10, PLAIN)]);
    let row = s.error_row * s.cols();
    s.set_byte(row, 0xC1);
    s.error_saved = Some(s.bytes()[row..row + 80].to_vec());
    s.set_byte(row, 0xC2);
    s.keyboard = Keyboard::HostError;
    key(&mut s, Key::Reset);
    assert_eq!(s.byte(row), 0xC1);
    assert!(s.keyboard.is_open());
    assert!(s.error_saved.is_none());
}

#[test]
fn paste_fills_input_fields_and_skips_protected_text() {
    let mut s = screen(&[(5, 10, 3, PLAIN), (5, 20, 3, PLAIN), (6, 10, 5, PLAIN)]);
    let o = paste(&mut s, "ABCDEF\nline2");
    assert!(o.error.is_none() && o.send.is_empty());
    assert_eq!(text(&s, 0), "ABC");
    assert_eq!(text(&s, 1), "DEF");
    assert_eq!(text(&s, 2), "line2");
    // Stops at the end of the screen's input fields instead of wrapping.
    let mut s = screen(&[(5, 10, 3, PLAIN)]);
    paste(&mut s, "ABCDEFGH");
    assert_eq!(text(&s, 0), "ABC");
}

#[test]
fn error_texts_exist() {
    for c in [5, 7, 8, 9, 10, 11, 12, 14, 16, 18, 19, 27] {
        assert_ne!(error_text(c), error_text(9999), "{c}");
    }
}
