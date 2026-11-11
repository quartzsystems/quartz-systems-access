// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! The Change Password dialog. Before sign-on (after an expired password)
//! it changes the password and signs on; signed on, it changes the session's
//! own profile's password only, so the user ID is fixed. The system's
//! password rules decide; a refusal, including one from a password
//! validation program (CPF22C6), is shown exactly as the system sent it.

use fltk::app;
use fltk::input::{Input, SecretInput};
use fltk::prelude::*;
use fltk::window::Window;
use qap_client::{Error, Secret};
use qsa_config::System;
use qsa_msg::id;
use qsa_net::{ConnectError, TlsError};
use qsa_ui::chan::{self, Emit};
use qsa_ui::controls;
use qsa_ui::msgdetails;
use qsa_ui::propdialog::{center_on, show_placed};
use qsa_ui::statusbar::{Status as Shown, StatusLine};
use qsa_ui::theme::{self, colors};

use crate::app::Shared;
use crate::qap;

/// Dialog width.
pub const W: i32 = 400;
/// Dialog height.
pub const H: i32 = 224;

/// The dialog's widgets.
pub struct ChangePasswordDialog {
    pub window: Window,
    pub user: Input,
    pub old: SecretInput,
    pub new: SecretInput,
    pub confirm: SecretInput,
    pub status: StatusLine,
}

/// Builds the dialog for `system`. `fixed` keeps the user ID as given (a
/// signed-on session's own profile). It is not shown. `true` arrives for
/// OK, `false` for Cancel.
pub fn build(
    system: &str,
    user: &str,
    fixed: bool,
) -> (ChangePasswordDialog, chan::Receiver<bool>) {
    let (tx, rx) = chan::channel::<bool>();
    let m = 12;
    let lw = 150;
    let fw = 180;
    let fh = theme::FIELD_H;
    let (bw, bh) = (theme::BUTTON_W, theme::BUTTON_H);
    let mut window = Window::new(0, 0, W, H, None);
    crate::help::attach(&mut window, crate::help::topic::SIGNING_ON);
    window.set_label(&format!("Change Password - {system}"));
    window.set_color(colors::FACE);
    window.make_modal(true);

    let mut y = m;
    controls::label(m, y, lw, fh, "System:");
    let mut name = controls::label(m + lw, y, W - 2 * m - lw, fh, system);
    name.set_label_font(theme::UI_FONT_BOLD);
    y += fh + 8;
    controls::label(m, y, lw, fh, "User ID:");
    let mut user_field = controls::field(m + lw, y, fw, fh);
    user_field.set_maximum_size(128);
    user_field.set_value(user);
    if fixed {
        user_field.set_readonly(true);
        user_field.set_color(colors::FACE);
    }
    let mut row = |label: &str| {
        y += fh + 7;
        controls::label(m, y, lw, fh, label);
        let mut f = controls::secret_field(m + lw, y, fw, fh);
        f.set_maximum_size(128);
        f
    };
    let old = row("Current password:");
    let new = row("New password:");
    let confirm = row("Confirm new password:");
    y += fh + 12;
    let status = StatusLine::new(m, y, W - 2 * m, bh);

    let mut ok = controls::default_button(W - m - 2 * bw - 6, H - m - bh, bw, bh, "OK");
    ok.emit_to(tx, true);
    let mut cancel = controls::button(W - m - bw, H - m - bh, bw, bh, "Cancel");
    cancel.emit_to(tx, false);
    window.end();
    window.set_callback(move |_| tx.send(false));
    (
        ChangePasswordDialog {
            window,
            user: user_field,
            old,
            new,
            confirm,
            status,
        },
        rx,
    )
}

/// Changes a password on `system`. Signed on, it is the session's own
/// profile's; otherwise `user`'s, signing on as well. `old` fills in the
/// current password (after an expired password at sign-on). True once
/// changed.
pub fn run(
    parent: Option<&dyn WindowExt>,
    state: &Shared,
    system: &str,
    user: Option<&str>,
    old: Option<Secret>,
) -> bool {
    let Some(sys) = state.borrow().systems.get(system).cloned() else {
        msgdetails::show_message(
            parent,
            &qsa_msg::message(id::SYSTEM_NOT_CONFIGURED, &[system]),
        );
        return false;
    };
    let profile = qap::signed_on(state, &sys.name);
    let user = profile
        .clone()
        .or(user.map(str::to_owned))
        .unwrap_or_else(|| sys.default_user.clone());
    let (mut d, rx) = build(&sys.name, &user, profile.is_some());
    if let Some(old) = &old {
        d.old.set_value(old.expose());
    }
    drop(old);
    center_on(&mut d.window, parent);
    show_placed(&mut d.window);
    let _ = if d.user.value().is_empty() {
        d.user.take_focus()
    } else if d.old.value().is_empty() {
        d.old.take_focus()
    } else {
        d.new.take_focus()
    };
    while d.window.shown() {
        app::wait();
        match rx.recv() {
            Some(false) => d.window.hide(),
            Some(true) if attempt(&mut d, state, sys.clone()) => {
                d.window.hide();
                return true;
            }
            _ => {}
        }
    }
    for f in [&mut d.old, &mut d.new, &mut d.confirm] {
        f.set_value("");
    }
    false
}

/// One press of OK; true when changed.
fn attempt(d: &mut ChangePasswordDialog, state: &Shared, mut sys: System) -> bool {
    let user = d.user.value().trim().to_owned();
    if user.is_empty() {
        d.status.set("User ID is required.", Shown::Error);
        return false;
    }
    let old = Secret::new(d.old.value());
    let new = Secret::new(d.new.value());
    let confirm = Secret::new(d.confirm.value());
    d.new.set_value("");
    d.confirm.set_value("");
    if new != confirm {
        let m = qsa_msg::message(id::PASSWORDS_DIFFER, &[]);
        d.status.set(&m.to_string(), Shown::Error);
        let _ = d.new.take_focus();
        return false;
    }
    drop(confirm);
    d.old.set_value("");
    loop {
        let h = qap::client(state, &sys);
        let r = qap::busy(|| qap::lock(&h).change_password(&user, &old, &new).map(|_| ()));
        let e = match r {
            Ok(()) => {
                let m = qsa_msg::message(id::PASSWORD_CHANGED, &[&user, &sys.name]);
                crate::log!("{m}");
                d.status.set(&m.to_string(), Shown::Neutral);
                return true;
            }
            // A signed-on session stays signed on whatever the answer.
            Err(e) => e,
        };
        let parent = Some(&d.window as &dyn WindowExt);
        match e {
            Error::Connect(ConnectError::Tls(TlsError::Untrusted {
                reason,
                certificate,
                info,
            })) => {
                let status = &mut d.status;
                if crate::trust::ask_and_trust(
                    parent,
                    state,
                    &sys.name,
                    &reason,
                    &info,
                    &certificate,
                    |m| status.set(&m.to_string(), Shown::Neutral),
                ) {
                    if let Some(s) = state.borrow().systems.get(&sys.name) {
                        sys = s.clone();
                    }
                    continue;
                }
                let m = qsa_msg::message(id::CERT_UNTRUSTED, &[&sys.name, &reason]);
                d.status.set(&m.to_string(), Shown::Error);
                return false;
            }
            e => {
                let x = qap::explain(&sys, &e);
                d.status.set(&x.line(), Shown::Error);
                qap::show(parent, &x);
                return false;
            }
        }
    }
}
