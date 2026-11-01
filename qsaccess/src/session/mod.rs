// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! 5250 display sessions.
//!
//! - [`start`]: the Start 5250 Display dialog.
//! - [`window`]: the session window; [`worker`]: its network thread.
//! - [`keys`]: FLTK key events to 5250 keys through the keyboard map;
//!   [`keymapdlg`]: the Keyboard Map dialog.
//! - [`cert`]: the untrusted-certificate dialog; [`pdf`]: Print Screen.
//!
//! Every open session window is kept here; the main loop calls [`pump`]
//! after each wait, which hands each window its commands and network
//! events.

pub mod cert;
pub mod keymapdlg;
pub mod keys;
pub mod pdf;
pub mod start;
pub mod window;
pub mod worker;

use std::cell::RefCell;
use std::rc::Rc;

use fltk::prelude::*;

use crate::app::Shared;
use window::SessionWindow;

thread_local! {
    static SESSIONS: RefCell<Vec<Rc<SessionWindow>>> = const { RefCell::new(Vec::new()) };
}

/// Asks for the options, then opens a session window and connects.
pub fn start(parent: &fltk::window::Window, state: &Shared, system: Option<&str>) {
    let Some(opts) = start::run(Some(parent), state, system) else {
        return;
    };
    let s = SessionWindow::build(state, opts, None);
    let mut w = s.window.clone();
    qsa_ui::propdialog::center_on(&mut w, Some(parent));
    w.show();
    s.connect(None);
    SESSIONS.with(|v| v.borrow_mut().push(s));
}

/// Gives every session window its waiting work; forgets closed ones.
pub fn pump() {
    let all: Vec<Rc<SessionWindow>> = SESSIONS.with(|v| v.borrow().clone());
    let mut closed = Vec::new();
    for s in &all {
        if !s.pump() {
            closed.push(Rc::as_ptr(s));
        }
    }
    if !closed.is_empty() {
        SESSIONS.with(|v| v.borrow_mut().retain(|s| !closed.contains(&Rc::as_ptr(s))));
    }
}

/// Whether any session window is open.
pub fn any_open() -> bool {
    SESSIONS.with(|v| v.borrow().iter().any(|s| s.window.shown()))
}
