// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! The glyph set: 16×16 and 32×32 icons drawn in code, and the
//! application and file-type icons made from them at any size.
//!
//! Glyphs are described with a few vector shapes on a 16-unit grid and
//! rendered on a 2× supersampled canvas, then averaged down: crisp, with just
//! enough smoothing on diagonals. Outlines are one dark pixel at 16×16 and
//! 32×32 (thicker in proportion above that); fills are saturated and shade
//! lightly from top to bottom.
//!
//! This crate has no dependencies, so that build scripts can draw the icons
//! the binaries embed.

/// An RGB colour.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rgb(pub u8, pub u8, pub u8);

const fn hex(v: u32) -> Rgb {
    Rgb((v >> 16) as u8, (v >> 8) as u8, v as u8)
}

/// The glyph palette: saturated tones, each with a light shade and a dark edge.
pub mod palette {
    use super::{Rgb, hex};
    pub const STEEL_LIGHT: Rgb = hex(0xF4F4F4);
    pub const STEEL: Rgb = hex(0xC6C6C6);
    pub const STEEL_EDGE: Rgb = hex(0x3E3E3E);
    pub const GRAPHITE_LIGHT: Rgb = hex(0xEAE5D7);
    pub const GRAPHITE: Rgb = hex(0xBFB7A3);
    pub const GRAPHITE_EDGE: Rgb = hex(0x57513F);
    pub const SCREEN_TOP: Rgb = hex(0x0E1E44);
    pub const SCREEN_BOTTOM: Rgb = hex(0x02060E);
    pub const PROMPT: Rgb = hex(0x22E022);
    pub const PAPER: Rgb = hex(0xFFFFFF);
    pub const PAPER_SHADE: Rgb = hex(0xEDEDED);
    pub const PAPER_EDGE: Rgb = hex(0x4A4A4A);
    pub const RULE: Rgb = hex(0x8EA6CF);
    pub const BLUE_LIGHT: Rgb = hex(0x7BA6F0);
    pub const BLUE: Rgb = hex(0x2A5FC6);
    pub const BLUE_EDGE: Rgb = hex(0x163A82);
    pub const AMBER_LIGHT: Rgb = hex(0xFFE58C);
    pub const AMBER: Rgb = hex(0xF2BE45);
    pub const AMBER_BACK: Rgb = hex(0xDCA22C);
    pub const AMBER_EDGE: Rgb = hex(0x7D5A0C);
    pub const GREEN: Rgb = hex(0x23A02A);
    pub const GREEN_EDGE: Rgb = hex(0x0F5E14);
    pub const LED_GREEN: Rgb = hex(0x18D018);
    pub const LED_BLUE: Rgb = hex(0x2F8CFF);
    pub const RED: Rgb = hex(0xE2311F);
    pub const RED_EDGE: Rgb = hex(0x821408);
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
    Connect,
    Disconnect,
    Copy,
    Paste,
    Keypad,
    Keyboard,
    /// A padlock: the connection is encrypted.
    Lock,
}

impl Glyph {
    /// Every glyph, in a stable order.
    pub const ALL: [Glyph; 15] = [
        Glyph::Terminal,
        Glyph::Printer,
        Glyph::Table,
        Glyph::Folder,
        Glyph::Disk,
        Glyph::System,
        Glyph::Check,
        Glyph::Cross,
        Glyph::Connect,
        Glyph::Disconnect,
        Glyph::Copy,
        Glyph::Paste,
        Glyph::Keypad,
        Glyph::Keyboard,
        Glyph::Lock,
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
            Glyph::Connect => "connect",
            Glyph::Disconnect => "disconnect",
            Glyph::Copy => "copy",
            Glyph::Paste => "paste",
            Glyph::Keypad => "keypad",
            Glyph::Keyboard => "keyboard",
            Glyph::Lock => "lock",
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
const SS: usize = 2;

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
        Canvas::of(size.px() as usize)
    }

    /// A canvas for a `px`-pixel square.
    fn of(px: usize) -> Canvas {
        let n = px * SS;
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

    /// A rectangle `[x, y, w, h]` with an outline of width `t` in `edge` and
    /// body `fill`.
    fn boxed(&mut self, rect: [f32; 4], r: f32, t: f32, edge: Rgb, fill: Paint) {
        let [x, y, w, h] = rect;
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
                // Colour is the average of covered samples; alpha is coverage.
                if let (Some(r), Some(g), Some(b)) =
                    (r.checked_div(a), g.checked_div(a), b.checked_div(a))
                {
                    out[i] = r as u8;
                    out[i + 1] = g as u8;
                    out[i + 2] = b as u8;
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
    // Outline width in grid units: one pixel at either size.
    let t = if c.large() { 0.5 } else { 1.0 };
    match glyph {
        Glyph::Terminal => {
            // Monitor with a dark screen and prompt lines, on a stand.
            c.rrect(6.0, 12.0, 4.0, 2.0, 0.0, Shade(STEEL, STEEL_EDGE));
            c.boxed(
                [3.5, 13.25, 9.0, 2.0],
                0.75,
                t,
                STEEL_EDGE,
                Shade(STEEL_LIGHT, STEEL),
            );
            c.boxed(
                [0.75, 1.25, 14.5, 11.0],
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
                [3.75, 0.75, 8.5, 6.0],
                0.5,
                t,
                PAPER_EDGE,
                Shade(PAPER, PAPER_SHADE),
            );
            c.stroke(&[(5.25, 2.75), (10.25, 2.75)], 0.5, RULE);
            c.stroke(&[(5.25, 4.5), (9.25, 4.5)], 0.5, RULE);
            c.boxed(
                [0.75, 5.75, 14.5, 6.75],
                1.5,
                t,
                STEEL_EDGE,
                Shade(STEEL_LIGHT, STEEL),
            );
            c.dot(12.5, 7.75, 0.7, LED_GREEN);
            c.rrect(3.0, 10.0, 10.0, 1.0, 0.5, Flat(GRAPHITE));
            c.boxed(
                [3.75, 10.5, 8.5, 4.75],
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
                [0.75, 1.75, 14.5, 12.5],
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
                [0.75, 4.0, 14.5, 8.5],
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
            // A server unit in a light case, with drive bays and a blue lamp.
            c.boxed(
                [3.25, 0.75, 9.5, 14.5],
                1.25,
                t,
                GRAPHITE_EDGE,
                Shade(GRAPHITE_LIGHT, GRAPHITE),
            );
            for y in [2.5, 5.0, 7.5] {
                c.boxed(
                    [4.75, y, 6.5, 1.75],
                    0.4,
                    0.4,
                    GRAPHITE_EDGE,
                    Shade(STEEL, STEEL_EDGE),
                );
            }
            c.dot(9.75, 12.5, 0.85, LED_BLUE);
            c.stroke(&[(5.0, 12.5), (7.75, 12.5)], 0.5, GRAPHITE_EDGE);
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
        Glyph::Connect => {
            // Two plugs pushed together, the cable on either side.
            c.stroke(&[(0.5, 8.0), (3.0, 8.0)], 1.6, GRAPHITE_EDGE);
            c.stroke(&[(13.0, 8.0), (15.5, 8.0)], 1.6, GRAPHITE_EDGE);
            c.boxed(
                [2.5, 4.5, 5.5, 7.0],
                1.0,
                t,
                STEEL_EDGE,
                Shade(STEEL_LIGHT, STEEL),
            );
            c.boxed(
                [8.0, 4.5, 5.5, 7.0],
                1.0,
                t,
                STEEL_EDGE,
                Shade(STEEL_LIGHT, STEEL),
            );
            c.dot(8.0, 8.0, 1.3, LED_GREEN);
        }
        Glyph::Lock => {
            // A steel shackle over an amber body with a keyhole.
            let shackle: Vec<(f32, f32)> = (0..=12)
                .map(|i| {
                    let a = std::f32::consts::PI * (1.0 + i as f32 / 12.0);
                    (8.0 + 3.2 * a.cos(), 6.5 + 3.2 * a.sin())
                })
                .collect();
            let mut path = vec![(4.8, 8.0)];
            path.extend(shackle);
            path.push((11.2, 8.0));
            c.stroke(&path, 2.2, STEEL_EDGE);
            c.stroke(&path, 1.0, STEEL);
            c.boxed(
                [2.5, 7.5, 11.0, 7.5],
                1.0,
                t,
                AMBER_EDGE,
                Shade(AMBER_LIGHT, AMBER),
            );
            c.dot(8.0, 10.4, 1.1, AMBER_EDGE);
            c.stroke(&[(8.0, 10.4), (8.0, 12.6)], 1.0, AMBER_EDGE);
        }
        Glyph::Disconnect => {
            // The two plugs apart, prongs showing, a red bar between.
            c.stroke(&[(0.5, 8.0), (2.0, 8.0)], 1.6, GRAPHITE_EDGE);
            c.stroke(&[(14.0, 8.0), (15.5, 8.0)], 1.6, GRAPHITE_EDGE);
            c.boxed(
                [1.5, 4.5, 4.5, 7.0],
                1.0,
                t,
                STEEL_EDGE,
                Shade(STEEL_LIGHT, STEEL),
            );
            c.stroke(&[(6.0, 6.25), (7.0, 6.25)], 1.0, STEEL_EDGE);
            c.stroke(&[(6.0, 9.75), (7.0, 9.75)], 1.0, STEEL_EDGE);
            c.boxed(
                [10.0, 4.5, 4.5, 7.0],
                1.0,
                t,
                STEEL_EDGE,
                Shade(STEEL_LIGHT, STEEL),
            );
            c.stroke(&[(9.25, 2.5), (6.75, 13.5)], 2.2, RED_EDGE);
            c.stroke(&[(9.25, 2.5), (6.75, 13.5)], 1.2, RED);
        }
        Glyph::Copy => {
            // Two sheets, one over the other.
            c.boxed(
                [1.25, 0.75, 8.5, 10.5],
                0.5,
                t,
                PAPER_EDGE,
                Shade(PAPER, PAPER_SHADE),
            );
            c.boxed(
                [6.25, 4.75, 8.5, 10.5],
                0.5,
                t,
                PAPER_EDGE,
                Shade(PAPER, PAPER_SHADE),
            );
            for y in [7.5, 9.5, 11.5] {
                c.stroke(&[(8.0, y), (13.0, y)], 0.6, RULE);
            }
        }
        Glyph::Paste => {
            // A clipboard with a sheet on it.
            c.boxed(
                [1.75, 2.0, 12.5, 13.25],
                1.0,
                t,
                AMBER_EDGE,
                Shade(AMBER, AMBER_BACK),
            );
            c.boxed([4.0, 4.75, 8.0, 9.25], 0.25, t, PAPER_EDGE, Flat(PAPER));
            c.boxed(
                [5.25, 0.75, 5.5, 3.0],
                0.75,
                t,
                STEEL_EDGE,
                Shade(STEEL_LIGHT, STEEL),
            );
            for y in [7.25, 9.25, 11.25] {
                c.stroke(&[(5.5, y), (10.5, y)], 0.6, RULE);
            }
        }
        Glyph::Keypad => {
            // A frame with three rows of three keys.
            c.boxed(
                [1.25, 0.75, 13.5, 14.5],
                1.0,
                t,
                STEEL_EDGE,
                Shade(STEEL_LIGHT, STEEL),
            );
            for row in 0..3 {
                for col in 0..3 {
                    let (x, y) = (2.75 + col as f32 * 3.75, 2.25 + row as f32 * 3.75);
                    c.boxed(
                        [x, y, 3.0, 3.0],
                        0.5,
                        t,
                        STEEL_EDGE,
                        Shade(PAPER, PAPER_SHADE),
                    );
                }
            }
            c.boxed([2.75, 13.0, 10.5, 1.0], 0.0, 0.0, BLUE, Flat(BLUE));
        }
        Glyph::Keyboard => {
            // A keyboard: a wide body, two rows of keys and a space bar.
            c.boxed(
                [0.5, 3.75, 15.0, 8.75],
                1.0,
                t,
                STEEL_EDGE,
                Shade(STEEL_LIGHT, STEEL),
            );
            for row in 0..2 {
                for col in 0..5 {
                    let (x, y) = (2.0 + col as f32 * 2.5, 5.25 + row as f32 * 2.25);
                    c.rrect(x, y, 1.75, 1.5, 0.25, Flat(PAPER));
                }
            }
            c.rrect(4.5, 9.75, 7.0, 1.5, 0.25, Flat(PAPER));
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

/// RGBA pixels of a glyph at any size, `px` square (16 and 32 give the
/// same as [`rgba`]).
pub fn rgba_px(glyph: Glyph, px: usize) -> Vec<u8> {
    let mut canvas = Canvas::of(px);
    draw(glyph, &mut canvas);
    canvas.finish()
}

/// The icons the program and its files are shown with.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Icon {
    /// The application.
    App,
    /// A display session file (`.qsd`).
    Session,
    /// A keyboard map file (`.qkm`).
    KeyMap,
    /// A macro file (`.qsm`).
    Macro,
    /// A printer session file (`.qsp`).
    PrinterSession,
    /// A Data Transfer request file (`.qdx`).
    Transfer,
}

impl Icon {
    /// Every icon, in a stable order.
    pub const ALL: [Icon; 6] = [
        Icon::App,
        Icon::Session,
        Icon::KeyMap,
        Icon::Macro,
        Icon::PrinterSession,
        Icon::Transfer,
    ];

    /// The icon's name: the file name stem it is written under. A file
    /// icon is named after its MIME type, as icon themes expect.
    pub fn name(self) -> &'static str {
        match self {
            Icon::App => "quartz-access",
            Icon::Session => "application-x-qsaccess-session",
            Icon::KeyMap => "application-x-qsaccess-keymap",
            Icon::Macro => "application-x-qsaccess-macro",
            Icon::PrinterSession => "application-x-qsaccess-printer",
            Icon::Transfer => "application-x-qsaccess-transfer",
        }
    }

    /// The glyph a file icon carries on its page.
    fn badge(self) -> Option<Glyph> {
        match self {
            Icon::App => None,
            Icon::Session => Some(Glyph::Terminal),
            Icon::KeyMap => Some(Glyph::Keyboard),
            Icon::Macro => Some(Glyph::Table),
            Icon::PrinterSession => Some(Glyph::Printer),
            Icon::Transfer => Some(Glyph::Copy),
        }
    }
}

/// `top` drawn over `under` at (`x`, `y`), both RGBA.
fn composite(under: &mut [u8], size: usize, top: &[u8], tsize: usize, x: usize, y: usize) {
    for ty in 0..tsize {
        for tx in 0..tsize {
            let (ux, uy) = (x + tx, y + ty);
            if ux >= size || uy >= size {
                continue;
            }
            let t = &top[(ty * tsize + tx) * 4..][..4];
            let u = &mut under[(uy * size + ux) * 4..][..4];
            let ta = u32::from(t[3]);
            let ua = u32::from(u[3]);
            let out_a = ta + ua * (255 - ta) / 255;
            if out_a == 0 {
                continue;
            }
            for k in 0..3 {
                let c = (u32::from(t[k]) * ta + u32::from(u[k]) * ua * (255 - ta) / 255) / out_a;
                u[k] = c as u8;
            }
            u[3] = out_a as u8;
        }
    }
}

/// RGBA pixels of an icon, `px` square. The application icon is the
/// terminal glyph; a file icon is a page with a dog-eared corner and the
/// file kind's glyph at its lower right.
pub fn icon_rgba(icon: Icon, px: usize) -> Vec<u8> {
    let Some(badge) = icon.badge() else {
        return rgba_px(Glyph::Terminal, px);
    };
    use palette::*;
    let mut c = Canvas::of(px);
    let t = if c.large() { 0.5 } else { 1.0 };
    let page = [
        (2.0, 0.5),
        (10.0, 0.5),
        (14.0, 4.5),
        (14.0, 15.5),
        (2.0, 15.5),
    ];
    c.polygon(&page, Paint::Flat(PAPER_EDGE));
    let inner = [
        (2.0 + t, 0.5 + t),
        (10.0 - t * 0.4, 0.5 + t),
        (14.0 - t, 4.5 + t * 0.4),
        (14.0 - t, 15.5 - t),
        (2.0 + t, 15.5 - t),
    ];
    c.polygon(&inner, Paint::Shade(PAPER, PAPER_SHADE));
    c.polygon(
        &[(10.0, 0.5), (10.0, 4.5), (14.0, 4.5)],
        Paint::Flat(PAPER_EDGE),
    );
    c.polygon(
        &[
            (10.0 + t, 0.5 + 2.4 * t),
            (10.0 + t, 4.5 - t),
            (14.0 - 2.4 * t, 4.5 - t),
        ],
        Paint::Flat(PAPER_SHADE),
    );
    for i in 0..3 {
        let y = 3.5 + 2.0 * i as f32;
        c.rrect(4.0, y, 4.5, t.max(0.75), 0.0, Paint::Flat(RULE));
    }
    let mut out = c.finish();
    let bpx = (px * 5).div_ceil(8);
    let b = rgba_px(badge, bpx);
    composite(&mut out, px, &b, bpx, px - bpx, px - bpx);
    out
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
    fn any_size_matches_the_two_set_sizes() {
        for glyph in Glyph::ALL {
            assert_eq!(rgba_px(glyph, 16), rgba(glyph, Size::Small));
            assert_eq!(rgba_px(glyph, 32), rgba(glyph, Size::Large));
        }
    }

    #[test]
    fn icons_at_every_size_are_distinct_and_filled() {
        for px in [16, 32, 48, 128, 256] {
            let all: Vec<Vec<u8>> = Icon::ALL.iter().map(|i| icon_rgba(*i, px)).collect();
            for (i, a) in all.iter().enumerate() {
                assert_eq!(a.len(), px * px * 4);
                let opaque = a.chunks(4).filter(|p| p[3] == 0xFF).count();
                assert!(
                    opaque * 4 > px * px,
                    "{:?} at {px} is mostly empty",
                    Icon::ALL[i]
                );
                for b in &all[i + 1..] {
                    assert_ne!(a, b);
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
