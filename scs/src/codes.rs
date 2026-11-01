// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! SCS control codes.
//!
//! Single-byte controls sit in `00`-`3F`. Format commands start with
//! [`CSP`] (`2B`), a class byte and a count byte that counts itself and
//! the parameters; in the `D1` to `D4` classes the first parameter is a
//! function code.

/// Null: ignored.
pub const NUL: u8 = 0x00;
/// ASCII transparency: `03 n` then `n` bytes of ASCII.
pub const ATRN: u8 = 0x03;
/// Vertical channel select: `04 channel`.
pub const VCS: u8 = 0x04;
/// Horizontal tab.
pub const HT: u8 = 0x05;
/// Required new line: a new line that also ends any indent.
pub const RNL: u8 = 0x06;
/// Graphic escape: `08 c`, a character from the graphic escape set.
pub const GE: u8 = 0x08;
/// Superscript: up half a line.
pub const SPS: u8 = 0x09;
/// Repeat: ignored.
pub const RPT: u8 = 0x0A;
/// Vertical tab.
pub const VT: u8 = 0x0B;
/// Form feed.
pub const FF: u8 = 0x0C;
/// Carriage return: to the left margin, same line.
pub const CR: u8 = 0x0D;
/// Enable presentation: printing resumes.
pub const ENP: u8 = 0x14;
/// New line.
pub const NL: u8 = 0x15;
/// Backspace.
pub const BS: u8 = 0x16;
/// Unit backspace: ignored for fixed-pitch fonts.
pub const UBS: u8 = 0x1A;
/// Interchange record separator: a new line.
pub const IRS: u8 = 0x1E;
/// Word underscore: underlines the word just printed.
pub const WUS: u8 = 0x23;
/// Inhibit presentation: nothing prints until [`ENP`].
pub const INP: u8 = 0x24;
/// Line feed: down a line, same column.
pub const LF: u8 = 0x25;
/// Set attribute: `28 type value`; ignored.
pub const SA: u8 = 0x28;
/// Switch: ignored.
pub const SW: u8 = 0x2A;
/// Format command prefix.
pub const CSP: u8 = 0x2B;
/// Bell: ignored.
pub const BEL: u8 = 0x2F;
/// Index return: a required new line.
pub const IRT: u8 = 0x33;
/// Presentation position: `34 function value`.
pub const PP: u8 = 0x34;
/// Transparent: `35 n` then `n` bytes printed as characters.
pub const TRN: u8 = 0x35;
/// Numeric backspace.
pub const NBS: u8 = 0x36;
/// Subscript: down half a line.
pub const SBS: u8 = 0x38;
/// Indent tab: a tab that also indents following lines.
pub const IT: u8 = 0x39;
/// Required form feed.
pub const RFF: u8 = 0x3A;
/// Substitute: prints the graphic error character.
pub const SUB: u8 = 0x3F;
/// Space.
pub const SP: u8 = 0x40;
/// Required space: a blank character.
pub const RSP: u8 = 0x41;
/// Syllable hyphen: printed as a hyphen.
pub const SHY: u8 = 0xCA;
/// Numeric space: a blank character.
pub const NSP: u8 = 0xE1;

/// [`PP`] functions.
pub mod pp {
    /// Absolute horizontal: to column `n`.
    pub const AHPP: u8 = 0xC0;
    /// Absolute vertical: to line `n`, on the next page when above.
    pub const AVPP: u8 = 0xC4;
    /// Relative horizontal: right `n` columns.
    pub const RHPP: u8 = 0xC8;
    /// Relative vertical: down `n` lines.
    pub const RVPP: u8 = 0x4C;
}

/// Format command classes (the byte after [`CSP`]).
pub mod class {
    /// Set horizontal format: maximum position, margins, tab stops.
    pub const SHF: u8 = 0xC1;
    /// Set vertical format: page length, margins, vertical tab stops.
    pub const SVF: u8 = 0xC2;
    /// Set line density, in 1/72 inch.
    pub const SLD: u8 = 0xC6;
    /// Set graphic error action: the character printed for errors.
    pub const SGEA: u8 = 0xC8;
    /// Execute PMP: ignored.
    pub const EPMP: u8 = 0xCA;
    /// Font and code page commands.
    pub const D1: u8 = 0xD1;
    /// Page and line commands.
    pub const D2: u8 = 0xD2;
    /// Text orientation.
    pub const D3: u8 = 0xD3;
    /// Underscore and overstrike.
    pub const D4: u8 = 0xD4;
}

/// Functions of class [`class::D1`].
pub mod d1 {
    /// Set code page through global IDs: `GCGID CPGID`.
    pub const SCG: u8 = 0x01;
    /// Set font global: `GFID FWD [FA]`.
    pub const SFG: u8 = 0x05;
    /// Set code page through a local ID.
    pub const SCGL: u8 = 0x81;
    /// Set form feed control: ignored.
    pub const SFFC: u8 = 0x87;
    /// Begin emphasis.
    pub const BES: u8 = 0x8A;
    /// Bolding algorithm control: ignored.
    pub const BAC: u8 = 0x8B;
    /// End emphasis.
    pub const EES: u8 = 0x8E;
}

/// Functions of class [`class::D2`].
pub mod d2 {
    /// Set horizontal tab stops: `FF (AL TAB)...`.
    pub const STAB: u8 = 0x01;
    /// Justify text field: not supported.
    pub const JTF: u8 = 0x03;
    /// Set indent level.
    pub const SIL: u8 = 0x07;
    /// Set line spacing, in half lines per new line.
    pub const SLS: u8 = 0x09;
    /// Release left margin: ignored.
    pub const RLM: u8 = 0x0B;
    /// Set justify mode: not supported.
    pub const SJM: u8 = 0x0D;
    /// Set horizontal margins, in 1/1440 inch.
    pub const SHM: u8 = 0x11;
    /// Set single line distance, in 1/1440 inch.
    pub const SSLD: u8 = 0x15;
    /// Set character distance (pitch).
    pub const SCD: u8 = 0x29;
    /// Set presentation page size, in 1/1440 inch.
    pub const SPPS: u8 = 0x40;
    /// Set initial conditions: ignored.
    pub const SIC: u8 = 0x45;
    /// Page presentation media: paper source, output bin, duplex.
    pub const PPM: u8 = 0x48;
    /// Set vertical margins, in 1/1440 inch.
    pub const SVM: u8 = 0x49;
    /// Set print setup: ignored.
    pub const SPSU: u8 = 0x4C;
    /// Set exception action: ignored.
    pub const SEA: u8 = 0x85;
}

/// Functions of class [`class::D3`].
pub mod d3 {
    /// Set text orientation: `cccc pppp`.
    pub const STO: u8 = 0xF6;
}

/// Functions of class [`class::D4`].
pub mod d4 {
    /// Begin underscore: `MODE [BYPASS]`.
    pub const BUS: u8 = 0x0A;
    /// End underscore.
    pub const EUS: u8 = 0x0E;
    /// Begin overstrike: `CHAR [BYPASS ...]`.
    pub const BOS: u8 = 0x72;
    /// End overstrike.
    pub const EOS: u8 = 0x76;
}

/// The bypass value that leaves white space alone in underscore and
/// overstrike.
pub const BYPASS_WHITE_SPACE: u8 = 0x80;
