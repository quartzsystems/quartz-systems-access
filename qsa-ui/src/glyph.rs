// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! The glyph set: flat 16×16 and 32×32 icons drawn in code.
//!
//! Glyphs are laid out on a 16-unit grid. At 32×32 fills double while outlines
//! and rules stay one pixel wide, so the large size reads finer, not blockier.
//! Colours come only from the 16-colour palette plus the face grey.

use fltk::enums::ColorDepth;
use fltk::image::RgbImage;

/// One palette entry, RGB.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rgb(pub u8, pub u8, pub u8);

/// The 16-colour palette plus the face grey.
pub mod palette {
    use super::Rgb;
    pub const BLACK: Rgb = Rgb(0x00, 0x00, 0x00);
    pub const MAROON: Rgb = Rgb(0x80, 0x00, 0x00);
    pub const GREEN: Rgb = Rgb(0x00, 0x80, 0x00);
    pub const OLIVE: Rgb = Rgb(0x80, 0x80, 0x00);
    pub const NAVY: Rgb = Rgb(0x00, 0x00, 0x80);
    pub const PURPLE: Rgb = Rgb(0x80, 0x00, 0x80);
    pub const TEAL: Rgb = Rgb(0x00, 0x80, 0x80);
    pub const SILVER: Rgb = Rgb(0xC0, 0xC0, 0xC0);
    pub const GREY: Rgb = Rgb(0x80, 0x80, 0x80);
    pub const RED: Rgb = Rgb(0xFF, 0x00, 0x00);
    pub const LIME: Rgb = Rgb(0x00, 0xFF, 0x00);
    pub const YELLOW: Rgb = Rgb(0xFF, 0xFF, 0x00);
    pub const BLUE: Rgb = Rgb(0x00, 0x00, 0xFF);
    pub const FUCHSIA: Rgb = Rgb(0xFF, 0x00, 0xFF);
    pub const AQUA: Rgb = Rgb(0x00, 0xFF, 0xFF);
    pub const WHITE: Rgb = Rgb(0xFF, 0xFF, 0xFF);
    /// The face grey, `#D4D0C8`.
    pub const FACE: Rgb = Rgb(0xD4, 0xD0, 0xC8);

    /// Every colour a glyph may use.
    pub const ALL: [Rgb; 17] = [
        BLACK, MAROON, GREEN, OLIVE, NAVY, PURPLE, TEAL, SILVER, GREY, RED, LIME, YELLOW, BLUE,
        FUCHSIA, AQUA, WHITE, FACE,
    ];
}

/// The glyphs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Glyph {
    Terminal,
    Printer,
    Table,
    Folder,
    Disk,
    System,
    Check,
    Cross,
}

impl Glyph {
    /// Every glyph, in a stable order.
    pub const ALL: [Glyph; 8] = [
        Glyph::Terminal,
        Glyph::Printer,
        Glyph::Table,
        Glyph::Folder,
        Glyph::Disk,
        Glyph::System,
        Glyph::Check,
        Glyph::Cross,
    ];

    /// Short lower-case name, used in file names and tests.
    pub fn name(self) -> &'static str {
        match self {
            Glyph::Terminal => "terminal",
            Glyph::Printer => "printer",
            Glyph::Table => "table",
            Glyph::Folder => "folder",
            Glyph::Disk => "disk",
            Glyph::System => "system",
            Glyph::Check => "check",
            Glyph::Cross => "cross",
        }
    }
}

/// Glyph sizes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Size {
    /// 16×16: toolbars, menus, lists, task pane.
    Small,
    /// 32×32: dialogs and the welcome panel.
    Large,
}

impl Size {
    /// Edge length in pixels.
    pub fn px(self) -> i32 {
        match self {
            Size::Small => 16,
            Size::Large => 32,
        }
    }
}

/// An RGBA pixel buffer on the 16-unit grid.
struct Canvas {
    size: i32,
    k: i32,
    px: Vec<u8>,
}

impl Canvas {
    fn new(size: Size) -> Self {
        let s = size.px();
        Canvas {
            size: s,
            k: s / 16,
            px: vec![0; (s * s * 4) as usize],
        }
    }

    fn large(&self) -> bool {
        self.k > 1
    }

    fn set(&mut self, x: i32, y: i32, c: Rgb) {
        if x < 0 || y < 0 || x >= self.size || y >= self.size {
            return;
        }
        let i = ((y * self.size + x) * 4) as usize;
        self.px[i..i + 4].copy_from_slice(&[c.0, c.1, c.2, 0xFF]);
    }

    fn fill_px(&mut self, x: i32, y: i32, w: i32, h: i32, c: Rgb) {
        for yy in y..y + h {
            for xx in x..x + w {
                self.set(xx, yy, c);
            }
        }
    }

    /// Filled rectangle in grid units.
    fn fill(&mut self, x: i32, y: i32, w: i32, h: i32, c: Rgb) {
        let k = self.k;
        self.fill_px(x * k, y * k, w * k, h * k, c);
    }

    /// One-pixel outline round a rectangle in grid units.
    fn frame(&mut self, x: i32, y: i32, w: i32, h: i32, c: Rgb) {
        let k = self.k;
        let (x0, y0, x1, y1) = (x * k, y * k, (x + w) * k - 1, (y + h) * k - 1);
        self.fill_px(x0, y0, x1 - x0 + 1, 1, c);
        self.fill_px(x0, y1, x1 - x0 + 1, 1, c);
        self.fill_px(x0, y0, 1, y1 - y0 + 1, c);
        self.fill_px(x1, y0, 1, y1 - y0 + 1, c);
    }

    /// Filled rectangle with a one-pixel outline.
    fn panel(&mut self, x: i32, y: i32, w: i32, h: i32, body: Rgb, edge: Rgb) {
        self.fill(x, y, w, h, body);
        self.frame(x, y, w, h, edge);
    }

    /// One-pixel horizontal rule starting at grid row `y`.
    fn hrule(&mut self, x: i32, y: i32, w: i32, c: Rgb) {
        let k = self.k;
        self.fill_px(x * k, y * k, w * k, 1, c);
    }

    /// One-pixel vertical rule starting at grid column `x`.
    fn vrule(&mut self, x: i32, y: i32, h: i32, c: Rgb) {
        let k = self.k;
        self.fill_px(x * k, y * k, 1, h * k, c);
    }

    /// A stroke through grid points, `t` grid units thick.
    fn stroke(&mut self, points: &[(i32, i32)], t: i32, c: Rgb) {
        for pair in points.windows(2) {
            let (ax, ay) = pair[0];
            let (bx, by) = pair[1];
            let steps = (bx - ax).abs().max((by - ay).abs()).max(1) * self.k;
            for i in 0..=steps {
                let fx = ax * self.k + (bx - ax) * self.k * i / steps;
                let fy = ay * self.k + (by - ay) * self.k * i / steps;
                self.fill_px(fx, fy, t * self.k, t * self.k, c);
            }
        }
    }
}

fn draw(glyph: Glyph, c: &mut Canvas) {
    use palette::*;
    match glyph {
        Glyph::Terminal => {
            // Monitor with a green-on-black screen, on a stand.
            c.panel(1, 1, 14, 11, SILVER, BLACK);
            c.panel(3, 3, 10, 7, BLACK, GREY);
            c.hrule(4, 4, 5, LIME);
            c.hrule(4, 6, 7, LIME);
            c.hrule(4, 8, 3, LIME);
            if c.large() {
                c.hrule(4, 5, 6, LIME);
                c.hrule(4, 7, 4, LIME);
                c.fill_px(16, 17, 2, 2, LIME);
            }
            c.fill(6, 12, 4, 1, GREY);
            c.panel(3, 13, 10, 2, SILVER, BLACK);
        }
        Glyph::Printer => {
            // Paper in, body, paper out.
            c.panel(4, 1, 8, 6, WHITE, BLACK);
            c.panel(1, 6, 14, 6, SILVER, BLACK);
            c.hrule(2, 7, 12, WHITE);
            c.fill(11, 8, 2, 1, LIME);
            c.hrule(3, 10, 10, BLACK);
            c.panel(4, 11, 8, 4, WHITE, BLACK);
            c.hrule(5, 13, 6, GREY);
            if c.large() {
                c.hrule(5, 2, 6, GREY);
                c.hrule(5, 4, 5, GREY);
                c.hrule(5, 12, 5, GREY);
            }
        }
        Glyph::Table => {
            // White grid with a navy header row.
            c.panel(1, 2, 14, 12, WHITE, BLACK);
            c.fill(1, 2, 14, 3, NAVY);
            c.frame(1, 2, 14, 12, BLACK);
            for y in [8, 11] {
                c.hrule(2, y, 12, GREY);
            }
            c.vrule(6, 5, 9, GREY);
            c.vrule(10, 5, 9, GREY);
            if c.large() {
                c.vrule(6, 2, 3, WHITE);
                c.vrule(10, 2, 3, WHITE);
            }
        }
        Glyph::Folder => {
            // Closed yellow folder with a tab.
            c.panel(1, 3, 6, 3, YELLOW, OLIVE);
            c.panel(1, 5, 14, 9, YELLOW, OLIVE);
            c.hrule(2, 6, 12, WHITE);
            c.vrule(2, 6, 7, WHITE);
            if c.large() {
                c.hrule(2, 4, 4, WHITE);
            }
        }
        Glyph::Disk => {
            // Diskette: navy body, sliding shutter, white label.
            c.panel(2, 1, 12, 14, NAVY, BLACK);
            c.panel(5, 1, 6, 5, SILVER, BLACK);
            c.fill(8, 2, 2, 3, NAVY);
            c.panel(4, 8, 8, 6, WHITE, BLACK);
            c.hrule(5, 10, 6, GREY);
            c.hrule(5, 12, 6, GREY);
            c.fill(3, 13, 1, 1, BLACK);
        }
        Glyph::System => {
            // System unit: tower with drive bays and a power lamp.
            c.panel(4, 1, 8, 14, SILVER, BLACK);
            c.vrule(5, 2, 12, WHITE);
            c.panel(6, 3, 5, 2, FACE, GREY);
            c.panel(6, 6, 5, 2, FACE, GREY);
            c.hrule(6, 10, 5, GREY);
            c.hrule(6, 11, 5, GREY);
            c.fill(9, 12, 1, 1, LIME);
            if c.large() {
                c.hrule(7, 4, 3, BLACK);
                c.hrule(7, 7, 3, BLACK);
            }
        }
        Glyph::Check => {
            c.stroke(&[(2, 8), (6, 12), (13, 3)], 2, GREEN);
        }
        Glyph::Cross => {
            c.stroke(&[(3, 3), (12, 12)], 2, RED);
            c.stroke(&[(12, 3), (3, 12)], 2, RED);
        }
    }
}

/// RGBA pixels of a glyph, row-major, `size.px()` square. Unused pixels are
/// fully transparent.
pub fn rgba(glyph: Glyph, size: Size) -> Vec<u8> {
    let mut canvas = Canvas::new(size);
    draw(glyph, &mut canvas);
    canvas.px
}

/// The glyph as an FLTK image, ready for buttons, menus and frames.
pub fn image(glyph: Glyph, size: Size) -> RgbImage {
    let s = size.px();
    RgbImage::new(&rgba(glyph, size), s, s, ColorDepth::Rgba8)
        .expect("glyph buffer is always the right size")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn glyphs_use_only_the_palette() {
        for glyph in Glyph::ALL {
            for size in [Size::Small, Size::Large] {
                let px = rgba(glyph, size);
                assert_eq!(px.len(), (size.px() * size.px() * 4) as usize);
                let mut opaque = 0;
                for p in px.chunks(4) {
                    match p[3] {
                        0 => {}
                        0xFF => {
                            opaque += 1;
                            let c = Rgb(p[0], p[1], p[2]);
                            assert!(palette::ALL.contains(&c), "{glyph:?} {size:?} uses {c:?}");
                        }
                        a => panic!("{glyph:?} {size:?} has partial alpha {a}"),
                    }
                }
                assert!(opaque > 10, "{glyph:?} {size:?} is empty");
            }
        }
    }

    #[test]
    fn glyphs_are_distinct() {
        for size in [Size::Small, Size::Large] {
            let all: Vec<_> = Glyph::ALL.iter().map(|g| rgba(*g, size)).collect();
            for i in 0..all.len() {
                for j in i + 1..all.len() {
                    assert_ne!(all[i], all[j]);
                }
            }
        }
    }
}
