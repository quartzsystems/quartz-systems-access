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
    /// Not a value (an SQL null): grey.
    Muted,
}

impl Status {
    /// The text colour for the status.
    pub fn color(self) -> Color {
        match self {
            Status::Neutral => colors::TEXT,
            Status::Active => colors::STATUS_ACTIVE,
            Status::Error => colors::STATUS_ERROR,
            Status::Muted => colors::DISABLED_TEXT,
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

/// What a click on the indicator does.
type OnIndicator = Rc<RefCell<Option<Box<dyn FnMut()>>>>;

/// The status bar: a wide message panel and a narrower status panel, and
/// between them, when set, an indicator panel that opens something when
/// clicked ([`StatusBar::set_indicator`]).
#[derive(Clone)]
pub struct StatusBar {
    group: Group,
    message: Panel,
    indicator: Panel,
    status: Panel,
    on_indicator: OnIndicator,
}

impl StatusBar {
    /// Height of the bar.
    pub const HEIGHT: i32 = 22;
    /// Width of the status panel on the right.
    pub const STATUS_W: i32 = 180;
    /// Width of the indicator panel.
    pub const INDICATOR_W: i32 = 150;

    /// A status bar across the bottom of `parent_w`, at `y`.
    pub fn new(x: i32, y: i32, w: i32) -> StatusBar {
        let mut group = Group::new(x, y, w, Self::HEIGHT, None);
        group.set_frame(FrameType::FlatBox);
        group.set_color(colors::FACE);
        let sw = Self::STATUS_W;
        let iw = Self::INDICATOR_W;
        let message = Panel::new(x + 2, y + 3, w - sw - 6, Self::HEIGHT - 5);
        let mut indicator = Panel::new(x + w - sw - iw - 4, y + 3, iw, Self::HEIGHT - 5);
        let status = Panel::new(x + w - sw - 2, y + 3, sw, Self::HEIGHT - 5);
        group.end();
        group.resizable(message.widget());
        indicator.widget.hide();
        let on_indicator: OnIndicator = Rc::new(RefCell::new(None));
        let cb = on_indicator.clone();
        indicator.widget.handle(move |_, ev| {
            if ev != fltk::enums::Event::Push {
                return false;
            }
            let taken = cb.borrow_mut().take();
            if let Some(mut f) = taken {
                f();
                cb.borrow_mut().get_or_insert(f);
            }
            true
        });
        StatusBar {
            group,
            message,
            indicator,
            status,
            on_indicator,
        }
    }

    /// Shows `text` in the indicator panel, or hides it (`None`); the
    /// message panel gives it the room.
    pub fn set_indicator(&mut self, text: Option<&str>) {
        let (x, y, w) = (self.group.x(), self.group.y(), self.group.w());
        let (sw, iw, h) = (Self::STATUS_W, Self::INDICATOR_W, Self::HEIGHT - 5);
        match text {
            Some(t) => {
                self.indicator.set(t, Status::Neutral);
                self.indicator
                    .widget
                    .resize(x + w - sw - iw - 4, y + 3, iw, h);
                self.indicator.widget.show();
                self.message.widget.resize(x + 2, y + 3, w - sw - iw - 8, h);
            }
            None => {
                self.indicator.widget.hide();
                self.message.widget.resize(x + 2, y + 3, w - sw - 6, h);
            }
        }
        self.group.redraw();
    }

    /// The indicator's text, when it is shown.
    pub fn indicator(&self) -> Option<String> {
        self.indicator
            .widget
            .visible()
            .then(|| self.indicator.text())
    }

    /// Called when the indicator is clicked.
    pub fn on_indicator(&mut self, f: impl FnMut() + 'static) {
        *self.on_indicator.borrow_mut() = Some(Box::new(f));
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
