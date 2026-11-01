// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! Hotspots: text on the screen that acts when clicked.
//!
//! - Function-key labels (`F3=Exit`, `F12=Cancel`, `F24=More keys` ...)
//!   send that function key.
//! - `More...` sends Page Down; `Bottom` sends Page Up.
//!
//! Only protected text is looked at: nothing inside an input field, and
//! nothing in a field that is not displayed. A hotspot shows only as the
//! hand pointer; the screen itself is drawn exactly as without them.

use qsa_config::qsd::{HOTSPOT_FUNCTION_KEYS, HOTSPOT_PAGING, Hotspots};
use tn5250::input::Key;
use tn5250::screen::Screen;

/// One hotspot: a run of cells on a row (from 0) and its key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hotspot {
    pub row: usize,
    /// First and last column, from 0.
    pub first: usize,
    pub last: usize,
    pub key: Key,
}

impl Hotspot {
    pub fn contains(&self, row: usize, col: usize) -> bool {
        row == self.row && (self.first..=self.last).contains(&col)
    }
}

/// The hotspot at a cell.
pub fn at(list: &[Hotspot], row: usize, col: usize) -> Option<Hotspot> {
    list.iter().find(|h| h.contains(row, col)).copied()
}

/// One row of protected text: each cell's character, or `None` where the
/// cell is in an input field or shows nothing.
fn protected_row(s: &Screen, row: usize) -> Vec<Option<char>> {
    (0..s.cols())
        .map(|c| {
            let addr = s.addr(row, c);
            if s.field_at(addr).is_some() {
                return None;
            }
            let cell = s.cell(row, c);
            if cell.is_attr || tn5250::screen::attr::is_nondisplay(cell.attr) {
                None
            } else {
                Some(cell.ch)
            }
        })
        .collect()
}

/// `F` and a key number 1 to 24 then `=` at `i`: the number and the index
/// just after the `=`.
fn fkey_at(cells: &[Option<char>], i: usize) -> Option<(u8, usize)> {
    if cells.get(i).copied().flatten() != Some('F') {
        return None;
    }
    // The label must start a word.
    if i > 0 && cells[i - 1].is_some_and(|c| c.is_alphanumeric()) {
        return None;
    }
    let mut j = i + 1;
    let mut n: u32 = 0;
    while let Some(Some(c)) = cells.get(j)
        && let Some(d) = c.to_digit(10)
    {
        n = n * 10 + d;
        j += 1;
        if j - i > 3 {
            return None;
        }
    }
    if j == i + 1 || !(1..=24).contains(&n) || cells.get(j).copied().flatten() != Some('=') {
        return None;
    }
    Some((n as u8, j + 1))
}

/// Where a function-key label's text ends: before two blanks running, the
/// next label, a cell that is not protected text, or the end of the row.
fn label_end(cells: &[Option<char>], from: usize) -> usize {
    let mut end = from;
    let mut i = from;
    while i < cells.len() {
        match cells[i] {
            None => break,
            Some(' ') => {
                if cells.get(i + 1).copied().flatten() == Some(' ') {
                    break;
                }
            }
            Some(_) => {
                if fkey_at(cells, i).is_some() {
                    break;
                }
                end = i;
            }
        }
        i += 1;
    }
    end
}

/// Whether `word` stands on its own at `i`: not part of a longer word.
fn word_at(cells: &[Option<char>], i: usize, word: &str) -> bool {
    let n = word.chars().count();
    let fits = word
        .chars()
        .enumerate()
        .all(|(k, w)| cells.get(i + k).copied().flatten() == Some(w));
    let before_ok = i == 0 || !cells[i - 1].is_some_and(char::is_alphanumeric);
    let after_ok = !cells
        .get(i + n)
        .copied()
        .flatten()
        .is_some_and(char::is_alphanumeric);
    fits && before_ok && after_ok
}

/// Every hotspot on the screen that `settings` switches on.
pub fn detect(s: &Screen, settings: &Hotspots) -> Vec<Hotspot> {
    let fkeys = settings.on(HOTSPOT_FUNCTION_KEYS);
    let paging = settings.on(HOTSPOT_PAGING);
    let mut out = Vec::new();
    if !fkeys && !paging {
        return out;
    }
    for row in 0..s.rows() {
        let cells = protected_row(s, row);
        let mut i = 0;
        while i < cells.len() {
            if fkeys && let Some((n, after)) = fkey_at(&cells, i) {
                let last = label_end(&cells, after).max(after - 1);
                out.push(Hotspot {
                    row,
                    first: i,
                    last,
                    key: Key::F(n),
                });
                i = last + 1;
                continue;
            }
            if paging {
                let found = [("More...", Key::PageDown), ("Bottom", Key::PageUp)]
                    .into_iter()
                    .find(|(w, _)| word_at(&cells, i, w));
                if let Some((w, key)) = found {
                    let n = w.chars().count();
                    out.push(Hotspot {
                        row,
                        first: i,
                        last: i + n - 1,
                        key,
                    });
                    i += n;
                    continue;
                }
            }
            i += 1;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use tn5250::screen::field::{Ffw, Field};

    /// A 24 x 80 screen with `lines` written from row 0 as protected text.
    fn screen(lines: &[&str]) -> Screen {
        let mut s = Screen::default_24x80();
        for (r, line) in lines.iter().enumerate() {
            for (c, ch) in line.chars().enumerate() {
                let b = s.page().to_byte(ch).unwrap_or(0x40);
                let a = s.addr(r, c);
                s.set_byte(a, b);
            }
        }
        s.recompute_attrs();
        s
    }

    fn all() -> Hotspots {
        Hotspots::default()
    }

    fn keys(list: &[Hotspot]) -> Vec<(usize, usize, usize, Key)> {
        list.iter()
            .map(|h| (h.row, h.first, h.last, h.key))
            .collect()
    }

    #[test]
    fn function_key_labels() {
        let s = screen(&[
            " F3=Exit   F5=Refresh   F12=Cancel   F13=Repeat   F24=More keys",
            "F4=Prompt F9=Retrieve",
            "XF3=Exit  F25=Nothing  F0=Nothing  F3 = Spaced",
        ]);
        let got = keys(&detect(&s, &all()));
        assert_eq!(
            got,
            vec![
                (0, 1, 7, Key::F(3)),
                (0, 11, 20, Key::F(5)),
                (0, 24, 33, Key::F(12)),
                (0, 37, 46, Key::F(13)),
                (0, 50, 62, Key::F(24)),
                (1, 0, 8, Key::F(4)),
                (1, 10, 20, Key::F(9)),
            ]
        );
        assert_eq!(
            at(&detect(&s, &all()), 0, 5).map(|h| h.key),
            Some(Key::F(3))
        );
        assert_eq!(at(&detect(&s, &all()), 0, 9), None);
    }

    #[test]
    fn page_labels() {
        let s = screen(&[
            "                                                                More...",
            "                                                                 Bottom",
            "Furthermore... BottomLine More",
        ]);
        let got = keys(&detect(&s, &all()));
        assert_eq!(
            got,
            vec![(0, 64, 70, Key::PageDown), (1, 65, 70, Key::PageUp)]
        );
    }

    #[test]
    fn kinds_switch_off() {
        let s = screen(&["F3=Exit   More..."]);
        let mut h = all();
        h.set(HOTSPOT_PAGING, false);
        assert_eq!(keys(&detect(&s, &h)), vec![(0, 0, 6, Key::F(3))]);
        h.set(HOTSPOT_FUNCTION_KEYS, false);
        h.set(HOTSPOT_PAGING, true);
        assert_eq!(keys(&detect(&s, &h)), vec![(0, 10, 16, Key::PageDown)]);
        h.enabled = false;
        assert!(detect(&s, &h).is_empty());
    }

    #[test]
    fn never_inside_input_fields() {
        let mut s = screen(&["Type F3=Exit here       More..."]);
        // An input field over "F3=Exit here": its text is the user's.
        assert!(s.define_field(Field {
            start: s.addr(0, 5),
            len: 12,
            ffw: Ffw(0x4000),
            fcw: Vec::new(),
            attr: 0x20,
        }));
        let got = keys(&detect(&s, &all()));
        assert_eq!(got, vec![(0, 24, 30, Key::PageDown)]);
    }

    #[test]
    fn on_the_recorded_sign_on_display() {
        let p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../fixtures/replay/qs400-signon-24x80.qsr");
        let rec = tn5250::replay::Recording::read(&p).unwrap();
        let s = tn5250::replay::replay(&rec).terminal.screen;
        // The sign-on display has no function-key or page labels, and the
        // input fields are never looked at.
        assert!(detect(&s, &all()).is_empty());
    }
}
