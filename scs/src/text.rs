// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! Plain text from the page model: for "save as text" and for tests.
//!
//! Each page becomes its lines from line 1 to the last line with anything
//! on it, every character at its column; trailing blanks are trimmed.
//! Where characters were printed over each other the first one that is
//! not blank shows. Pages are separated by a form feed at the start of the
//! next page's first line.

use std::collections::BTreeMap;

use crate::model::{Document, Page};

/// The whole job as text.
pub fn to_text(doc: &Document) -> String {
    let mut out = String::new();
    for (i, page) in doc.pages.iter().enumerate() {
        if i > 0 {
            out.push('\u{000C}');
        }
        out.push_str(&page_text(page));
    }
    out
}

/// One page as text, each line ending in a newline.
pub fn page_text(page: &Page) -> String {
    let mut lines: BTreeMap<u16, BTreeMap<u16, char>> = BTreeMap::new();
    for run in &page.runs {
        let cells = lines.entry(run.line).or_default();
        for (i, ch) in run.text.chars().enumerate() {
            let col = run.col.saturating_add(i as u16);
            let cell = cells.entry(col).or_insert(' ');
            if *cell == ' ' {
                *cell = ch;
            }
        }
    }
    let last = lines
        .iter()
        .rev()
        .find(|(_, cells)| cells.values().any(|&c| c != ' '))
        .map_or(0, |(&l, _)| l);
    let mut out = String::new();
    for line in 1..=last {
        let mut text = String::new();
        if let Some(cells) = lines.get(&line) {
            let mut col = 1u16;
            for (&c, &ch) in cells {
                if c < col {
                    continue;
                }
                text.extend(std::iter::repeat_n(' ', usize::from(c - col)));
                text.push(ch);
                col = c + 1;
            }
        }
        out.push_str(text.trim_end());
        out.push('\n');
    }
    out
}
