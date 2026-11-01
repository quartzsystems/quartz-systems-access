// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

use super::*;
use crate::codepage;
use crate::screen::Shift;

fn screen() -> Screen {
    Screen::default_24x80()
}

fn wide() -> Screen {
    Screen::new(ScreenSize::S27x132, codepage::default_page())
}

fn put_get(data: &[u8]) -> Record {
    Record::new(opcode::PUT_GET, data.to_vec())
}

fn run(s: &mut Screen, data: &[u8]) -> Applied {
    apply(s, &put_get(data))
}

fn ebcdic(t: &str) -> Vec<u8> {
    codepage::default_page().encode(t)
}

/// `ESC WTD 00 00` followed by `body`.
fn wtd(body: &[u8]) -> Vec<u8> {
    let mut v = vec![ESC, cmd::WRITE_TO_DISPLAY, 0x00, 0x00];
    v.extend_from_slice(body);
    v
}

fn row(s: &Screen, r: usize) -> String {
    s.text_rows()[r - 1].clone()
}

fn assert_nr(a: &Applied, code: u32) {
    assert_eq!(a.errors.len(), 1, "{a:?}");
    assert_eq!(a.errors[0].code, code, "{}", a.errors[0]);
    let r = a.replies.last().unwrap();
    assert_eq!(r.flags, flag::ERR);
    assert_eq!(r.data, code.to_be_bytes());
}

#[test]
fn clear_unit() {
    let mut s = wide();
    run(&mut s, &[ESC, cmd::CLEAR_UNIT_ALTERNATE, 0x00]);
    assert_eq!((s.rows(), s.cols()), (27, 132));
    let mut body = vec![order::SBA, 1, 1];
    body.extend(ebcdic("X"));
    run(&mut s, &wtd(&body));
    run(&mut s, &[ESC, cmd::CLEAR_UNIT]);
    assert_eq!((s.rows(), s.cols()), (24, 80));
    assert!(s.bytes().iter().all(|&b| b == 0));
    assert_eq!(s.cursor, 0);
}

#[test]
fn clear_unit_alternate_needs_a_wide_display() {
    let mut s = screen();
    let a = run(&mut s, &[ESC, cmd::CLEAR_UNIT_ALTERNATE, 0x00]);
    assert_nr(&a, nr::CLEAR_UNIT_ALTERNATE_NOT_VALID);
    assert_eq!(s.cols(), 80);
    // 80: clear, same size.
    assert!(
        run(&mut s, &[ESC, cmd::CLEAR_UNIT_ALTERNATE, 0x80])
            .errors
            .is_empty()
    );
    let a = run(&mut wide(), &[ESC, cmd::CLEAR_UNIT_ALTERNATE, 0x42]);
    assert_nr(&a, nr::CLEAR_UNIT_ALTERNATE_NOT_VALID);
}

#[test]
fn clear_format_table_keeps_the_text() {
    let mut s = screen();
    let mut body = vec![
        order::SBA,
        2,
        1,
        order::SF,
        0x40,
        0x00,
        attr::GREEN_UL,
        0,
        5,
    ];
    body.extend(ebcdic("ABC"));
    run(&mut s, &wtd(&body));
    assert_eq!(s.fields.len(), 1);
    run(&mut s, &[ESC, cmd::CLEAR_FORMAT_TABLE]);
    assert!(s.fields.is_empty());
    assert!(row(&s, 2).starts_with(" ABC"));
}

#[test]
fn write_to_display_with_set_buffer_address() {
    let mut s = screen();
    let mut body = vec![order::SBA, 1, 36, attr::WHITE];
    body.extend(ebcdic("Sign On"));
    body.push(attr::GREEN);
    let a = run(&mut s, &wtd(&body));
    assert!(a.errors.is_empty());
    assert_eq!(&row(&s, 1)[35..43], " Sign On");
    let c = s.cell(0, 36);
    assert_eq!((c.ch, c.attr), ('S', attr::WHITE));
}

#[test]
fn data_wraps_at_the_end_of_the_screen() {
    let mut s = screen();
    let mut body = vec![order::SBA, 24, 80];
    body.extend(ebcdic("AB"));
    run(&mut s, &wtd(&body));
    assert_eq!(s.byte(1919), 0xC1);
    assert_eq!(s.byte(0), 0xC2);
}

#[test]
fn bad_addresses_are_refused() {
    for (r, c) in [(0, 1), (25, 1), (1, 0), (1, 81)] {
        let mut s = screen();
        assert_nr(
            &run(&mut s, &wtd(&[order::SBA, r, c])),
            nr::ADDRESS_NOT_VALID,
        );
    }
}

#[test]
fn repeat_to_address() {
    let mut s = screen();
    let star = ebcdic("*")[0];
    run(&mut s, &wtd(&[order::SBA, 2, 1, order::RA, 2, 10, star]));
    assert_eq!(&row(&s, 2)[..11], "********** ");
    // To an address before the current one: refused.
    let a = run(&mut s, &wtd(&[order::SBA, 3, 5, order::RA, 3, 1, star]));
    assert_nr(&a, nr::REPEAT_ADDRESS_BEFORE_CURRENT);
}

#[test]
fn repeat_to_the_last_cell_stops() {
    let mut s = screen();
    let a = run(&mut s, &wtd(&[order::SBA, 24, 79, order::RA, 24, 80, 0xC1]));
    assert!(a.errors.is_empty());
    assert_eq!(&row(&s, 24)[78..], "AA");
    assert_eq!(s.byte(0), 0);
}

#[test]
fn erase_to_address() {
    let mut s = screen();
    let mut body = vec![order::SBA, 1, 1];
    body.extend(ebcdic("ABCDEFG"));
    body.extend_from_slice(&[order::SBA, 1, 2, order::EA, 1, 4, 2, 0xFF]);
    run(&mut s, &wtd(&body));
    assert_eq!(&row(&s, 1)[..7], "A   EFG");
    let a = run(&mut s, &wtd(&[order::SBA, 1, 1, order::EA, 1, 4, 9]));
    assert_nr(&a, nr::BAD_LENGTH);
}

#[test]
fn transparent_data_writes_control_bytes_as_data() {
    let mut s = screen();
    let a = run(
        &mut s,
        &wtd(&[order::SBA, 1, 1, order::TD, 0, 3, 0x04, 0x11, 0xC1]),
    );
    assert!(a.errors.is_empty());
    assert_eq!(&s.bytes()[..3], &[0x04, 0x11, 0xC1]);
    let a = run(&mut s, &wtd(&[order::TD, 0, 9, 0xC1]));
    assert_nr(&a, nr::PREMATURE_END);
}

#[test]
fn insert_cursor_and_move_cursor() {
    let mut s = screen();
    run(&mut s, &wtd(&[order::IC, 5, 10]));
    assert_eq!(s.insert_cursor, Some(s.addr(4, 9)));
    assert_eq!(
        s.cursor, 0,
        "Insert Cursor moves the cursor only when the keyboard opens"
    );
    run(&mut s, &[ESC, cmd::READ_MDT_FIELDS, 0, 0]);
    assert_eq!(s.cursor, s.addr(4, 9));
    run(&mut s, &wtd(&[order::MC, 7, 1]));
    assert_eq!(s.cursor, s.addr(6, 0));
}

#[test]
fn start_of_header() {
    let mut s = screen();
    let a = run(
        &mut s,
        &wtd(&[order::SOH, 7, 0x00, 0x00, 0x02, 22, 0x00, 0x00, 0x04]),
    );
    assert!(a.errors.is_empty());
    assert_eq!(s.error_row, 21);
    assert_eq!(s.header.resequence, 2);
    assert_eq!(s.header.no_data_keys, 0x04, "F3");
    // Error row 0: the last row.
    run(&mut s, &wtd(&[order::SOH, 4, 0, 0, 0, 0]));
    assert_eq!(s.error_row, 23);
    assert_nr(&run(&mut s, &wtd(&[order::SOH, 9])), nr::BAD_LENGTH);
}

#[test]
fn start_of_field_with_ffw_and_fcw() {
    let mut s = screen();
    // FFW: monocase, mandatory enter, alpha only; FCW: cursor progression to 2.
    let body = [
        order::SBA,
        6,
        52,
        order::SF,
        0x41,
        0x28,
        0x80,
        0x02,
        attr::GREEN_UL,
        0x00,
        0x0A,
    ];
    let a = run(&mut s, &wtd(&body));
    assert!(a.errors.is_empty(), "{a:?}");
    let f = &s.fields[0];
    assert_eq!((f.start, f.len), (s.addr(5, 52), 10));
    assert_eq!(f.ffw.shift(), Shift::AlphaOnly);
    assert!(f.ffw.monocase() && f.ffw.mandatory_enter());
    assert_eq!(f.fcw, [0x8002]);
    assert_eq!(f.progression(), Some(2));
    assert_eq!(s.byte(s.addr(5, 51)), attr::GREEN_UL);
    // The data written next goes into the field.
    let mut s2 = screen();
    let mut body = vec![
        order::SBA,
        2,
        1,
        order::SF,
        0x40,
        0x00,
        attr::GREEN_UL,
        0,
        4,
    ];
    body.extend(ebcdic("QS"));
    run(&mut s2, &wtd(&body));
    assert_eq!(s2.field_data(&s2.fields[0]), &[0xD8, 0xE2, 0, 0]);
}

#[test]
fn start_of_field_without_ffw_is_output_only() {
    let mut s = screen();
    run(
        &mut s,
        &wtd(&[order::SBA, 3, 1, order::SF, attr::WHITE, 0, 10]),
    );
    assert!(s.fields.is_empty());
    assert_eq!(s.byte(s.addr(2, 0)), attr::WHITE);
}

#[test]
fn start_of_field_errors() {
    let mut s = screen();
    let a = run(
        &mut s,
        &wtd(&[order::SBA, 24, 75, order::SF, 0x40, 0, attr::GREEN, 0, 10]),
    );
    assert_nr(&a, nr::FIELD_PAST_END);
    let a = run(
        &mut s,
        &wtd(&[order::SBA, 1, 1, order::SF, 0x40, 0, 0x41, 0, 10]),
    );
    assert_nr(&a, nr::ORDER_NOT_VALID);
    let a = run(&mut s, &wtd(&[order::SBA, 1, 1, order::SF, 0x40]));
    assert_nr(&a, nr::PREMATURE_END);
}

#[test]
fn a_field_spanning_rows() {
    let mut s = screen();
    let body = [
        order::SBA,
        20,
        7,
        order::SF,
        0x40,
        0x00,
        attr::GREEN_UL,
        0,
        153,
    ];
    run(&mut s, &wtd(&body));
    let f = &s.fields[0];
    assert_eq!(s.rc(f.start), (19, 7));
    assert_eq!(s.rc(f.end() - 1), (20, 79));
}

#[test]
fn write_extended_attribute_and_display_structured_field_are_skipped() {
    let mut s = screen();
    let mut body = vec![order::SBA, 1, 1, order::WEA, 0x03, 0x22];
    // A 6-byte display structured field (create window, say).
    body.extend_from_slice(&[order::WDSF, 0x00, 0x06, 0xD9, 0x51, 0x80, 0x00]);
    body.extend(ebcdic("OK"));
    let a = run(&mut s, &wtd(&body));
    assert!(a.errors.is_empty(), "{a:?}");
    assert!(row(&s, 1).starts_with("OK"));
}

#[test]
fn an_unknown_order_is_refused() {
    let mut s = screen();
    assert_nr(&run(&mut s, &wtd(&[0x07])), nr::ORDER_NOT_VALID);
}

#[test]
fn cc1_nulls_and_resets_fields() {
    let mut s = screen();
    let body = [
        order::SBA,
        2,
        1,
        order::SF,
        0x48,
        0x00,
        attr::GREEN_UL,
        0,
        3,
        0xC1,
        0xC2,
        0xC3,
        order::SBA,
        3,
        1,
        order::SF,
        0x40,
        0x00,
        attr::GREEN_UL,
        0,
        3,
        0xC4,
        0xC5,
        0xC6,
    ];
    run(&mut s, &wtd(&body));
    assert!(s.fields[0].ffw.mdt() && !s.fields[1].ffw.mdt());
    // 0x80: null the modified input fields.
    run(&mut s, &[ESC, cmd::WRITE_TO_DISPLAY, 0x80, 0x00]);
    assert_eq!(s.field_data(&s.fields[0]), &[0, 0, 0]);
    assert_eq!(s.field_data(&s.fields[1]), &[0xC4, 0xC5, 0xC6]);
    assert!(s.fields[0].ffw.mdt(), "nulling leaves MDT alone");
    // 0x40: reset MDT on input fields.
    run(&mut s, &[ESC, cmd::WRITE_TO_DISPLAY, 0x40, 0x00]);
    assert!(!s.fields[0].ffw.mdt());
    assert_eq!(s.keyboard, Keyboard::System);
    // 0xE0: null every input field.
    run(&mut s, &[ESC, cmd::WRITE_TO_DISPLAY, 0xE0, 0x00]);
    assert_eq!(s.field_data(&s.fields[1]), &[0, 0, 0]);
}

#[test]
fn cc2_unlock_alarm_and_message_light() {
    let mut s = screen();
    run(
        &mut s,
        &[ESC, cmd::WRITE_TO_DISPLAY, 0x00, 0x08 | 0x04 | 0x01],
    );
    assert_eq!(s.keyboard, Keyboard::Open);
    assert!(s.alarm && s.message_waiting);
    run(&mut s, &[ESC, cmd::WRITE_TO_DISPLAY, 0x00, 0x02]);
    assert!(!s.message_waiting);
}

#[test]
fn write_error_code_keeps_the_line_and_inhibits_input() {
    let mut s = screen();
    let mut body = vec![order::SBA, 24, 1];
    body.extend(ebcdic("F3=Exit"));
    run(&mut s, &wtd(&body));
    run(&mut s, &[ESC, cmd::READ_MDT_FIELDS, 0, 0]);
    let mut wec = vec![ESC, cmd::WRITE_ERROR_CODE, attr::WHITE];
    wec.extend(ebcdic("Value not valid."));
    wec.extend_from_slice(&[attr::GREEN, order::IC, 2, 5]);
    run(&mut s, &wec);
    assert!(row(&s, 24).starts_with(" Value not valid."));
    assert_eq!(s.keyboard, Keyboard::HostError);
    assert_eq!(s.cursor, s.addr(1, 4));
    let saved = s.error_saved.clone().unwrap();
    assert_eq!(&saved[..7], &ebcdic("F3=Exit")[..]);
    // A second error keeps the first saved line.
    run(&mut s, &[ESC, cmd::WRITE_ERROR_CODE, 0xC1]);
    assert_eq!(s.error_saved.as_ref().unwrap(), &saved);
}

#[test]
fn write_error_code_to_window_starts_at_its_column() {
    let mut s = screen();
    let mut wec = vec![ESC, cmd::WRITE_ERROR_CODE_TO_WINDOW, 10, 60];
    wec.extend(ebcdic("Bad"));
    run(&mut s, &wec);
    assert_eq!(&row(&s, 24)[9..12], "Bad");
}

#[test]
fn read_commands_open_the_keyboard_and_place_the_cursor() {
    for (c, want) in [
        (cmd::READ_INPUT_FIELDS, Read::InputFields),
        (cmd::READ_MDT_FIELDS, Read::MdtFields),
        (cmd::READ_MDT_ALTERNATE, Read::MdtAlternate),
    ] {
        let mut s = screen();
        let mut data = vec![ESC, cmd::CLEAR_UNIT];
        data.extend(wtd(&[
            order::SOH,
            7,
            0,
            0,
            0,
            24,
            0,
            0,
            0,
            order::SBA,
            3,
            1,
            order::SF,
            0x60,
            0,
            attr::WHITE,
            0,
            5,
            order::SBA,
            6,
            53,
            order::SF,
            0x40,
            0,
            attr::GREEN_UL,
            0,
            10,
        ]));
        data.extend_from_slice(&[ESC, c, 0x00, 0x00]);
        let a = run(&mut s, &data);
        assert!(a.errors.is_empty() && a.replies.is_empty(), "{a:?}");
        assert_eq!(s.pending_read, Some(want));
        assert_eq!(s.keyboard, Keyboard::Open);
        assert_eq!(s.cursor, s.addr(5, 53), "first input field, bypass skipped");
    }
}

#[test]
fn read_screen_returns_the_buffer() {
    let mut s = screen();
    run(&mut s, &wtd(&[order::SBA, 1, 1, attr::WHITE, 0xC1]));
    let a = run(&mut s, &[ESC, cmd::READ_SCREEN]);
    let r = &a.replies[0];
    assert_eq!(r.opcode, opcode::READ_SCREEN);
    assert_eq!(r.data.len(), 1920);
    assert_eq!(&r.data[..2], &[attr::WHITE, 0xC1]);
    // The opcode alone asks too.
    let a = apply(&mut s, &Record::new(opcode::READ_SCREEN, vec![]));
    assert_eq!(a.replies[0].data.len(), 1920);
}

#[test]
fn read_immediate_returns_fields_at_once() {
    let mut s = screen();
    let body = [
        order::SBA,
        2,
        1,
        order::SF,
        0x48,
        0,
        attr::GREEN_UL,
        0,
        2,
        0xC1,
    ];
    run(&mut s, &wtd(&body));
    let a = run(&mut s, &[ESC, cmd::READ_IMMEDIATE]);
    assert_eq!(a.replies[0].data, [1, 1, 0x00, 0xC1, 0x40]);
}

#[test]
fn save_and_restore_screen() {
    let mut s = screen();
    let mut body = vec![order::SBA, 5, 5, order::SF, 0x40, 0, attr::GREEN_UL, 0, 4];
    body.extend(ebcdic("SAVE"));
    run(&mut s, &wtd(&body));
    let before = s.snapshot();
    let a = run(&mut s, &[ESC, cmd::SAVE_SCREEN]);
    let saved = a.replies[0].clone();
    assert_eq!(saved.opcode, opcode::SAVE_SCREEN);
    assert_eq!(&saved.data[..2], &[ESC, cmd::RESTORE_SCREEN]);
    assert_eq!(s.saved.len(), 1);
    run(&mut s, &[ESC, cmd::CLEAR_UNIT]);
    // The host sends the bytes back in a Restore Screen record.
    let a = apply(&mut s, &Record::new(opcode::RESTORE_SCREEN, saved.data));
    assert!(a.errors.is_empty(), "{a:?}");
    assert_eq!(s.snapshot(), before);
    assert!(row(&s, 5).contains("SAVE"));
    // Partial save takes its parameters.
    let a = run(&mut s, &[ESC, cmd::SAVE_PARTIAL_SCREEN, 0, 1, 1, 24, 80]);
    assert_eq!(a.replies.len(), 1);
}

#[test]
fn restore_of_foreign_data_is_refused() {
    let mut s = screen();
    let a = run(&mut s, &[ESC, cmd::RESTORE_SCREEN, 0x01, 0x02]);
    assert_nr(&a, nr::RESTORE_DATA_NOT_VALID);
}

#[test]
fn roll_up_and_down() {
    let mut s = screen();
    for r in 1..=5u8 {
        let mut body = vec![order::SBA, r, 1];
        body.extend(ebcdic(&r.to_string()));
        run(&mut s, &wtd(&body));
    }
    // Up two lines within rows 2 to 5.
    run(&mut s, &[ESC, cmd::ROLL, 0x02, 2, 5]);
    let firsts: String = (1..=5)
        .map(|r| row(&s, r).chars().next().unwrap())
        .collect();
    assert_eq!(firsts, "145  ");
    // Down one line within rows 1 to 3.
    run(&mut s, &[ESC, cmd::ROLL, 0x81, 1, 3]);
    let firsts: String = (1..=5)
        .map(|r| row(&s, r).chars().next().unwrap())
        .collect();
    assert_eq!(firsts, " 14  ");
    assert_nr(
        &run(&mut s, &[ESC, cmd::ROLL, 0x01, 5, 2]),
        nr::ADDRESS_NOT_VALID,
    );
}

#[test]
fn query_is_answered() {
    let mut s = screen();
    let a = run(
        &mut s,
        &[
            ESC,
            cmd::WRITE_STRUCTURED_FIELD,
            0x00,
            0x05,
            0xD9,
            0x70,
            0x00,
        ],
    );
    assert!(a.errors.is_empty());
    assert_eq!(a.replies[0].data[2], 0x88);
}

#[test]
fn other_structured_fields_get_a_negative_response() {
    let mut s = screen();
    let a = run(
        &mut s,
        &[
            ESC,
            cmd::WRITE_STRUCTURED_FIELD,
            0x00,
            0x06,
            0xD9,
            0x30,
            0x00,
            0x01,
        ],
    );
    assert_nr(&a, nr::STRUCTURED_FIELD_NOT_SUPPORTED);
    let a = run(&mut s, &[ESC, cmd::WRITE_STRUCTURED_FIELD, 0x00, 0x02]);
    assert_nr(&a, nr::BAD_LENGTH);
}

#[test]
fn record_level_opcodes() {
    let mut s = screen();
    let a = apply(&mut s, &Record::new(opcode::CANCEL_INVITE, vec![]));
    assert_eq!(a.replies, [Record::new(opcode::CANCEL_INVITE, vec![])]);
    apply(&mut s, &Record::new(opcode::MESSAGE_LIGHT_ON, vec![]));
    assert!(s.message_waiting);
    apply(&mut s, &Record::new(opcode::MESSAGE_LIGHT_OFF, vec![]));
    assert!(!s.message_waiting);
    let a = apply(&mut s, &Record::new(opcode::SAVE_SCREEN, vec![]));
    assert_eq!(a.replies[0].opcode, opcode::SAVE_SCREEN);
}

#[test]
fn malformed_records() {
    let mut s = screen();
    assert_nr(&run(&mut s, &[0xC1]), nr::DATA_OUTSIDE_COMMAND);
    assert_nr(&run(&mut s, &[ESC]), nr::PREMATURE_END);
    assert_nr(&run(&mut s, &[ESC, 0x99]), nr::COMMAND_NOT_VALID);
    assert_nr(
        &run(&mut s, &[ESC, cmd::WRITE_TO_DISPLAY, 0x00]),
        nr::PREMATURE_END,
    );
}

/// Pseudo-random records never panic, and the screen stays consistent.
#[test]
fn random_records_never_panic() {
    let mut x: u64 = 0x9E37_79B9_7F4A_7C15;
    let mut next = move || {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        x
    };
    let mut s = wide();
    for _ in 0..20_000 {
        let n = (next() % 64) as usize;
        let mut d: Vec<u8> = (0..n).map(|_| next() as u8).collect();
        // Mostly well-formed starts, to reach deeper.
        if next() % 2 == 0 && !d.is_empty() {
            d[0] = ESC;
        }
        let op = [
            opcode::PUT_GET,
            opcode::RESTORE_SCREEN,
            opcode::SAVE_SCREEN,
            0,
        ][next() as usize % 4];
        apply(&mut s, &Record::new(op, d));
        assert_eq!(s.bytes().len(), s.rows() * s.cols());
        assert!(s.cursor < s.size());
        assert!(s.fields.iter().all(|f| f.end() <= s.size()));
    }
}
