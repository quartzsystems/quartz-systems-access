// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! Message Arrived: a message that arrived on a watched queue, as that
//! queue's option in Messages > Options says (inquiries only by default).
//!
//! - One window, not modal, for every system: arrivals queue in it, with
//!   Previous and Next and "2 of 3".
//! - It opens without taking the focus from the window being typed in (a
//!   5250 session, say), and asks to be looked at instead: the taskbar
//!   entry flashes on Windows, the urgency hint is set on X11
//!   ([`qsa_ui::attention`]). Later arrivals do the same and never bring
//!   it forward.
//! - It shows the message, its second-level text as sent and who sent it;
//!   for an inquiry waiting, a reply field and Reply. An inquiry answered
//!   meanwhile, here, in Messages or elsewhere, shows its reply instead.
//! - Open Messages opens Messages on the system; Close empties the queue.
//!
//! A sender's copy and a reply never open it.

use std::cell::RefCell;
use std::rc::Rc;

use fltk::button::Button;
use fltk::enums::{Align, CallbackTrigger, FrameType};
use fltk::frame::Frame;
use fltk::input::Input;
use fltk::prelude::*;
use fltk::text::{TextBuffer, TextDisplay, WrapMode};
use fltk::window::Window;
use qap_client::messages::needs_reply;
use qap_client::proto::ops::messages::{Key, Queue, QueuedMessage};
use qsa_msg::id;
use qsa_ui::chan::{self, Emit};
use qsa_ui::statusbar::StatusLine;
use qsa_ui::theme::{self, colors};
use qsa_ui::{Status, controls};

use crate::app::Shared;
use crate::qap::{self, lock};

use super::model;

/// Window width and height.
pub const W: i32 = 540;
pub const H: i32 = 380;

/// One message that arrived.
#[derive(Clone, Debug)]
pub struct Item {
    pub system: String,
    pub queue: Queue,
    /// The queue's name in the window, `Own` as the profile.
    pub queue_name: String,
    pub message: QueuedMessage,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Act {
    Previous,
    Next,
    Reply,
    Open,
    Close,
}

/// The window.
pub struct ArrivedWindow {
    pub window: Window,
    pub header: Frame,
    pub from: Frame,
    pub text: Frame,
    pub help: TextDisplay,
    help_text: TextBuffer,
    pub reply_label: Frame,
    pub reply: Input,
    pub reply_button: Button,
    pub previous: Button,
    pub next: Button,
    pub position: Frame,
    pub status: StatusLine,
    rx: chan::Receiver<Act>,
    st: RefCell<St>,
}

struct St {
    state: Option<Shared>,
    items: Vec<Item>,
    at: usize,
}

thread_local! {
    static ARRIVED: RefCell<Option<Rc<ArrivedWindow>>> = const { RefCell::new(None) };
}

impl ArrivedWindow {
    /// The window, empty, not shown (screenshots use it so, with
    /// [`ArrivedWindow::push`]).
    pub fn build() -> Rc<ArrivedWindow> {
        let (tx, rx) = chan::channel::<Act>();
        let m = 12;
        let fh = theme::FIELD_H;
        let (bw, bh) = (theme::BUTTON_W, theme::BUTTON_H);
        let mut window = Window::new(0, 0, W, H, None);
        window.set_label("Message Arrived");
        window.set_icon(Some(qsa_ui::glyph::app_icon()));
        window.set_color(colors::FACE);

        let mut y = m;
        let mut header = Frame::new(m, y, W - 2 * m, fh, None);
        header.set_align(Align::Left | Align::Inside);
        header.set_label_font(theme::UI_FONT_BOLD);
        header.set_label_size(theme::UI_SIZE);
        y += fh;
        let mut from = Frame::new(m, y, W - 2 * m, fh, None);
        from.set_align(Align::Left | Align::Inside);
        from.set_label_size(theme::UI_SIZE);
        y += fh + 6;
        let th = 2 * fh;
        let mut text = Frame::new(m, y, W - 2 * m, th, None);
        text.set_align(Align::Left | Align::Top | Align::Inside | Align::Wrap);
        text.set_label_size(theme::UI_SIZE);
        text.set_label_font(theme::UI_FONT_BOLD);
        y += th + 4;
        let reply_y = H - m - bh - 8 - bh - 8 - bh;
        let hh = reply_y - 8 - y;
        let help_text = TextBuffer::default();
        let mut help = TextDisplay::new(m, y, W - 2 * m, hh, None);
        help.set_buffer(Some(help_text.clone()));
        help.set_frame(FrameType::BorderBox);
        help.set_text_font(theme::UI_FONT);
        help.set_text_size(theme::UI_SIZE);
        help.wrap_mode(WrapMode::AtBounds, 0);

        let mut reply_label = controls::label(m, reply_y, 160, bh, "Reply (empty for default):");
        reply_label.set_align(Align::Left | Align::Inside);
        let mut reply = controls::field(m + 160, reply_y + (bh - fh) / 2, W - 2 * m - 166 - bw, fh);
        reply.set_trigger(CallbackTrigger::EnterKeyAlways);
        reply.set_callback(move |_| tx.send(Act::Reply));
        let mut reply_button = controls::button(W - m - bw, reply_y, bw, bh, "Reply");
        reply_button.emit_to(tx, Act::Reply);

        let status = StatusLine::new(m, reply_y + bh + 8, W - 2 * m, bh);
        let by = H - m - bh;
        let mut previous = controls::button(m, by, bw, bh, "Previous");
        previous.emit_to(tx, Act::Previous);
        let mut next = controls::button(m + bw + 6, by, bw, bh, "Next");
        next.emit_to(tx, Act::Next);
        let mut position = Frame::new(m + 2 * bw + 18, by, 80, bh, None);
        position.set_align(Align::Left | Align::Inside);
        position.set_label_size(theme::UI_SIZE);
        let ow = bw + 30;
        let mut open = controls::button(W - m - bw - 6 - ow, by, ow, bh, "Open Messages");
        open.emit_to(tx, Act::Open);
        let mut close = controls::default_button(W - m - bw, by, bw, bh, "Close");
        close.emit_to(tx, Act::Close);
        window.end();
        window.set_callback(move |_| tx.send(Act::Close));
        Rc::new(ArrivedWindow {
            window,
            header,
            from,
            text,
            help,
            help_text,
            reply_label,
            reply,
            reply_button,
            previous,
            next,
            position,
            status,
            rx,
            st: RefCell::new(St {
                state: None,
                items: Vec::new(),
                at: 0,
            }),
        })
    }

    /// Adds `item` at the end; the one shown stays shown (the first item
    /// is shown at once). Times shown with `sent`.
    pub fn push(&self, item: Item, sent: &dyn Fn(i64) -> String) {
        {
            let mut st = self.st.borrow_mut();
            let dup = st.items.iter().any(|i| {
                i.system.eq_ignore_ascii_case(&item.system)
                    && i.queue == item.queue
                    && i.message.key == item.message.key
            });
            if dup {
                return;
            }
            st.items.push(item);
        }
        self.show_item(sent);
    }

    /// Goes to item `i` (0-based).
    pub fn go(&self, i: usize, sent: &dyn Fn(i64) -> String) {
        {
            let mut st = self.st.borrow_mut();
            st.at = i.min(st.items.len().saturating_sub(1));
        }
        self.show_item(sent);
    }

    /// How many messages are queued.
    pub fn len(&self) -> usize {
        self.st.borrow().items.len()
    }

    /// Whether nothing is queued.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The inquiry `key` on `system`'s `queue` was answered with `reply`.
    pub fn answered(&self, system: &str, queue: &Queue, key: &Key, reply: &str) {
        let changed = {
            let mut st = self.st.borrow_mut();
            let mut changed = false;
            for i in st.items.iter_mut().filter(|i| {
                i.system.eq_ignore_ascii_case(system) && i.queue == *queue && i.message.key == *key
            }) {
                i.message.needs_reply = false;
                i.message.reply.get_or_insert_with(|| reply.to_owned());
                changed = true;
            }
            changed
        };
        if changed {
            self.show_item(&crate::splf::model::local_time);
        }
    }

    fn current(&self) -> Option<Item> {
        let st = self.st.borrow();
        st.items.get(st.at).cloned()
    }

    fn show_item(&self, sent: &dyn Fn(i64) -> String) {
        let (at, n) = {
            let st = self.st.borrow();
            (st.at, st.items.len())
        };
        let Some(it) = self.current() else { return };
        let m = &it.message;
        let what = if m.id.is_empty() {
            model::type_word(m.kind).to_owned()
        } else {
            format!("{} {}", model::type_word(m.kind), m.id)
        };
        self.header
            .clone()
            .set_label(&format!("{what} on {} ({})", it.queue_name, it.system));
        let job = model::job_text(m);
        let from = if job.is_empty() {
            format!("From {}, sent {}", m.from_user.trim(), sent(m.sent))
        } else {
            format!(
                "From {} (job {job}), sent {}",
                m.from_user.trim(),
                sent(m.sent)
            )
        };
        self.from.clone().set_label(&from);
        self.text.clone().set_label(m.text.trim());
        self.help_text.clone().set_text(m.help.trim_end());
        let waiting = needs_reply(m);
        let (mut rl, mut r, mut b) = (
            self.reply_label.clone(),
            self.reply.clone(),
            self.reply_button.clone(),
        );
        if waiting {
            rl.set_label("Reply (empty for default):");
            r.show();
            b.show();
        } else {
            rl.set_label(&match &m.reply {
                Some(reply) => format!("Answered: {reply}"),
                None => String::new(),
            });
            r.set_value("");
            r.hide();
            b.hide();
        }
        self.position
            .clone()
            .set_label(&format!("{} of {n}", at + 1));
        for (b, on) in [(&self.previous, at > 0), (&self.next, at + 1 < n)] {
            let mut b = b.clone();
            if on {
                b.activate();
            } else {
                b.deactivate();
            }
        }
        self.window.clone().redraw();
    }

    fn reply(&self) {
        let Some(it) = self.current().filter(|i| needs_reply(&i.message)) else {
            return;
        };
        let Some(state) = self.st.borrow().state.clone() else {
            return;
        };
        let Some(h) = state.borrow_mut().qap.signed_on_client(&it.system) else {
            self.status
                .clone()
                .set("Not signed on to that system now.", Status::Error);
            return;
        };
        let text = self.reply.value().trim().to_owned();
        let key = it.message.key;
        let t = text.clone();
        match qap::busy(|| lock(&h).reply_message(&it.queue, key, &t)) {
            Ok(()) => {
                let what = if it.message.id.is_empty() {
                    format!("the inquiry from {}", it.message.from_user.trim())
                } else {
                    it.message.id.clone()
                };
                let m = qsa_msg::message(id::REPLY_SENT, &[&what]);
                crate::log!("{}: {m}", it.system);
                self.status.clone().set(&m.to_string(), Status::Neutral);
                let shown = if text.is_empty() { "(default)" } else { &text };
                self.answered(&it.system, &it.queue, &key, shown);
            }
            Err(e) => {
                let sys = state
                    .borrow()
                    .systems
                    .get(&it.system)
                    .cloned()
                    .unwrap_or_else(|| qsa_config::System::new(&it.system));
                let x = qap::explain(&sys, &e);
                self.status.clone().set(&x.line(), Status::Error);
                qap::show(Some(&self.window), &x);
            }
        }
    }

    fn pump(&self) {
        while let Some(a) = self.rx.recv() {
            match a {
                Act::Previous | Act::Next => {
                    let at = self.st.borrow().at;
                    let to = if a == Act::Next {
                        at + 1
                    } else {
                        at.saturating_sub(1)
                    };
                    self.status.clone().set("", Status::Neutral);
                    self.go(to, &crate::splf::model::local_time);
                }
                Act::Reply => self.reply(),
                Act::Open => {
                    let (state, system) = {
                        let st = self.st.borrow();
                        (
                            st.state.clone(),
                            st.items.get(st.at).map(|i| i.system.clone()),
                        )
                    };
                    if let (Some(state), Some(system)) = (state, system) {
                        super::open(Some(&self.window), &state, &system);
                    }
                }
                Act::Close => {
                    self.window.clone().hide();
                    let mut st = self.st.borrow_mut();
                    st.items.clear();
                    st.at = 0;
                }
            }
        }
    }
}

/// Puts `item` in Message Arrived: shown without taking the focus when it
/// is not shown, and asking to be looked at either way.
pub fn arrive(state: &Shared, item: Item) {
    let w = ARRIVED.with(|a| {
        let mut a = a.borrow_mut();
        a.get_or_insert_with(ArrivedWindow::build).clone()
    });
    w.st.borrow_mut().state = Some(state.clone());
    crate::log!(
        "{}: message arrived on {}{}",
        item.system,
        item.queue_name,
        if item.message.id.is_empty() {
            String::new()
        } else {
            format!(": {}", item.message.id)
        }
    );
    w.push(item, &crate::splf::model::local_time);
    let mut win = w.window.clone();
    if !win.shown() {
        // On the main monitor, never under the pointer; moved back if the
        // system put it elsewhere, which does not activate it.
        qsa_ui::propdialog::center_on(&mut win, None);
        let (x, y) = (win.x(), win.y());
        qsa_ui::attention::show_without_focus(&mut win);
        if (win.x(), win.y()) != (x, y) {
            win.set_pos(x, y);
        }
    }
    qsa_ui::attention::ask_attention(&win);
    super::tick();
}

/// A reply arrived on `system`'s `queue`: the inquiry it answers there is
/// answered.
pub fn reply_arrived(system: &str, queue: &Queue, reply: &QueuedMessage) {
    let Some(k) = reply.answers else { return };
    if let Some(w) = ARRIVED.with(|a| a.borrow().clone()) {
        w.answered(system, queue, &k, reply.text.trim());
    }
}

/// Gives Message Arrived its waiting work.
pub fn pump() {
    if let Some(w) = ARRIVED.with(|a| a.borrow().clone()) {
        w.pump();
    }
}

/// Whether Message Arrived is open.
pub fn any_open() -> bool {
    ARRIVED.with(|a| a.borrow().as_ref().is_some_and(|w| w.window.shown()))
}
