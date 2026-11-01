// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! The print font: Courier Prime (SIL Open Font License 1.1), bundled in
//! regular, bold and italic. Every font ID the data selects prints in it.
//!
//! The face is monospace, so a run is sized so that one character's
//! advance is exactly one column at the run's pitch, and the run then
//! lines up with the page model's positions character by character.

use scs::{Pitch, Run};

/// The family name the faces register under.
pub const FAMILY: &str = "Courier Prime";

pub const REGULAR: &[u8] = include_bytes!("../fonts/CourierPrime-Regular.ttf");
pub const BOLD: &[u8] = include_bytes!("../fonts/CourierPrime-Bold.ttf");
pub const ITALIC: &[u8] = include_bytes!("../fonts/CourierPrime-Italic.ttf");

/// The faces' metrics, in font units (the same in all three faces).
pub const UNITS_PER_EM: f64 = 2048.0;
pub const ADVANCE: f64 = 1228.0;
pub const ASCENT: f64 = 1600.0;
pub const DESCENT: f64 = 700.0;

/// One of the bundled faces.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Face {
    Regular,
    Bold,
    Italic,
}

impl Face {
    pub const ALL: [Face; 3] = [Face::Regular, Face::Bold, Face::Italic];

    /// The face a run prints in. Bold italic prints bold.
    pub fn of(run: &Run) -> Face {
        if run.bold {
            Face::Bold
        } else if run.italic {
            Face::Italic
        } else {
            Face::Regular
        }
    }

    /// The face's TrueType file.
    pub fn bytes(self) -> &'static [u8] {
        match self {
            Face::Regular => REGULAR,
            Face::Bold => BOLD,
            Face::Italic => ITALIC,
        }
    }
}

/// The em size, in 1/1440 inch, that makes one character `pitch` wide.
pub fn em(pitch: Pitch) -> f64 {
    pitch.advance() * UNITS_PER_EM / ADVANCE
}

/// The baseline of text in a line `height` tall, in 1/1440 inch below the
/// line's top: the face's ascent and descent centred in the line.
pub fn baseline(height: u32, pitch: Pitch) -> f64 {
    let em = em(pitch);
    let ascent = em * ASCENT / UNITS_PER_EM;
    let descent = em * DESCENT / UNITS_PER_EM;
    (f64::from(height) - ascent - descent) / 2.0 + ascent
}

/// Where an underline goes, below the baseline, and how thick it is, both
/// in 1/1440 inch.
pub fn underline(pitch: Pitch) -> (f64, f64) {
    let em = em(pitch);
    (em * 0.12, (em * 0.05).max(10.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_character_is_one_column_wide() {
        for p in [Pitch::P10, Pitch::P12, Pitch::P15, Pitch::P16_7, Pitch::P20] {
            assert!((em(p) * ADVANCE / UNITS_PER_EM - p.advance()).abs() < 1e-9);
        }
        // 10 pitch is about 12 point (240 twips).
        assert!((em(Pitch::P10) - 240.16).abs() < 0.1);
    }

    #[test]
    fn baseline_sits_inside_the_line() {
        let b = baseline(240, Pitch::P10);
        assert!(b > 120.0 && b < 240.0, "{b}");
        // Tighter lines move it up.
        assert!(baseline(160, Pitch::P10) < b);
    }

    #[test]
    fn faces_are_truetype() {
        for f in Face::ALL {
            assert_eq!(&f.bytes()[..4], &[0, 1, 0, 0]);
        }
    }
}
