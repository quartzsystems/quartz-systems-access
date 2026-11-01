// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! Showing `QAD` messages: a small message box with a Details... button, and
//! the message details dialog with Message, Cause and Recovery, in that order.

use fltk::app;
use fltk::draw;
use fltk::enums::{Align, FrameType};
use fltk::frame::Frame;
use fltk::prelude::*;
use fltk::window::Window;
use qsa_msg::{Kind, Message};

use crate::controls;
use crate::glyph::Glyph;
use crate::groupbox::{self, CONTENT_SIDE, CONTENT_TOP};
use crate::msgbox::MessageBox;
use crate::propdialog::center_on;
use crate::theme::{self, colors};

/// Width of the details dialog.
pub const DETAILS_W: i32 = 460;

/// The glyph shown beside a message of this kind, if any.
pub fn glyph_for(m: &Message) -> Option<Glyph> {
    match m.kind() {
        Kind::Information | Kind::Warning => None,
        Kind::Error => Some(Glyph::Cross),
    }
}

/// Builds the details dialog for `m`. It is not shown.
pub fn details_window(m: &Message) -> Window {
    let w = DETAILS_W;
    let margin = 10;
    let inner = w - 2 * margin - 2 * CONTENT_SIDE;
    draw::set_font(theme::UI_FONT, theme::UI_SIZE);
    let section_h = |text: &str| draw::wrap_measure(text, inner, false).1.max(14);
    let fit = |t: &str| crate::msgbox::break_long_words(t, inner, draw::width);
    let sections = [
        ("Message", fit(&m.text)),
        ("Cause", fit(&m.cause)),
        ("Recovery", fit(&m.recovery)),
    ];
    let heights: Vec<i32> = sections.iter().map(|(_, t)| section_h(t)).collect();
    let mut h = margin + 20 + 6;
    for sh in &heights {
        h += CONTENT_TOP + sh + 10 + 6;
    }
    h += theme::BUTTON_H + margin;

    let mut win = Window::new(0, 0, w, h, None);
    win.set_label(&format!("Message Details - {}", m.id));
    win.set_color(colors::FACE);
    win.make_modal(true);

    let mut y = margin;
    controls::label(margin, y, 70, 20, "Message ID:");
    let mut id = controls::label(margin + 70, y, 90, 20, &m.id);
    id.set_label_font(theme::UI_FONT_BOLD);
    controls::label(margin + 180, y, 60, 20, "Severity:");
    controls::label(margin + 240, y, 40, 20, &format!("{:02}", m.severity));
    y += 20 + 6;

    for ((title, text), sh) in sections.iter().zip(&heights) {
        let gh = CONTENT_TOP + sh + 10;
        let g = groupbox::group_box(margin, y, w - 2 * margin, gh, title);
        let mut body = Frame::new(margin + CONTENT_SIDE, y + CONTENT_TOP, inner, *sh, None);
        body.set_frame(FrameType::NoBox);
        body.set_label(text);
        body.set_align(Align::Left | Align::Top | Align::Inside | Align::Wrap);
        body.set_label_size(theme::UI_SIZE);
        g.end();
        y += gh + 6;
    }

    let mut close = controls::default_button(
        w - margin - theme::BUTTON_W,
        h - margin - theme::BUTTON_H,
        theme::BUTTON_W,
        theme::BUTTON_H,
        "Close",
    );
    win.end();
    let mut wc = win.clone();
    close.set_callback(move |_| wc.hide());
    win
}

/// Shows the details dialog for `m` and waits until it is closed.
pub fn show_details(parent: Option<&dyn WindowExt>, m: &Message) {
    let mut win = details_window(m);
    center_on(&mut win, parent);
    win.show();
    while win.shown() {
        app::wait();
    }
}

/// The small message box for `m`: first-level text, OK and Details....
pub fn message_box(m: &Message) -> MessageBox {
    let mb = MessageBox::new(
        "QS/400 Access for Desktop",
        &format!("{}\n\nMessage ID: {}", m.text, m.id),
    )
    .buttons(&["OK", "Details..."], 0, 0);
    match glyph_for(m) {
        Some(g) => mb.glyph(g),
        None => mb,
    }
}

/// Shows the small message box for `m`. Details... opens the details dialog;
/// the box stays until OK or Escape.
pub fn show_message(parent: Option<&dyn WindowExt>, m: &Message) {
    while message_box(m).run(parent) == 1 {
        show_details(parent, m);
    }
}
