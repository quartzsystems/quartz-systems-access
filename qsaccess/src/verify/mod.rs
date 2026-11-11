// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! The Verify Connection dialog: each check of [`checks`] with OK, Failed
//! or Skipped and a detail, filled in as the checks run on their own thread,
//! then `QAD4050`, `QAD4051` or `QAD4052`. A certificate that is not trusted
//! goes through the untrusted-certificate flow, and the checks run again if
//! it is trusted. The test sign-on is optional and real: the dialog says
//! that a failed one counts toward `QMAXSIGN` and is audited.

pub mod checks;

use std::sync::mpsc;

use fltk::app;
use fltk::enums::Align;
use fltk::frame::Frame;
use fltk::input::{Input, SecretInput};
use fltk::prelude::*;
use fltk::window::Window;
use qap_client::Secret;
use qsa_config::System;
use qsa_msg::Kind;
use qsa_ui::chan::{self, Emit};
use qsa_ui::controls::{self, Switch};
use qsa_ui::groupbox::{self, CONTENT_SIDE, CONTENT_TOP};
use qsa_ui::listtable::{Cell, ListTable};
use qsa_ui::msgdetails;
use qsa_ui::propdialog::{center_on, show_placed};
use qsa_ui::statusbar::{Status, StatusLine};
use qsa_ui::theme::{self, colors};

use crate::app::Shared;
use checks::{Check, Outcome, Plan};

/// Dialog width.
pub const W: i32 = 640;
/// Dialog height.
pub const H: i32 = 304;

/// Columns of the results: title and width.
pub const COLUMNS: [(&str, i32); 3] = [("Check", 190), ("Result", 64), ("Detail", 362)];

/// What the warning beside the test sign-on says.
pub const SIGN_ON_WARNING: &str = "The test sign-on is a real sign-on, signed off at once. \
    A failed one counts toward the system's limit of sign-on attempts (QMAXSIGN), which \
    can disable the profile, and is audited like any sign-on.";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Request {
    Verify,
    Close,
    Details,
}

/// The dialog's widgets.
pub struct VerifyDialog {
    pub window: Window,
    pub list: ListTable,
    pub user: Input,
    pub password: SecretInput,
    pub status: StatusLine,
    verify: Switch,
}

/// The rows: every check, with its result where there is one.
pub fn rows(system: &System, results: &[(Check, Outcome)]) -> Vec<Vec<Cell>> {
    Check::ALL
        .iter()
        .map(|c| {
            let (result, detail) = match results.iter().find(|(r, _)| r == c) {
                Some((_, o)) => {
                    let (text, status) = match o {
                        Outcome::Ok(_) => ("OK", Status::Active),
                        Outcome::Failed { .. } => ("Failed", Status::Error),
                        Outcome::Skipped(_) => ("Skipped", Status::Neutral),
                    };
                    (
                        Cell {
                            text: text.into(),
                            status,
                        },
                        Cell::from(o.detail()),
                    )
                }
                None => (Cell::from(""), Cell::from("")),
            };
            vec![Cell::from(c.label(system)), result, detail]
        })
        .collect()
}

/// Builds the dialog for `system`. It is not shown.
fn build_with(system: &System) -> (VerifyDialog, chan::Receiver<Request>) {
    let (tx, rx) = chan::channel::<Request>();
    let m = 10;
    let fh = theme::FIELD_H;
    let (bw, bh) = (theme::BUTTON_W, theme::BUTTON_H);
    let mut window = Window::new(0, 0, W, H, None);
    crate::help::attach(&mut window, crate::help::topic::VERIFY_CONNECTION);
    window.set_label(&format!("Verify Connection - {}", system.name));
    window.set_color(colors::FACE);
    window.make_modal(true);

    let lh = 22 + 7 * 18 + 6;
    let mut list = ListTable::new(m, m, W - 2 * m, lh, &COLUMNS);
    list.set_rows(rows(system, &[]));
    list.on_activate(move |sel| {
        if sel.is_some() {
            tx.send(Request::Details);
        }
    });

    let gy = m + lh + 10;
    let gh = CONTENT_TOP + fh + 8 + 34 + 6;
    let g = groupbox::group_box(m, gy, W - 2 * m, gh, "Test sign-on (optional)");
    let x = m + CONTENT_SIDE;
    let y = gy + CONTENT_TOP;
    controls::label(x, y, 56, fh, "User ID:");
    let mut user = controls::field(x + 56, y, 150, fh);
    user.set_maximum_size(128);
    user.set_value(&system.default_user);
    controls::label(x + 230, y, 64, fh, "Password:");
    let mut password = controls::secret_field(x + 294, y, 150, fh);
    password.set_maximum_size(128);
    let mut warn = Frame::new(x, y + fh + 8, W - 2 * m - 2 * CONTENT_SIDE, 34, None);
    warn.set_label(SIGN_ON_WARNING);
    warn.set_align(Align::Left | Align::Top | Align::Inside | Align::Wrap);
    warn.set_label_size(theme::UI_SIZE);
    g.end();

    let status = StatusLine::new(m, H - m - bh, W - 3 * m - 2 * bw - 6, bh);
    let mut verify = controls::default_button(W - m - 2 * bw - 6, H - m - bh, bw, bh, "Verify");
    verify.emit_to(tx, Request::Verify);
    let mut close = controls::button(W - m - bw, H - m - bh, bw, bh, "Close");
    close.emit_to(tx, Request::Close);
    window.end();
    window.set_callback(move |_| tx.send(Request::Close));
    let verify = Switch::for_button(&mut verify);
    (
        VerifyDialog {
            window,
            list,
            user,
            password,
            status,
            verify,
        },
        rx,
    )
}

/// Builds the dialog for `system` showing `results` (for screenshots).
pub fn build(system: &System, results: &[(Check, Outcome)]) -> VerifyDialog {
    let (mut d, _) = build_with(system);
    d.list.set_rows(rows(system, results));
    if results.len() == Check::ALL.len() {
        let m = checks::summary(&system.name, results);
        d.status.set(&m.to_string(), shown(&m));
    }
    d
}

fn shown(m: &qsa_msg::Message) -> Status {
    if m.kind() == Kind::Error {
        Status::Error
    } else {
        Status::Neutral
    }
}

/// Opens Verify Connection for `system` and runs it until closed.
pub fn open(parent: Option<&dyn WindowExt>, state: &Shared, system: &str) {
    let Some(mut sys) = state.borrow().systems.get(system).cloned() else {
        return;
    };
    let (mut d, rx) = build_with(&sys);
    center_on(&mut d.window, parent);
    show_placed(&mut d.window);
    let mut results: Vec<(Check, Outcome)> = Vec::new();
    let mut running: Option<mpsc::Receiver<(Check, Outcome)>> = None;
    let mut again = false;
    while d.window.shown() {
        if again {
            again = false;
            running = Some(start(&mut d, &sys, &mut results));
        }
        app::wait();
        if let Some(r) = &running {
            while let Ok(res) = r.try_recv() {
                results.push(res);
            }
            d.list.set_rows(rows(&sys, &results));
            if results.len() == Check::ALL.len() {
                running = None;
                d.verify.set_enabled(true);
                let m = checks::summary(&sys.name, &results);
                crate::log!("{m}");
                d.status.set(&m.to_string(), shown(&m));
                if trust(&d, state, &sys.name, &results) {
                    // Run again with the certificate trusted.
                    if let Some(s) = state.borrow().systems.get(&sys.name) {
                        sys = s.clone();
                    }
                    again = true;
                }
            }
        }
        match rx.recv() {
            Some(Request::Close) => d.window.hide(),
            Some(Request::Verify) if running.is_none() => {
                running = Some(start(&mut d, &sys, &mut results));
            }
            Some(Request::Details) => details(&d, &results),
            _ => {}
        }
    }
    d.password.set_value("");
}

/// Starts the checks on their own thread.
fn start(
    d: &mut VerifyDialog,
    sys: &System,
    results: &mut Vec<(Check, Outcome)>,
) -> mpsc::Receiver<(Check, Outcome)> {
    results.clear();
    d.list.set_rows(rows(sys, results));
    d.verify.set_enabled(false);
    d.status.set(
        &format!("Verifying the connection to {}...", sys.name),
        Status::Neutral,
    );
    let user = d.user.value().trim().to_owned();
    let sign_on = (!user.is_empty()).then(|| (user, Secret::new(d.password.value())));
    d.password.set_value("");
    let plan = Plan::for_system(sys, sign_on);
    let (tx, rx) = mpsc::channel();
    let spawned = std::thread::Builder::new()
        .name(format!("verify {}", sys.name))
        .spawn(move || {
            checks::run(&plan, |c, o| {
                if tx.send((c, o)).is_ok() {
                    app::awake();
                }
            });
        });
    if let Err(e) = spawned {
        crate::log!("verify: {e}");
    }
    rx
}

/// Offers to trust each certificate that was not trusted; true when one
/// was.
fn trust(d: &VerifyDialog, state: &Shared, system: &str, results: &[(Check, Outcome)]) -> bool {
    let mut trusted = false;
    let mut seen: Vec<&[u8]> = Vec::new();
    for (_, o) in results {
        let Outcome::Failed {
            untrusted: Some(u), ..
        } = o
        else {
            continue;
        };
        if seen.contains(&u.certificate.as_slice()) {
            continue;
        }
        seen.push(&u.certificate);
        let mut status = d.status.clone();
        trusted |= crate::trust::ask_and_trust(
            Some(&d.window),
            state,
            system,
            &u.reason,
            &u.info,
            &u.certificate,
            |m| status.set(&m.to_string(), Status::Neutral),
        );
    }
    trusted
}

/// The selected row's messages: the system's as sent, or this program's.
fn details(d: &VerifyDialog, results: &[(Check, Outcome)]) {
    let Some(check) = d.list.selected().and_then(|i| Check::ALL.get(i)) else {
        return;
    };
    let parent = Some(&d.window as &dyn WindowExt);
    if let Some((
        _,
        Outcome::Failed {
            message, server, ..
        },
    )) = results.iter().find(|(c, _)| c == check)
    {
        match server {
            Some(sm) => {
                msgdetails::show_system_message(parent, Some(message), &crate::qap::shown(sm));
            }
            None => msgdetails::show_message(parent, message),
        }
    }
}
