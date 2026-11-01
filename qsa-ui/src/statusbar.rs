// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! The status bar at the foot of every main window.

use std::cell::RefCell;
use std::rc::Rc;

use fltk::draw;
use fltk::enums::{Align, Color, FrameType};
use fltk::group::Group;
use fltk::prelude::*;
use fltk::widget::Widget;

use crate::theme::{self, colors};

/// What a status word means. Status words are coloured by text only.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    /// Plain text.
    Neutral,
    /// Active, running, connected: green.
    Active,
    /// Ended, error, not connected: red.
    Error,
}

impl Status {
    /// The text colour for the status.
    pub fn color(self) -> Color {
        match self {
            Status::Neutral => colors::TEXT,
            Status::Active => colors::STATUS_ACTIVE,
            Status::Error => colors::STATUS_ERROR,
        }
    }
}

/// A sunken panel showing one line of text, padded and clipped.
#[derive(Clone)]
pub struct Panel {
    widget: Widget,
    text: Rc<RefCell<(String, Color)>>,
}

impl Panel {
    /// A new, empty panel.
    pub fn new(x: i32, y: i32, w: i32, h: i32) -> Panel {
        let mut widget = Widget::new(x, y, w, h, None);
        let text = Rc::new(RefCell::new((String::new(), colors::TEXT)));
        let t = text.clone();
        widget.draw(move |wd| {
            let (x, y, w, h) = (wd.x(), wd.y(), wd.w(), wd.h());
            draw::draw_box(FrameType::ThinDownBox, x, y, w, h, colors::FACE);
            let (s, c) = &*t.borrow();
            draw::set_font(theme::UI_FONT, theme::UI_SIZE);
            draw::set_draw_color(*c);
            draw::push_clip(x + 1, y + 1, w - 2, h - 2);
            draw::draw_text2(s, x + 4, y, w - 8, h, Align::Left);
            draw::pop_clip();
        });
        Panel { widget, text }
    }

    /// Sets the text and its colour.
    pub fn set(&mut self, text: &str, status: Status) {
        *self.text.borrow_mut() = (text.to_owned(), status.color());
        self.widget.redraw();
    }

    /// The current text.
    pub fn text(&self) -> String {
        self.text.borrow().0.clone()
    }

    /// The underlying widget.
    pub fn widget(&self) -> &Widget {
        &self.widget
    }
}

/// One line of text with no box: the status line of a dialog.
#[derive(Clone)]
pub struct StatusLine {
    widget: Widget,
    text: Rc<RefCell<(String, Color)>>,
}

impl StatusLine {
    /// A new, empty status line.
    pub fn new(x: i32, y: i32, w: i32, h: i32) -> StatusLine {
        let mut widget = Widget::new(x, y, w, h, None);
        let text = Rc::new(RefCell::new((String::new(), colors::TEXT)));
        let t = text.clone();
        widget.draw(move |wd| {
            let (x, y, w, h) = (wd.x(), wd.y(), wd.w(), wd.h());
            draw::draw_rect_fill(x, y, w, h, colors::FACE);
            let (s, c) = &*t.borrow();
            draw::set_font(theme::UI_FONT, theme::UI_SIZE);
            draw::set_draw_color(*c);
            draw::push_clip(x, y, w, h);
            draw::draw_text2(s, x, y, w, h, Align::Left);
            draw::pop_clip();
        });
        StatusLine { widget, text }
    }

    /// Sets the text and its colour.
    pub fn set(&mut self, text: &str, status: Status) {
        *self.text.borrow_mut() = (text.to_owned(), status.color());
        self.widget.redraw();
    }

    /// The current text.
    pub fn text(&self) -> String {
        self.text.borrow().0.clone()
    }
}

/// The status bar: a wide message panel and a narrower status panel.
#[derive(Clone)]
pub struct StatusBar {
    group: Group,
    message: Panel,
    status: Panel,
}

impl StatusBar {
    /// Height of the bar.
    pub const HEIGHT: i32 = 22;
    /// Width of the status panel on the right.
    pub const STATUS_W: i32 = 180;

    /// A status bar across the bottom of `parent_w`, at `y`.
    pub fn new(x: i32, y: i32, w: i32) -> StatusBar {
        let mut group = Group::new(x, y, w, Self::HEIGHT, None);
        group.set_frame(FrameType::FlatBox);
        group.set_color(colors::FACE);
        let sw = Self::STATUS_W;
        let message = Panel::new(x + 2, y + 3, w - sw - 6, Self::HEIGHT - 5);
        let status = Panel::new(x + w - sw - 2, y + 3, sw, Self::HEIGHT - 5);
        group.end();
        group.resizable(message.widget());
        StatusBar {
            group,
            message,
            status,
        }
    }

    /// Shows a message in the wide panel.
    pub fn set_message(&mut self, text: &str) {
        self.message.set(text, Status::Neutral);
    }

    /// The message currently shown.
    pub fn message(&self) -> String {
        self.message.text()
    }

    /// Shows a status word in the right-hand panel.
    pub fn set_status(&mut self, text: &str, status: Status) {
        self.status.set(text, status);
    }

    /// The underlying group.
    pub fn group(&self) -> &Group {
        &self.group
    }
}
