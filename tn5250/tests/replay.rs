// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! Every recording in `fixtures/replay/` replays to its golden. A fixture
//! whose recording is not there yet is skipped. `QSA_UPDATE_GOLDEN=1`
//! writes the goldens from the replays instead of comparing.

use std::path::PathBuf;

use tn5250::replay::{Golden, Recording, replay};

/// The fixtures, by name: `NAME.qsr` and `NAME.golden`.
const FIXTURES: &[&str] = &[
    // Recorded by hand; see fixtures/README.md.
    "sign-on",
    "sign-on-wrkactjob-f3",
    "screen-27x132",
    // QS/400's sign-on display, Enter on it, and the 27×132 sign-on.
    "qs400-signon-24x80",
    "qs400-signon-27x132",
];

fn dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../fixtures/replay")
}

#[test]
fn fixtures_replay_to_their_goldens() {
    let update = std::env::var_os("QSA_UPDATE_GOLDEN").is_some();
    let mut failures = Vec::new();
    for name in FIXTURES {
        let rec_path = dir().join(format!("{name}.qsr"));
        let golden_path = dir().join(format!("{name}.golden"));
        if !rec_path.exists() {
            eprintln!("skipped {name}: {} not recorded yet", rec_path.display());
            continue;
        }
        let rec = Recording::read(&rec_path).unwrap_or_else(|e| panic!("{name}: {e}"));
        let r = replay(&rec);
        for (i, want, got) in &r.divergences {
            failures.push(format!(
                "{name}: event {i} sent {}, recording has {}",
                tn5250::log::hex(got),
                tn5250::log::hex(want)
            ));
        }
        for e in &r.errors {
            failures.push(format!("{name}: {e}"));
        }
        let got = Golden::of(&r.terminal.screen);
        if update {
            std::fs::write(&golden_path, got.to_text()).unwrap();
            eprintln!("wrote {}", golden_path.display());
            continue;
        }
        let text = std::fs::read_to_string(&golden_path)
            .unwrap_or_else(|e| panic!("{}: {e}", golden_path.display()));
        let want = Golden::parse(&text)
            .unwrap_or_else(|| panic!("{} is not a golden", golden_path.display()));
        if let Some(d) = Golden::diff(&want, &got) {
            failures.push(format!("{name}: screen differs from its golden\n{d}"));
        }
    }
    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
}

/// Switching screen sizes: a synthetic stream for a 27×132 display, as
/// QS/400 sends it once its screen-mode fix is in: wide screens come with
/// Clear Unit Alternate, 24×80-only screens with Clear Unit and their errors
/// on row 24. The screen must follow both ways, every time, cleanly.
#[test]
fn switching_screen_sizes() {
    use tn5250::negotiate::ScreenSize;
    use tn5250::record::{Record, opcode};
    use tn5250::replay::Event;
    use tn5250::screen::Keyboard;

    const ESC: u8 = 0x04;
    const CLEAR_UNIT: u8 = 0x40;
    const CLEAR_UNIT_ALTERNATE: u8 = 0x20;
    const WRITE_TO_DISPLAY: u8 = 0x11;
    const WRITE_ERROR_CODE: u8 = 0x21;
    const SBA: u8 = 0x11;
    let page = tn5250::codepage::default_page();
    let text = |s: &str| page.encode(s);
    let at = |row: u8, col: u8, s: &str| {
        let mut v = vec![SBA, row, col];
        v.extend(text(s));
        v
    };
    let wide = |label: &str| {
        let mut v = vec![
            ESC,
            CLEAR_UNIT_ALTERNATE,
            0x00,
            ESC,
            WRITE_TO_DISPLAY,
            0x00,
            0x00,
        ];
        v.extend(at(1, 1, label));
        v.extend(at(27, 120, "ROW 27"));
        v
    };
    let narrow = |label: &str| {
        let mut v = vec![ESC, CLEAR_UNIT, ESC, WRITE_TO_DISPLAY, 0x00, 0x00];
        v.extend(at(1, 1, label));
        v.extend([ESC, WRITE_ERROR_CODE]);
        v.extend(text("ERROR ON ROW 24"));
        v
    };
    let steps = [
        (wide("WIDE ONE"), (27, 132)),
        (narrow("NARROW ONE"), (24, 80)),
        (wide("WIDE TWO"), (27, 132)),
        (narrow("NARROW TWO"), (24, 80)),
        (wide("WIDE THREE"), (27, 132)),
    ];
    let mut rec = Recording {
        size: ScreenSize::S27x132,
        ccsid: 37,
        printer: None,
        events: Vec::new(),
    };
    for (i, (data, (rows, cols))) in steps.iter().enumerate() {
        rec.events.push((
            i as u64,
            Event::In(Record::new(opcode::PUT_GET, data.clone()).encode()),
        ));
        let r = replay(&rec);
        assert!(r.errors.is_empty(), "step {i}: {:?}", r.errors);
        let s = &r.terminal.screen;
        assert_eq!((s.rows(), s.cols()), (*rows, *cols), "step {i}");
        let lines = s.text_rows();
        assert_eq!(lines.len(), *rows);
        assert!(lines.iter().all(|l| l.chars().count() == *cols), "step {i}");
        if *cols == 132 {
            assert!(lines[0].starts_with("WIDE"), "step {i}: {}", lines[0]);
            assert_eq!(&lines[26][119..125], "ROW 27", "step {i}");
            assert!(
                lines[1..26].iter().all(|l| l.trim().is_empty()),
                "step {i}: left over"
            );
        } else {
            assert!(lines[0].starts_with("NARROW"), "step {i}: {}", lines[0]);
            assert!(
                lines[23].starts_with("ERROR ON ROW 24"),
                "step {i}: {}",
                lines[23]
            );
            assert!(
                lines[1..23].iter().all(|l| l.trim().is_empty()),
                "step {i}: left over"
            );
            assert_eq!(s.keyboard, Keyboard::HostError);
        }
    }
}
