// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! The page model: what a job prints, independent of any output.
//!
//! Positions and sizes are in 1/1440 inch ("twips") from the top-left
//! corner of the page as it is printed: a landscape page is wider than it
//! is tall.

/// 1/1440 inch per inch.
pub const TWIPS_PER_INCH: u32 = 1440;

/// A paper size.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Paper {
    #[default]
    Letter,
    Legal,
    A4,
}

impl Paper {
    /// Every paper, in a stable order.
    pub const ALL: [Paper; 3] = [Paper::Letter, Paper::Legal, Paper::A4];

    /// Width and height in portrait, in 1/1440 inch.
    pub fn size(self) -> (u32, u32) {
        match self {
            Paper::Letter => (12240, 15840),
            Paper::Legal => (12240, 20160),
            // 210 by 297 millimetres.
            Paper::A4 => (11906, 16838),
        }
    }

    /// The name used in settings files.
    pub fn name(self) -> &'static str {
        match self {
            Paper::Letter => "letter",
            Paper::Legal => "legal",
            Paper::A4 => "a4",
        }
    }

    /// The paper with this settings name, in any case.
    pub fn from_name(name: &str) -> Option<Paper> {
        Paper::ALL
            .into_iter()
            .find(|p| p.name().eq_ignore_ascii_case(name.trim()))
    }
}

/// Page orientation.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Orientation {
    #[default]
    Portrait,
    Landscape,
}

/// Characters per 10 inches: 100 for 10 pitch, 167 for 16.7. Kept as an
/// integer so that pitches compare exactly; positions are computed from it
/// without rounding error building up along a line.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Pitch(pub u16);

impl Pitch {
    pub const P10: Pitch = Pitch(100);
    pub const P12: Pitch = Pitch(120);
    pub const P15: Pitch = Pitch(150);
    pub const P16_7: Pitch = Pitch(167);
    pub const P20: Pitch = Pitch(200);

    /// Characters per inch.
    pub fn cpi(self) -> f64 {
        f64::from(self.0.max(1)) / 10.0
    }

    /// The width of one character, in 1/1440 inch (not rounded).
    pub fn advance(self) -> f64 {
        14400.0 / f64::from(self.0.max(1))
    }

    /// The left edge of 1-based column `col`, in 1/1440 inch from column 1.
    pub fn column_x(self, col: u16) -> u32 {
        (f64::from(col.max(1) - 1) * self.advance()).round() as u32
    }

    /// The pitch whose character is `width` 1/1440 inch wide.
    pub fn from_width(width: u16) -> Option<Pitch> {
        (width > 0).then(|| {
            let p = (14400.0 / f64::from(width)).round() as u16;
            Pitch(p.clamp(10, 1000))
        })
    }
}

impl Default for Pitch {
    fn default() -> Pitch {
        Pitch::P10
    }
}

/// Margins of a page, in 1/1440 inch from its edges.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Margins {
    pub left: u32,
    pub top: u32,
    pub right: u32,
    pub bottom: u32,
}

/// Text that prints in one piece: one font, pitch and emphasis, characters
/// side by side at the pitch.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Run {
    /// Left edge of the first character.
    pub x: u32,
    /// Top of the line the run is on (moved half a line for superscript
    /// and subscript).
    pub y: u32,
    /// The line's height: the line spacing in force.
    pub height: u32,
    pub text: String,
    /// The font ID the data asked for. Every ID prints in the one bundled
    /// face; it is kept for logs and tests.
    pub font_id: u16,
    pub pitch: Pitch,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    /// The 1-based line and column of the first character, for text
    /// output.
    pub line: u16,
    pub col: u16,
}

impl Run {
    /// The left edge of the `i`th character (0-based) of the run.
    pub fn char_x(&self, i: usize) -> u32 {
        self.x + (i as f64 * self.pitch.advance()).round() as u32
    }
}

/// One printed page.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Page {
    /// Width and height as printed (landscape: width > height).
    pub width: u32,
    pub height: u32,
    pub orientation: Orientation,
    pub margins: Margins,
    /// Runs in reading order: top to bottom, left to right. Characters
    /// printed over different ones come last, as runs of their own.
    pub runs: Vec<Run>,
}

impl Page {
    /// Whether nothing prints on the page.
    pub fn is_blank(&self) -> bool {
        self.runs
            .iter()
            .all(|r| r.text.trim().is_empty() && !r.underline)
    }
}

/// A print job.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Document {
    pub pages: Vec<Page>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pitches_are_exact() {
        assert_eq!(Pitch::P10.column_x(11), 1440);
        assert_eq!(Pitch::P12.column_x(13), 1440);
        assert_eq!(Pitch::P15.column_x(16), 1440);
        assert_eq!(Pitch::P20.column_x(21), 1440);
        // 16.7 pitch: 167 characters in ten inches, without drift.
        assert_eq!(Pitch::P16_7.column_x(168), 14400);
        assert_eq!(Pitch::from_width(144), Some(Pitch::P10));
        assert_eq!(Pitch::from_width(86), Some(Pitch::P16_7));
        assert_eq!(Pitch::from_width(72), Some(Pitch::P20));
        assert_eq!(Pitch::from_width(0), None);
    }

    #[test]
    fn paper_names() {
        assert_eq!(Paper::from_name(" A4 "), Some(Paper::A4));
        assert_eq!(Paper::from_name("tabloid"), None);
        assert_eq!(Paper::Letter.size(), (8 * 1440 + 720, 11 * 1440));
    }
}
