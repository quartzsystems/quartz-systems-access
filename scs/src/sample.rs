// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! Writing SCS: a small [`Builder`], and the built-in test page that a
//! printer session prints without the server.

use tn5250::codepage::{self, CodePage};

use crate::codes::{self, class, d1, d2, d3, d4, pp};

/// Builds an SCS job. Text is written in CCSID 37, which the job selects
/// at its start; characters the code page lacks are left out.
#[derive(Debug)]
pub struct Builder {
    out: Vec<u8>,
    page: &'static dyn CodePage,
}

impl Default for Builder {
    fn default() -> Builder {
        Builder::new()
    }
}

impl Builder {
    pub fn new() -> Builder {
        let mut b = Builder {
            out: Vec::new(),
            page: codepage::default_page(),
        };
        // Code page 37, whatever the device's.
        b.d(class::D1, &[d1::SCG, 0x02, 0xB9, 0x00, 37]);
        b
    }

    /// A format command: `2B class count params`.
    fn d(&mut self, class: u8, params: &[u8]) -> &mut Builder {
        self.out
            .extend_from_slice(&[codes::CSP, class, (params.len() + 1) as u8]);
        self.out.extend_from_slice(params);
        self
    }

    fn byte(&mut self, b: u8) -> &mut Builder {
        self.out.push(b);
        self
    }

    pub fn text(&mut self, s: &str) -> &mut Builder {
        let bytes = self.page.encode(s);
        self.out.extend(bytes);
        self
    }

    pub fn nl(&mut self) -> &mut Builder {
        self.byte(codes::NL)
    }

    pub fn ff(&mut self) -> &mut Builder {
        self.byte(codes::FF)
    }

    pub fn cr(&mut self) -> &mut Builder {
        self.byte(codes::CR)
    }

    pub fn bs(&mut self) -> &mut Builder {
        self.byte(codes::BS)
    }

    pub fn ht(&mut self) -> &mut Builder {
        self.byte(codes::HT)
    }

    pub fn wus(&mut self) -> &mut Builder {
        self.byte(codes::WUS)
    }

    /// To column `col` (1 to 255).
    pub fn ahpp(&mut self, col: u8) -> &mut Builder {
        self.out.extend_from_slice(&[codes::PP, pp::AHPP, col]);
        self
    }

    /// To line `line` (1 to 255).
    pub fn avpp(&mut self, line: u8) -> &mut Builder {
        self.out.extend_from_slice(&[codes::PP, pp::AVPP, line]);
        self
    }

    /// Maximum position, left and right margins and tab stops, in columns.
    pub fn shf(&mut self, mpp: u8, lm: u8, rm: u8, tabs: &[u8]) -> &mut Builder {
        let mut p = vec![mpp, lm, rm];
        p.extend_from_slice(tabs);
        self.d(class::SHF, &p)
    }

    /// Page length, top and bottom margins, in lines.
    pub fn svf(&mut self, mpl: u8, tm: u8, bm: u8) -> &mut Builder {
        self.d(class::SVF, &[mpl, tm, bm])
    }

    /// Line spacing in 1/72 inch: 24, 18, 12, 9 and 8 are 3, 4, 6, 8 and
    /// 9 lines per inch.
    pub fn sld(&mut self, ld: u8) -> &mut Builder {
        self.d(class::SLD, &[ld])
    }

    /// Characters per inch: 5, 10, 12 or 15.
    pub fn scd(&mut self, cpi: u8) -> &mut Builder {
        self.d(class::D2, &[d2::SCD, 0, cpi])
    }

    /// A font ID with its character width in 1/1440 inch.
    pub fn sfg(&mut self, id: u16, width: u16) -> &mut Builder {
        let [a, b] = id.to_be_bytes();
        let [c, d] = width.to_be_bytes();
        self.d(class::D1, &[d1::SFG, a, b, c, d, 1])
    }

    /// Page rotation: 0 portrait, `0x2D00` landscape, `0xFFFF` default.
    pub fn sto(&mut self, rotation: u16) -> &mut Builder {
        let [a, b] = rotation.to_be_bytes();
        self.d(class::D3, &[d3::STO, 0, 0, a, b])
    }

    pub fn bes(&mut self) -> &mut Builder {
        self.d(class::D1, &[d1::BES, 0])
    }

    pub fn ees(&mut self) -> &mut Builder {
        self.d(class::D1, &[d1::EES, 0])
    }

    pub fn bus(&mut self) -> &mut Builder {
        self.d(class::D4, &[d4::BUS, 0, 0])
    }

    pub fn eus(&mut self) -> &mut Builder {
        self.d(class::D4, &[d4::EUS])
    }

    /// Begin overstrike with `ch`, white space left alone.
    pub fn bos(&mut self, ch: char) -> &mut Builder {
        let c = self.page.to_byte(ch).unwrap_or(0x60);
        self.d(class::D4, &[d4::BOS, c, codes::BYPASS_WHITE_SPACE])
    }

    pub fn eos(&mut self) -> &mut Builder {
        self.d(class::D4, &[d4::EOS])
    }

    pub fn bytes(&self) -> Vec<u8> {
        self.out.clone()
    }
}

/// The test page: pitches, line densities, tabs, underline, overstrike, a
/// Latin-1 character table, and a landscape page.
pub fn test_page() -> Vec<u8> {
    const ALPHABET: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZ 0123456789";
    let mut b = Builder::new();
    b.svf(66, 3, 63)
        .shf(85, 6, 80, &[6, 21, 36, 51, 66])
        .sto(0)
        .avpp(3)
        .ahpp(6)
        .bes()
        .text("QS/400 Access for Desktop: printer test page")
        .ees()
        .nl()
        .nl();

    b.text("Pitch").nl();
    for cpi in [10u8, 12, 15] {
        b.scd(cpi)
            .text(&format!("{cpi} characters per inch   {ALPHABET}"))
            .nl();
    }
    b.sfg(11, 86)
        .text(&format!("16.7 characters per inch {ALPHABET}"))
        .nl();
    b.sfg(11, 72)
        .text(&format!("20 characters per inch   {ALPHABET}"))
        .nl();
    b.sfg(11, 144).nl();

    b.text("Lines per inch").nl();
    for (ld, lpi) in [(24u8, 3u8), (18, 4), (12, 6), (9, 8), (8, 9)] {
        b.sld(ld)
            .text(&format!("{lpi} lines per inch, first line"))
            .nl()
            .text(&format!("{lpi} lines per inch, second line"))
            .nl();
    }
    b.sld(12).nl();

    b.text("Tabs at columns 21, 36, 51 and 66").nl();
    b.text("one")
        .ht()
        .text("two")
        .ht()
        .text("three")
        .ht()
        .text("four")
        .ht()
        .text("five")
        .nl()
        .nl();

    b.text("Underline: ")
        .bus()
        .text("begin and end underscore")
        .eus()
        .text(", and a single ")
        .text("word")
        .wus()
        .text(" underscored")
        .nl();
    b.text("Overstrike: ")
        .text("bold by printing twice")
        .cr()
        .ahpp(18)
        .text("bold by printing twice")
        .text(", O")
        .bs()
        .text("/ and 0")
        .bs()
        .text("- overlaid, ")
        .bos('-')
        .text("struck through")
        .eos()
        .nl()
        .nl();

    b.text("Latin-1 characters").nl();
    for row in [
        0x20u32, 0x30, 0x40, 0x50, 0x60, 0x70, 0xA0, 0xB0, 0xC0, 0xD0, 0xE0, 0xF0,
    ] {
        b.text(&format!("{row:02X}  "));
        for c in row..row + 16 {
            let ch = char::from_u32(c)
                .filter(|ch| !ch.is_control())
                .unwrap_or(' ');
            b.text(&ch.to_string()).text(" ");
        }
        b.nl();
    }
    b.ff();

    // Landscape: 132 columns at 15 characters per inch.
    b.sto(0x2D00)
        .scd(15)
        .svf(51, 3, 48)
        .shf(132, 1, 132, &[])
        .avpp(3)
        .ahpp(1)
        .text("Landscape page: 132 columns at 15 characters per inch")
        .nl()
        .nl();
    let ruler: String = (1..=132)
        .map(|c| match c % 10 {
            0 => char::from_digit((c / 10) % 10, 10).unwrap_or('0'),
            5 => '+',
            _ => '.',
        })
        .collect();
    b.text(&ruler).nl();
    let digits: String = (1..=132)
        .map(|c| char::from_digit(c % 10, 10).unwrap_or('0'))
        .collect();
    b.text(&digits).nl().ff();
    b.bytes()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Orientation, Pitch};
    use crate::{Options, parse, to_text};

    #[test]
    fn the_test_page_reads_back() {
        let d = parse(&test_page(), &Options::default());
        assert_eq!(d.pages.len(), 2);
        assert_eq!(d.pages[0].orientation, Orientation::Portrait);
        assert_eq!(d.pages[1].orientation, Orientation::Landscape);
        let t = to_text(&d);
        assert!(t.contains("     QS/400 Access for Desktop: printer test page\n"));
        assert!(t.contains("16.7 characters per inch"));
        assert!(t.contains("one            two            three"), "{t}");
        assert!(t.contains("F0  ð ñ ò ó ô õ ö ÷ ø ù ú û ü ý þ ÿ"), "{t}");
        // Every pitch, and the bold, underlined and overlaid text.
        let runs: Vec<_> = d.pages.iter().flat_map(|p| &p.runs).collect();
        for p in [Pitch::P10, Pitch::P12, Pitch::P15, Pitch::P16_7, Pitch::P20] {
            assert!(runs.iter().any(|r| r.pitch == p), "{p:?}");
        }
        assert!(
            runs.iter()
                .any(|r| r.bold && r.text.contains("bold by printing twice"))
        );
        assert!(runs.iter().any(|r| r.underline && r.text == "word"));
        assert!(runs.iter().any(|r| r.text == "/"));
        // The 132-column ruler fits the landscape page.
        assert!(t.contains("\nLandscape page"), "{t}");
        let ruler = d.pages[1]
            .runs
            .iter()
            .find(|r| r.text.starts_with("1234567890") && r.text.len() == 132)
            .expect("the ruler");
        assert!(ruler.char_x(132) <= d.pages[1].width);
    }
}
