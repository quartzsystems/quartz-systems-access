// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! The glyph set: 16×16 and 32×32 icons drawn in code.
//!
//! Glyphs are described with a few vector shapes on a 16-unit grid and
//! rendered on a 4× supersampled canvas, then averaged down, so edges are
//! smooth at both sizes. Fills shade softly from top to bottom and outlines
//! are a darker tone of the fill rather than black.

use fltk::enums::ColorDepth;
use fltk::image::RgbImage;

/// An RGB colour.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rgb(pub u8, pub u8, pub u8);

const fn hex(v: u32) -> Rgb {
    Rgb((v >> 16) as u8, (v >> 8) as u8, v as u8)
}

/// The glyph palette: muted tones, each with a light and dark shade.
pub mod palette {
    use super::{Rgb, hex};
    pub const STEEL_LIGHT: Rgb = hex(0xF4F6F8);
    pub const STEEL: Rgb = hex(0xC4CBD4);
    pub const STEEL_EDGE: Rgb = hex(0x4E5864);
    pub const GRAPHITE_LIGHT: Rgb = hex(0x4A5563);
    pub const GRAPHITE: Rgb = hex(0x2B323C);
    pub const GRAPHITE_EDGE: Rgb = hex(0x1C2128);
    pub const SCREEN_TOP: Rgb = hex(0x22344A);
    pub const SCREEN_BOTTOM: Rgb = hex(0x162230);
    pub const PROMPT: Rgb = hex(0x7FDBA8);
    pub const PAPER: Rgb = hex(0xFFFFFF);
    pub const PAPER_SHADE: Rgb = hex(0xEEF1F4);
    pub const PAPER_EDGE: Rgb = hex(0x5E6874);
    pub const RULE: Rgb = hex(0xA9B2BE);
    pub const BLUE_LIGHT: Rgb = hex(0x6EA2E3);
    pub const BLUE: Rgb = hex(0x3A73C2);
    pub const BLUE_EDGE: Rgb = hex(0x24528F);
    pub const AMBER_LIGHT: Rgb = hex(0xF7D374);
    pub const AMBER: Rgb = hex(0xE8AE34);
    pub const AMBER_BACK: Rgb = hex(0xD4972A);
    pub const AMBER_EDGE: Rgb = hex(0xA87318);
    pub const GREEN: Rgb = hex(0x2F9E5E);
    pub const GREEN_EDGE: Rgb = hex(0x1F7444);
    pub const LED_GREEN: Rgb = hex(0x45D483);
    pub const LED_BLUE: Rgb = hex(0x5AB0FF);
    pub const RED: Rgb = hex(0xD5483E);
    pub const RED_EDGE: Rgb = hex(0xA3302A);
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

/// Supersampling factor per axis.
const SS: usize = 4;

/// A fill: flat, or shaded from `top` to `bottom` over the shape's height.
#[derive(Clone, Copy)]
enum Paint {
    Flat(Rgb),
    Shade(Rgb, Rgb),
}

impl Paint {
    fn at(self, t: f32) -> Rgb {
        match self {
            Paint::Flat(c) => c,
            Paint::Shade(a, b) => {
                let mix = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
                Rgb(mix(a.0, b.0), mix(a.1, b.1), mix(a.2, b.2))
            }
        }
    }
}

/// An opaque supersampled canvas; coordinates are in 16-grid units.
struct Canvas {
    n: usize,
    scale: f32,
    px: Vec<Option<Rgb>>,
}

impl Canvas {
    fn new(size: Size) -> Canvas {
        let n = size.px() as usize * SS;
        Canvas {
            n,
            scale: n as f32 / 16.0,
            px: vec![None; n * n],
        }
    }

    /// Whether this is the 32×32 canvas.
    fn large(&self) -> bool {
        self.n > 16 * SS
    }

    /// Paints every sample whose centre satisfies `inside`, within the
    /// bounding box (in grid units).
    fn paint(
        &mut self,
        bbox: (f32, f32, f32, f32),
        paint: Paint,
        inside: impl Fn(f32, f32) -> bool,
    ) {
        let (x0, y0, x1, y1) = bbox;
        let s = self.scale;
        let lo = |v: f32| ((v * s).floor().max(0.0) as usize).min(self.n);
        let hi = |v: f32| ((v * s).ceil().max(0.0) as usize).min(self.n);
        let span = (y1 - y0).max(f32::EPSILON);
        for sy in lo(y0)..hi(y1) {
            for sx in lo(x0)..hi(x1) {
                let gx = (sx as f32 + 0.5) / s;
                let gy = (sy as f32 + 0.5) / s;
                if inside(gx, gy) {
                    let t = ((gy - y0) / span).clamp(0.0, 1.0);
                    self.px[sy * self.n + sx] = Some(paint.at(t));
                }
            }
        }
    }

    /// A rectangle with corner radius `r`.
    fn rrect(&mut self, x: f32, y: f32, w: f32, h: f32, r: f32, paint: Paint) {
        self.paint((x, y, x + w, y + h), paint, |px, py| {
            let cx = px.clamp(x + r, x + w - r);
            let cy = py.clamp(y + r, y + h - r);
            let (dx, dy) = (px - cx, py - cy);
            px >= x && px <= x + w && py >= y && py <= y + h && dx * dx + dy * dy <= r * r
        });
    }

    /// A rectangle with an outline of width `t` in `edge` and body `fill`.
    fn boxed(&mut self, x: f32, y: f32, w: f32, h: f32, r: f32, t: f32, edge: Rgb, fill: Paint) {
        self.rrect(x, y, w, h, r, Paint::Flat(edge));
        self.rrect(
            x + t,
            y + t,
            w - 2.0 * t,
            h - 2.0 * t,
            (r - t).max(0.0),
            fill,
        );
    }

    /// A polygon (even-odd rule).
    fn polygon(&mut self, pts: &[(f32, f32)], paint: Paint) {
        let x0 = pts.iter().map(|p| p.0).fold(f32::MAX, f32::min);
        let x1 = pts.iter().map(|p| p.0).fold(f32::MIN, f32::max);
        let y0 = pts.iter().map(|p| p.1).fold(f32::MAX, f32::min);
        let y1 = pts.iter().map(|p| p.1).fold(f32::MIN, f32::max);
        self.paint((x0, y0, x1, y1), paint, |px, py| {
            let mut inside = false;
            let mut j = pts.len() - 1;
            for i in 0..pts.len() {
                let (xi, yi) = pts[i];
                let (xj, yj) = pts[j];
                if (yi > py) != (yj > py) && px < (xj - xi) * (py - yi) / (yj - yi) + xi {
                    inside = !inside;
                }
                j = i;
            }
            inside
        });
    }

    /// A polyline of width `t` with round joins and caps.
    fn stroke(&mut self, pts: &[(f32, f32)], t: f32, c: Rgb) {
        let r = t / 2.0;
        for seg in pts.windows(2) {
            let ((ax, ay), (bx, by)) = (seg[0], seg[1]);
            let bbox = (
                ax.min(bx) - r,
                ay.min(by) - r,
                ax.max(bx) + r,
                ay.max(by) + r,
            );
            self.paint(bbox, Paint::Flat(c), |px, py| {
                let (vx, vy) = (bx - ax, by - ay);
                let len2 = vx * vx + vy * vy;
                let u = if len2 == 0.0 {
                    0.0
                } else {
                    (((px - ax) * vx + (py - ay) * vy) / len2).clamp(0.0, 1.0)
                };
                let (dx, dy) = (px - (ax + u * vx), py - (ay + u * vy));
                dx * dx + dy * dy <= r * r
            });
        }
    }

    /// A filled circle.
    fn dot(&mut self, cx: f32, cy: f32, r: f32, c: Rgb) {
        self.paint(
            (cx - r, cy - r, cx + r, cy + r),
            Paint::Flat(c),
            |px, py| (px - cx).powi(2) + (py - cy).powi(2) <= r * r,
        );
    }

    /// Averages the samples down to RGBA pixels.
    fn finish(self) -> Vec<u8> {
        let size = self.n / SS;
        let mut out = vec![0u8; size * size * 4];
        for y in 0..size {
            for x in 0..size {
                let (mut r, mut g, mut b, mut a) = (0u32, 0u32, 0u32, 0u32);
                for sy in 0..SS {
                    for sx in 0..SS {
                        if let Some(c) = self.px[(y * SS + sy) * self.n + x * SS + sx] {
                            r += c.0 as u32;
                            g += c.1 as u32;
                            b += c.2 as u32;
                            a += 1;
                        }
                    }
                }
                let i = (y * size + x) * 4;
                if a > 0 {
                    // Colour is the average of covered samples; alpha is coverage.
                    out[i] = (r / a) as u8;
                    out[i + 1] = (g / a) as u8;
                    out[i + 2] = (b / a) as u8;
                    out[i + 3] = (a * 255 / (SS * SS) as u32) as u8;
                }
            }
        }
        out
    }
}

fn draw(glyph: Glyph, c: &mut Canvas) {
    use Paint::*;
    use palette::*;
    // Outline width in grid units: one full pixel at 16, a pixel and a half at 32.
    let t = if c.large() { 0.75 } else { 1.0 };
    match glyph {
        Glyph::Terminal => {
            // Monitor with a dark screen and prompt lines, on a stand.
            c.rrect(6.0, 12.0, 4.0, 2.0, 0.0, Shade(STEEL, STEEL_EDGE));
            c.boxed(
                3.5,
                13.25,
                9.0,
                2.0,
                0.75,
                t,
                STEEL_EDGE,
                Shade(STEEL_LIGHT, STEEL),
            );
            c.boxed(
                0.75,
                1.25,
                14.5,
                11.0,
                1.25,
                t,
                STEEL_EDGE,
                Shade(STEEL_LIGHT, STEEL),
            );
            c.rrect(2.25, 2.75, 11.5, 8.0, 0.5, Shade(SCREEN_TOP, SCREEN_BOTTOM));
            c.stroke(&[(3.75, 4.75), (5.25, 5.75), (3.75, 6.75)], 0.9, PROMPT);
            c.stroke(&[(6.5, 6.9), (9.5, 6.9)], 0.9, PROMPT);
            c.stroke(&[(3.9, 8.9), (11.5, 8.9)], 0.6, GRAPHITE_LIGHT);
        }
        Glyph::Printer => {
            // Sheet in, body, sheet out.
            c.boxed(
                3.75,
                0.75,
                8.5,
                6.0,
                0.5,
                t,
                PAPER_EDGE,
                Shade(PAPER, PAPER_SHADE),
            );
            c.stroke(&[(5.25, 2.75), (10.25, 2.75)], 0.5, RULE);
            c.stroke(&[(5.25, 4.5), (9.25, 4.5)], 0.5, RULE);
            c.boxed(
                0.75,
                5.75,
                14.5,
                6.75,
                1.5,
                t,
                STEEL_EDGE,
                Shade(STEEL_LIGHT, STEEL),
            );
            c.dot(12.5, 7.75, 0.7, LED_GREEN);
            c.rrect(3.0, 10.0, 10.0, 1.0, 0.5, Flat(GRAPHITE));
            c.boxed(
                3.75,
                10.5,
                8.5,
                4.75,
                0.5,
                t,
                PAPER_EDGE,
                Shade(PAPER, PAPER_SHADE),
            );
            c.stroke(&[(5.25, 12.75), (10.25, 12.75)], 0.5, RULE);
        }
        Glyph::Table => {
            // A sheet with a blue header row and light rules.
            c.boxed(
                0.75,
                1.75,
                14.5,
                12.5,
                1.0,
                t,
                PAPER_EDGE,
                Shade(PAPER, PAPER_SHADE),
            );
            c.rrect(1.5, 2.5, 13.0, 3.0, 0.5, Shade(BLUE_LIGHT, BLUE));
            for y in [8.5, 11.25] {
                c.stroke(&[(1.75, y), (14.25, y)], 0.5, RULE);
            }
            for x in [6.0, 10.25] {
                c.stroke(&[(x, 5.75), (x, 13.25)], 0.5, RULE);
            }
        }
        Glyph::Folder => {
            // Back with tab, papers, front flap.
            c.polygon(
                &[
                    (0.75, 3.0),
                    (6.0, 3.0),
                    (7.5, 4.5),
                    (15.25, 4.5),
                    (15.25, 14.0),
                    (0.75, 14.0),
                ],
                Flat(AMBER_EDGE),
            );
            c.polygon(
                &[
                    (1.5, 3.75),
                    (5.75, 3.75),
                    (7.25, 5.25),
                    (14.5, 5.25),
                    (14.5, 13.25),
                    (1.5, 13.25),
                ],
                Flat(AMBER_BACK),
            );
            c.rrect(2.75, 5.75, 10.5, 4.0, 0.25, Flat(PAPER));
            c.polygon(
                &[(0.75, 7.0), (15.25, 7.0), (14.75, 14.0), (1.25, 14.0)],
                Flat(AMBER_EDGE),
            );
            c.polygon(
                &[(1.6, 7.75), (14.4, 7.75), (14.0, 13.25), (2.0, 13.25)],
                Shade(AMBER_LIGHT, AMBER),
            );
        }
        Glyph::Disk => {
            // A drive: slab with a slot and a lamp.
            c.boxed(
                0.75,
                4.0,
                14.5,
                8.5,
                1.5,
                t,
                STEEL_EDGE,
                Shade(STEEL_LIGHT, STEEL),
            );
            c.stroke(&[(2.5, 9.0), (13.5, 9.0)], 0.5, PAPER);
            c.rrect(2.5, 6.25, 7.0, 1.25, 0.6, Flat(GRAPHITE));
            c.dot(12.25, 6.9, 0.85, LED_GREEN);
            c.stroke(&[(2.5, 10.75), (6.0, 10.75)], 0.5, STEEL_EDGE);
        }
        Glyph::System => {
            // A graphite server unit with drive bays and a blue lamp.
            c.boxed(
                3.25,
                0.75,
                9.5,
                14.5,
                1.25,
                t,
                GRAPHITE_EDGE,
                Shade(GRAPHITE_LIGHT, GRAPHITE),
            );
            for y in [2.5, 5.0, 7.5] {
                c.boxed(
                    4.75,
                    y,
                    6.5,
                    1.75,
                    0.4,
                    0.4,
                    GRAPHITE_EDGE,
                    Shade(STEEL, STEEL_EDGE),
                );
            }
            c.dot(9.75, 12.5, 0.85, LED_BLUE);
            c.stroke(&[(5.0, 12.5), (7.75, 12.5)], 0.5, GRAPHITE_LIGHT);
        }
        Glyph::Check => {
            c.stroke(&[(2.5, 8.5), (6.25, 12.25), (13.5, 3.75)], 3.0, GREEN_EDGE);
            c.stroke(&[(2.5, 8.5), (6.25, 12.25), (13.5, 3.75)], 1.9, GREEN);
        }
        Glyph::Cross => {
            for seg in [[(3.5, 3.5), (12.5, 12.5)], [(12.5, 3.5), (3.5, 12.5)]] {
                c.stroke(&seg, 3.0, RED_EDGE);
            }
            for seg in [[(3.5, 3.5), (12.5, 12.5)], [(12.5, 3.5), (3.5, 12.5)]] {
                c.stroke(&seg, 1.9, RED);
            }
        }
    }
}

/// RGBA pixels of a glyph, row-major, `size.px()` square. Unused pixels are
/// fully transparent; edges carry partial alpha.
pub fn rgba(glyph: Glyph, size: Size) -> Vec<u8> {
    let mut canvas = Canvas::new(size);
    draw(glyph, &mut canvas);
    canvas.finish()
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
    fn glyphs_have_content_and_smooth_edges() {
        for glyph in Glyph::ALL {
            for size in [Size::Small, Size::Large] {
                let px = rgba(glyph, size);
                assert_eq!(px.len(), (size.px() * size.px() * 4) as usize);
                let opaque = px.chunks(4).filter(|p| p[3] == 0xFF).count();
                let partial = px.chunks(4).filter(|p| p[3] > 0 && p[3] < 0xFF).count();
                assert!(opaque > 20, "{glyph:?} {size:?} is nearly empty");
                assert!(partial > 0, "{glyph:?} {size:?} has no antialiased edge");
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

    #[test]
    fn rendering_is_deterministic() {
        for glyph in Glyph::ALL {
            assert_eq!(rgba(glyph, Size::Large), rgba(glyph, Size::Large));
        }
    }
}
