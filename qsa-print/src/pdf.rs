// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! The page model to PDF.
//!
//! Each page keeps its size and orientation; each run is placed in the
//! bundled face, sized to its pitch. Only the faces a job uses are
//! embedded, each subset to the characters it prints.
//!
//! Every page keeps a margin clear (a quarter inch unless the session sets
//! one), as a PDF is mostly read on a screen. Text already inside the
//! margin stays exactly where the data put it; text nearer an edge is moved
//! in; and a page whose text would then run past the far margin is scaled
//! down evenly to fit (see [`fit`]).

use std::collections::HashMap;

use printpdf::{
    Color, Line, LinePoint, Mm, Op, ParsedFont, PdfDocument, PdfFontHandle, PdfPage,
    PdfSaveOptions, Point, Pt, Rgb, TextItem,
};
use scs::{Document, Page, Paper};

use crate::font::{self, Face};

/// The margin kept clear on every side when the session sets none: a
/// quarter inch, in 1/1440 inch.
pub const DEFAULT_MARGIN: u32 = 360;

/// How a job is written.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Options {
    /// The document title.
    pub title: String,
    /// The margin kept clear on every side, in 1/1440 inch: the session's
    /// margins override; `None` for [`DEFAULT_MARGIN`].
    pub margin: Option<u32>,
    /// The page size of a job with no pages (one blank page).
    pub paper: Paper,
}

/// Where a page's text goes: `x' = margin + (x - left) * scale`, and the
/// same downwards.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Fit {
    pub margin: f64,
    pub left: f64,
    pub top: f64,
    pub scale: f64,
}

impl Fit {
    pub fn x(&self, x: f64) -> f64 {
        self.margin + (x - self.left) * self.scale
    }

    pub fn y(&self, y: f64) -> f64 {
        self.margin + (y - self.top) * self.scale
    }
}

/// How `page`'s text is placed inside a clear `margin` on every side.
pub fn fit(page: &Page, margin: u32) -> Fit {
    let m = f64::from(margin);
    let (w, h) = (f64::from(page.width), f64::from(page.height));
    let mut l = m;
    let mut t = m;
    let mut r = f64::MIN;
    let mut b = f64::MIN;
    for run in &page.runs {
        l = l.min(f64::from(run.x));
        t = t.min(f64::from(run.y));
        r = r.max(f64::from(run.char_x(run.text.chars().count())));
        b = b.max(f64::from(run.y + run.height));
    }
    let room = |size: f64, near: f64, far: f64| {
        let need = far - near;
        if need <= 0.0 {
            1.0
        } else {
            ((size - 2.0 * m) / need).clamp(0.05, 1.0)
        }
    };
    Fit {
        margin: m,
        left: l,
        top: t,
        scale: room(w, l, r).min(room(h, t, b)),
    }
}

/// A PDF could not be made.
#[derive(Debug, thiserror::Error)]
pub enum PdfError {
    #[error("the print font could not be read: {0}")]
    Font(String),
}

fn twips_to_pt(t: f64) -> f32 {
    (t / 20.0) as f32
}

fn mm(twips: u32) -> Mm {
    Mm((f64::from(twips) * 25.4 / 1440.0) as f32)
}

/// The job as a PDF file's bytes.
pub fn render(doc: &Document, options: &Options) -> Result<Vec<u8>, PdfError> {
    let mut pdf = PdfDocument::new(&options.title);
    let mut fonts: HashMap<Face, printpdf::FontId> = HashMap::new();
    let black = Color::Rgb(Rgb {
        r: 0.0,
        g: 0.0,
        b: 0.0,
        icc_profile: None,
    });
    let mut pages = Vec::with_capacity(doc.pages.len().max(1));
    for page in &doc.pages {
        let height_pt = f64::from(page.height) / 20.0;
        let fit = fit(page, options.margin.unwrap_or(DEFAULT_MARGIN));
        let mut ops = vec![
            Op::SetFillColor { col: black.clone() },
            Op::SetOutlineColor { col: black.clone() },
        ];
        for run in &page.runs {
            let face = Face::of(run);
            let id = match fonts.get(&face) {
                Some(id) => id.clone(),
                None => {
                    let parsed = ParsedFont::from_bytes(face.bytes(), 0, &mut Vec::new())
                        .ok_or_else(|| PdfError::Font(format!("{face:?}")))?;
                    let id = pdf.add_font(&parsed);
                    fonts.insert(face, id.clone());
                    id
                }
            };
            let x = fit.x(f64::from(run.x));
            let base = fit.y(f64::from(run.y) + font::baseline(run.height, run.pitch));
            let base_pt = height_pt - base / 20.0;
            ops.extend([
                Op::StartTextSection,
                Op::SetFont {
                    font: PdfFontHandle::External(id),
                    size: Pt(twips_to_pt(font::em(run.pitch) * fit.scale)),
                },
                Op::SetTextCursor {
                    pos: Point {
                        x: Pt(twips_to_pt(x)),
                        y: Pt(base_pt as f32),
                    },
                },
                Op::ShowText {
                    items: vec![TextItem::Text(run.text.clone())],
                },
                Op::EndTextSection,
            ]);
            if run.underline {
                let (below, thick) = font::underline(run.pitch);
                let n = run.text.chars().count();
                let x0 = twips_to_pt(x);
                let x1 = twips_to_pt(x + n as f64 * run.pitch.advance() * fit.scale);
                let y = (base_pt - below * fit.scale / 20.0) as f32;
                ops.extend([
                    Op::SetOutlineThickness {
                        pt: Pt(twips_to_pt(thick * fit.scale)),
                    },
                    Op::DrawLine {
                        line: Line {
                            points: vec![
                                LinePoint {
                                    p: Point {
                                        x: Pt(x0),
                                        y: Pt(y),
                                    },
                                    bezier: false,
                                },
                                LinePoint {
                                    p: Point {
                                        x: Pt(x1),
                                        y: Pt(y),
                                    },
                                    bezier: false,
                                },
                            ],
                            is_closed: false,
                        },
                    },
                ]);
            }
        }
        pages.push(PdfPage::new(mm(page.width), mm(page.height), ops));
    }
    if pages.is_empty() {
        let (w, h) = options.paper.size();
        pages.push(PdfPage::new(mm(w), mm(h), Vec::new()));
    }
    let save = PdfSaveOptions {
        subset_fonts: true,
        ..PdfSaveOptions::default()
    };
    Ok(pdf.with_pages(pages).save(&save, &mut Vec::new()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use scs::{Options as ScsOptions, sample::Builder};

    fn page_of(b: &Builder) -> Page {
        scs::parse(&b.bytes(), &ScsOptions::default())
            .pages
            .remove(0)
    }

    #[test]
    fn text_inside_the_margin_stays_put() {
        // Column 6 at 10 pitch, line 3: half an inch and a third in.
        let mut b = Builder::new();
        b.avpp(3).ahpp(6).text("INSIDE");
        let f = fit(&page_of(&b), DEFAULT_MARGIN);
        assert_eq!((f.x(720.0), f.y(480.0), f.scale), (720.0, 480.0, 1.0));
    }

    #[test]
    fn text_at_the_edge_moves_in() {
        let mut b = Builder::new();
        b.text("EDGE");
        let f = fit(&page_of(&b), DEFAULT_MARGIN);
        assert_eq!((f.x(0.0), f.y(0.0), f.scale), (360.0, 360.0, 1.0));
    }

    #[test]
    fn a_full_page_is_scaled_to_fit_inside_the_margins() {
        // 66 lines at 6 lines per inch fill letter paper's 11 inches.
        let mut b = Builder::new();
        for _ in 0..66 {
            b.text("LINE").nl();
        }
        let p = page_of(&b);
        let f = fit(&p, DEFAULT_MARGIN);
        assert!(f.scale < 1.0);
        let bottom = f.y(f64::from(66 * 240));
        assert!(
            (bottom - f64::from(p.height - DEFAULT_MARGIN)).abs() < 0.5,
            "{bottom}"
        );
        // No margin: as the data put it.
        let f = fit(&p, 0);
        assert_eq!((f.x(100.0), f.scale), (100.0, 1.0));
    }
}
