// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! The 5250 display widget: the cell grid drawn as the QS/400 service
//! console draws it ([`crate::console`]), with the status line below.
//!
//! The widget keeps its own RGB back buffer at device resolution. An update
//! re-rasterizes only the cells that changed and redraws only the scanlines
//! they cover. On a resize the largest integer scale that fits is chosen and
//! the grid is centred on the background colour; nothing is ever smoothed or
//! stretched. On a desktop scaled above 100 % the buffer is drawn as one
//! full-resolution image, so FLTK maps its pixels one to one.
//!
//! The mouse selects a rectangle of cells; a click without a drag reports the
//! cell. Keys go to the callback given to [`TerminalView::on_key`].

use std::cell::RefCell;
use std::rc::Rc;

use fltk::enums::{ColorDepth, Damage, Event};
use fltk::prelude::*;
use fltk::{app, draw, widget::Widget};
use tn5250::screen::Screen;

use crate::console::{self, Canvas, GLYPH_H, GLYPH_W, Layout, Look, Oia, Paint};

/// A key as FLTK reported it, for the keyboard map.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeyPress {
    pub key: fltk::enums::Key,
    /// Text the key produced, if any.
    pub text: String,
    pub state: fltk::enums::Shortcut,
    /// A key coming up rather than going down.
    pub up: bool,
}

type KeyCb = Box<dyn FnMut(&KeyPress) -> bool>;
type ClickCb = Box<dyn FnMut(usize, usize)>;

struct Inner {
    rows: usize,
    cols: usize,
    cells: Vec<Paint>,
    status: Vec<Paint>,
    look: Look,
    /// What the canvas holds, to find the damaged cells.
    drawn: Vec<Paint>,
    drawn_status: Vec<Paint>,
    drawn_look: Option<Look>,
    canvas: Canvas,
    layout: Option<Layout>,
    /// Device scale the canvas was made for.
    device: f32,
    /// Scanlines changed since the last draw.
    dirty: Option<(usize, usize)>,
    selection: Option<((usize, usize), (usize, usize))>,
    anchor: Option<(usize, usize)>,
    dragged: bool,
    on_key: Option<KeyCb>,
    on_click: Option<ClickCb>,
}

impl Inner {
    fn new() -> Inner {
        Inner {
            rows: 24,
            cols: 80,
            cells: vec![Paint::text(' ', console::Color::Green); 24 * 80],
            status: vec![Paint::text(' ', console::Color::Green); 80],
            look: Look::default(),
            drawn: Vec::new(),
            drawn_status: Vec::new(),
            drawn_look: None,
            canvas: Canvas::new(1, 1, 0),
            layout: None,
            device: 1.0,
            dirty: None,
            selection: None,
            anchor: None,
            dragged: false,
            on_key: None,
            on_click: None,
        }
    }

    fn selected(&self, r: usize, c: usize) -> bool {
        self.selection.is_some_and(|((r0, c0), (r1, c1))| {
            (r0.min(r1)..=r0.max(r1)).contains(&r) && (c0.min(c1)..=c0.max(c1)).contains(&c)
        })
    }

    /// Brings the canvas up to date for a `w` by `h` device-pixel area,
    /// painting only what changed.
    fn update(&mut self, w: usize, h: usize, device: f32) {
        let fresh = Layout::fit(w, h, self.rows, self.cols, 1);
        let full = self.layout != Some(fresh)
            || self.canvas.w != w
            || self.canvas.h != h
            || self.drawn_look != Some(self.look)
            || self.device != device
            || self.drawn.len() != self.cells.len();
        let sel: Vec<Paint> = self
            .cells
            .iter()
            .enumerate()
            .map(|(i, p)| Paint {
                selected: self.selected(i / self.cols, i % self.cols),
                ..*p
            })
            .collect();
        let l = fresh;
        let band = GLYPH_H * l.scale;
        if full {
            self.canvas = Canvas::new(w, h, self.look.scheme.background());
            console::render(&mut self.canvas, l, &sel, Some(&self.status), self.look);
            self.dirty = Some((0, h));
        } else {
            let mut lo = usize::MAX;
            let mut hi = 0;
            for (i, (p, old)) in sel.iter().zip(&self.drawn).enumerate() {
                if p != old {
                    let (r, c) = (i / l.cols, i % l.cols);
                    let (x, y) = l.cell_xy(r, c);
                    console::paint_cell(&mut self.canvas, x, y, l.scale, *p, self.look);
                    lo = lo.min(y);
                    hi = hi.max(y + band);
                }
            }
            for (c, (p, old)) in self.status.iter().zip(&self.drawn_status).enumerate() {
                if p != old {
                    let (x, y) = l.cell_xy(l.rows, c);
                    console::paint_cell(&mut self.canvas, x, y, l.scale, *p, self.look);
                    lo = lo.min(y);
                    hi = hi.max(y + band);
                }
            }
            if lo < hi {
                let (a, b) = self.dirty.unwrap_or((lo, hi));
                self.dirty = Some((a.min(lo), b.max(hi).min(h)));
            }
        }
        self.drawn = sel;
        self.drawn_status = self.status.clone();
        self.drawn_look = Some(self.look);
        self.layout = Some(l);
        self.device = device;
    }
}

/// The 5250 display widget.
#[derive(Clone)]
pub struct TerminalView {
    widget: Widget,
    inner: Rc<RefCell<Inner>>,
}

fn device_scale(w: &Widget) -> f32 {
    w.window()
        .map(|win| app::screen_scale(win.screen_num()))
        .unwrap_or(1.0)
}

impl TerminalView {
    pub fn new(x: i32, y: i32, w: i32, h: i32) -> TerminalView {
        let mut widget = Widget::new(x, y, w, h, None);
        widget.set_frame(fltk::enums::FrameType::NoBox);
        let inner = Rc::new(RefCell::new(Inner::new()));
        let i2 = inner.clone();
        widget.draw(move |w| {
            let sc = device_scale(w);
            let dw = (w.w() as f32 * sc).round().max(1.0) as usize;
            let dh = (w.h() as f32 * sc).round().max(1.0) as usize;
            let mut inner = i2.borrow_mut();
            let partial = w.damage_type() == Damage::User1 && sc == 1.0;
            inner.update(dw, dh, sc);
            let c = &inner.canvas;
            if sc == 1.0 {
                let (y0, y1) = match (partial, inner.dirty) {
                    (true, Some(band)) => band,
                    _ => (0, c.h),
                };
                if y1 > y0 {
                    let s = &c.px[y0 * c.w * 3..y1 * c.w * 3];
                    let _ = draw::draw_image(
                        s,
                        w.x(),
                        w.y() + y0 as i32,
                        c.w as i32,
                        (y1 - y0) as i32,
                        ColorDepth::Rgb8,
                    );
                }
            } else if let Ok(mut img) =
                fltk::image::RgbImage::new(&c.px, c.w as i32, c.h as i32, ColorDepth::Rgb8)
            {
                img.scale(w.w(), w.h(), false, true);
                img.draw(w.x(), w.y(), w.w(), w.h());
            }
            inner.dirty = None;
        });
        let i3 = inner.clone();
        widget.handle(move |w, ev| match ev {
            Event::Focus | Event::Unfocus => {
                w.redraw();
                true
            }
            Event::KeyDown | Event::KeyUp | Event::Shortcut => {
                if ev == Event::Shortcut
                    && app::focus().map(|f| f.as_widget_ptr()) != Some(w.as_widget_ptr())
                {
                    return false;
                }
                let press = KeyPress {
                    key: app::event_key(),
                    text: app::event_text(),
                    state: app::event_state(),
                    up: ev == Event::KeyUp,
                };
                let cb = i3.borrow_mut().on_key.take();
                match cb {
                    Some(mut cb) => {
                        let took = cb(&press);
                        i3.borrow_mut().on_key = Some(cb);
                        took
                    }
                    None => false,
                }
            }
            Event::Push => {
                let _ = w.take_focus();
                let cell = Self::cell_under(w, &i3.borrow());
                let mut inner = i3.borrow_mut();
                inner.anchor = cell;
                inner.dragged = false;
                true
            }
            Event::Drag => {
                let cell = Self::cell_under(w, &i3.borrow());
                let mut inner = i3.borrow_mut();
                if let (Some(a), Some(c)) = (inner.anchor, cell) {
                    if c != a || inner.dragged {
                        inner.dragged = true;
                        inner.selection = Some((a, c));
                        drop(inner);
                        w.set_damage_type(Damage::User1);
                    }
                }
                true
            }
            Event::Released => {
                let (clicked, cell) = {
                    let inner = i3.borrow();
                    (!inner.dragged, inner.anchor)
                };
                if clicked {
                    let had = i3.borrow_mut().selection.take().is_some();
                    if had {
                        w.set_damage_type(Damage::User1);
                    }
                    if let Some((r, c)) = cell {
                        let cb = i3.borrow_mut().on_click.take();
                        if let Some(mut cb) = cb {
                            cb(r, c);
                            i3.borrow_mut().on_click = Some(cb);
                        }
                    }
                }
                true
            }
            _ => false,
        });
        TerminalView { widget, inner }
    }

    fn cell_under(w: &Widget, inner: &Inner) -> Option<(usize, usize)> {
        let l = inner.layout?;
        let sc = device_scale(w);
        let x = ((app::event_x() - w.x()) as f32 * sc).max(0.0) as usize;
        let y = ((app::event_y() - w.y()) as f32 * sc).max(0.0) as usize;
        l.cell_at(x, y)
    }

    /// The widget, for placing it in a window.
    pub fn widget(&self) -> &Widget {
        &self.widget
    }

    /// The size the widget needs at a scale: the grid and the status line.
    pub fn size_for(rows: usize, cols: usize, scale: usize) -> (i32, i32) {
        (
            (cols * GLYPH_W * scale) as i32,
            ((rows + 1) * GLYPH_H * scale) as i32,
        )
    }

    /// Shows a screen and its status line. `cursor` draws the cursor.
    pub fn show_screen(&self, s: &Screen, oia: &Oia, cursor: bool) {
        let (rows, cols) = (s.rows(), s.cols());
        let mut cells = Vec::with_capacity(rows * cols);
        for r in 0..rows {
            for c in 0..cols {
                let mut p = Paint::of(s.cell(r, c));
                p.cursor = cursor && s.addr(r, c) == s.cursor;
                cells.push(p);
            }
        }
        {
            let mut inner = self.inner.borrow_mut();
            if (inner.rows, inner.cols) != (rows, cols) {
                inner.selection = None;
            }
            inner.rows = rows;
            inner.cols = cols;
            inner.cells = cells;
            inner.status = console::oia_cells(oia, cols);
        }
        let mut w = self.widget.clone();
        w.set_damage_type(Damage::User1);
    }

    pub fn set_look(&self, look: Look) {
        self.inner.borrow_mut().look = look;
        self.widget.clone().redraw();
    }

    pub fn look(&self) -> Look {
        self.inner.borrow().look
    }

    /// Keys typed while the widget has focus. The callback returns whether
    /// it used the key.
    pub fn on_key(&self, cb: impl FnMut(&KeyPress) -> bool + 'static) {
        self.inner.borrow_mut().on_key = Some(Box::new(cb));
    }

    /// A click on a cell without a drag (0-based row and column).
    pub fn on_click(&self, cb: impl FnMut(usize, usize) + 'static) {
        self.inner.borrow_mut().on_click = Some(Box::new(cb));
    }

    /// The selected rectangle, top-left and bottom-right, 0-based.
    pub fn selection(&self) -> Option<((usize, usize), (usize, usize))> {
        self.inner
            .borrow()
            .selection
            .map(|((r0, c0), (r1, c1))| ((r0.min(r1), c0.min(c1)), (r0.max(r1), c0.max(c1))))
    }

    /// Selects a rectangle (0-based, inclusive); `None` clears.
    pub fn set_selection(&self, sel: Option<((usize, usize), (usize, usize))>) {
        self.inner.borrow_mut().selection = sel;
        self.widget.clone().set_damage_type(Damage::User1);
    }

    /// The selected cells' text, one line per row, trailing blanks dropped.
    pub fn selected_text(&self, s: &Screen) -> Option<String> {
        let ((r0, c0), (r1, c1)) = self.selection()?;
        Some(block_text(s, r0, c0, r1, c1))
    }

    /// Renders the current state into a fresh canvas of `w` by `h` pixels,
    /// for tests and Print Screen.
    pub fn render(&self, w: usize, h: usize) -> (Canvas, Layout) {
        let mut inner = self.inner.borrow_mut();
        inner.drawn.clear();
        inner.update(w, h, 1.0);
        let l = inner.layout.expect("laid out");
        (inner.canvas.clone(), l)
    }
}

/// A rectangle of screen text, one line per row, trailing blanks dropped.
pub fn block_text(s: &Screen, r0: usize, c0: usize, r1: usize, c1: usize) -> String {
    let rows = s.text_rows();
    let mut out = Vec::new();
    for row in rows.iter().take(r1.min(s.rows() - 1) + 1).skip(r0) {
        let line: String = row
            .chars()
            .skip(c0)
            .take(c1.min(s.cols() - 1) + 1 - c0)
            .collect();
        out.push(line.trim_end().to_owned());
    }
    out.join("\n")
}

fltk::widget_extends!(TerminalView, Widget, widget);
