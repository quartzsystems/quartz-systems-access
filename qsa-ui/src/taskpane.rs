// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! The task pane: groups of text links under tinted, bold header bands, on a
//! white, scrollable pane.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use fltk::draw;
use fltk::enums::{Align, Cursor, Event, FrameType, Key};
use fltk::group::{Scroll, ScrollType};
use fltk::prelude::*;
use fltk::widget::Widget;

use crate::glyph::{self, Glyph, Size};
use crate::theme::{self, colors};

/// The task pane.
pub struct TaskPane {
    scroll: Scroll,
    next_y: i32,
    groups: usize,
}

impl TaskPane {
    /// Height of a group title row.
    pub const TITLE_H: i32 = 20;
    /// Height of a link row.
    pub const LINK_H: i32 = 20;
    /// Gap above every group but the first.
    pub const GROUP_GAP: i32 = 8;
    const PAD: i32 = 6;

    /// A task pane at the given place. Add groups and links, then call
    /// [`TaskPane::end`].
    pub fn new(x: i32, y: i32, w: i32, h: i32) -> TaskPane {
        let mut scroll = Scroll::new(x, y, w, h, None);
        scroll.set_frame(FrameType::DownBox);
        scroll.set_color(colors::WINDOW);
        scroll.set_type(ScrollType::Vertical);
        TaskPane {
            next_y: y + 2 + 4,
            scroll,
            groups: 0,
        }
    }

    fn inner_x(&self) -> i32 {
        self.scroll.x() + 2 + Self::PAD
    }

    fn inner_w(&self) -> i32 {
        self.scroll.w() - 4 - 2 * Self::PAD - self.scroll.scrollbar_size()
    }

    /// Starts a group: a tinted header band with a bold title.
    pub fn add_group(&mut self, title: &str) -> Widget {
        if self.groups > 0 {
            self.next_y += Self::GROUP_GAP;
        }
        self.groups += 1;
        let mut w = Widget::new(
            self.inner_x(),
            self.next_y,
            self.inner_w(),
            Self::TITLE_H,
            None,
        );
        w.set_label(title);
        w.draw(|w| {
            let (x, y, ww, h) = (w.x(), w.y(), w.w(), w.h());
            draw::draw_rect_fill(x, y, ww, h, colors::TASK_HEADER);
            theme::draw_separator_h(x, y, ww);
            theme::draw_separator_h(x, y + h - 1, ww);
            draw::set_font(theme::UI_FONT_BOLD, theme::UI_SIZE);
            draw::set_draw_color(colors::TEXT);
            draw::draw_text2(&w.label(), x + 6, y, ww - 12, h, Align::Left);
        });
        self.next_y += Self::TITLE_H + 2;
        w
    }

    /// Adds a link to the current group. `cb` runs on click, Enter or Space.
    pub fn add_link(
        &mut self,
        glyph: Option<Glyph>,
        text: &str,
        mut cb: impl FnMut() + 'static,
    ) -> Widget {
        let mut w = Widget::new(
            self.inner_x() + 8,
            self.next_y,
            self.inner_w() - 8,
            Self::LINK_H,
            None,
        );
        self.next_y += Self::LINK_H;
        w.set_label(text);
        let hover = Rc::new(Cell::new(false));
        let pressed = Rc::new(Cell::new(false));
        let image = RefCell::new(glyph.map(|g| glyph::image(g, Size::Small)));
        let h = hover.clone();
        w.draw(move |w| {
            let (x, y, ww, hh) = (w.x(), w.y(), w.w(), w.h());
            draw::draw_rect_fill(x, y, ww, hh, colors::WINDOW);
            let mut tx = x + 2;
            if let Some(img) = image.borrow_mut().as_mut() {
                img.draw(x + 2, y + (hh - 16) / 2, 16, 16);
                tx += 16 + 6;
            }
            draw::set_font(theme::UI_FONT, theme::UI_SIZE);
            draw::set_draw_color(colors::LINK);
            let label = w.label();
            let tw = draw::width(&label) as i32;
            draw::draw_text2(&label, tx, y, ww - (tx - x), hh, Align::Left);
            let base = y + (hh + draw::height()) / 2 - draw::descent() + 1;
            if h.get() {
                draw::draw_rect_fill(tx, base, tw, 1, colors::LINK);
            }
            if fltk::app::focus().is_some_and(|f| f.as_widget_ptr() == w.as_widget_ptr()) {
                draw::set_draw_color(colors::TEXT);
                draw::draw_focus_rect(tx - 2, y + 1, tw + 4, hh - 2);
            }
        });
        let h = hover.clone();
        w.handle(move |w, ev| match ev {
            Event::Enter => {
                h.set(true);
                set_cursor(w, Cursor::Hand);
                w.redraw();
                true
            }
            Event::Leave => {
                h.set(false);
                pressed.set(false);
                set_cursor(w, Cursor::Default);
                w.redraw();
                true
            }
            Event::Push => {
                pressed.set(true);
                let _ = w.take_focus();
                true
            }
            Event::Released => {
                if pressed.replace(false) && h.get() {
                    cb();
                }
                true
            }
            Event::Focus | Event::Unfocus => {
                w.redraw();
                true
            }
            Event::KeyDown
                if matches!(fltk::app::event_key(), Key::Enter | Key::KPEnter)
                    || fltk::app::event_text() == " " =>
            {
                cb();
                true
            }
            _ => false,
        });
        w
    }

    /// Finishes the pane.
    pub fn end(&mut self) {
        self.scroll.end();
    }

    /// The underlying scroll group.
    pub fn scroll(&self) -> &Scroll {
        &self.scroll
    }
}

fn set_cursor(w: &Widget, cursor: Cursor) {
    if let Some(mut win) = w.window() {
        win.set_cursor(cursor);
    }
}
