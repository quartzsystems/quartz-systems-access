// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! SCS to the page model.
//!
//! The parser keeps a cursor as a 1-based line and column. A character
//! prints at the column's position for the pitch in force and at the
//! line's position for the line spacing in force; both are in 1/1440 inch
//! from the page's top-left corner.
//!
//! - Overprinting the same character in the same place makes it bold; a
//!   different character is kept as well, drawn over the first.
//! - A form feed ends the page. A form feed on an empty page makes a blank
//!   page, except at the start of the job and right after the page ended by
//!   itself (past the bottom margin, or an absolute move up the page).
//! - Without an explicit page size the page is the chosen paper, turned to
//!   landscape when the text is wider than the paper.
//! - Unknown or unsupported controls are skipped by their length and logged
//!   once per job at debug level; a command cut short by the end of the job
//!   ends the parse.

use std::collections::{BTreeSet, HashMap};

use tn5250::codepage::{self, CodePage};

use crate::codes::{self, BYPASS_WHITE_SPACE, class, d1, d2, d3, d4, pp};
use crate::font::{self, DEFAULT_FONT};
use crate::model::{Document, Margins, Orientation, Page, Paper, Pitch, Run};

/// What a job is read with.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Options {
    /// The code page the job starts in.
    pub ccsid: u16,
    /// Paper used when the data does not set a page size.
    pub paper: Paper,
    /// The font the printer device was created with: the starting pitch
    /// and face.
    pub font: u16,
}

impl Default for Options {
    fn default() -> Options {
        Options {
            ccsid: 37,
            paper: Paper::Letter,
            font: DEFAULT_FONT,
        }
    }
}

/// Reads a whole print job.
pub fn parse(data: &[u8], options: &Options) -> Document {
    let mut p = Parser::new(options);
    p.run(data);
    p.finish()
}

/// [`parse`], also returning what was logged once for the job.
#[cfg(test)]
fn parse_logged(data: &[u8], options: &Options) -> (Document, Vec<String>) {
    let mut p = Parser::new(options);
    p.run(data);
    let logged = p.logged.iter().cloned().collect();
    (p.finish(), logged)
}

/// Default page length in lines, and default line spacing (6 lines per
/// inch).
const DEFAULT_MPL: u16 = 66;
const DEFAULT_SPACING: u32 = 240;
/// The character printed for characters that cannot print, until the data
/// sets another.
const DEFAULT_ERROR_CHAR: char = '-';

#[derive(Clone, Debug)]
struct Glyph {
    x: u32,
    y: u32,
    height: u32,
    ch: char,
    font_id: u16,
    pitch: Pitch,
    bold: bool,
    italic: bool,
    underline: bool,
    line: u16,
    col: u16,
}

/// How the current page began.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PageStart {
    /// The start of the job.
    Job,
    /// A form feed.
    FormFeed,
    /// The previous page ended by itself.
    Overflow,
}

struct Parser {
    options: Options,
    page_cp: &'static dyn CodePage,
    // Font.
    font_id: u16,
    pitch: Pitch,
    font_bold: bool,
    font_italic: bool,
    emphasis: bool,
    // Horizontal format.
    auto_new_line: bool,
    mpp: u16,
    lm: u16,
    htabs: Vec<u16>,
    indent: u16,
    // Vertical format.
    spacing: u32,
    /// Half lines a new line moves (set line spacing).
    nl_half_lines: u16,
    tm: u16,
    bm: u16,
    vtabs: Vec<u16>,
    // Cursor.
    line: u16,
    col: u16,
    /// Top of the current line.
    y: u32,
    /// Superscript (negative) and subscript (positive), in half lines.
    half: i32,
    // Page.
    page_size: Option<(u32, u32)>,
    orientation: Option<Orientation>,
    page_start: PageStart,
    // Modes.
    underline: Option<u8>,
    overstrike: Option<(char, u8)>,
    inhibit: bool,
    error_char: char,
    /// The first glyph of the word being printed, for word underscore.
    word_start: usize,
    // Output.
    glyphs: Vec<Glyph>,
    cells: HashMap<(u32, u32), Vec<usize>>,
    pages: Vec<Page>,
    // Logged once per job.
    logged: BTreeSet<String>,
}

fn be16(p: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_be_bytes([*p.get(at)?, *p.get(at + 1)?]))
}

/// The code page an SCS code page ID selects, among the built-in tables.
fn code_page(cpgid: u16) -> Option<&'static dyn CodePage> {
    let id = match cpgid {
        1141 => 273,
        1146 => 285,
        1147 => 297,
        1148 => 500,
        other => other,
    };
    codepage::by_ccsid(id)
}

/// The code page a local ID selects (`FF`: the job's own).
fn local_id_code_page(lcid: u8) -> Option<u16> {
    Some(match lcid {
        0x00 => 500,
        0x01 => 37,
        0x02 => 273,
        0x03 => 274,
        0x04 => 275,
        0x05 | 0x08 => 297,
        0x06 => 277,
        0x07 => 278,
        0x09 => 280,
        0x0A | 0x0B => 281,
        0x0C => 282,
        0x0D | 0x0E => 284,
        0x0F => 285,
        _ => return None,
    })
}

impl Parser {
    fn new(options: &Options) -> Parser {
        let f = font::info(options.font);
        Parser {
            options: options.clone(),
            page_cp: codepage::by_ccsid(options.ccsid).unwrap_or_else(codepage::default_page),
            font_id: options.font,
            pitch: f.pitch.unwrap_or_default(),
            font_bold: f.bold,
            font_italic: f.italic,
            emphasis: false,
            auto_new_line: false,
            mpp: 132,
            lm: 1,
            htabs: Vec::new(),
            indent: 1,
            spacing: DEFAULT_SPACING,
            nl_half_lines: 2,
            tm: 1,
            bm: DEFAULT_MPL,
            vtabs: Vec::new(),
            line: 1,
            col: 1,
            y: 0,
            half: 0,
            page_size: None,
            orientation: None,
            page_start: PageStart::Job,
            underline: None,
            overstrike: None,
            inhibit: false,
            error_char: DEFAULT_ERROR_CHAR,
            word_start: 0,
            glyphs: Vec::new(),
            cells: HashMap::new(),
            pages: Vec::new(),
            logged: BTreeSet::new(),
        }
    }

    /// Logs `what` at debug level the first time it happens in the job.
    fn once(&mut self, what: String) {
        if !self.logged.contains(&what) {
            tn5250::debug!("scs: {what}");
            self.logged.insert(what);
        }
    }

    fn run(&mut self, data: &[u8]) {
        let mut pos = 0;
        while pos < data.len() {
            pos = self.step(data, pos);
        }
    }

    /// Reads the control or character at `pos`; returns where the next one
    /// starts.
    fn step(&mut self, data: &[u8], pos: usize) -> usize {
        let b = data[pos];
        // The operand of a two- or three-byte control, or the end of the
        // job if it is cut short.
        let arg = |i: usize| data.get(pos + i).copied();
        match b {
            codes::NUL | codes::RPT | codes::SW | codes::BEL => pos + 1,
            codes::ENP => {
                self.inhibit = false;
                pos + 1
            }
            codes::INP => {
                self.inhibit = true;
                pos + 1
            }
            codes::ATRN | codes::TRN => {
                let Some(n) = arg(1) else {
                    return self.truncated(b, data.len());
                };
                let start = pos + 2;
                let end = (start + n as usize).min(data.len());
                if b == codes::TRN {
                    for &c in &data[start..end] {
                        let ch = if c < 0x40 || c == 0xFF {
                            self.error_char
                        } else {
                            self.page_cp.to_char(c)
                        };
                        self.graphic(ch);
                    }
                } else {
                    self.once("ASCII transparent data skipped".to_owned());
                }
                end
            }
            codes::VCS => {
                let Some(ch) = arg(1) else {
                    return self.truncated(b, data.len());
                };
                self.channel(ch);
                pos + 2
            }
            codes::HT => {
                self.tab(false);
                pos + 1
            }
            codes::IT => {
                self.tab(true);
                pos + 1
            }
            codes::RNL | codes::IRT => {
                self.indent = self.lm;
                self.new_line();
                pos + 1
            }
            codes::NL | codes::IRS => {
                self.new_line();
                pos + 1
            }
            codes::GE => {
                if arg(1).is_none() {
                    return self.truncated(b, data.len());
                }
                self.once("graphic escape character printed as the error character".to_owned());
                self.graphic(self.error_char);
                pos + 2
            }
            codes::SPS => {
                self.half -= 1;
                pos + 1
            }
            codes::SBS => {
                self.half += 1;
                pos + 1
            }
            codes::VT => {
                self.vertical_tab();
                pos + 1
            }
            codes::FF | codes::RFF => {
                if b == codes::RFF {
                    self.indent = self.lm;
                }
                self.form_feed();
                pos + 1
            }
            codes::CR => {
                self.col = self.lm;
                self.half = 0;
                self.word_start = self.glyphs.len();
                pos + 1
            }
            codes::BS | codes::NBS => {
                self.col = self.col.saturating_sub(1).max(1);
                self.word_start = self.glyphs.len();
                pos + 1
            }
            codes::UBS => pos + 1,
            codes::WUS => {
                let line = self.line;
                for g in &mut self.glyphs[self.word_start..] {
                    if g.line == line {
                        g.underline = true;
                    }
                }
                pos + 1
            }
            codes::LF => {
                self.down(1);
                pos + 1
            }
            codes::SA => {
                if arg(2).is_none() {
                    return self.truncated(b, data.len());
                }
                self.once("set attribute ignored".to_owned());
                pos + 3
            }
            codes::CSP => self.format_command(data, pos),
            codes::PP => {
                let (Some(f), Some(n)) = (arg(1), arg(2)) else {
                    return self.truncated(b, data.len());
                };
                self.presentation_position(f, n);
                pos + 3
            }
            codes::SUB => {
                self.graphic(self.error_char);
                pos + 1
            }
            0x01..=0x3F => {
                self.once(format!("control {b:02X} skipped"));
                pos + 1
            }
            codes::SP => {
                self.space();
                pos + 1
            }
            codes::RSP | codes::NSP => {
                self.graphic(' ');
                pos + 1
            }
            codes::SHY => {
                self.graphic('-');
                pos + 1
            }
            _ => {
                let ch = self.page_cp.to_char(b);
                let ch = if ch.is_control() { self.error_char } else { ch };
                self.graphic(ch);
                pos + 1
            }
        }
    }

    fn truncated(&mut self, b: u8, end: usize) -> usize {
        self.once(format!("control {b:02X} cut short by the end of the job"));
        end
    }

    // Printing.

    /// Prints a character (a blank one too) and moves one column right.
    fn graphic(&mut self, ch: char) {
        if self.auto_new_line && self.col > self.mpp {
            self.new_line();
        }
        if !self.inhibit {
            let underline = self.underline.is_some();
            if ch != ' ' || underline {
                self.place(ch, underline);
            }
            if let Some((over, bypass)) = self.overstrike
                && (ch != ' ' || bypass != BYPASS_WHITE_SPACE)
            {
                self.place(over, false);
            }
        }
        self.col = self.col.saturating_add(1);
    }

    /// A space: white space, which underscore and overstrike may leave
    /// alone, and the end of a word.
    fn space(&mut self) {
        if self.auto_new_line && self.col > self.mpp {
            self.new_line();
        }
        if !self.inhibit {
            if let Some(bypass) = self.underline
                && bypass != BYPASS_WHITE_SPACE
            {
                self.place(' ', true);
            }
            if let Some((over, bypass)) = self.overstrike
                && bypass != BYPASS_WHITE_SPACE
            {
                self.place(over, false);
            }
        }
        self.col = self.col.saturating_add(1);
        self.word_start = self.glyphs.len();
    }

    /// Puts a character at the cursor.
    fn place(&mut self, ch: char, underline: bool) {
        let x = self.pitch.column_x(self.col);
        let shift = self.half * (self.spacing as i32 / 2);
        let y = (i64::from(self.y) + i64::from(shift)).clamp(0, i64::from(u32::MAX)) as u32;
        let key = (x, y);
        if let Some(ids) = self.cells.get(&key) {
            // The same character again prints bold.
            if let Some(&i) = ids.iter().find(|&&i| self.glyphs[i].ch == ch) {
                self.glyphs[i].bold = true;
                self.glyphs[i].underline |= underline;
                return;
            }
            // An underlined blank under a character, or a character over
            // an underlined blank: one cell.
            if ch == ' ' {
                let i = ids[0];
                self.glyphs[i].underline |= underline;
                return;
            }
            if let Some(&i) = ids.iter().find(|&&i| self.glyphs[i].ch == ' ') {
                self.glyphs[i].ch = ch;
                self.glyphs[i].underline |= underline;
                return;
            }
        }
        let g = Glyph {
            x,
            y,
            height: self.spacing,
            ch,
            font_id: self.font_id,
            pitch: self.pitch,
            bold: self.font_bold || self.emphasis,
            italic: self.font_italic,
            underline,
            line: self.line,
            col: self.col,
        };
        self.cells.entry(key).or_default().push(self.glyphs.len());
        self.glyphs.push(g);
    }

    // Moving.

    fn new_line(&mut self) {
        let half = u32::from(self.nl_half_lines);
        self.line = self.line.saturating_add((self.nl_half_lines / 2).max(1));
        self.y = self.y.saturating_add(self.spacing * half / 2);
        self.col = self.indent.max(self.lm);
        self.half = 0;
        self.word_start = self.glyphs.len();
        self.check_bottom();
    }

    /// Down `n` lines, same column.
    fn down(&mut self, n: u16) {
        self.line = self.line.saturating_add(n);
        self.y = self.y.saturating_add(self.spacing * u32::from(n));
        self.half = 0;
        self.word_start = self.glyphs.len();
        self.check_bottom();
    }

    /// To line `n` of this page.
    fn go_to_line(&mut self, n: u16) {
        self.line = n.max(1);
        self.y = u32::from(self.line - 1) * self.spacing;
        self.half = 0;
        self.word_start = self.glyphs.len();
    }

    /// Past the bottom margin: on to the top margin of the next page.
    fn check_bottom(&mut self) {
        if self.line > self.bm {
            self.end_page(PageStart::Overflow);
            self.go_to_line(self.tm);
        }
    }

    fn tab(&mut self, indent: bool) {
        let next = self.htabs.iter().copied().find(|&t| t > self.col);
        match next {
            Some(t) => self.col = t,
            None => {
                if !self.htabs.is_empty() {
                    self.once("horizontal tab with no tab stop to the right".to_owned());
                }
                self.col = self.col.saturating_add(1);
            }
        }
        if indent {
            self.indent = self.col;
        }
        self.word_start = self.glyphs.len();
    }

    fn vertical_tab(&mut self) {
        match self.vtabs.iter().copied().find(|&t| t > self.line) {
            Some(t) if t <= self.bm => {
                let col = self.col;
                let n = t - self.line;
                self.down(n);
                self.col = col;
            }
            _ => self.down(1),
        }
    }

    fn channel(&mut self, select: u8) {
        let channel = match select {
            0x81..=0x89 => select - 0x80,
            0x7A..=0x7C => select - 0x7A + 10,
            _ => {
                self.once(format!("vertical channel select {select:02X} not valid"));
                return;
            }
        };
        let target = if channel == 1 {
            Some(self.tm)
        } else {
            self.vtabs
                .get(usize::from(channel) - 2)
                .copied()
                .filter(|&t| t > 0)
        };
        match target {
            None => self.down(1),
            Some(t) if t > self.line => {
                let n = t - self.line;
                self.down(n);
            }
            Some(t) => {
                self.end_page(PageStart::Overflow);
                self.go_to_line(t);
            }
        }
    }

    fn presentation_position(&mut self, function: u8, n: u8) {
        let n16 = u16::from(n);
        match function {
            pp::AHPP => self.col = n16.max(1),
            pp::RHPP => self.col = self.col.saturating_add(n16),
            pp::AVPP => {
                let n16 = n16.max(1);
                if n16 < self.line || n16 > self.bm {
                    self.end_page(PageStart::Overflow);
                    let line = if n16 > self.bm { self.tm } else { n16 };
                    self.go_to_line(line);
                } else {
                    self.go_to_line(n16);
                }
            }
            pp::RVPP => self.down(n16),
            other => self.once(format!(
                "presentation position function {other:02X} skipped"
            )),
        }
        self.word_start = self.glyphs.len();
    }

    fn form_feed(&mut self) {
        if !self.glyphs.is_empty() {
            self.end_page(PageStart::FormFeed);
        } else if self.page_start == PageStart::FormFeed {
            // Asked for: a blank page.
            self.close_page();
            self.page_start = PageStart::FormFeed;
        } else if self.page_start == PageStart::Overflow {
            // The page ended by itself just before; this feed is that
            // page's.
            self.page_start = PageStart::FormFeed;
        }
        self.go_to_line(self.tm);
        self.col = self.indent.max(self.lm);
    }

    /// Ends the page if anything is on it.
    fn end_page(&mut self, start: PageStart) {
        if !self.glyphs.is_empty() {
            self.close_page();
            self.page_start = start;
        }
    }

    // Format commands.

    fn format_command(&mut self, data: &[u8], pos: usize) -> usize {
        let (Some(&class), Some(&count)) = (data.get(pos + 1), data.get(pos + 2)) else {
            return self.truncated(codes::CSP, data.len());
        };
        // The count counts itself and the parameters.
        let end = pos + 2 + usize::from(count.max(1));
        if end > data.len() {
            self.once(format!(
                "format command 2B {class:02X} cut short by the end of the job"
            ));
            return data.len();
        }
        let params = &data[pos + 3..end];
        match class {
            class::SHF => self.shf(params),
            class::SVF => self.svf(params),
            class::SLD => {
                if let Some(&ld) = params.first() {
                    self.spacing = if ld == 0 { 240 } else { u32::from(ld) * 20 };
                }
            }
            class::SGEA => {
                if let Some(&dg) = params.first()
                    && dg >= 0x40
                {
                    let ch = self.page_cp.to_char(dg);
                    if !ch.is_control() {
                        self.error_char = ch;
                    }
                }
            }
            class::EPMP => {}
            class::D1 => self.d1(params),
            class::D2 => self.d2(params),
            class::D3 => self.d3(params),
            class::D4 => self.d4(params),
            other => self.once(format!("format command 2B {other:02X} skipped")),
        }
        end
    }

    fn shf(&mut self, p: &[u8]) {
        let mpp = p.first().copied().unwrap_or(0);
        self.mpp = if mpp == 0 {
            // 13.2 inches at the pitch in force.
            (13.2 * self.pitch.cpi()).round() as u16
        } else {
            u16::from(mpp)
        };
        self.lm = p
            .get(1)
            .map(|&lm| u16::from(lm))
            .filter(|&lm| lm > 0 && lm <= self.mpp)
            .unwrap_or(1);
        self.indent = self.lm;
        let mut tabs: Vec<u16> = p
            .iter()
            .skip(3)
            .map(|&t| u16::from(t))
            .filter(|&t| t > 0)
            .collect();
        tabs.sort_unstable();
        tabs.dedup();
        self.htabs = tabs;
        self.auto_new_line = true;
    }

    fn svf(&mut self, p: &[u8]) {
        let mpl = match p.first() {
            None | Some(0) => DEFAULT_MPL,
            Some(&n) => u16::from(n),
        };
        self.tm = p
            .get(1)
            .map(|&t| u16::from(t))
            .filter(|&t| t > 0 && t <= mpl)
            .unwrap_or(1);
        self.bm = p
            .get(2)
            .map(|&b| u16::from(b))
            .filter(|&b| b >= self.tm && b <= mpl)
            .unwrap_or(mpl);
        let mut tabs: Vec<u16> = p
            .iter()
            .skip(3)
            .map(|&t| u16::from(t))
            .filter(|&t| t > 0)
            .collect();
        tabs.dedup();
        self.vtabs = tabs;
        if self.line < self.tm && self.glyphs.is_empty() {
            self.go_to_line(self.tm);
        }
    }

    fn d1(&mut self, p: &[u8]) {
        let Some(&f) = p.first() else {
            self.once("format command 2B D1 with no function".to_owned());
            return;
        };
        match f {
            d1::SCG => {
                if let Some(cp) = be16(p, 3) {
                    self.select_code_page(cp);
                }
            }
            d1::SFG => {
                let Some(id) = be16(p, 1) else {
                    self.once("set font global without a font ID".to_owned());
                    return;
                };
                let info = font::info(id);
                let pitch = be16(p, 3)
                    .and_then(Pitch::from_width)
                    .or(info.pitch)
                    .unwrap_or(self.pitch);
                self.font_id = id;
                self.pitch = pitch;
                self.font_bold = info.bold;
                self.font_italic = info.italic;
                self.once(format!(
                    "font ID {id} printed in the bundled face at {} characters per inch{}{}",
                    pitch.cpi(),
                    if info.bold { ", bold" } else { "" },
                    if info.italic { ", italic" } else { "" }
                ));
            }
            d1::SCGL => match p.get(1).copied() {
                Some(0xFF) => {
                    self.page_cp = codepage::by_ccsid(self.options.ccsid)
                        .unwrap_or_else(codepage::default_page);
                }
                Some(lcid) => match local_id_code_page(lcid) {
                    Some(cp) => self.select_code_page(cp),
                    None => self.once(format!("code page local ID {lcid:02X} not valid")),
                },
                None => {}
            },
            d1::BES => self.emphasis = true,
            d1::EES => self.emphasis = false,
            d1::SFFC | d1::BAC => {}
            other => self.once(format!("format command 2B D1 {other:02X} skipped")),
        }
    }

    fn select_code_page(&mut self, cpgid: u16) {
        match code_page(cpgid) {
            Some(cp) => self.page_cp = cp,
            None => self.once(format!(
                "code page {cpgid} is not built in; code page {} kept",
                self.page_cp.ccsid()
            )),
        }
    }

    fn d2(&mut self, p: &[u8]) {
        let Some(&f) = p.first() else {
            self.once("format command 2B D2 with no function".to_owned());
            return;
        };
        match f {
            d2::STAB => {
                let fixed = p.get(1) == Some(&1);
                let mut tabs: Vec<u16> = p
                    .get(2..)
                    .unwrap_or_default()
                    .chunks_exact(3)
                    .filter_map(|t| {
                        let v = u16::from_be_bytes([t[1], t[2]]);
                        (v > 0).then(|| {
                            if fixed {
                                (f64::from(v) / self.pitch.advance()).round() as u16 + 1
                            } else {
                                v
                            }
                        })
                    })
                    .collect();
                tabs.sort_unstable();
                tabs.dedup();
                self.htabs = tabs;
            }
            d2::SIL => {
                let il = usize::from(p.get(1).copied().unwrap_or(0));
                self.indent = if il == 0 {
                    self.lm
                } else {
                    match self.htabs.get(il - 1) {
                        Some(&t) => t,
                        None => {
                            self.once(format!("indent level {il} has no tab stop"));
                            self.indent
                        }
                    }
                };
            }
            d2::SLS => {
                if let Some(&ls) = p.get(1) {
                    self.nl_half_lines = u16::from(ls);
                }
            }
            d2::SHM => {
                if let Some(lm) = be16(p, 1).filter(|&v| v > 0) {
                    self.lm = (f64::from(lm) / self.pitch.advance()).round() as u16 + 1;
                    self.indent = self.lm;
                }
                if let Some(rm) = be16(p, 3).filter(|&v| v > 0) {
                    self.mpp = ((f64::from(rm) / self.pitch.advance()) as u16).max(self.lm);
                }
            }
            d2::SSLD => {
                if let Some(d) = be16(p, 1).filter(|&v| v > 0) {
                    self.spacing = u32::from(d);
                }
            }
            d2::SCD => match be16(p, 1) {
                None | Some(0) => {}
                Some(0xFF) => {
                    self.pitch = font::info(self.options.font).pitch.unwrap_or_default();
                }
                Some(0x0B) | Some(12) => self.pitch = Pitch::P12,
                Some(16) | Some(17) => self.pitch = Pitch::P16_7,
                Some(cd @ (5 | 10 | 15 | 20)) => self.pitch = Pitch(cd * 10),
                Some(cd) => self.once(format!("character distance {cd} not valid")),
            },
            d2::SPPS => {
                if !self.glyphs.is_empty() {
                    self.end_page(PageStart::Overflow);
                }
                let (w0, h0) = self.page_size.unwrap_or_else(|| self.options.paper.size());
                let w = be16(p, 1).filter(|&v| v > 0).map_or(w0, u32::from);
                let h = be16(p, 3).filter(|&v| v > 0).map_or(h0, u32::from);
                self.page_size = Some((w, h));
            }
            d2::PPM => {
                if !self.glyphs.is_empty() {
                    self.end_page(PageStart::Overflow);
                }
                if let Some(&source) = p.get(4) {
                    self.once(format!(
                        "page presentation media: paper source {source:02X}"
                    ));
                }
            }
            d2::SVM => {
                if let Some(tm) = be16(p, 1).filter(|&v| v > 0) {
                    self.tm = ((u32::from(tm) + self.spacing / 2) / self.spacing).max(1) as u16;
                }
                if let Some(bm) = be16(p, 3).filter(|&v| v > 0) {
                    self.bm = ((u32::from(bm) / self.spacing) as u16).max(self.tm);
                }
            }
            d2::JTF | d2::SJM => {
                self.once("justification not supported; text left as sent".to_owned())
            }
            d2::RLM | d2::SIC | d2::SPSU | d2::SEA => {}
            other => self.once(format!("format command 2B D2 {other:02X} skipped")),
        }
    }

    fn d3(&mut self, p: &[u8]) {
        match p.first() {
            Some(&d3::STO) => match be16(p, 3) {
                Some(0x0000) | Some(0x5A00) => self.orientation = Some(Orientation::Portrait),
                Some(0x2D00) | Some(0x8700) | Some(0xFFFE) => {
                    self.orientation = Some(Orientation::Landscape)
                }
                Some(0xFFFF) => self.orientation = None,
                Some(other) => self.once(format!("text orientation {other:04X} not valid")),
                None => self.once("set text orientation without a page rotation".to_owned()),
            },
            Some(&other) => self.once(format!("format command 2B D3 {other:02X} skipped")),
            None => {}
        }
    }

    fn d4(&mut self, p: &[u8]) {
        match p.first() {
            Some(&d4::BUS) => self.underline = Some(p.get(2).copied().unwrap_or(0)),
            Some(&d4::EUS) => self.underline = None,
            Some(&d4::BOS) => match p.get(1) {
                Some(&c) if c >= 0x40 => {
                    let ch = self.page_cp.to_char(c);
                    let ch = if ch.is_control() { self.error_char } else { ch };
                    self.overstrike = Some((ch, p.get(2).copied().unwrap_or(0)));
                }
                _ => self.once("begin overstrike without a character".to_owned()),
            },
            Some(&d4::EOS) => self.overstrike = None,
            Some(&other) => self.once(format!("format command 2B D4 {other:02X} skipped")),
            None => {}
        }
    }

    // Pages.

    /// Closes the current page, blank or not.
    fn close_page(&mut self) {
        let glyphs = std::mem::take(&mut self.glyphs);
        self.cells.clear();
        self.word_start = 0;
        let (paper_w, paper_h) = self.options.paper.size();
        let (mut w, mut h) = self.page_size.unwrap_or((paper_w, paper_h));
        let text_w = glyphs
            .iter()
            .map(|g| g.x + g.pitch.advance().round() as u32)
            .chain(
                self.auto_new_line
                    .then(|| self.pitch.column_x(self.mpp.saturating_add(1))),
            )
            .max()
            .unwrap_or(0);
        let orientation = match self.orientation {
            Some(o) => o,
            None if self.page_size.is_some() => {
                if w > h {
                    Orientation::Landscape
                } else {
                    Orientation::Portrait
                }
            }
            None if text_w > paper_w => Orientation::Landscape,
            None => Orientation::Portrait,
        };
        if (orientation == Orientation::Landscape) == (w < h) {
            std::mem::swap(&mut w, &mut h);
        }
        let right_edge = self.pitch.column_x(self.mpp.saturating_add(1));
        let bottom_edge = u32::from(self.bm) * self.spacing;
        let margins = Margins {
            left: self.pitch.column_x(self.lm),
            top: u32::from(self.tm - 1) * self.spacing,
            right: if self.auto_new_line {
                w.saturating_sub(right_edge)
            } else {
                0
            },
            bottom: h.saturating_sub(bottom_edge),
        };
        self.pages.push(Page {
            width: w,
            height: h,
            orientation,
            margins,
            runs: runs(glyphs),
        });
    }

    fn finish(mut self) -> Document {
        if !self.glyphs.is_empty() {
            self.close_page();
        }
        Document { pages: self.pages }
    }
}

/// Glyphs to runs: by line, left to right; neighbours with the same look
/// join, with blanks between them filled in when they are not underlined.
/// A glyph over another comes after it, as a run of its own.
fn runs(glyphs: Vec<Glyph>) -> Vec<Run> {
    let mut order: Vec<usize> = (0..glyphs.len()).collect();
    order.sort_by_key(|&i| (glyphs[i].y, glyphs[i].x, i));
    let mut base: Vec<&Glyph> = Vec::new();
    let mut over: Vec<&Glyph> = Vec::new();
    let mut last: Option<(u32, u32)> = None;
    for &i in &order {
        let g = &glyphs[i];
        if last == Some((g.x, g.y)) {
            over.push(g);
        } else {
            base.push(g);
            last = Some((g.x, g.y));
        }
    }
    let same_look = |a: &Run, g: &Glyph| {
        a.y == g.y
            && a.height == g.height
            && a.font_id == g.font_id
            && a.pitch == g.pitch
            && a.bold == g.bold
            && a.italic == g.italic
            && a.line == g.line
    };
    let mut out: Vec<Run> = Vec::new();
    for g in base {
        if let Some(r) = out.last_mut()
            && same_look(r, g)
        {
            let next_col = r.col + r.text.chars().count() as u16;
            let gap = g.col.saturating_sub(next_col);
            let lined_up = g.col >= next_col && r.char_x(usize::from(g.col - r.col)) == g.x;
            if lined_up && r.underline == g.underline && (gap == 0 || !g.underline) {
                r.text.extend(std::iter::repeat_n(' ', usize::from(gap)));
                r.text.push(g.ch);
                continue;
            }
        }
        out.push(run_of(g));
    }
    out.extend(over.into_iter().map(run_of));
    out
}

fn run_of(g: &Glyph) -> Run {
    Run {
        x: g.x,
        y: g.y,
        height: g.height,
        text: g.ch.to_string(),
        font_id: g.font_id,
        pitch: g.pitch,
        bold: g.bold,
        italic: g.italic,
        underline: g.underline,
        line: g.line,
        col: g.col,
    }
}

#[cfg(test)]
mod tests;
