// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! The group box: a light outline with its title set into the top border.

use fltk::draw;
use fltk::enums::{Align, FrameType, LabelType};
use fltk::group::Group;
use fltk::prelude::*;

use crate::theme::{self, colors};

/// Offset from the top of a group box to its first row of content.
pub const CONTENT_TOP: i32 = 20;
/// Inset from the left and right edges to content.
pub const CONTENT_SIDE: i32 = 10;

/// A group box. The returned group is current: add the content, then call
/// `end()` on it.
pub fn group_box(x: i32, y: i32, w: i32, h: i32, title: &str) -> Group {
    // Take the background of whatever the box sits on: the face, or a white
    // tab page. Read it before making the box, which becomes current itself.
    let bg = Group::try_current().map_or(colors::FACE, |p| p.color());
    let mut g = Group::new(x, y, w, h, None);
    g.set_frame(FrameType::FlatBox);
    g.set_color(bg);
    g.set_label(title);
    g.set_align(Align::Inside | Align::TopLeft);
    g.set_label_type(LabelType::None);
    g.draw(|g| {
        draw::set_font(theme::UI_FONT, theme::UI_SIZE);
        let title = g.label();
        let th = draw::height();
        let top = g.y() + th / 2;
        theme::draw_panel(g.x(), top, g.w(), g.h() - th / 2);
        if !title.is_empty() {
            let tw = draw::width(&title) as i32;
            draw::draw_rect_fill(g.x() + 6, g.y(), tw + 6, th, g.color());
            draw::set_draw_color(colors::TEXT);
            draw::draw_text2(&title, g.x() + 9, g.y(), tw + 2, th, Align::Left);
        }
    });
    g
}
