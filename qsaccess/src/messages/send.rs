// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! Send Message: a message to a user profile's queue or to a message
//! queue, on the shared connection (version 6's `SendMessage`). `*Own` is
//! not offered: a message to oneself goes to one's own profile.
//!
//! An inquiry ("Inquiry (wait for a reply)") gets its reply on the
//! sender's own queue, so the dialog makes sure that queue is watched
//! before it sends. Its lower part then says "Waiting for reply..." and,
//! when it arrives, the reply: the `Reply` on the sender's own queue whose
//! `answers` is the key `SendMessage` returned, and nothing else. The
//! dialog does not hold anything up: closing it leaves the reply to arrive
//! in Messages as any message does.

use std::cell::RefCell;
use std::rc::Rc;

use fltk::button::{Button, CheckButton, RadioRoundButton};
use fltk::enums::{Align, FrameType};
use fltk::frame::Frame;
use fltk::group::Group;
use fltk::input::{Input, MultilineInput};
use fltk::prelude::*;
use fltk::window::Window;
use qap_client::messages::is_reply_to;
use qap_client::proto::ops::messages::{Key, Queue, QueuedMessage, Recipient};
use qap_client::{Watch, WatchEvent};
use qsa_config::System;
use qsa_msg::id;
use qsa_ui::chan::{self, Emit};
use qsa_ui::statusbar::StatusLine;
use qsa_ui::theme::{self, colors};
use qsa_ui::{Status, controls};

use crate::app::Shared;
use crate::qap::{self, Handle, lock};

use super::model;

/// Dialog width and height.
pub const W: i32 = 500;
pub const H: i32 = 370;

/// The most text one message takes.
pub const MAX_TEXT: usize = 512;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Act {
    To,
    Send,
    Close,
}

/// The dialog.
pub struct SendDialog {
    pub window: Window,
    pub to_user: RadioRoundButton,
    pub to_queue: RadioRoundButton,
    pub user: Input,
    pub queue: Input,
    pub text: MultilineInput,
    pub inquiry: CheckButton,
    /// "Waiting for reply..." and then the reply.
    pub waiting: Frame,
    pub reply: Frame,
    pub status: StatusLine,
    pub send: Button,
    rx: chan::Receiver<Act>,
    st: RefCell<St>,
}

struct St {
    system: System,
    handle: Option<Handle>,
    /// The inquiry sent: the key its reply answers, and to whom it went.
    waiting_for: Option<(Key, String)>,
}

thread_local! {
    static DIALOGS: RefCell<Vec<Rc<SendDialog>>> = const { RefCell::new(Vec::new()) };
}

impl SendDialog {
    /// The dialog for `system`, to `to` when given; not shown, without a
    /// connection (screenshots), until [`open`] gives it one.
    pub fn build(system: &System, to: Option<&str>) -> Rc<SendDialog> {
        let (tx, rx) = chan::channel::<Act>();
        let m = 12;
        let fh = theme::FIELD_H;
        let (bw, bh) = (theme::BUTTON_W, theme::BUTTON_H);
        let mut window = Window::new(0, 0, W, H, None);
        window.set_label(&format!("Send Message - {}", system.name));
        window.set_icon(Some(qsa_ui::glyph::app_icon()));
        window.set_color(colors::FACE);

        let gh = 2 * fh + 38;
        let g = qsa_ui::groupbox::group_box(m, m, W - 2 * m, gh, "To");
        let (gx, gy) = (
            m + qsa_ui::groupbox::CONTENT_SIDE,
            m + qsa_ui::groupbox::CONTENT_TOP,
        );
        let lw = 120;
        let fw = W - 2 * m - 2 * qsa_ui::groupbox::CONTENT_SIDE - lw;
        let radios = Group::new(gx, gy, lw, 2 * fh + 8, None);
        let mut to_user = controls::radio(gx, gy, lw, fh, "User profile:");
        to_user.emit_to(tx, Act::To);
        let mut to_queue = controls::radio(gx, gy + fh + 8, lw, fh, "Message queue:");
        to_queue.emit_to(tx, Act::To);
        radios.end();
        let mut user = controls::field(gx + lw, gy, 140, fh);
        user.set_maximum_size(10);
        let mut queue = controls::field(gx + lw, gy + fh + 8, fw, fh);
        queue.set_maximum_size(21);
        queue.set_tooltip("LIBRARY/NAME, or NAME to look for it in the library list.");
        g.end();
        to_user.set_value(true);
        if let Some(t) = to {
            user.set_value(t);
        }

        let mut y = m + gh + 8;
        let mut l = controls::label(m, y, 80, fh, "Message:");
        l.set_align(Align::Left | Align::Inside);
        y += fh;
        let th = 96;
        let mut text = MultilineInput::new(m, y, W - 2 * m, th, None);
        text.set_text_size(theme::UI_SIZE);
        text.set_maximum_size(MAX_TEXT as i32);
        text.set_wrap(true);
        y += th + 8;
        let inquiry = controls::check(m, y, 260, fh, "Inquiry (wait for a reply)");
        y += fh + 10;

        let mut waiting = Frame::new(m, y, W - 2 * m, fh, None);
        waiting.set_align(Align::Left | Align::Inside);
        waiting.set_label_size(theme::UI_SIZE);
        let mut reply = Frame::new(m, y + fh, W - 2 * m, 2 * fh, None);
        reply.set_align(Align::Left | Align::Top | Align::Inside | Align::Wrap);
        reply.set_label_size(theme::UI_SIZE);
        reply.set_label_font(theme::UI_FONT_BOLD);
        reply.set_frame(FrameType::NoBox);

        let status = StatusLine::new(m, H - m - bh, W - 3 * m - 2 * bw - 6, bh);
        let mut send = controls::default_button(W - m - 2 * bw - 6, H - m - bh, bw, bh, "Send");
        send.emit_to(tx, Act::Send);
        let mut close = controls::button(W - m - bw, H - m - bh, bw, bh, "Close");
        close.emit_to(tx, Act::Close);
        window.end();
        window.set_callback(move |_| tx.send(Act::Close));
        let d = Rc::new(SendDialog {
            window,
            to_user,
            to_queue,
            user,
            queue,
            text,
            inquiry,
            waiting,
            reply,
            status,
            send,
            rx,
            st: RefCell::new(St {
                system: system.clone(),
                handle: None,
                waiting_for: None,
            }),
        });
        d.to_changed();
        d
    }

    /// The system's name.
    pub fn system(&self) -> String {
        self.st.borrow().system.name.clone()
    }

    fn to_changed(&self) {
        let (mut u, mut q) = (self.user.clone(), self.queue.clone());
        if self.to_user.value() {
            u.activate();
            q.deactivate();
        } else {
            q.activate();
            u.deactivate();
        }
    }

    /// Where the message goes, and its name for messages: `None` with why
    /// when it is not filled in.
    fn recipient(&self) -> Result<(Recipient, String), String> {
        if self.to_user.value() {
            let p = self.user.value().trim().to_ascii_uppercase();
            if p.is_empty() {
                return Err("Type the user profile to send to.".into());
            }
            Ok((Recipient::User { profile: p.clone() }, p))
        } else {
            let t = self.queue.value();
            let q = model::named_queue(&t).ok_or("Type the message queue as LIBRARY/NAME.")?;
            let name = model::queue_name(&q, "");
            Ok((Recipient::Queue { queue: q }, name))
        }
    }

    /// Shows "Waiting for reply..." for an inquiry to `to` (screenshots and
    /// a send).
    pub fn show_waiting(&self, key: Key, to: &str) {
        self.st.borrow_mut().waiting_for = Some((key, to.to_owned()));
        self.waiting
            .clone()
            .set_label(&format!("Inquiry sent to {to}. Waiting for reply..."));
        self.reply.clone().set_label("");
        self.window.clone().redraw();
    }

    /// The reply to the inquiry this dialog waits for, if `m` is it.
    pub fn take_reply(&self, m: &QueuedMessage) -> bool {
        let Some((key, _)) = self.st.borrow().waiting_for.clone() else {
            return false;
        };
        if !is_reply_to(m, &key) {
            return false;
        }
        self.st.borrow_mut().waiting_for = None;
        let msg = qsa_msg::message(id::REPLY_RECEIVED, &[m.from_user.trim(), m.text.trim()]);
        crate::log!("{}: {msg}", self.system());
        // QAD9005 in full goes to the log; the dialog shows its parts.
        self.waiting
            .clone()
            .set_label(&format!("Reply received from {}:", m.from_user.trim()));
        self.reply.clone().set_label(m.text.trim());
        self.status.clone().set("", Status::Neutral);
        self.window.clone().redraw();
        true
    }

    fn send(&self) {
        let (to, name) = match self.recipient() {
            Ok(r) => r,
            Err(why) => {
                self.status.clone().set(&why, Status::Error);
                return;
            }
        };
        let text = self.text.value();
        if text.trim().is_empty() {
            self.status
                .clone()
                .set("Type the message to send.", Status::Error);
            return;
        }
        let inquiry = self.inquiry.value();
        let Some(h) = self.st.borrow().handle.clone() else {
            return;
        };
        let r = qap::busy(|| {
            let mut c = lock(&h);
            // The reply comes to one's own queue: watched before the send,
            // so it cannot arrive unseen.
            if inquiry && let Err(e) = c.watch(Watch::Queue(Queue::Own)) {
                crate::log!("watching the own queue for a reply: {e}");
            }
            c.send_message(&to, &text, inquiry)
        });
        let sys = self.st.borrow().system.clone();
        match r {
            Ok(key) => {
                let m = qsa_msg::message(id::MESSAGE_SENT, &[&name]);
                crate::log!("{}: {m}", sys.name);
                self.status.clone().set(&m.to_string(), Status::Neutral);
                match key {
                    Some(k) if inquiry => self.show_waiting(k, &name),
                    _ => {
                        self.waiting.clone().set_label("");
                        self.reply.clone().set_label("");
                    }
                }
                self.text.clone().set_value("");
            }
            Err(e) => {
                let x = qap::explain(&sys, &e);
                self.status.clone().set(&x.line(), Status::Error);
                qap::show(Some(&self.window), &x);
            }
        }
    }

    fn pump(&self) -> bool {
        while let Some(a) = self.rx.recv() {
            match a {
                Act::To => self.to_changed(),
                Act::Send => self.send(),
                Act::Close => self.window.clone().hide(),
            }
        }
        self.window.shown()
    }
}

/// Opens Send Message for `system`, to `to` when given, signing on first
/// when this program has no session on it. Before version 6: `QAD5001`.
pub fn open(parent: Option<&dyn WindowExt>, state: &Shared, system: &str, to: Option<&str>) {
    let Some(sys) = state.borrow().systems.get(system).cloned() else {
        return;
    };
    let Some(h) = qap::session(parent, state, &sys.name) else {
        return;
    };
    let op = qap_client::proto::Op::SendMessage;
    let unsupported = h.try_lock().ok().and_then(|mut c| {
        let has = c.version().unwrap_or(1);
        c.supports(op).err().map(|_| has)
    });
    if let Some(has) = unsupported {
        let m = qap::version_needed(super::FUNCTION, &sys.name, op, has);
        crate::log!("{m}");
        qsa_ui::msgdetails::show_message(parent, &m);
        return;
    }
    let d = SendDialog::build(&sys, to);
    d.st.borrow_mut().handle = Some(h);
    let mut win = d.window.clone();
    qsa_ui::propdialog::center_on(&mut win, parent);
    qsa_ui::propdialog::show_placed(&mut win);
    DIALOGS.with(|v| v.borrow_mut().push(d));
    super::tick();
}

/// What `system`'s watches reported: a reply one of its dialogs waits for.
pub fn watch_events(system: &str, events: &[WatchEvent]) {
    let dialogs: Vec<Rc<SendDialog>> = DIALOGS.with(|v| {
        v.borrow()
            .iter()
            .filter(|d| d.window.shown() && d.system().eq_ignore_ascii_case(system))
            .cloned()
            .collect()
    });
    for e in events {
        if let WatchEvent::Arrived {
            queue: Queue::Own,
            message,
            ..
        } = e
        {
            for d in &dialogs {
                if d.take_reply(message) {
                    break;
                }
            }
        }
    }
}

/// Gives every Send Message dialog its waiting work; forgets closed ones.
pub fn pump() {
    let dialogs: Vec<Rc<SendDialog>> = DIALOGS.with(|v| v.borrow().clone());
    let open: Vec<bool> = dialogs.iter().map(|d| d.pump()).collect();
    DIALOGS.with(|v| {
        let mut i = 0;
        v.borrow_mut().retain(|_| {
            let keep = open.get(i).copied().unwrap_or(true);
            i += 1;
            keep
        });
    });
}

/// Whether any Send Message dialog is open.
pub fn any_open() -> bool {
    DIALOGS.with(|v| v.borrow().iter().any(|d| d.window.shown()))
}
