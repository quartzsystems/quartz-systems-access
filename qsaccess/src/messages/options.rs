// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! Messages > Options, per system, kept in `settings.toml` (`[messages]`):
//!
//! - My messages: always watched while signed on; what an arrival does
//!   (Inquiries only, the default, All messages, or None).
//! - System operator (`QSYSOPR`): watched only when chosen, off by
//!   default, and offered only when the profile may read it (a
//!   `ListMessages` of one message answers that); what an arrival does.
//!
//! OK applies the choice to the session at once: `QSYSOPR` watched or no
//! longer watched.

use fltk::button::CheckButton;
use fltk::enums::Align;
use fltk::menu::Choice;
use fltk::prelude::*;
use fltk::window::Window;
use qap_client::proto::ops::messages::Queue;
use qsa_config::settings::{Arrival, MessageOptions};
use qsa_ui::chan::{self, Emit};
use qsa_ui::theme::{self, colors};
use qsa_ui::{controls, groupbox};

use crate::app::Shared;
use crate::qap::{self, lock};

/// Dialog width and height.
pub const W: i32 = 440;
pub const H: i32 = 286;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Act {
    Operator,
    Ok,
    Cancel,
}

/// The dialog.
pub struct OptionsDialog {
    pub window: Window,
    pub own: Choice,
    pub operator: CheckButton,
    pub operator_arrival: Choice,
    rx: chan::Receiver<Act>,
    /// Whether the profile may read `QSYSOPR`.
    authorized: bool,
}

fn arrival_choice(x: i32, y: i32, w: i32, value: Arrival) -> Choice {
    let mut c = controls::combo(x, y, w, theme::FIELD_H);
    for a in Arrival::ALL {
        c.add_choice(a.label());
    }
    let at = Arrival::ALL.iter().position(|a| *a == value).unwrap_or(0);
    c.set_value(at as i32);
    c
}

fn arrival_of(c: &Choice) -> Arrival {
    usize::try_from(c.value())
        .ok()
        .and_then(|i| Arrival::ALL.get(i).copied())
        .unwrap_or_default()
}

impl OptionsDialog {
    /// The dialog for `system` as `profile` with `o`; `QSYSOPR` offered
    /// when `authorized`. Not shown (screenshots use it so).
    pub fn build(
        system: &str,
        profile: &str,
        o: MessageOptions,
        authorized: bool,
    ) -> OptionsDialog {
        let (tx, rx) = chan::channel::<Act>();
        let m = 12;
        let fh = theme::FIELD_H;
        let (bw, bh) = (theme::BUTTON_W, theme::BUTTON_H);
        let mut window = Window::new(0, 0, W, H, None);
        crate::help::attach(&mut window, crate::help::topic::MESSAGES);
        window.set_label(&format!("Messages Options - {system}"));
        window.set_color(colors::FACE);
        window.make_modal(true);
        let (cs, ct) = (groupbox::CONTENT_SIDE, groupbox::CONTENT_TOP);
        let lw = 170;
        let cw = W - 2 * m - 2 * cs - lw;

        let mut y = m;
        let gh = ct + fh + 12;
        let g = groupbox::group_box(m, y, W - 2 * m, gh, &format!("My messages ({profile})"));
        let mut l = controls::label(m + cs, y + ct, lw, fh, "When a message arrives:");
        l.set_align(Align::Left | Align::Inside);
        let own = arrival_choice(m + cs + lw, y + ct, cw, o.own_arrival);
        g.end();
        y += gh + 8;

        let gh = ct + 3 * fh + 28;
        let g = groupbox::group_box(m, y, W - 2 * m, gh, "System operator (QSYSOPR)");
        let mut operator = controls::check(
            m + cs,
            y + ct,
            W - 2 * m - 2 * cs,
            fh,
            "Watch the system operator's messages",
        );
        operator.set_value(o.operator && authorized);
        operator.emit_to(tx, Act::Operator);
        let mut l = controls::label(m + cs, y + ct + fh + 8, lw, fh, "When a message arrives:");
        l.set_align(Align::Left | Align::Inside);
        let operator_arrival = arrival_choice(m + cs + lw, y + ct + fh + 8, cw, o.operator_arrival);
        let mut note = controls::label(m + cs, y + ct + 2 * fh + 16, W - 2 * m - 2 * cs, fh, "");
        note.set_align(Align::Left | Align::Inside);
        if !authorized {
            note.set_label("Your user profile may not display QSYSOPR's messages.");
            operator.deactivate();
        }
        g.end();

        let mut ok = controls::default_button(W - m - 2 * bw - 6, H - m - bh, bw, bh, "OK");
        ok.emit_to(tx, Act::Ok);
        let mut cancel = controls::button(W - m - bw, H - m - bh, bw, bh, "Cancel");
        cancel.emit_to(tx, Act::Cancel);
        window.end();
        window.set_callback(move |_| tx.send(Act::Cancel));
        let d = OptionsDialog {
            window,
            own,
            operator,
            operator_arrival,
            rx,
            authorized,
        };
        d.operator_changed();
        d
    }

    fn operator_changed(&self) {
        let mut c = self.operator_arrival.clone();
        if self.operator.value() && self.authorized {
            c.activate();
        } else {
            c.deactivate();
        }
    }

    /// What the dialog shows now.
    pub fn options(&self) -> MessageOptions {
        MessageOptions {
            own_arrival: arrival_of(&self.own),
            operator: self.operator.value() && self.authorized,
            operator_arrival: arrival_of(&self.operator_arrival),
        }
    }

    /// Runs the dialog; the options chosen with OK.
    pub fn run(&self, parent: Option<&dyn WindowExt>) -> Option<MessageOptions> {
        let mut win = self.window.clone();
        qsa_ui::propdialog::center_on(&mut win, parent);
        qsa_ui::propdialog::show_placed(&mut win);
        while win.shown() {
            if fltk::app::wait_for(0.05).is_err() {
                return None;
            }
            while let Some(a) = self.rx.recv() {
                match a {
                    Act::Operator => self.operator_changed(),
                    Act::Ok => {
                        win.hide();
                        return Some(self.options());
                    }
                    Act::Cancel => {
                        win.hide();
                        return None;
                    }
                }
            }
        }
        None
    }
}

/// Messages > Options for `system`: asks whether the profile may read
/// `QSYSOPR`, shows the dialog, and with OK keeps the options and watches
/// `QSYSOPR` or stops.
pub fn open(parent: Option<&dyn WindowExt>, state: &Shared, system: &str) {
    let Some(h) = state.borrow_mut().qap.signed_on_client(system) else {
        return;
    };
    let authorized = match qap::busy(|| lock(&h).may_read_queue(&Queue::SystemOperator)) {
        Ok(may) => may,
        Err(e) => {
            crate::log!("QSYSOPR on {system}: {e}");
            false
        }
    };
    let profile = qap::signed_on(state, system).unwrap_or_default();
    let before = state.borrow().settings.messages.for_system(system);
    let d = OptionsDialog::build(system, &profile, before, authorized);
    let Some(after) = d.run(parent) else {
        return;
    };
    {
        let mut st = state.borrow_mut();
        st.settings.messages.set(system, after);
        if let Err(e) = st.save_settings() {
            crate::log!("settings not saved: {e}");
        }
    }
    if after.operator != before.operator {
        super::watching::operator(parent, state, system, after.operator);
    }
}
