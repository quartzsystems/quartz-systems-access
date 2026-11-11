// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! The Sign On dialog: a user ID and password for a system's QAP server.
//! The system's checks are the 5250 sign-on display's, and so are its
//! messages, which are shown as the system sent them. An expired password
//! leads to Change Password. The password is never saved, and the field is
//! cleared as soon as it is read.

use fltk::app;
use fltk::input::{Input, SecretInput};
use fltk::prelude::*;
use fltk::window::Window;
use qap_client::{Error, Secret, Status};
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
pub const W: i32 = 380;
/// Dialog height.
pub const H: i32 = 168;

/// The dialog's widgets.
pub struct SignOnDialog {
    pub window: Window,
    pub user: Input,
    pub password: SecretInput,
    pub status: StatusLine,
}

/// Builds the dialog for `system`, the user ID filled in with `user`. It is
/// not shown. `true` arrives for Sign On, `false` for Cancel.
pub fn build(system: &str, user: &str) -> (SignOnDialog, chan::Receiver<bool>) {
    let (tx, rx) = chan::channel::<bool>();
    let m = 12;
    let lw = 76;
    let fh = theme::FIELD_H;
    let (bw, bh) = (theme::BUTTON_W, theme::BUTTON_H);
    let mut window = Window::new(0, 0, W, H, None);
    crate::help::attach(&mut window, crate::help::topic::SIGNING_ON);
    window.set_label(&format!("Sign On - {system}"));
    window.set_color(colors::FACE);
    window.make_modal(true);

    let mut y = m;
    controls::label(m, y, lw, fh, "System:");
    let mut name = controls::label(m + lw, y, W - 2 * m - lw, fh, system);
    name.set_label_font(theme::UI_FONT_BOLD);
    y += fh + 8;
    controls::label(m, y, lw, fh, "User ID:");
    let mut user_field = controls::field(m + lw, y, 180, fh);
    user_field.set_maximum_size(128);
    user_field.set_value(user);
    y += fh + 7;
    controls::label(m, y, lw, fh, "Password:");
    let mut password = controls::secret_field(m + lw, y, 180, fh);
    password.set_maximum_size(128);
    y += fh + 12;
    let status = StatusLine::new(m, y, W - 2 * m, bh);

    let mut ok = controls::default_button(W - m - 2 * bw - 6, H - m - bh, bw, bh, "Sign On");
    ok.emit_to(tx, true);
    let mut cancel = controls::button(W - m - bw, H - m - bh, bw, bh, "Cancel");
    cancel.emit_to(tx, false);
    window.end();
    window.set_callback(move |_| tx.send(false));
    (
        SignOnDialog {
            window,
            user: user_field,
            password,
            status,
        },
        rx,
    )
}

/// Asks for a user ID and password and signs on to `system`. True once
/// signed on (here or through Change Password).
pub fn run(parent: Option<&dyn WindowExt>, state: &Shared, system: &str) -> bool {
    let Some(sys) = state.borrow().systems.get(system).cloned() else {
        msgdetails::show_message(
            parent,
            &qsa_msg::message(id::SYSTEM_NOT_CONFIGURED, &[system]),
        );
        return false;
    };
    let (mut d, rx) = build(&sys.name, &sys.default_user);
    center_on(&mut d.window, parent);
    show_placed(&mut d.window);
    let _ = if d.user.value().is_empty() {
        d.user.take_focus()
    } else {
        d.password.take_focus()
    };
    while d.window.shown() {
        app::wait();
        match rx.recv() {
            Some(false) => d.window.hide(),
            Some(true) => {
                if attempt(&mut d, state, sys.clone()) {
                    d.window.hide();
                    // A resume after this one is news; the sign-on is not.
                    state.borrow_mut().qap.resumed(&sys.name);
                    crate::qap::warn_expiry(parent, state, &sys.name);
                    return true;
                }
                let _ = d.password.take_focus();
            }
            None => {}
        }
    }
    false
}

/// One press of Sign On; true when signed on.
fn attempt(d: &mut SignOnDialog, state: &Shared, mut sys: System) -> bool {
    let user = d.user.value().trim().to_owned();
    if user.is_empty() {
        d.status.set("User ID is required.", Shown::Error);
        return false;
    }
    let password = Secret::new(d.password.value());
    d.password.set_value("");
    let port = sys.ports.qap.to_string();
    let m = qsa_msg::message(id::CONNECTING, &[&sys.name, &port]);
    d.status.set(&m.to_string(), Shown::Neutral);
    loop {
        let h = qap::client(state, &sys);
        let r = qap::busy(|| {
            qap::lock(&h)
                .sign_on(&user, &password)
                .map(|p| p.profile.clone())
        });
        let e = match r {
            Ok(profile) => {
                let m = qsa_msg::message(id::SIGNON_OK, &[&sys.name, &profile]);
                crate::log!("{m}");
                d.status.set(&m.to_string(), Shown::Neutral);
                return true;
            }
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
                    // Connect again, trusting the certificate now.
                    if let Some(s) = state.borrow().systems.get(&sys.name) {
                        sys = s.clone();
                    }
                    continue;
                }
                let m = qsa_msg::message(id::CERT_UNTRUSTED, &[&sys.name, &reason]);
                d.status.set(&m.to_string(), Shown::Error);
                return false;
            }
            e if e.status() == Some(Status::PasswordExpired) => {
                let m = qsa_msg::message(id::PASSWORD_EXPIRED, &[&user, &sys.name]);
                crate::log!("{m}");
                d.status.set(&m.to_string(), Shown::Error);
                let buttons = ["Change Password...", "Cancel"];
                let choice = match e.server_message() {
                    Some(sm) => {
                        msgdetails::ask_system_message(parent, Some(&m), &qap::shown(sm), &buttons)
                    }
                    None => msgdetails::ask_message(parent, &m, &buttons),
                };
                return choice == 0
                    && crate::chgpwd::run(parent, state, &sys.name, Some(&user), Some(password));
            }
            e => {
                let x = qap::explain(&sys, &e);
                crate::log!("{}", x.message);
                d.status.set(&x.line(), Shown::Error);
                qap::show(parent, &x);
                return false;
            }
        }
    }
}
