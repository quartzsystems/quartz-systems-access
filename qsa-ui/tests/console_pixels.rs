// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! The 5250 display at 1× matches the QS/400 service console pixel for
//! pixel. Each fixture in `fixtures/console/` is a recording from
//! `fixtures/replay/` and the console's picture of its final screen:
//!
//! - `NAME.png`: rendered by the console's own code (`reference/`);
//! - `NAME.capture.png`, when present: captured from a real console.
//!
//! The grid is compared, not the status line below it, which the console
//! does not have.

use std::path::PathBuf;

use qsa_ui::console::{self, Canvas, GLYPH_H, GLYPH_W, Layout, Look, Paint};
use tn5250::replay::{Recording, replay};

const FIXTURES: &[&str] = &["qs400-signon-24x80"];

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../fixtures")
}

/// Our 1× rendering of a recording's final screen, grid only.
fn ours(name: &str) -> Canvas {
    let rec = Recording::read(&root().join(format!("replay/{name}.qsr"))).unwrap();
    let s = replay(&rec).terminal.screen;
    let (rows, cols) = (s.rows(), s.cols());
    let mut cells = Vec::new();
    for r in 0..rows {
        for c in 0..cols {
            let mut p = Paint::of(s.cell(r, c));
            p.cursor = s.addr(r, c) == s.cursor;
            cells.push(p);
        }
    }
    let (w, h) = (cols * GLYPH_W, rows * GLYPH_H);
    let l = Layout::fit(w, h, rows, cols, 0);
    let mut c = Canvas::new(w, h, console::palette::BACKGROUND);
    console::render(&mut c, l, &cells, None, Look::default());
    c
}

fn compare(name: &str, want_file: &str) -> Option<String> {
    let path = root().join("console").join(want_file);
    if !path.exists() {
        return None;
    }
    let want = qsa_ui::snapshot::read_png(&path).unwrap();
    let got = ours(name);
    if (want.w as usize, want.h as usize) != (got.w, got.h) {
        return Some(format!(
            "{want_file}: size {}x{}, ours {}x{}",
            want.w, want.h, got.w, got.h
        ));
    }
    let mut first = None;
    let mut n = 0;
    for y in 0..got.h {
        for x in 0..got.w {
            let w = want.at(x as u32, y as u32);
            let g = got.at(x, y).to_be_bytes();
            if w != [g[1], g[2], g[3]] {
                n += 1;
                first.get_or_insert((x, y, w, got.at(x, y)));
            }
        }
    }
    let (x, y, w, g) = first?;
    Some(format!(
        "{want_file}: {n} pixels differ; first at ({x}, {y}) (cell row {}, column {}): console {:02X}{:02X}{:02X}, ours {g:06X}",
        y / GLYPH_H + 1,
        x / GLYPH_W + 1,
        w[0],
        w[1],
        w[2]
    ))
}

#[test]
fn the_grid_matches_the_console_pixel_for_pixel() {
    let mut failures = Vec::new();
    for name in FIXTURES {
        assert!(
            root().join(format!("console/{name}.png")).exists(),
            "{name}.png missing"
        );
        failures.extend(compare(name, &format!("{name}.png")));
        failures.extend(compare(name, &format!("{name}.capture.png")));
    }
    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
}
