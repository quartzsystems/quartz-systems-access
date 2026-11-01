// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! Input fields: the Field Format Word, Field Control Words and the entry
//! in the format table.

/// How a field accepts characters: bits 5-7 of the FFW's first byte.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shift {
    /// Anything; lower case allowed.
    AlphaShift,
    /// Letters, comma, period, minus and blank.
    AlphaOnly,
    /// Anything; the keyboard starts in numeric shift.
    NumericShift,
    /// Digits, plus, comma, period, minus and blank.
    NumericOnly,
    Katakana,
    /// Digits only.
    DigitsOnly,
    /// Magnetic stripe or light pen only: no keyboard entry.
    Io,
    /// Digits, and the sign set by Field+ or Field−; the last position
    /// holds the sign.
    SignedNumeric,
}

/// Bits 5-7 of the FFW's second byte.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Adjust {
    None,
    /// Right adjust, fill with zeros on Field Exit.
    RightZero,
    /// Right adjust, fill with blanks on Field Exit.
    RightBlank,
    /// Every position must be filled once any is.
    MandatoryFill,
}

/// The Field Format Word.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Ffw(pub u16);

impl Ffw {
    pub const BYPASS: u16 = 0x2000;
    pub const DUP: u16 = 0x1000;
    pub const MDT: u16 = 0x0800;
    pub const AUTO_ENTER: u16 = 0x0080;
    pub const EXIT_REQUIRED: u16 = 0x0040;
    pub const MONOCASE: u16 = 0x0020;
    pub const MANDATORY_ENTER: u16 = 0x0008;

    fn has(self, bit: u16) -> bool {
        self.0 & bit != 0
    }

    /// Protected: the cursor skips it and nothing can be typed.
    pub fn bypass(self) -> bool {
        self.has(Self::BYPASS)
    }
    pub fn dup_enabled(self) -> bool {
        self.has(Self::DUP)
    }
    /// Modified data tag.
    pub fn mdt(self) -> bool {
        self.has(Self::MDT)
    }
    pub fn auto_enter(self) -> bool {
        self.has(Self::AUTO_ENTER)
    }
    pub fn exit_required(self) -> bool {
        self.has(Self::EXIT_REQUIRED)
    }
    pub fn monocase(self) -> bool {
        self.has(Self::MONOCASE)
    }
    pub fn mandatory_enter(self) -> bool {
        self.has(Self::MANDATORY_ENTER)
    }

    pub fn shift(self) -> Shift {
        match (self.0 >> 8) & 0x07 {
            0 => Shift::AlphaShift,
            1 => Shift::AlphaOnly,
            2 => Shift::NumericShift,
            3 => Shift::NumericOnly,
            4 => Shift::Katakana,
            5 => Shift::DigitsOnly,
            6 => Shift::Io,
            _ => Shift::SignedNumeric,
        }
    }

    pub fn adjust(self) -> Adjust {
        match self.0 & 0x07 {
            5 => Adjust::RightZero,
            6 => Adjust::RightBlank,
            7 => Adjust::MandatoryFill,
            _ => Adjust::None,
        }
    }

    pub fn set_mdt(&mut self, on: bool) {
        if on {
            self.0 |= Self::MDT;
        } else {
            self.0 &= !Self::MDT;
        }
    }
}

/// Field Control Word types (the first byte).
pub mod fcw {
    /// Cursor progression: the second byte is the next field's number.
    pub const CURSOR_PROGRESSION: u8 = 0x80;
    /// Continued entry field: the second byte says which part.
    pub const CONTINUED: u8 = 0x86;
}

/// One input field in the format table.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Field {
    /// Buffer address of the first data position; the field's attribute
    /// sits just before it.
    pub start: usize,
    pub len: usize,
    pub ffw: Ffw,
    /// Field Control Words, as received.
    pub fcw: Vec<u16>,
    /// The field's screen attribute.
    pub attr: u8,
}

impl Field {
    /// One past the last data position.
    pub fn end(&self) -> usize {
        self.start + self.len
    }

    pub fn contains(&self, addr: usize) -> bool {
        addr >= self.start && addr < self.end()
    }

    /// The field number to go to on Field Advance, 1-based, if a cursor
    /// progression FCW names one.
    pub fn progression(&self) -> Option<usize> {
        self.fcw
            .iter()
            .find(|w| (*w >> 8) as u8 == fcw::CURSOR_PROGRESSION)
            .map(|w| (w & 0xFF) as usize)
            .filter(|&n| n > 0)
    }

    /// Whether the field shows nothing (passwords).
    pub fn nondisplay(&self) -> bool {
        crate::screen::attr::is_nondisplay(self.attr)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ffw_bits() {
        // Bypass, MDT; alpha only; auto enter, monocase; right adjust blank.
        let f = Ffw(0x2800 | 0x0100 | 0x00A0 | 0x0006);
        assert!(f.bypass() && f.mdt() && !f.dup_enabled());
        assert_eq!(f.shift(), Shift::AlphaOnly);
        assert!(f.auto_enter() && f.monocase() && !f.exit_required());
        assert_eq!(f.adjust(), Adjust::RightBlank);
        assert_eq!(Ffw(0x4007).adjust(), Adjust::MandatoryFill);
        assert_eq!(Ffw(0x4700).shift(), Shift::SignedNumeric);
        let mut g = Ffw(0x4000);
        g.set_mdt(true);
        assert_eq!(g.0, 0x4800);
        g.set_mdt(false);
        assert_eq!(g.0, 0x4000);
    }

    #[test]
    fn cursor_progression() {
        let f = Field {
            start: 10,
            len: 5,
            ffw: Ffw(0x4000),
            fcw: vec![0x8603, 0x8004],
            attr: 0x24,
        };
        assert_eq!(f.progression(), Some(4));
        assert!(f.contains(14) && !f.contains(15));
    }
}
