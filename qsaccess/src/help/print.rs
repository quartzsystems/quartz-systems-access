// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! Print for Help: a topic's plain text as pages of the print font, made
//! into a PDF by `qsa-print` as printer output is.

use qsa_print::pdf::{self, PdfError};
use scs::{Document, Margins, Orientation, Page, Paper, Pitch, Run};

use super::{FIXED, HEADING, Topic};

/// The margin on every side: three quarters of an inch, in 1/1440 inch.
const MARGIN: u32 = 1080;
/// Six lines to the inch.
const LINE: u32 = 240;
/// Twelve characters to the inch: 80 columns in under seven inches.
const PITCH: Pitch = Pitch::P12;
/// The paper a topic is printed for.
const PAPER: Paper = Paper::Letter;

/// The lines of a topic as they print: whether each is a heading, and its
/// text.
fn lines(t: &Topic) -> Vec<(bool, &str)> {
    t.text
        .lines()
        .map(|l| match l.strip_prefix(HEADING) {
            Some(h) => (true, h),
            None => (false, l.strip_prefix(FIXED).unwrap_or(l)),
        })
        .collect()
}

/// A topic as pages: its text in order, headings in bold, a heading never
/// the last line of a page.
pub fn document(t: &Topic) -> Document {
    let (width, height) = PAPER.size();
    let per_page = ((height - 2 * MARGIN) / LINE) as usize;
    let new_page = || Page {
        width,
        height,
        orientation: Orientation::Portrait,
        margins: Margins {
            left: MARGIN,
            top: MARGIN,
            right: MARGIN,
            bottom: MARGIN,
        },
        runs: Vec::new(),
    };
    let lines = lines(t);
    let mut pages = vec![new_page()];
    let mut row = 0usize;
    for (i, (heading, text)) in lines.iter().enumerate() {
        // A heading goes with the text under it.
        let next_has_text = lines.get(i + 2).is_some_and(|(_, t)| !t.is_empty());
        if row >= per_page || (*heading && next_has_text && row + 3 > per_page) {
            pages.push(new_page());
            row = 0;
        }
        // No blank lines at the top of a page.
        if text.is_empty() && row == 0 {
            continue;
        }
        if !text.is_empty() {
            let page = pages.last_mut().expect("a page was started");
            page.runs.push(Run {
                x: MARGIN,
                y: MARGIN + row as u32 * LINE,
                height: LINE,
                text: (*text).to_owned(),
                font_id: scs::font::DEFAULT_FONT,
                pitch: PITCH,
                bold: *heading,
                italic: false,
                underline: false,
                line: (row + 1) as u16,
                col: 1,
            });
        }
        row += 1;
    }
    Document { pages }
}

/// A topic as a PDF file's bytes.
pub fn pdf(t: &Topic) -> Result<Vec<u8>, PdfError> {
    pdf::render(
        &document(t),
        &pdf::Options {
            title: t.title.to_owned(),
            margin: None,
            paper: PAPER,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::help::{TOPICS, find, topic};

    #[test]
    fn a_topic_prints_every_line_in_order_with_its_headings_in_bold() {
        let t = find(topic::CERTIFICATE_MANAGEMENT).unwrap();
        let doc = document(t);
        let printed: Vec<&Run> = doc.pages.iter().flat_map(|p| &p.runs).collect();
        let wanted: Vec<(bool, &str)> = lines(t)
            .into_iter()
            .filter(|(_, l)| !l.is_empty())
            .collect();
        assert_eq!(printed.len(), wanted.len());
        for (run, (heading, text)) in printed.iter().zip(&wanted) {
            assert_eq!((run.bold, run.text.as_str()), (*heading, *text));
            assert_eq!((run.x, run.pitch), (MARGIN, PITCH));
        }
        assert_eq!((printed[0].bold, printed[0].text.as_str()), (true, t.title));
        assert_eq!(printed[0].y, MARGIN);
    }

    #[test]
    fn pages_keep_their_margins_and_long_topics_take_several() {
        let (_, height) = PAPER.size();
        for t in TOPICS {
            let doc = document(t);
            for p in &doc.pages {
                assert!(!p.runs.is_empty(), "{}: an empty page", t.slug);
                for r in &p.runs {
                    assert!(
                        r.y >= MARGIN && r.y + r.height <= height - MARGIN,
                        "{}",
                        t.slug
                    );
                }
            }
        }
        let reference = document(find(topic::MESSAGES_REFERENCE).unwrap());
        assert!(reference.pages.len() > 10);
    }

    #[test]
    fn a_topic_makes_a_pdf() {
        let bytes = pdf(find(topic::KEYBOARD_REFERENCE).unwrap()).unwrap();
        assert!(bytes.starts_with(b"%PDF-"));
    }
}
