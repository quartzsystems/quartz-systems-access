// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! The spooled file viewer: an SCS file's pages drawn from the page model
//! in the print font (Courier Prime, as `qsa-print` prints them), each
//! character at its column, so a page looks as it does printed or saved.
//!
//! - Fit width, or 100 % (96 pixels to the inch).
//! - First, Previous, a page number to type, Next, Last; Page Up and Page
//!   Down.
//! - Find in the page text, every match on the page highlighted, Find Next
//!   (F3) going on through the pages.
//! - Save as PDF..., Save as Text..., Print... from the data read, and Copy
//!   for the page's text.
//! - The file is read a batch of pages at a time ([`Reading`]); pages show
//!   as they arrive, so page 1 is there before the read ends. Cancel stops
//!   it; the pages read stay.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;
use std::sync::OnceLock;

use fltk::app;
use fltk::button::Button;
use fltk::draw;
use fltk::enums::{Align, CallbackTrigger, Color, Event, Font, FrameType, Key, Shortcut};
use fltk::frame::Frame;
use fltk::group::{Scroll, ScrollType};
use fltk::input::Input;
use fltk::menu::Choice;
use fltk::prelude::*;
use fltk::widget::Widget;
use fltk::window::Window;
use qap_client::spool::ReadEnd;
use qsa_config::System;
use qsa_msg::id;
use qsa_print::font::{self as pfont, Face};
use qsa_ui::chan::{self, Emit};
use qsa_ui::theme::{self, colors};
use qsa_ui::{Status, StatusBar, controls, msgdetails};
use scs::{Document, Page};

use super::output::{self, Purpose, ReadEvent, Reading};

/// Window width and height.
pub const W: i32 = 940;
pub const H: i32 = 720;

/// Pixels to the inch at 100 %.
pub const DPI: f64 = 96.0;

/// Space around the page, pixels.
const PAD: i32 = 12;

/// The FLTK slots the print faces are drawn from.
fn slot(face: Face) -> Font {
    match face {
        Face::Regular => Font::Times,
        Face::Bold => Font::TimesBold,
        Face::Italic => Font::TimesItalic,
    }
}

/// Registers the print faces once.
fn load_faces() {
    static DONE: OnceLock<()> = OnceLock::new();
    DONE.get_or_init(|| {
        for (face, file, gdi, pango) in [
            (
                Face::Regular,
                "CourierPrime-Regular.ttf",
                " Courier Prime",
                "Courier Prime",
            ),
            (
                Face::Bold,
                "CourierPrime-Bold.ttf",
                "BCourier Prime",
                "Courier Prime Bold",
            ),
            (
                Face::Italic,
                "CourierPrime-Italic.ttf",
                "ICourier Prime",
                "Courier Prime Italic",
            ),
        ] {
            if let Err(e) = theme::load_face(file, face.bytes(), slot(face), gdi, pango) {
                crate::log!("viewer font: {e}");
            }
        }
    });
}

/// How big a page is drawn.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Zoom {
    FitWidth,
    Actual,
}

/// Where one character of a page is, in 1/1440 inch.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CellBox {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

/// A page's text by line and column, and where each character is: what
/// Find searches and highlights.
#[derive(Clone, Debug, Default)]
pub struct PageText {
    /// Line number to its characters by column (1-based); the first
    /// character that is not blank wins, as in the text rendering.
    lines: BTreeMap<u16, BTreeMap<u32, (char, CellBox)>>,
}

impl PageText {
    pub fn of(page: &Page) -> PageText {
        let mut lines: BTreeMap<u16, BTreeMap<u32, (char, CellBox)>> = BTreeMap::new();
        for run in &page.runs {
            let cells = lines.entry(run.line).or_default();
            for (i, ch) in run.text.chars().enumerate() {
                let col = u32::from(run.col) + i as u32;
                let b = CellBox {
                    x: f64::from(run.char_x(i)),
                    y: f64::from(run.y),
                    w: run.pitch.advance(),
                    h: f64::from(run.height),
                };
                let cell = cells.entry(col).or_insert((' ', b));
                if cell.0 == ' ' {
                    *cell = (ch, b);
                }
            }
        }
        PageText { lines }
    }

    /// Every match of `needle` (any case), as the boxes of its characters.
    pub fn find(&self, needle: &str) -> Vec<Vec<CellBox>> {
        let needle: Vec<char> = needle.to_lowercase().chars().collect();
        if needle.is_empty() || needle.iter().all(|c| c.is_whitespace()) {
            return Vec::new();
        }
        let mut out = Vec::new();
        for cells in self.lines.values() {
            let Some(&last) = cells.keys().next_back() else {
                continue;
            };
            // The line as text from column 1, blanks where nothing is.
            let text: Vec<(char, Option<CellBox>)> = (1..=last)
                .map(|c| match cells.get(&c) {
                    Some((ch, b)) => (ch.to_lowercase().next().unwrap_or(*ch), Some(*b)),
                    None => (' ', None),
                })
                .collect();
            let n = needle.len();
            let mut i = 0;
            while i + n <= text.len() {
                if text[i..i + n]
                    .iter()
                    .map(|(c, _)| *c)
                    .eq(needle.iter().copied())
                {
                    out.push(text[i..i + n].iter().filter_map(|(_, b)| *b).collect());
                    i += n;
                } else {
                    i += 1;
                }
            }
        }
        out
    }
}

/// What the page area draws.
#[derive(Default)]
struct View {
    page: Option<Page>,
    scale: f64,
    matches: Vec<Vec<CellBox>>,
    current: Option<usize>,
}

/// Draws `v` in the widget at `(x, y)`. Text is placed on the page as the
/// PDF and printed copies place it: kept inside the default margin and
/// scaled down to fit when it is wider or longer than the page
/// ([`qsa_print::pdf::fit`]).
fn draw_page(v: &View, x: i32, y: i32) {
    let Some(page) = &v.page else { return };
    let s = v.scale;
    let fit = qsa_print::pdf::fit(page, qsa_print::pdf::DEFAULT_MARGIN);
    let px = |t: f64| (t * s).round() as i32;
    let (ox, oy) = (x + PAD, y + PAD);
    let (pw, ph) = (px(f64::from(page.width)), px(f64::from(page.height)));
    draw::draw_rect_fill(ox + 3, oy + 3, pw, ph, Color::from_rgb(0xA0, 0xA0, 0xA0));
    draw::draw_rect_fill(ox, oy, pw, ph, Color::White);
    draw::set_draw_color(colors::LIST_BORDER);
    draw::draw_rect(ox, oy, pw, ph);
    for (i, m) in v.matches.iter().enumerate() {
        let colour = if Some(i) == v.current {
            Color::from_rgb(0xFF, 0x96, 0x32)
        } else {
            Color::from_rgb(0xFF, 0xE8, 0x5A)
        };
        for b in m {
            draw::draw_rect_fill(
                ox + px(fit.x(b.x)),
                oy + px(fit.y(b.y)),
                px(b.w * fit.scale).max(1),
                px(b.h * fit.scale).max(1),
                colour,
            );
        }
    }
    draw::push_clip(ox, oy, pw, ph);
    draw::set_draw_color(Color::Black);
    for run in &page.runs {
        let size = (pfont::em(run.pitch) * fit.scale * s).round().max(1.0) as i32;
        draw::set_font(slot(Face::of(run)), size);
        let base = oy + px(fit.y(f64::from(run.y) + pfont::baseline(run.height, run.pitch)));
        let mut buf = [0u8; 4];
        for (i, ch) in run.text.chars().enumerate() {
            if ch == ' ' {
                continue;
            }
            let cx = ox + px(fit.x(f64::from(run.char_x(i))));
            draw::draw_text(ch.encode_utf8(&mut buf), cx, base);
        }
        if run.underline {
            let (below, thick) = pfont::underline(run.pitch);
            let n = run.text.chars().count();
            let x0 = ox + px(fit.x(f64::from(run.x)));
            let x1 = ox + px(fit.x(f64::from(run.char_x(n))));
            let uy = base + px(below * fit.scale);
            draw::draw_rect_fill(
                x0,
                uy,
                (x1 - x0).max(1),
                px(thick * fit.scale).max(1),
                Color::Black,
            );
        }
    }
    draw::pop_clip();
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Cmd {
    First,
    Previous,
    Next,
    Last,
    GoTo,
    Zoom,
    Find,
    FindNext,
    SavePdf,
    SaveText,
    Print,
    Copy,
    Cancel,
    Close,
}

/// One viewer window.
pub struct Viewer {
    pub window: Window,
    page_field: Input,
    of: Frame,
    zoom: Choice,
    pub find: Input,
    scroll: Scroll,
    area: Widget,
    pub status: StatusBar,
    saves: [Button; 3],
    cancel: Button,
    rx: chan::Receiver<Cmd>,
    view: Rc<RefCell<View>>,
    st: RefCell<St>,
}

struct St {
    system: System,
    options: scs::Options,
    doc: Document,
    /// 0-based page shown.
    page: usize,
    reading: Option<Reading>,
    /// The read has ended with every page.
    complete: bool,
    /// The match Find Next is at: page and index.
    at: Option<(usize, usize)>,
    title: String,
}

thread_local! {
    static WINDOWS: RefCell<Vec<Rc<Viewer>>> = const { RefCell::new(Vec::new()) };
}

impl Viewer {
    /// The window for the file `title`, showing nothing yet.
    pub fn build(system: &System, title: &str, options: scs::Options) -> Rc<Viewer> {
        load_faces();
        let (tx, rx) = chan::channel::<Cmd>();
        let mut window = Window::new(0, 0, W, H, None);
        window.set_label(&format!("{title} - {}", system.name));
        window.set_icon(Some(qsa_ui::glyph::app_icon()));
        window.set_color(colors::FACE);
        window.size_range(560, 360, 0, 0);
        let (m, fh) = (8, theme::FIELD_H);
        let (bw, bh) = (theme::BUTTON_W, theme::BUTTON_H);
        let mut x = m;
        let nav = |label: &str, c: Cmd, x: &mut i32| {
            let mut b = controls::button(*x, m, bw, bh, label);
            b.emit_to(tx, c);
            *x += bw + 6;
            b
        };
        nav("First", Cmd::First, &mut x);
        nav("Previous", Cmd::Previous, &mut x);
        let mut page_field = controls::field(x, m + 1, 48, fh);
        page_field.set_trigger(CallbackTrigger::EnterKeyAlways);
        page_field.set_callback(move |_| tx.send(Cmd::GoTo));
        page_field.set_tooltip("Type a page number and press Enter.");
        x += 52;
        let mut of = Frame::new(x, m, 64, bh, None);
        of.set_align(Align::Left | Align::Inside);
        of.set_label_size(theme::UI_SIZE);
        x += 66;
        nav("Next", Cmd::Next, &mut x);
        nav("Last", Cmd::Last, &mut x);
        x += 8;
        let mut zl = controls::label(x, m, 40, bh, "Zoom:");
        zl.set_align(Align::Left | Align::Inside);
        x += 40;
        let mut zoom = controls::combo(x, m, 100, bh);
        zoom.add_choice("Fit Width");
        zoom.add_choice("100 %");
        zoom.set_value(0);
        zoom.set_callback(move |_| tx.send(Cmd::Zoom));
        x += 108;
        let mut fl = controls::label(x, m, 34, bh, "Find:");
        fl.set_align(Align::Left | Align::Inside);
        x += 34;
        let mut find = controls::field(x, m + 1, W - x - m - bw - 6, fh);
        find.set_trigger(CallbackTrigger::EnterKeyAlways);
        find.set_callback(move |_| tx.send(Cmd::Find));
        let mut next = controls::button(W - m - bw, m, bw, bh, "Find Next");
        next.emit_to(tx, Cmd::FindNext);
        next.set_tooltip("F3");

        let sy = m + bh + 8;
        let by = H - StatusBar::HEIGHT - 8 - bh;
        let mut scroll = Scroll::new(0, sy, W, by - 8 - sy, None);
        scroll.set_frame(FrameType::FlatBox);
        scroll.set_color(Color::from_rgb(0xC8, 0xC8, 0xC8));
        scroll.set_type(ScrollType::Both);
        theme::style_scrollbar(&mut scroll.scrollbar());
        theme::style_scrollbar(&mut scroll.hscrollbar());
        let mut area = Widget::new(0, sy, W - 17, 100, None);
        area.set_frame(FrameType::NoBox);
        scroll.end();
        let view = Rc::new(RefCell::new(View {
            scale: DPI / 1440.0,
            ..View::default()
        }));
        let v = Rc::clone(&view);
        area.draw(move |w| draw_page(&v.borrow(), w.x(), w.y()));

        let mut bx = m;
        let mut button = |label: &str, c: Cmd, w: i32| {
            let mut b = controls::button(bx, by, w, bh, label);
            b.emit_to(tx, c);
            bx += w + 6;
            b
        };
        let saves = [
            button("Save as PDF...", Cmd::SavePdf, 100),
            button("Save as Text...", Cmd::SaveText, 100),
            button("Print...", Cmd::Print, bw),
        ];
        let mut copy = button("Copy", Cmd::Copy, bw);
        copy.set_tooltip("Copies the text of the page shown (Ctrl+C).");
        let mut cancel =
            controls::button(W - 2 * m - 2 * bw - 30, by, bw + 30, bh, "Cancel Reading");
        cancel.emit_to(tx, Cmd::Cancel);
        cancel.hide();
        let mut close = controls::button(W - m - bw, by, bw, bh, "Close");
        close.emit_to(tx, Cmd::Close);
        let status = StatusBar::new(0, H - StatusBar::HEIGHT, W);
        window.resizable(&scroll);
        window.end();
        window.set_callback(move |_| tx.send(Cmd::Close));
        window.handle(move |_, ev| {
            if ev != Event::KeyDown && ev != Event::Shortcut {
                return false;
            }
            let k = app::event_key();
            let ctrl = app::event_state().contains(Shortcut::Ctrl);
            let c = match k {
                Key::PageUp => Cmd::Previous,
                Key::PageDown => Cmd::Next,
                Key::Home if ctrl => Cmd::First,
                Key::End if ctrl => Cmd::Last,
                k if k == Key::F3 => Cmd::FindNext,
                k if ctrl && k == Key::from_char('c') => Cmd::Copy,
                _ => return false,
            };
            tx.send(c);
            true
        });
        Rc::new(Viewer {
            window,
            page_field,
            of,
            zoom,
            find,
            scroll,
            area,
            status,
            saves,
            cancel,
            rx,
            view,
            st: RefCell::new(St {
                system: system.clone(),
                options,
                doc: Document::default(),
                page: 0,
                reading: None,
                complete: false,
                at: None,
                title: title.to_owned(),
            }),
        })
    }

    /// Shows `doc`, complete, on `page` (0-based) at `zoom`, finding
    /// `find` (screenshots).
    pub fn show_doc(&self, doc: Document, page: usize, zoom: Zoom, find: &str) {
        {
            let mut st = self.st.borrow_mut();
            st.doc = doc;
            st.page = page;
            st.complete = true;
        }
        self.zoom.clone().set_value(match zoom {
            Zoom::FitWidth => 0,
            Zoom::Actual => 1,
        });
        self.find.clone().set_value(find);
        self.show_page();
        if !find.is_empty() {
            self.find_next();
        }
        let n = self.st.borrow().doc.pages.len();
        self.status
            .clone()
            .set_message(&format!("{}: {n} pages.", self.st.borrow().title));
    }

    fn pages(&self) -> usize {
        self.st.borrow().doc.pages.len()
    }

    /// The scale for the page shown at the zoom chosen.
    fn scale(&self, page: &Page) -> f64 {
        match self.zoom.value() {
            1 => DPI / 1440.0,
            _ => {
                // Room for the scroll bar and the shadow beside the page.
                let avail = f64::from((self.scroll.w() - 17 - 2 * PAD - 4).max(100));
                avail / f64::from(page.width.max(1))
            }
        }
    }

    /// Draws the page kept as current, with Find's matches on it.
    fn show_page(&self) {
        let (page, n, complete, at) = {
            let st = self.st.borrow();
            (
                st.doc.pages.get(st.page).cloned(),
                st.doc.pages.len(),
                st.complete,
                st.at,
            )
        };
        let i = self.st.borrow().page;
        let needle = self.find.value();
        {
            let mut v = self.view.borrow_mut();
            v.matches = page
                .as_ref()
                .map(|p| PageText::of(p).find(&needle))
                .unwrap_or_default();
            v.current = at.filter(|(p, _)| *p == i).map(|(_, m)| m);
            v.scale = page.as_ref().map_or(DPI / 1440.0, |p| self.scale(p));
            v.page = page;
        }
        let (w, h) = {
            let v = self.view.borrow();
            v.page.as_ref().map_or((100, 100), |p| {
                (
                    (f64::from(p.width) * v.scale).round() as i32 + 2 * PAD + 3,
                    (f64::from(p.height) * v.scale).round() as i32 + 2 * PAD + 3,
                )
            })
        };
        let mut area = self.area.clone();
        let mut scroll = self.scroll.clone();
        let w = w.max(scroll.w() - 17);
        area.resize(
            scroll.x() - scroll.xposition(),
            scroll.y() - scroll.yposition(),
            w,
            h,
        );
        self.page_field.clone().set_value(&if n == 0 {
            String::new()
        } else {
            (i + 1).to_string()
        });
        self.of.clone().set_label(&if complete {
            format!("of {n}")
        } else {
            format!("of {n}...")
        });
        scroll.redraw();
    }

    fn go(&self, page: usize) {
        let n = self.pages();
        if n == 0 {
            return;
        }
        let p = page.min(n - 1);
        if p != self.st.borrow().page {
            self.st.borrow_mut().page = p;
            let mut s = self.scroll.clone();
            s.scroll_to(s.xposition(), 0);
        }
        self.show_page();
    }

    /// The next match after the one Find is at, through the pages.
    fn find_next(&self) {
        let needle = self.find.value();
        if needle.trim().is_empty() {
            return;
        }
        let (pages, from) = {
            let st = self.st.borrow();
            let from = st
                .at
                .filter(|(p, _)| *p == st.page)
                .unwrap_or((st.page, usize::MAX));
            (st.doc.pages.clone(), from)
        };
        let n = pages.len();
        for step in 0..=n {
            let p = (from.0 + step) % n.max(1);
            let Some(page) = pages.get(p) else { break };
            let found = PageText::of(page).find(&needle).len();
            let start = if step == 0 { from.1.wrapping_add(1) } else { 0 };
            if start < found {
                self.st.borrow_mut().at = Some((p, start));
                self.st.borrow_mut().page = p;
                self.show_page();
                self.status.clone().set_message(&format!(
                    "\"{needle}\" on page {}: match {} of {found}.",
                    p + 1,
                    start + 1
                ));
                return;
            }
        }
        self.st.borrow_mut().at = None;
        self.show_page();
        self.status
            .clone()
            .set_message(&format!("\"{needle}\" not found in the pages read."));
    }

    /// Reads the temporary file again, after a batch came.
    fn reload(&self) {
        let doc = {
            let st = self.st.borrow();
            let Some(r) = &st.reading else { return };
            output::document(&r.path, &st.options)
        };
        match doc {
            Ok(d) => {
                self.st.borrow_mut().doc = d;
                self.show_page();
            }
            Err(e) => crate::log!("viewer: {e}"),
        }
    }

    fn set_saving(&self, on: bool) {
        for b in &self.saves {
            let mut b = b.clone();
            if on {
                b.activate();
            } else {
                b.deactivate();
            }
        }
    }

    fn pump(&self) -> bool {
        while let Some(c) = self.rx.recv() {
            self.command(c);
        }
        let events = {
            let mut st = self.st.borrow_mut();
            st.reading.as_mut().map(Reading::poll).unwrap_or_default()
        };
        let mut grew = false;
        let mut end = None;
        for e in events {
            match e {
                ReadEvent::Progress(_) => grew = true,
                ReadEvent::Done(r) => end = Some(r),
            }
        }
        if grew {
            self.reload();
            let line = self.st.borrow().reading.as_ref().map(Reading::line);
            if let Some(l) = line {
                self.status.clone().set_message(&l);
            }
        }
        if let Some(r) = end {
            self.ended(r);
        }
        self.window.shown()
    }

    fn ended(&self, r: Result<ReadEnd, qap_client::Error>) {
        self.cancel.clone().hide();
        let (name, sys) = {
            let st = self.st.borrow();
            (st.title.clone(), st.system.clone())
        };
        match r {
            Ok(ReadEnd::Done { pages, .. }) => {
                self.st.borrow_mut().complete = true;
                self.reload();
                self.set_saving(true);
                self.status
                    .clone()
                    .set_message(&format!("{name}: {pages} pages."));
            }
            Ok(ReadEnd::Cancelled(_)) => {
                let m = qsa_msg::message(id::SPOOL_READ_CANCELLED, &[&name]);
                crate::log!("{m}");
                self.status.clone().set_message(&m.to_string());
                self.show_page();
            }
            Err(e) => {
                let (file, format) = {
                    let st = self.st.borrow();
                    let r = st.reading.as_ref().expect("a read ended");
                    (r.file.clone(), r.format)
                };
                let x = output::explain_read(&sys, &file, format, &e);
                self.status.clone().set_message(&x.line());
                crate::qap::show(Some(&self.window), &x);
            }
        }
    }

    fn command(&self, c: Cmd) {
        let page = self.st.borrow().page;
        let n = self.pages();
        match c {
            Cmd::First => self.go(0),
            Cmd::Previous => self.go(page.saturating_sub(1)),
            Cmd::Next => self.go(page + 1),
            Cmd::Last => self.go(n.saturating_sub(1)),
            Cmd::GoTo => match self.page_field.value().trim().parse::<usize>() {
                Ok(p) if p >= 1 && p <= n => self.go(p - 1),
                _ => {
                    self.status
                        .clone()
                        .set_message(&format!("Type a page number from 1 to {n}."));
                    self.show_page();
                }
            },
            Cmd::Zoom => self.show_page(),
            Cmd::Find => {
                self.st.borrow_mut().at = None;
                self.find_next();
            }
            Cmd::FindNext => self.find_next(),
            Cmd::Copy => {
                let text = self
                    .st
                    .borrow()
                    .doc
                    .pages
                    .get(page)
                    .map(scs::text::page_text)
                    .unwrap_or_default();
                app::copy(&text);
                self.status
                    .clone()
                    .set_message(&format!("The text of page {} was copied.", page + 1));
            }
            Cmd::SavePdf | Cmd::SaveText | Cmd::Print => self.output(c),
            Cmd::Cancel => {
                if let Some(r) = &self.st.borrow().reading {
                    r.cancel();
                }
            }
            Cmd::Close => {
                if let Some(r) = &self.st.borrow().reading {
                    r.cancel();
                }
                self.window.clone().hide();
            }
        }
    }

    /// Save as PDF or text, or Print, from the data read.
    fn output(&self, c: Cmd) {
        let (file, complete) = {
            let st = self.st.borrow();
            (st.reading.as_ref().map(|r| r.file.clone()), st.complete)
        };
        let Some(file) = file else { return };
        if !complete {
            return;
        }
        let purpose = match c {
            Cmd::SavePdf | Cmd::SaveText => {
                let (title, pattern, ext) = if c == Cmd::SavePdf {
                    ("Save as PDF", "PDF Files (*.pdf)\tAll Files (*)", "pdf")
                } else {
                    ("Save as Text", "Text Files (*.txt)\tAll Files (*)", "txt")
                };
                let name = super::model::save_name(&file.id, ext);
                let Some(path) = crate::files::choose_save(title, pattern, &name) else {
                    return;
                };
                let path = crate::files::with_extension(path, ext);
                if c == Cmd::SavePdf {
                    Purpose::SavePdf(path)
                } else {
                    Purpose::SaveText(path)
                }
            }
            _ => {
                let (all, default) = match output::printers() {
                    Ok(p) if !p.0.is_empty() => p,
                    Ok(_) => (Vec::new(), None),
                    Err(why) => {
                        let m = qsa_msg::message(id::SYSTEM_PRINTING_UNAVAILABLE, &[&why]);
                        msgdetails::show_message(Some(&self.window), &m);
                        return;
                    }
                };
                if all.is_empty() {
                    let m = qsa_msg::message(
                        id::SYSTEM_PRINTING_UNAVAILABLE,
                        &["no printer is set up on this computer"],
                    );
                    msgdetails::show_message(Some(&self.window), &m);
                    return;
                }
                let Some(p) =
                    super::dialogs::ask_print(Some(&self.window), &file, &all, default.as_deref())
                else {
                    return;
                };
                Purpose::Print(p)
            }
        };
        let r = {
            let st = self.st.borrow();
            let reading = st.reading.as_ref().expect("checked");
            output::finish(reading, &purpose, &st.options)
        };
        match r {
            Ok(Some(m)) => {
                crate::log!("{m}");
                self.status.clone().set_message(&m.to_string());
            }
            Ok(None) => {}
            Err(why) => {
                let m = match &purpose {
                    Purpose::Print(p) => qsa_msg::message(id::PRINTER_ERROR, &[p, &why]),
                    _ => qsa_msg::message(id::OUTPUT_FOLDER_NOT_WRITABLE, &[&why]),
                };
                self.status.clone().set_message(&m.to_string());
                msgdetails::show_message(Some(&self.window), &m);
            }
        }
    }
}

/// Opens a viewer for `reading`, an SCS file being read; its pages show as
/// they arrive.
pub fn open(
    parent: Option<&dyn WindowExt>,
    system: &System,
    options: scs::Options,
    reading: Reading,
) {
    let title = reading.file.id.file.clone();
    let v = Viewer::build(system, &title, options);
    v.set_saving(false);
    v.cancel.clone().show();
    v.status.clone().set_message(&reading.line());
    v.status.clone().set_status(
        &super::model::job_text(&reading.file.id.job),
        Status::Neutral,
    );
    v.st.borrow_mut().reading = Some(reading);
    v.show_page();
    let mut win = v.window.clone();
    qsa_ui::propdialog::center_on(&mut win, parent);
    qsa_ui::propdialog::show_placed(&mut win);
    WINDOWS.with(|w| w.borrow_mut().push(v));
    super::tick();
}

/// Gives every viewer its waiting work; forgets closed ones.
pub fn pump() {
    let windows: Vec<Rc<Viewer>> = WINDOWS.with(|v| v.borrow().clone());
    let open: Vec<bool> = windows.iter().map(|v| v.pump()).collect();
    WINDOWS.with(|v| {
        let mut i = 0;
        v.borrow_mut().retain(|_| {
            let keep = open.get(i).copied().unwrap_or(true);
            i += 1;
            keep
        });
    });
}

/// Whether any viewer is open.
pub fn any_open() -> bool {
    WINDOWS.with(|v| v.borrow().iter().any(|w| w.window.shown()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc() -> Document {
        let mut b = scs::sample::Builder::new();
        b.text("HELLO WORLD").nl().text("  hello again, world").nl();
        scs::parse(&b.bytes(), &scs::Options::default())
    }

    #[test]
    fn find_matches_any_case_and_boxes_each_character() {
        let d = doc();
        let t = PageText::of(&d.pages[0]);
        let m = t.find("world");
        assert_eq!(m.len(), 2);
        assert_eq!(m[0].len(), 5);
        // Column 7 of 10 pitch: six characters in.
        assert!((m[0][0].x - d.pages[0].runs[0].char_x(6) as f64).abs() < 1.0);
        assert!(m[1][0].y > m[0][0].y, "the second on the next line");
        assert_eq!(t.find("o w").len(), 1);
        assert!(t.find("").is_empty() && t.find("   ").is_empty());
        assert!(t.find("absent").is_empty());
    }
}
