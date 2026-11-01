// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

use super::*;
use crate::text::to_text;

/// Text in CCSID 37.
fn e(s: &str) -> Vec<u8> {
    codepage::default_page().encode(s)
}

/// A job built from pieces: text and raw controls.
fn job(parts: &[&[u8]]) -> Vec<u8> {
    parts.concat()
}

fn doc(data: &[u8]) -> Document {
    parse(data, &Options::default())
}

fn text(data: &[u8]) -> String {
    to_text(&doc(data))
}

const NL: &[u8] = &[codes::NL];
const FF: &[u8] = &[codes::FF];

/// `2B class count params`, the count filled in.
fn csp(class: u8, params: &[u8]) -> Vec<u8> {
    let mut v = vec![codes::CSP, class, (params.len() + 1) as u8];
    v.extend_from_slice(params);
    v
}

/// The printed character at a line and column, with its own position and
/// the run it is in.
struct Cell<'a> {
    ch: char,
    x: u32,
    y: u32,
    height: u32,
    pitch: Pitch,
    bold: bool,
    underline: bool,
    font_id: u16,
    run: &'a Run,
}

fn run_at(d: &Document, page: usize, line: u16, col: u16) -> Cell<'_> {
    d.pages[page]
        .runs
        .iter()
        .find_map(|r| {
            let i = usize::from(col.checked_sub(r.col)?);
            let ch = r.text.chars().nth(i).filter(|&c| c != ' ')?;
            (r.line == line).then(|| Cell {
                ch,
                x: r.char_x(i),
                y: r.y,
                height: r.height,
                pitch: r.pitch,
                bold: r.bold,
                underline: r.underline,
                font_id: r.font_id,
                run: r,
            })
        })
        .unwrap_or_else(|| panic!("nothing at {line},{col}: {:#?}", d.pages[page].runs))
}

#[test]
fn new_line_carriage_return_and_line_feed() {
    let t = text(&job(&[
        &e("FIRST"),
        NL,
        &e("SECOND"),
        &[codes::LF],
        &e("X"),
        &[codes::CR],
        &e("Y"),
        &[codes::IRS],
        &e("Z"),
    ]));
    assert_eq!(
        t,
        "FIRST\nSECOND\n      X\nZ\n".replace("      X", "Y     X")
    );
}

#[test]
fn positions_are_in_twips_from_the_corner() {
    let d = doc(&job(&[&e("AB"), NL, &e("  C")]));
    let a = run_at(&d, 0, 1, 1);
    assert_eq!((a.x, a.y, a.height, a.run.text.as_str()), (0, 0, 240, "AB"));
    let c = run_at(&d, 0, 2, 3);
    assert_eq!((c.x, c.y), (288, 240));
    assert_eq!(d.pages[0].orientation, Orientation::Portrait);
    assert_eq!((d.pages[0].width, d.pages[0].height), Paper::Letter.size());
}

#[test]
fn form_feeds_make_pages_but_not_stray_blank_ones() {
    // A leading form feed, two pages, a trailing form feed.
    let d = doc(&job(&[FF, &e("ONE"), FF, &e("TWO"), FF]));
    assert_eq!(d.pages.len(), 2);
    assert_eq!(to_text(&d), "ONE\n\u{c}TWO\n");
    // Two form feeds in a row ask for a blank page.
    let d = doc(&job(&[&e("ONE"), FF, FF, &e("THREE")]));
    assert_eq!(d.pages.len(), 3);
    assert!(d.pages[1].is_blank());
    // The text after a form feed starts at the top margin.
    let d = doc(&job(&[
        &csp(class::SVF, &[20, 3, 18]),
        &e("A"),
        FF,
        &e("B"),
    ]));
    assert_eq!(run_at(&d, 1, 3, 1).ch, 'B');
}

#[test]
fn required_form_feed_also_ends_the_indent() {
    let d = doc(&job(&[
        &csp(class::SHF, &[80, 1, 80, 10]),
        &[codes::IT],
        &e("IN"),
        NL,
        &e("STILL"),
        &[codes::RFF],
        &e("OUT"),
    ]));
    assert_eq!(run_at(&d, 0, 1, 10).run.text, "IN");
    assert_eq!(run_at(&d, 0, 2, 10).run.text, "STILL");
    assert_eq!(run_at(&d, 1, 1, 1).run.text, "OUT");
}

#[test]
fn page_length_ends_pages_by_itself() {
    // Five lines a page, printing at lines 2 to 4.
    let mut data = csp(class::SVF, &[5, 2, 4]);
    for i in 0..6 {
        data.extend(e(&format!("L{i}")));
        data.push(codes::NL);
    }
    // The last new line ended page 2 by itself; a form feed right after
    // that makes no blank page.
    data.push(codes::FF);
    data.extend(e("END"));
    let d = doc(&data);
    assert_eq!(d.pages.len(), 3, "{}", to_text(&d));
    assert_eq!(to_text(&d), "\nL0\nL1\nL2\n\u{c}\nL3\nL4\nL5\n\u{c}\nEND\n");
    assert_eq!(d.pages[0].margins.top, 240);
}

#[test]
fn horizontal_tabs_from_set_horizontal_format() {
    let t = text(&job(&[
        &csp(class::SHF, &[60, 1, 60, 10, 20]),
        &e("A"),
        &[codes::HT],
        &e("B"),
        &[codes::HT],
        &e("C"),
        &[codes::HT],
        &e("D"),
    ]));
    // Past the last stop a tab moves one column.
    assert_eq!(t, "A        B         C D\n");
    // With no stops set, a tab moves one column.
    assert_eq!(text(&job(&[&e("A"), &[codes::HT], &e("B")])), "A B\n");
}

#[test]
fn tab_stops_set_in_columns_and_in_inches() {
    // Floating (columns): stops at 5 and 9.
    let stab = |fixed: u8, a: u16, b: u16| {
        let mut p = vec![d2::STAB, fixed];
        for v in [a, b] {
            p.push(0);
            p.extend_from_slice(&v.to_be_bytes());
        }
        csp(class::D2, &p)
    };
    let t = text(&job(&[
        &stab(0, 9, 5),
        &[codes::HT],
        &e("X"),
        &[codes::HT],
        &e("Y"),
    ]));
    assert_eq!(t, "    X   Y\n");
    // Fixed (1/1440 inch): 1 inch and 2 inches at 10 pitch are columns 11
    // and 21.
    let t = text(&job(&[
        &stab(1, 1440, 2880),
        &[codes::HT],
        &e("X"),
        &[codes::HT],
        &e("Y"),
    ]));
    assert_eq!(t, "          X         Y\n");
    // Set indent level to the second stop.
    let t = text(&job(&[
        &stab(0, 5, 9),
        &csp(class::D2, &[d2::SIL, 2]),
        NL,
        &e("Z"),
    ]));
    assert_eq!(t, "\n        Z\n");
}

#[test]
fn vertical_tabs_and_channels() {
    let svf = csp(class::SVF, &[30, 2, 28, 5, 9]);
    let t = text(&job(&[
        &svf,
        &e("TOP"),
        &[codes::VT],
        &e("FIVE"),
        &[codes::VT],
        &e("NINE"),
    ]));
    let lines: Vec<&str> = t.lines().collect();
    assert_eq!(lines[1], "TOP");
    assert_eq!(lines[4], "   FIVE");
    assert_eq!(lines[8], "       NINE");
    // No stop below: a line feed.
    let t = text(&job(&[&e("A"), &[codes::VT], &e("B")]));
    assert_eq!(t, "A\n B\n");
    // Channel 3 is the second tab stop; channel 1 the top margin, on the
    // next page when it is above.
    let d = doc(&job(&[
        &svf,
        &[codes::VCS, 0x83],
        &e("C3"),
        &[codes::VCS, 0x81],
        &e("C1"),
    ]));
    assert_eq!(run_at(&d, 0, 9, 1).run.text, "C3");
    assert_eq!(run_at(&d, 1, 2, 3).run.text, "C1");
}

#[test]
fn maximum_print_position_wraps_lines_to_the_left_margin() {
    let t = text(&job(&[&csp(class::SHF, &[10, 3, 10]), &e("ABCDEFGHIJKL")]));
    assert_eq!(t, "ABCDEFGHIJ\n  KL\n");
}

#[test]
fn presentation_position() {
    let pp = |f: u8, n: u8| vec![codes::PP, f, n];
    let d = doc(&job(&[
        &pp(pp::AVPP, 3),
        &pp(pp::AHPP, 10),
        &e("A"),
        &pp(pp::RHPP, 4),
        &e("B"),
        &pp(pp::RVPP, 2),
        &e("C"),
        // Up the page: the next page.
        &pp(pp::AVPP, 2),
        &e("D"),
    ]));
    assert_eq!(run_at(&d, 0, 3, 10).ch, 'A');
    assert_eq!(run_at(&d, 0, 3, 15).ch, 'B');
    assert_eq!(run_at(&d, 0, 5, 16).ch, 'C');
    assert_eq!(run_at(&d, 1, 2, 17).ch, 'D');
    // Absolute vertical to the line already on stays on the page.
    let d = doc(&job(&[
        &pp(pp::AVPP, 1),
        &e("X"),
        &pp(pp::AVPP, 1),
        &e("Y"),
    ]));
    assert_eq!(d.pages.len(), 1);
    // An unknown function is skipped with its operand.
    let (d, log) = parse_logged(&job(&[&pp(0x11, 0x40), &e("Q")]), &Options::default());
    assert_eq!(to_text(&d), "Q\n");
    assert!(log.iter().any(|l| l.contains("function 11")), "{log:?}");
}

#[test]
fn line_density() {
    // Lines per inch 3, 4, 6, 8, 9 from set line density (1/72 inch).
    for (ld, spacing) in [(24u8, 480u32), (18, 360), (12, 240), (9, 180), (8, 160)] {
        let d = doc(&job(&[&csp(class::SLD, &[ld]), &e("A"), NL, &e("B")]));
        assert_eq!(run_at(&d, 0, 2, 1).y, spacing, "LD {ld}");
        assert_eq!(run_at(&d, 0, 2, 1).height, spacing);
    }
    // Set single line distance, in 1/1440 inch.
    let d = doc(&job(&[
        &csp(class::D2, &[d2::SSLD, 0, 180]),
        &e("A"),
        NL,
        &e("B"),
    ]));
    assert_eq!(run_at(&d, 0, 2, 1).y, 180);
    // Set line spacing: three half lines a new line.
    let d = doc(&job(&[
        &csp(class::D2, &[d2::SLS, 3]),
        &e("A"),
        NL,
        &e("B"),
    ]));
    assert_eq!(run_at(&d, 0, 2, 1).y, 360);
}

#[test]
fn character_distance_and_font_selection() {
    let scd = |cd: u16| csp(class::D2, &[&[d2::SCD][..], &cd.to_be_bytes()].concat());
    for (cd, pitch) in [(10, Pitch::P10), (12, Pitch::P12), (15, Pitch::P15)] {
        let d = doc(&job(&[
            &scd(cd),
            &e("AB"),
            &[codes::PP, pp::AHPP, 11],
            &e("C"),
        ]));
        assert_eq!(run_at(&d, 0, 1, 1).pitch, pitch);
        assert_eq!(run_at(&d, 0, 1, 11).x, pitch.column_x(11));
    }
    // Set font global: the width gives the pitch; the ID gives bold.
    let sfg = |id: u16, fwd: u16| {
        csp(
            class::D1,
            &[&[d1::SFG][..], &id.to_be_bytes(), &fwd.to_be_bytes(), &[1]].concat(),
        )
    };
    let d = doc(&job(&[
        &sfg(254, 86),
        &e("A"),
        NL,
        &sfg(281, 72),
        &e("B"),
        NL,
        &sfg(46, 144),
        &e("C"),
    ]));
    assert_eq!(run_at(&d, 0, 1, 1).pitch, Pitch::P16_7);
    assert_eq!(run_at(&d, 0, 1, 1).font_id, 254);
    assert_eq!(run_at(&d, 0, 2, 1).pitch, Pitch::P20);
    let c = run_at(&d, 0, 3, 1);
    assert!(c.bold && c.pitch == Pitch::P10 && c.font_id == 46);
    // A width of zero falls back on the ID's pitch.
    let d = doc(&job(&[&sfg(223, 0), &e("A")]));
    assert_eq!(run_at(&d, 0, 1, 1).pitch, Pitch::P15);
    // The job starts at the device font's pitch.
    let opts = Options {
        font: 85,
        ..Options::default()
    };
    assert_eq!(parse(&e("A"), &opts).pages[0].runs[0].pitch, Pitch::P12);
}

#[test]
fn page_size_and_orientation() {
    // Set presentation page size: 14 by 8.5 inches is landscape.
    let spps = |w: u16, h: u16| {
        csp(
            class::D2,
            &[&[d2::SPPS][..], &w.to_be_bytes(), &h.to_be_bytes()].concat(),
        )
    };
    let d = doc(&job(&[&spps(20160, 12240), &e("WIDE")]));
    let p = &d.pages[0];
    assert_eq!(
        (p.width, p.height, p.orientation),
        (20160, 12240, Orientation::Landscape)
    );
    // Set text orientation to landscape turns the chosen paper.
    let sto = |rot: u16| {
        csp(
            class::D3,
            &[&[d3::STO][..], &[0, 0], &rot.to_be_bytes()].concat(),
        )
    };
    let opts = Options {
        paper: Paper::A4,
        ..Options::default()
    };
    let d = parse(&job(&[&sto(0x2D00), &e("A")]), &opts);
    assert_eq!(
        (d.pages[0].width, d.pages[0].height, d.pages[0].orientation),
        (16838, 11906, Orientation::Landscape)
    );
    // Portrait text orientation on a wide page size keeps portrait.
    let d = doc(&job(&[&sto(0), &spps(15840, 12240), &e("A")]));
    assert_eq!(d.pages[0].orientation, Orientation::Portrait);
    assert!(d.pages[0].width < d.pages[0].height);
    // Without either, text wider than the paper turns it.
    let d = doc(&job(&[&[codes::PP, pp::AHPP, 100], &e("FAR")]));
    assert_eq!(d.pages[0].orientation, Orientation::Landscape);
    // A page size in mid-page ends the page.
    let d = doc(&job(&[&e("A"), &spps(20160, 12240), &e("B")]));
    assert_eq!(d.pages.len(), 2);
    assert_eq!(d.pages[0].orientation, Orientation::Portrait);
    assert_eq!(d.pages[1].orientation, Orientation::Landscape);
}

#[test]
fn page_presentation_media_ends_a_page_in_progress() {
    let ppm = csp(class::D2, &[d2::PPM, 0, 0, 1, 2, 0, 0, 1, 1]);
    let (d, log) = parse_logged(&job(&[&ppm, &e("A"), &ppm, &e("B")]), &Options::default());
    assert_eq!(d.pages.len(), 2);
    assert!(log.iter().any(|l| l.contains("paper source 02")), "{log:?}");
}

#[test]
fn margins_in_inches() {
    // Left margin one inch, right margin at 3 inches; top margin a third
    // of an inch.
    let shm = csp(
        class::D2,
        &[
            &[d2::SHM][..],
            &1440u16.to_be_bytes(),
            &4320u16.to_be_bytes(),
        ]
        .concat(),
    );
    let svm = csp(class::D2, &[&[d2::SVM][..], &480u16.to_be_bytes()].concat());
    let d = doc(&job(&[&svm, &shm, &e("A"), NL, &e("B")]));
    assert_eq!(run_at(&d, 0, 2, 11).x, 1440);
    assert_eq!(d.pages[0].margins.left, 1440);
    assert_eq!(d.pages[0].margins.top, 240);
    // Form feed goes to the top margin line.
    let d = doc(&job(&[&svm, &e("A"), FF, &e("B")]));
    assert_eq!(run_at(&d, 1, 2, 1).ch, 'B');
}

#[test]
fn code_page_selection() {
    // X'4A' is a cent sign in 37 and an A umlaut in 273.
    let scg = |cp: u16| {
        csp(
            class::D1,
            &[&[d1::SCG][..], &[0x02, 0xB9], &cp.to_be_bytes()].concat(),
        )
    };
    assert_eq!(text(&job(&[&[0x4A], &scg(273), &[0x4A]])), "¢Ä\n");
    // Through a local ID, and back to the job's own.
    let scgl = |id: u8| csp(class::D1, &[d1::SCGL, id]);
    assert_eq!(
        text(&job(&[&scgl(0x02), &[0x4A], &scgl(0xFF), &[0x4A]])),
        "Ä¢\n"
    );
    // A code page that is not built in is logged and the current one kept.
    let (d, log) = parse_logged(&job(&[&scg(1025), &[0x4A]]), &Options::default());
    assert_eq!(to_text(&d), "¢\n");
    assert!(log.iter().any(|l| l.contains("1025")), "{log:?}");
    // The job's code page is the device's.
    let opts = Options {
        ccsid: 273,
        ..Options::default()
    };
    assert_eq!(to_text(&parse(&[0x4A], &opts)), "Ä\n");
}

#[test]
fn transparent_and_ascii_transparent_data() {
    // Transparent: bytes print as characters, control values as the error
    // character; its count does not count itself.
    let t = text(&job(&[&[codes::TRN, 3, 0xC1, 0x15, 0xC2], &e("C")]));
    assert_eq!(t, "A-BC\n");
    // ASCII transparency is skipped by its count.
    let (d, log) = parse_logged(
        &job(&[&[codes::ATRN, 2, 0x1B, 0x45], &e("OK")]),
        &Options::default(),
    );
    assert_eq!(to_text(&d), "OK\n");
    assert!(log.iter().any(|l| l.contains("ASCII")));
}

#[test]
fn underscore() {
    let bus = |bypass: u8| csp(class::D4, &[d4::BUS, 0, bypass]);
    let eus = csp(class::D4, &[d4::EUS]);
    let d = doc(&job(&[&bus(0), &e("A B"), &eus, &e(" C")]));
    let r = run_at(&d, 0, 1, 1);
    assert_eq!((r.run.text.as_str(), r.underline), ("A B", true));
    assert!(!run_at(&d, 0, 1, 5).underline);
    // Bypass: the blank is not underlined, so the run breaks there.
    let d = doc(&job(&[&bus(0x80), &e("A B"), &eus]));
    assert_eq!(d.pages[0].runs.len(), 2);
    assert!(
        d.pages[0]
            .runs
            .iter()
            .all(|r| r.underline && r.text.len() == 1)
    );
    // Word underscore: the word just printed.
    let d = doc(&job(&[&e("NOT WORD"), &[codes::WUS], &e(" AFTER")]));
    assert!(!run_at(&d, 0, 1, 1).underline);
    assert!(run_at(&d, 0, 1, 5).underline);
    assert_eq!(run_at(&d, 0, 1, 5).run.text, "WORD");
    assert!(!run_at(&d, 0, 1, 10).underline);
}

#[test]
fn overstrike() {
    // The same text again after a carriage return prints bold.
    let d = doc(&job(&[&e("BOLD"), &[codes::CR], &e("BOLD")]));
    assert_eq!(d.pages[0].runs.len(), 1);
    assert!(d.pages[0].runs[0].bold);
    // Backspace and a different character: both are drawn, the first in
    // the text.
    let d = doc(&job(&[&e("O"), &[codes::BS], &e("/")]));
    let runs = &d.pages[0].runs;
    assert_eq!(runs.len(), 2);
    assert_eq!((runs[0].text.as_str(), runs[1].text.as_str()), ("O", "/"));
    assert_eq!(runs[0].x, runs[1].x);
    assert!(!runs[0].bold);
    assert_eq!(to_text(&d), "O\n");
    // Begin overstrike with a slash over every character, white space
    // left alone.
    let bos = csp(class::D4, &[d4::BOS, 0x61, 0x80]);
    let d = doc(&job(&[
        &bos,
        &e("A B"),
        &csp(class::D4, &[d4::EOS]),
        &e("C"),
    ]));
    let slashes = d.pages[0].runs.iter().filter(|r| r.text == "/").count();
    assert_eq!(slashes, 2);
    assert_eq!(to_text(&d), "A BC\n");
}

#[test]
fn emphasis_superscript_and_subscript() {
    let d = doc(&job(&[
        &csp(class::D1, &[d1::BES, 0]),
        &e("B"),
        &csp(class::D1, &[d1::EES, 0]),
        &e("N"),
        &[codes::SPS],
        &e("2"),
        &[codes::SBS, codes::SBS],
        &e("3"),
    ]));
    assert!(run_at(&d, 0, 1, 1).bold);
    assert!(!run_at(&d, 0, 1, 2).bold);
    assert_eq!(run_at(&d, 0, 1, 3).y, 0);
    assert_eq!(run_at(&d, 0, 1, 4).y, 120);
    // Superscript at the top of the page stays on it.
    assert_eq!(to_text(&d), "BN23\n");
}

#[test]
fn graphic_error_action() {
    // Substitute prints the error character: a hyphen until set otherwise.
    assert_eq!(text(&[codes::SUB]), "-\n");
    let t = text(&job(&[
        &csp(class::SGEA, &[0x5C, 0]),
        &[codes::SUB, codes::GE, 0x41],
    ]));
    assert_eq!(t, "**\n");
    // A default graphic below X'40' is not valid and changes nothing.
    assert_eq!(
        text(&job(&[&csp(class::SGEA, &[0x15, 0]), &[codes::SUB]])),
        "-\n"
    );
}

#[test]
fn inhibit_and_enable_presentation() {
    let t = text(&job(&[
        &[codes::INP],
        &e("SECRET"),
        &[codes::ENP],
        &e("SHOWN"),
    ]));
    assert_eq!(t, "      SHOWN\n");
}

#[test]
fn unknown_controls_are_skipped_by_length_and_logged_once() {
    let (d, log) = parse_logged(
        &job(&[
            &[0x07],
            &e("A"),
            &[0x07],
            // Unknown class, four bytes in all.
            &csp(0xE5, &[1, 2]),
            &e("B"),
            // Unknown D2 function.
            &csp(class::D2, &[0x77, 9, 9]),
            &e("C"),
            &[codes::SA, 1, 2],
            &e("D"),
        ]),
        &Options::default(),
    );
    assert_eq!(to_text(&d), "ABCD\n");
    assert_eq!(log.iter().filter(|l| l.contains("control 07")).count(), 1);
    assert!(log.iter().any(|l| l.contains("2B E5")));
    assert!(log.iter().any(|l| l.contains("2B D2 77")));
}

#[test]
fn cut_short_commands_end_the_job_cleanly() {
    for tail in [
        &[codes::CSP][..],
        &[codes::CSP, class::D2],
        &[codes::CSP, class::D2, 9, d2::SPPS],
        &[codes::PP, pp::AHPP],
        &[codes::TRN],
        &[codes::TRN, 9, 0xC1],
        &[codes::VCS],
        &[codes::GE],
    ] {
        let d = doc(&job(&[&e("OK"), tail]));
        assert!(to_text(&d).starts_with("OK"), "{tail:02X?}");
    }
    // A zero count still moves on.
    assert_eq!(text(&job(&[&[codes::CSP, 0xE5, 0], &e("X")])), "X\n");
    assert_eq!(text(&[]), "");
}

/// RFC 4777 section 11.1: the SCS of the example print record, which sets
/// up a 13.2 by 11 inch page.
#[test]
fn rfc_example_setup_reads_cleanly() {
    let hex = "34C4012BD20345FF2BD2044C00022BD2040D00002BD20A8501010201030204022BD20309022BD2061100014A\
               402BD20601010000012BD306F60000FFFF2BD20A48000001000000010100\
               2BD10705000B0090012BD2044900F02BD206404A403DE02BD2041500F034\
               C4012BD10381FF002BC8034001";
    let mut data: Vec<u8> = (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
        .collect();
    data.extend(e("HELLO"));
    let (d, log) = parse_logged(&data, &Options::default());
    assert!(log.iter().all(|l| !l.contains("skipped")), "{log:?}");
    let p = &d.pages[0];
    assert_eq!((p.width, p.height), (19008, 15840));
    assert_eq!(p.orientation, Orientation::Landscape);
    let r = &p.runs[0];
    assert_eq!(
        (r.text.as_str(), r.line, r.col, r.pitch, r.font_id),
        ("HELLO", 1, 1, Pitch::P10, 11)
    );
}

#[test]
fn every_short_input_is_safe() {
    // Every one- and two-byte input, and every control before text.
    for a in 0..=255u8 {
        let _ = doc(&[a]);
        for b in 0..=255u8 {
            let _ = doc(&[a, b]);
        }
        let _ = doc(&[a, 0xFF, 0xFF, 0xFF, 0xC1]);
        let _ = doc(&[codes::CSP, class::D2, 0xFF, a]);
        let _ = doc(&[codes::CSP, a, 5, 1, 2, 3, 4, 0xC1]);
    }
}

#[test]
fn the_left_margin_stays_put_when_the_pitch_changes() {
    // Left margin at column 6 set at 10 pitch: half an inch.
    let scd = |cd: u8| csp(class::D2, &[d2::SCD, 0, cd]);
    let d = doc(&job(&[
        &csp(class::SHF, &[85, 6, 80]),
        NL,
        &e("TEN"),
        NL,
        &scd(15),
        &e("FIFTEEN AB"),
        &[codes::CR],
        &e("FIFTEEN"),
    ]));
    let ten = run_at(&d, 0, 2, 6);
    let fifteen = run_at(&d, 0, 3, 6);
    assert_eq!((ten.x, fifteen.x), (720, 720));
    assert_eq!(fifteen.pitch, Pitch::P15);
    // The next character is one 15-pitch column on.
    assert_eq!(run_at(&d, 0, 3, 7).x, 720 + 96);
    // Printed again after a carriage return: bold, in the same place.
    assert!(fifteen.bold);
    assert_eq!(d.pages[0].margins.left, 720);
    // Absolute positions count columns of the pitch in force.
    let d = doc(&job(&[&scd(12), &[codes::PP, pp::AHPP, 13], &e("X")]));
    assert_eq!(run_at(&d, 0, 1, 13).x, 1440);
}

#[test]
fn orientation_is_chosen_by_how_the_text_fits() {
    // 132 columns at 15 pitch (8.8 inches) and 66 lines at 6 lines per
    // inch (11 inches): portrait overflows a little, landscape a lot.
    let scd = csp(class::D2, &[d2::SCD, 0, 15]);
    let d = doc(&job(&[
        &scd,
        &csp(class::SVF, &[66, 1, 66]),
        &csp(class::SHF, &[132, 1, 132]),
        &e("X"),
    ]));
    assert_eq!(d.pages[0].orientation, Orientation::Portrait);
    // 80 lines at 8 lines per inch (10 inches) by 132 columns at 10 pitch
    // (13.2 inches) fits better in landscape.
    let d = doc(&job(&[
        &csp(class::SLD, &[9]),
        &csp(class::SVF, &[80, 1, 80]),
        &csp(class::SHF, &[132, 1, 132]),
        &e("X"),
    ]));
    assert_eq!(d.pages[0].orientation, Orientation::Landscape);
}

/// Found by fuzzing: relative moves past column 65535 overflowed when runs
/// were joined and when the text was laid out.
#[test]
fn columns_at_the_far_end_are_safe() {
    // Columns stop at the last one: everything after prints there.
    let mut data = Vec::new();
    for _ in 0..300 {
        data.extend_from_slice(&[codes::PP, pp::RHPP, 0xFF]);
    }
    data.extend(e("AB"));
    data.extend_from_slice(&[codes::PP, pp::RHPP, 0xFF]);
    data.extend(e("C"));
    let d = doc(&data);
    assert_eq!(d.pages.len(), 1);
    assert!(to_text(&d).trim_end().ends_with('A'));
    // A gap too wide to fill splits the run.
    let d = doc(&job(&[
        &e("A"),
        &[codes::PP, pp::AHPP, 255],
        &e("B"),
        &[codes::PP, pp::RHPP, 255, codes::PP, pp::RHPP, 255],
        &e("C"),
    ]));
    let runs = &d.pages[0].runs;
    assert_eq!(runs.len(), 2);
    assert_eq!(runs[0].text.chars().count(), 255);
    assert_eq!(runs[1].text, "C");
}
