// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! Showing `QAD` messages: a small message box with a Details... button, and
//! the message details dialog with Message, Cause and Recovery, in that order.
//!
//! A message the system sent ([`SystemMessage`]) is shown as it came: its
//! first-level text in the box, its second-level text in the details dialog,
//! neither reworded nor split.

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

/// What opens the reference entry of one of this program's own messages.
static MESSAGE_HELP: std::sync::OnceLock<fn(&str)> = std::sync::OnceLock::new();

/// Makes the message ID in the details dialog of this program's own
/// messages (`QAD` and four digits) a link: selecting it calls `open` with
/// the ID, for the application to show the message in its Help. Until this
/// is called, and for a system's messages, the ID is plain text.
pub fn set_message_help(open: fn(&str)) {
    let _ = MESSAGE_HELP.set(open);
}

/// Whether `id` is one of this program's own message IDs.
fn is_own(id: &str) -> bool {
    id.strip_prefix("QAD")
        .is_some_and(|d| d.len() == 4 && d.bytes().all(|b| b.is_ascii_digit()))
}

/// The glyph shown beside a message of this kind, if any.
pub fn glyph_for(m: &Message) -> Option<Glyph> {
    match m.kind() {
        Kind::Information | Kind::Warning => None,
        Kind::Error => Some(Glyph::Cross),
    }
}

/// Builds the details dialog for `m`. It is not shown.
pub fn details_window(m: &Message) -> Window {
    details(
        &m.id,
        m.severity,
        &[
            ("Message", &m.text),
            ("Cause", &m.cause),
            ("Recovery", &m.recovery),
        ],
    )
}

/// A message from the system (a QS/400 message ID such as `CPF1107`), with
/// its text already substituted by the system.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SystemMessage<'a> {
    pub id: &'a str,
    pub severity: u8,
    /// First level.
    pub text: &'a str,
    /// Second level, as the system sent it.
    pub help: &'a str,
}

/// Builds the details dialog for a system's message: its first and second
/// level as sent. It is not shown.
pub fn system_details_window(m: &SystemMessage) -> Window {
    let help = if m.help.trim().is_empty() {
        "The system sent no second-level text."
    } else {
        m.help
    };
    details(m.id, m.severity, &[("Message", m.text), ("Help", help)])
}

/// The details dialog: the ID and severity, then each section in a group.
fn details(id: &str, severity: u8, sections: &[(&str, &str)]) -> Window {
    let w = DETAILS_W;
    let margin = 10;
    let inner = w - 2 * margin - 2 * CONTENT_SIDE;
    // FLTK wraps the body labels this far in from their frames.
    let wrap = inner - 2 * crate::msgbox::LABEL_INSET;
    draw::set_font(theme::UI_FONT, theme::UI_SIZE);
    let section_h = |text: &str| draw::wrap_measure(text, wrap, false).1.max(14);
    let fit = |t: &str| crate::msgbox::break_long_words(t, wrap, draw::width);
    let sections: Vec<(&str, String)> = sections.iter().map(|(t, s)| (*t, fit(s))).collect();
    let heights: Vec<i32> = sections.iter().map(|(_, t)| section_h(t)).collect();
    let mut h = margin + 20 + 6;
    for sh in &heights {
        h += CONTENT_TOP + sh + 10 + 6;
    }
    h += theme::BUTTON_H + margin;

    let mut win = Window::new(0, 0, w, h, None);
    win.set_label(&format!("Message Details - {id}"));
    win.set_color(colors::FACE);
    win.make_modal(true);

    let mut y = margin;
    controls::label(margin, y, 70, 20, "Message ID:");
    match MESSAGE_HELP.get().filter(|_| is_own(id)) {
        Some(open) => {
            let open = *open;
            let mut link = controls::link(margin + 70, y, 90, 20, id);
            link.set_tooltip("Show this message in Help");
            let id = id.to_owned();
            link.set_callback(move |_| open(&id));
        }
        None => {
            let mut id = controls::label(margin + 70, y, 90, 20, id);
            id.set_label_font(theme::UI_FONT_BOLD);
        }
    }
    controls::label(margin + 180, y, 60, 20, "Severity:");
    controls::label(margin + 240, y, 40, 20, &format!("{severity:02}"));
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
    run_details(parent, details_window(m));
}

fn run_details(parent: Option<&dyn WindowExt>, mut win: Window) {
    center_on(&mut win, parent);
    crate::propdialog::show_placed(&mut win);
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

/// The small message box for `m` with `buttons` and Details... after them.
/// Escape chooses the last of `buttons`.
pub fn ask_message_box(m: &Message, buttons: &[&str]) -> MessageBox {
    let mut labels: Vec<&str> = buttons.to_vec();
    labels.push("Details...");
    let last = buttons.len().saturating_sub(1);
    let mb = MessageBox::new(
        "QS/400 Access for Desktop",
        &format!("{}\n\nMessage ID: {}", m.text, m.id),
    )
    .buttons(&labels, 0, last);
    match glyph_for(m) {
        Some(g) => mb.glyph(g),
        None => mb,
    }
}

/// [`ask_message_box`] until one of `buttons` is chosen; its index.
pub fn ask_message(parent: Option<&dyn WindowExt>, m: &Message, buttons: &[&str]) -> usize {
    loop {
        let i = ask_message_box(m, buttons).run(parent);
        if i < buttons.len() {
            return i;
        }
        show_details(parent, m);
    }
}

/// The message box for a system's message: this program's own message about
/// it first when there is one (`context`), then the system's first-level text
/// and ID as sent, with `buttons` and Details... after them.
pub fn system_message_box(
    context: Option<&Message>,
    m: &SystemMessage,
    buttons: &[&str],
) -> MessageBox {
    let text = match context {
        Some(c) => format!(
            "{} ({})

{}

Message ID: {}",
            c.text, c.id, m.text, m.id
        ),
        None => format!(
            "{}

Message ID: {}",
            m.text, m.id
        ),
    };
    let mut labels: Vec<&str> = buttons.to_vec();
    labels.push("Details...");
    let last = buttons.len().saturating_sub(1);
    let mb = MessageBox::new("QS/400 Access for Desktop", &text).buttons(&labels, 0, last);
    let error = context.map_or(m.severity >= 20, |c| c.kind() == Kind::Error);
    if error { mb.glyph(Glyph::Cross) } else { mb }
}

/// Shows [`system_message_box`] until one of `buttons` is chosen (Details...
/// opens the system's second-level text) and returns its index.
pub fn ask_system_message(
    parent: Option<&dyn WindowExt>,
    context: Option<&Message>,
    m: &SystemMessage,
    buttons: &[&str],
) -> usize {
    loop {
        let i = system_message_box(context, m, buttons).run(parent);
        if i < buttons.len() {
            return i;
        }
        run_details(parent, system_details_window(m));
    }
}

/// Shows a system's message with OK.
pub fn show_system_message(
    parent: Option<&dyn WindowExt>,
    context: Option<&Message>,
    m: &SystemMessage,
) {
    ask_system_message(parent, context, m, &["OK"]);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_this_programs_message_ids_link_to_help() {
        assert!(is_own("QAD0001"));
        assert!(is_own("QAD9021"));
        // A system's message, and things that only look like an ID.
        assert!(!is_own("CPF1107"));
        assert!(!is_own("QAD001"));
        assert!(!is_own("QAD00010"));
        assert!(!is_own("QADX001"));
        assert!(!is_own(""));
    }
}
