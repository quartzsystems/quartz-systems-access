// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! The toolbar: a row of 16×16 glyph buttons below the menu bar.

use fltk::button::Button;
use fltk::enums::{Event, FrameType};
use fltk::group::Group;
use fltk::prelude::*;
use fltk::widget::Widget;

use crate::glyph::{self, Glyph, Size};
use crate::theme::{self, colors};

/// The toolbar.
pub struct Toolbar {
    group: Group,
    next_x: i32,
}

impl Toolbar {
    /// Height of the bar, including the rule below it.
    pub const HEIGHT: i32 = 30;
    /// Edge of a tool button.
    pub const BUTTON: i32 = 24;

    /// A toolbar across `w` at `y`. Add buttons, then call [`Toolbar::end`].
    pub fn new(x: i32, y: i32, w: i32) -> Toolbar {
        let mut group = Group::new(x, y, w, Self::HEIGHT, None);
        group.set_frame(FrameType::FlatBox);
        group.set_color(colors::FACE);
        group.draw(|g| {
            theme::draw_separator_h(g.x(), g.y() + g.h() - 1, g.w());
        });
        Toolbar {
            group,
            next_x: x + 4,
        }
    }

    /// Adds a tool button. It is flat until the pointer is over it, then pale
    /// blue with a light blue edge.
    pub fn add(&mut self, glyph: Glyph, tooltip: &str, mut cb: impl FnMut() + 'static) -> Button {
        let y = self.group.y() + (Self::HEIGHT - Self::BUTTON) / 2;
        let mut b = Button::new(self.next_x, y, Self::BUTTON, Self::BUTTON, None);
        self.next_x += Self::BUTTON + 1;
        b.set_frame(FrameType::FlatBox);
        b.set_down_frame(theme::PRESSED_BUTTON_BOX);
        b.set_color(colors::FACE);
        b.set_tooltip(tooltip);
        b.set_image(Some(glyph::image(glyph, Size::Small)));
        b.clear_visible_focus();
        b.handle(move |b, ev| match ev {
            Event::Enter | Event::Leave if !theme::hover_effects() => false,
            Event::Enter => {
                b.set_color(colors::TOOL_HOVER);
                b.set_frame(theme::MENU_HOT_BOX);
                b.redraw();
                true
            }
            Event::Leave => {
                b.set_color(colors::FACE);
                b.set_frame(FrameType::FlatBox);
                b.redraw();
                true
            }
            _ => false,
        });
        b.set_callback(move |_| cb());
        b
    }

    /// Adds a vertical separator.
    pub fn separator(&mut self) {
        let x = self.next_x + 3;
        let mut sep = Widget::new(x, self.group.y() + 5, 2, Self::HEIGHT - 10, None);
        sep.draw(|s| theme::draw_separator_v(s.x(), s.y(), s.h()));
        self.next_x += 8;
    }

    /// Finishes adding buttons.
    pub fn end(&mut self) {
        self.group.end();
        let mut spacer = Widget::new(self.next_x, self.group.y(), 1, 1, None);
        spacer.hide();
        self.group.add(&spacer);
        self.group.resizable(&spacer);
    }

    /// The underlying group.
    pub fn group(&self) -> &Group {
        &self.group
    }
}
