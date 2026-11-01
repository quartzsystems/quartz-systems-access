// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! Font IDs.
//!
//! Every font ID prints in the one bundled monospace face. What an ID
//! still decides is the pitch (when the data does not give a character
//! width) and whether the face is bold or italic. IDs not listed here
//! keep the pitch in force and print upright and regular.

use crate::model::Pitch;

/// What a font ID means for printing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FontInfo {
    /// The ID's pitch, if it is known.
    pub pitch: Option<Pitch>,
    pub bold: bool,
    pub italic: bool,
}

/// The font ID a printer device is created with when none is chosen.
pub const DEFAULT_FONT: u16 = 11;

/// What is known about font ID `id`.
pub fn info(id: u16) -> FontInfo {
    let (pitch, bold, italic) = match id {
        // 10 pitch.
        5 | 11 | 12 | 19 => (Some(Pitch::P10), false, false),
        18 => (Some(Pitch::P10), false, true),
        46 => (Some(Pitch::P10), true, false),
        // 12 pitch.
        66 | 85 | 86 | 87 => (Some(Pitch::P12), false, false),
        92 => (Some(Pitch::P12), false, true),
        110 => (Some(Pitch::P12), true, false),
        // 15 pitch.
        222 | 223 => (Some(Pitch::P15), false, false),
        // 20 pitch.
        281 => (Some(Pitch::P20), false, false),
        _ => (None, false, false),
    };
    FontInfo {
        pitch,
        bold,
        italic,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_and_unknown_ids() {
        assert_eq!(info(DEFAULT_FONT).pitch, Some(Pitch::P10));
        assert!(info(46).bold && !info(46).italic);
        assert!(info(92).italic);
        assert_eq!(info(223).pitch, Some(Pitch::P15));
        assert_eq!(
            info(9999),
            FontInfo {
                pitch: None,
                bold: false,
                italic: false
            }
        );
    }
}
