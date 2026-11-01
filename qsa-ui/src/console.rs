// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! The QS/400 service console's look, for the 5250 display: its 9×16
//! bitmap font, its palette, and its rules for drawing a cell. The rules
//! and values are the console's (`lic/console` in the QS/400 repository;
//! `assets/console/SOURCE.md` names each source); this module only
//! reproduces them, so a 5250 screen here matches the console pixel for
//! pixel.
//!
//! [`render`] draws a grid into an RGB buffer at an integer scale. It is
//! pure, so the widget, the pixel test and Print Screen share it.

use std::sync::OnceLock;

use tn5250::screen::{Cell, Keyboard};

/// Cell width in pixels.
pub const GLYPH_W: usize = 9;
/// Cell height in pixels.
pub const GLYPH_H: usize = 16;

/// The vendored font, `assets/console/qs400-9x16.txt`.
pub const FONT_SOURCE: &str = include_str!("../assets/console/qs400-9x16.txt");

/// The 5250 column separator glyph (private use), as in the console font.
pub const COLUMN_SEPARATOR: char = '\u{e000}';

/// The console's hollow box for characters the font lacks.
static REPLACEMENT: [u16; GLYPH_H] = [
    0x000, 0x000, 0x1fc, 0x104, 0x104, 0x104, 0x104, 0x104, 0x104, 0x104, 0x104, 0x104, 0x1fc,
    0x000, 0x000, 0x000,
];

/// Glyph rows, leftmost pixel in bit 8, sorted by character.
type Glyphs = Vec<(char, [u16; GLYPH_H])>;

fn glyphs() -> &'static Glyphs {
    static G: OnceLock<Glyphs> = OnceLock::new();
    G.get_or_init(|| parse_font(FONT_SOURCE).expect("the vendored console font is valid"))
}

/// Reads the console font format: blocks of `# U+XXXX name` and 16 rows of
/// nine `#` or `.`; other `#` lines are comments.
pub fn parse_font(text: &str) -> Result<Glyphs, String> {
    let mut out: Glyphs = Vec::new();
    let mut lines = text.lines().enumerate();
    while let Some((n, line)) = lines.next() {
        let Some(rest) = line.strip_prefix("# U+") else {
            if line.starts_with('#') || line.trim().is_empty() {
                continue;
            }
            return Err(format!("line {}: unexpected {line:?}", n + 1));
        };
        let hex = rest.split_whitespace().next().unwrap_or("");
        let cp = u32::from_str_radix(hex, 16)
            .ok()
            .and_then(char::from_u32)
            .ok_or_else(|| format!("line {}: bad code point", n + 1))?;
        let mut rows = [0u16; GLYPH_H];
        for row in &mut rows {
            let (m, px) = lines
                .next()
                .ok_or_else(|| format!("U+{:04X}: rows missing", cp as u32))?;
            if px.len() != GLYPH_W {
                return Err(format!("line {}: row must be {GLYPH_W} wide", m + 1));
            }
            for (x, c) in px.chars().enumerate() {
                match c {
                    '#' => *row |= 1 << (GLYPH_W - 1 - x),
                    '.' => {}
                    _ => return Err(format!("line {}: unexpected {c:?}", m + 1)),
                }
            }
        }
        out.push((cp, rows));
    }
    out.sort_by_key(|g| g.0);
    Ok(out)
}

/// The rows of a character's glyph, or the hollow box.
pub fn glyph(c: char) -> &'static [u16; GLYPH_H] {
    let g = glyphs();
    match g.binary_search_by_key(&c, |e| e.0) {
        Ok(i) => &g[i].1,
        Err(_) => &REPLACEMENT,
    }
}

/// Whether the font has a glyph for a character.
pub fn has_glyph(c: char) -> bool {
    glyphs().binary_search_by_key(&c, |e| e.0).is_ok()
}

/// The console palette, `0x00RRGGBB` (`lic/console/src/palette.rs`).
pub mod palette {
    pub const BACKGROUND: u32 = 0x0000_0000;
    pub const GREEN: u32 = 0x0033_CC33;
    pub const WHITE: u32 = 0x00E6_E6E6;
    pub const RED: u32 = 0x00E0_3030;
    pub const BLUE: u32 = 0x0050_80FF;
    pub const TURQUOISE: u32 = 0x0020_C8C8;
    pub const PINK: u32 = 0x00E0_60C0;
    /// Defined by the console; its attribute mapping never selects it.
    pub const YELLOW: u32 = 0x00E6_E040;
}

/// A 5250 colour as the console knows them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Color {
    Green,
    White,
    Red,
    Blue,
    Turquoise,
    Pink,
}

/// How the console draws an attribute (`fb_attr` in
/// `lic/console/src/terminal.rs`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Style {
    pub color: Color,
    pub reverse: bool,
    pub underline: bool,
    pub column_sep: bool,
    pub nondisplay: bool,
}

fn is_nondisplay(a: u8) -> bool {
    a & 0x07 == 0x07
}

/// The style of a 5250 attribute byte.
pub fn style(a: u8) -> Style {
    let color = match a & 0x18 {
        0x00 if a & 0x02 != 0 => Color::White,
        0x00 => Color::Green,
        0x08 => Color::Red,
        // Turquoise and yellow (column separators): the console's palette
        // has one of them.
        0x10 => Color::Turquoise,
        _ if a & 0x02 != 0 => Color::Blue,
        _ => Color::Pink,
    };
    let nd = is_nondisplay(a);
    Style {
        color,
        reverse: a & 0x01 != 0 && !nd,
        underline: a & 0x04 != 0 && !nd,
        column_sep: (0x30..=0x37).contains(&a),
        nondisplay: nd,
    }
}

/// The plain style of an attribute byte's own cell: the console leaves it
/// blank and green.
pub const BLANK: Style = Style {
    color: Color::Green,
    reverse: false,
    underline: false,
    column_sep: false,
    nondisplay: false,
};

/// Colour scheme.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Scheme {
    /// The console's: green on black.
    #[default]
    Console,
    /// Black on the console's white, derived from the same palette.
    BlackOnWhite,
}

impl Scheme {
    pub fn background(self) -> u32 {
        match self {
            Scheme::Console => palette::BACKGROUND,
            Scheme::BlackOnWhite => palette::WHITE,
        }
    }

    pub fn foreground(self, c: Color) -> u32 {
        match (self, c) {
            (Scheme::BlackOnWhite, Color::Green | Color::White) => palette::BACKGROUND,
            (_, Color::Green) => palette::GREEN,
            (_, Color::White) => palette::WHITE,
            (_, Color::Red) => palette::RED,
            (_, Color::Blue) => palette::BLUE,
            (_, Color::Turquoise) => palette::TURQUOISE,
            (_, Color::Pink) => palette::PINK,
        }
    }

    pub fn cursor(self) -> u32 {
        match self {
            Scheme::Console => palette::WHITE,
            Scheme::BlackOnWhite => palette::BACKGROUND,
        }
    }
}

/// One cell to draw.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Paint {
    pub ch: char,
    pub style: Style,
    pub cursor: bool,
    /// Drawn reversed again on top, for a mouse selection.
    pub selected: bool,
}

impl Paint {
    /// A screen cell as the console paints it.
    pub fn of(cell: Cell) -> Paint {
        Paint {
            ch: if cell.is_attr { ' ' } else { cell.ch },
            style: if cell.is_attr {
                BLANK
            } else {
                style(cell.attr)
            },
            cursor: false,
            selected: false,
        }
    }

    /// Plain text in a colour, for the status line.
    pub fn text(ch: char, color: Color) -> Paint {
        Paint {
            ch,
            style: Style { color, ..BLANK },
            cursor: false,
            selected: false,
        }
    }
}

/// How cells are drawn: the scheme, and whether the session is gone (the
/// last screen stays, greyed: every colour halfway to the background).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Look {
    pub scheme: Scheme,
    pub greyed: bool,
}

fn grey(c: u32, bg: u32) -> u32 {
    let ch = |v: u32, s: u32| ((((v >> s) & 0xFF) + ((bg >> s) & 0xFF)) / 2) << s;
    ch(c, 16) | ch(c, 8) | ch(c, 0)
}

/// An RGB buffer, 3 bytes a pixel.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Canvas {
    pub w: usize,
    pub h: usize,
    pub px: Vec<u8>,
}

impl Canvas {
    pub fn new(w: usize, h: usize, rgb: u32) -> Canvas {
        let mut c = Canvas {
            w,
            h,
            px: vec![0; w * h * 3],
        };
        c.fill(0, 0, w, h, rgb);
        c
    }

    /// The `0x00RRGGBB` pixel at (x, y).
    pub fn at(&self, x: usize, y: usize) -> u32 {
        let i = (y * self.w + x) * 3;
        (u32::from(self.px[i]) << 16) | (u32::from(self.px[i + 1]) << 8) | u32::from(self.px[i + 2])
    }

    /// Fills a rectangle, clipped to the canvas.
    pub fn fill(&mut self, x: usize, y: usize, w: usize, h: usize, rgb: u32) {
        let [_, r, g, b] = rgb.to_be_bytes();
        let x1 = (x + w).min(self.w);
        for yy in y..(y + h).min(self.h) {
            let row = yy * self.w * 3;
            for xx in x..x1 {
                let i = row + xx * 3;
                self.px[i..i + 3].copy_from_slice(&[r, g, b]);
            }
        }
    }
}

/// Draws one cell with its top-left glyph pixel at (x, y), each glyph pixel
/// an s×s block. The console's `cells_line`, scanline by scanline.
pub fn paint_cell(c: &mut Canvas, x: usize, y: usize, s: usize, p: Paint, look: Look) {
    let scheme = look.scheme;
    let mut fg = scheme.foreground(p.style.color);
    let mut bg = scheme.background();
    if p.style.reverse {
        (fg, bg) = (bg, fg);
    }
    if p.selected {
        (fg, bg) = (bg, fg);
    }
    let mut cursor = scheme.cursor();
    if look.greyed {
        let base = scheme.background();
        fg = grey(fg, base);
        bg = if bg == base { bg } else { grey(bg, base) };
        cursor = grey(cursor, base);
    }
    let bits = if p.style.nondisplay {
        None
    } else {
        Some(glyph(p.ch))
    };
    for gy in 0..GLYPH_H {
        let row = bits.map_or(0, |b| b[gy]);
        let full = if p.cursor && gy == GLYPH_H - 1 {
            Some(cursor)
        } else if p.style.underline && gy == GLYPH_H - 2 {
            Some(fg)
        } else {
            None
        };
        if let Some(color) = full {
            c.fill(x, y + gy * s, GLYPH_W * s, s, color);
            continue;
        }
        let sep = p.style.column_sep && gy % 4 == 0;
        for gx in 0..GLYPH_W {
            let lit =
                row & (1 << (GLYPH_W - 1 - gx)) != 0 || (sep && (gx == 0 || gx == GLYPH_W - 1));
            c.fill(x + gx * s, y + gy * s, s, s, if lit { fg } else { bg });
        }
    }
}

/// Where a grid of `rows` by `cols` (plus `extra_rows` below it, for the
/// status line) sits in a `w` by `h` area: the console's layout, the
/// largest integer scale that fits, at least 1, centred.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Layout {
    pub scale: usize,
    pub x0: usize,
    pub y0: usize,
    pub rows: usize,
    pub cols: usize,
}

impl Layout {
    pub fn fit(w: usize, h: usize, rows: usize, cols: usize, extra_rows: usize) -> Layout {
        let total = rows + extra_rows;
        let s = (w / (cols * GLYPH_W)).min(h / (total * GLYPH_H)).max(1);
        Layout {
            scale: s,
            x0: w.saturating_sub(cols * GLYPH_W * s) / 2,
            y0: h.saturating_sub(total * GLYPH_H * s) / 2,
            rows,
            cols,
        }
    }

    /// The top-left pixel of a cell.
    pub fn cell_xy(&self, row: usize, col: usize) -> (usize, usize) {
        (
            self.x0 + col * GLYPH_W * self.scale,
            self.y0 + row * GLYPH_H * self.scale,
        )
    }

    /// The cell under a pixel.
    pub fn cell_at(&self, x: usize, y: usize) -> Option<(usize, usize)> {
        let (cw, ch) = (GLYPH_W * self.scale, GLYPH_H * self.scale);
        if x < self.x0 || y < self.y0 {
            return None;
        }
        let (c, r) = ((x - self.x0) / cw, (y - self.y0) / ch);
        (r < self.rows && c < self.cols).then_some((r, c))
    }
}

/// What the status line shows.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Oia {
    pub system: String,
    /// Connected, Connecting, Disconnected ...
    pub state: String,
    pub keyboard: Option<Keyboard>,
    pub insert: bool,
    pub message_waiting: bool,
    pub tls: bool,
    /// 1-based row and column.
    pub cursor: (usize, usize),
}

/// The status line as cells across `cols` columns. Columns are 1-based:
///
/// | Columns | Shows |
/// |---|---|
/// | 1-9 | keyboard: `X SYSTEM`, `X II`, `X II nnnn` (white) |
/// | 12-13 | `MW` when a message waits |
/// | 16-18 | `INS` in insert mode |
/// | 21-23 | `TLS` on a TLS session |
/// | 26- | system and session state |
/// | last 6 | cursor `RR/CCC` |
pub fn oia_cells(o: &Oia, cols: usize) -> Vec<Paint> {
    let mut cells = vec![Paint::text(' ', Color::Green); cols];
    let mut put = |col: usize, text: &str, color: Color, max: usize| {
        for (i, ch) in text.chars().take(max).enumerate() {
            if let Some(c) = cells.get_mut(col - 1 + i) {
                *c = Paint::text(ch, color);
            }
        }
    };
    let kb = match o.keyboard {
        Some(Keyboard::System) => "X SYSTEM".to_owned(),
        Some(Keyboard::HostError) => "X II".to_owned(),
        Some(Keyboard::OperatorError(c)) => format!("X II {c:04}"),
        Some(Keyboard::Open) | None => String::new(),
    };
    put(1, &kb, Color::White, 9);
    if o.message_waiting {
        put(12, "MW", Color::Green, 2);
    }
    if o.insert {
        put(16, "INS", Color::Green, 3);
    }
    if o.tls {
        put(21, "TLS", Color::Green, 3);
    }
    let room = cols.saturating_sub(26 + 7);
    let label = if o.state.is_empty() {
        o.system.clone()
    } else {
        format!("{} {}", o.system, o.state)
    };
    put(26, &label, Color::Green, room);
    let rc = format!("{:02}/{:03}", o.cursor.0, o.cursor.1);
    put(cols.saturating_sub(5).max(1), &rc, Color::Green, 6);
    cells
}

/// Draws whole rows of paints into a canvas at a layout. `cells` holds
/// `rows * cols` paints row by row; the status line, when given, goes on
/// the row below the grid.
pub fn render(c: &mut Canvas, l: Layout, cells: &[Paint], status: Option<&[Paint]>, look: Look) {
    for (i, p) in cells.iter().enumerate().take(l.rows * l.cols) {
        let (x, y) = l.cell_xy(i / l.cols, i % l.cols);
        paint_cell(c, x, y, l.scale, *p, look);
    }
    if let Some(st) = status {
        for (col, p) in st.iter().enumerate().take(l.cols) {
            let (x, y) = l.cell_xy(l.rows, col);
            paint_cell(c, x, y, l.scale, *p, look);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_vendored_font_has_the_console_glyph_set() {
        let g = glyphs();
        // ASCII 0x20-0x7E, eleven box-drawing glyphs, the column separator.
        assert_eq!(g.len(), 95 + 11 + 1);
        for b in 0x21u8..=0x7E {
            assert!(glyph(b as char).iter().any(|r| *r != 0), "{}", b as char);
        }
        assert!(glyph(' ').iter().all(|r| *r == 0));
        assert!(has_glyph(COLUMN_SEPARATOR) && has_glyph('\u{253c}'));
        assert!(std::ptr::eq(glyph('é'), &REPLACEMENT));
        assert!(
            g.iter()
                .all(|(_, rows)| rows.iter().all(|r| *r < 1 << GLYPH_W))
        );
    }

    #[test]
    fn attributes_map_as_on_the_console() {
        assert_eq!(style(0x20).color, Color::Green);
        assert_eq!(style(0x22).color, Color::White);
        assert!(style(0x21).reverse && !style(0x21).underline);
        assert!(style(0x24).underline);
        let nd = style(0x27);
        assert!(nd.nondisplay && !nd.underline && !nd.reverse);
        assert_eq!(style(0x28).color, Color::Red);
        assert_eq!(style(0x2A).color, Color::Red, "blink is not drawn");
        assert_eq!(style(0x30).color, Color::Turquoise);
        assert!(style(0x30).column_sep);
        assert_eq!(
            style(0x34).color,
            Color::Turquoise,
            "yellow draws turquoise"
        );
        assert_eq!(style(0x38).color, Color::Pink);
        assert_eq!(style(0x3A).color, Color::Blue);
        assert!(style(0x3E).underline && style(0x3F).nondisplay);
    }

    fn one(p: Paint, look: Look) -> Canvas {
        let mut c = Canvas::new(GLYPH_W, GLYPH_H, 0x123456);
        paint_cell(&mut c, 0, 0, 1, p, look);
        c
    }

    #[test]
    fn a_glyph_lights_its_bits_in_the_foreground() {
        let c = one(Paint::text('H', Color::Green), Look::default());
        for (y, bits) in glyph('H').iter().enumerate() {
            for x in 0..GLYPH_W {
                let lit = bits & (1 << (GLYPH_W - 1 - x)) != 0;
                let want = if lit {
                    palette::GREEN
                } else {
                    palette::BACKGROUND
                };
                assert_eq!(c.at(x, y), want, "({x}, {y})");
            }
        }
    }

    #[test]
    fn underline_cursor_reverse_and_column_separators() {
        let mut p = Paint::of(Cell {
            ch: ' ',
            attr: 0x24,
            is_attr: false,
        });
        let c = one(p, Look::default());
        assert!((0..GLYPH_W).all(|x| c.at(x, 14) == palette::GREEN));
        assert!((0..GLYPH_W).all(|x| c.at(x, 15) == palette::BACKGROUND));
        p.cursor = true;
        let c = one(p, Look::default());
        assert!((0..GLYPH_W).all(|x| c.at(x, 15) == palette::WHITE));
        // Reverse: the cell is green, the glyph black.
        let c = one(
            Paint::of(Cell {
                ch: ' ',
                attr: 0x21,
                is_attr: false,
            }),
            Look::default(),
        );
        assert_eq!(c.at(4, 4), palette::GREEN);
        // Column separators: first and last column every fourth scanline.
        let c = one(
            Paint::of(Cell {
                ch: ' ',
                attr: 0x30,
                is_attr: false,
            }),
            Look::default(),
        );
        assert_eq!(c.at(0, 0), palette::TURQUOISE);
        assert_eq!(c.at(8, 12), palette::TURQUOISE);
        assert_eq!(c.at(0, 1), palette::BACKGROUND);
        assert_eq!(c.at(4, 0), palette::BACKGROUND);
        // An attribute byte's cell is blank, even under an underline attribute.
        let c = one(
            Paint::of(Cell {
                ch: ' ',
                attr: 0x24,
                is_attr: true,
            }),
            Look::default(),
        );
        assert!((0..GLYPH_W).all(|x| c.at(x, 14) == palette::BACKGROUND));
    }

    #[test]
    fn nondisplay_draws_no_glyph() {
        let c = one(
            Paint::of(Cell {
                ch: 'X',
                attr: 0x27,
                is_attr: false,
            }),
            Look::default(),
        );
        assert!(c.px.chunks(3).all(|p| p == [0, 0, 0]));
    }

    #[test]
    fn scale_makes_uniform_blocks() {
        let mut c = Canvas::new(GLYPH_W * 3, GLYPH_H * 3, 0);
        paint_cell(
            &mut c,
            0,
            0,
            3,
            Paint::text('A', Color::White),
            Look::default(),
        );
        for (y, bits) in glyph('A').iter().enumerate() {
            for x in 0..GLYPH_W {
                let lit = bits & (1 << (GLYPH_W - 1 - x)) != 0;
                for (dx, dy) in [(0, 0), (2, 0), (0, 2), (2, 2)] {
                    assert_eq!(c.at(x * 3 + dx, y * 3 + dy) != 0, lit);
                }
            }
        }
    }

    #[test]
    fn layout_is_the_consoles() {
        // The console's own numbers, at 24×80 without a status line.
        let l = Layout::fit(1920, 1080, 24, 80, 0);
        assert_eq!((l.scale, l.x0, l.y0), (2, 240, 156));
        let l = Layout::fit(1024, 768, 24, 80, 0);
        assert_eq!((l.scale, l.x0, l.y0), (1, 152, 192));
        assert_eq!(Layout::fit(2560, 1440, 27, 132, 0).scale, 2);
        assert_eq!(Layout::fit(100, 100, 24, 80, 1).scale, 1);
        let l = Layout::fit(720, 400, 24, 80, 1);
        assert_eq!(l.cell_at(0, 0), Some((0, 0)));
        assert_eq!(l.cell_at(719, 383), Some((23, 79)));
        assert_eq!(l.cell_at(719, 390), None, "the status line is not a cell");
    }

    #[test]
    fn black_on_white_and_greyed() {
        let look = Look {
            scheme: Scheme::BlackOnWhite,
            greyed: false,
        };
        let c = one(Paint::text('H', Color::Green), look);
        assert_eq!(c.at(8, 0), palette::WHITE);
        let lit = (0..GLYPH_H)
            .flat_map(|y| (0..GLYPH_W).map(move |x| (x, y)))
            .find(|&(x, y)| glyph('H')[y] & (1 << (GLYPH_W - 1 - x)) != 0)
            .unwrap();
        assert_eq!(c.at(lit.0, lit.1), palette::BACKGROUND);
        let greyed = Look {
            scheme: Scheme::Console,
            greyed: true,
        };
        let c = one(Paint::text('H', Color::Green), greyed);
        assert_eq!(c.at(lit.0, lit.1), 0x0019_6619);
    }

    #[test]
    fn status_line_layout() {
        let o = Oia {
            system: "QS400".into(),
            state: "Connected".into(),
            keyboard: Some(Keyboard::OperatorError(5)),
            insert: true,
            message_waiting: true,
            tls: true,
            cursor: (6, 53),
        };
        let t: String = oia_cells(&o, 80).iter().map(|p| p.ch).collect();
        assert_eq!(&t[..9], "X II 0005");
        assert_eq!(&t[11..13], "MW");
        assert_eq!(&t[15..18], "INS");
        assert_eq!(&t[20..23], "TLS");
        assert_eq!(&t[25..40], "QS400 Connected");
        assert_eq!(&t[74..], "06/053");
        assert_eq!(oia_cells(&o, 80)[0].style.color, Color::White);
        assert_eq!(oia_cells(&o, 132).len(), 132);
    }
}
