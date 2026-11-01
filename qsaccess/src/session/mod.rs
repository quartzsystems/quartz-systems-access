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
//! - [`props`]: Session Properties; [`loadmap`]: loading a `.qkm` into an
//!   open session.
//!
//! Every open session window is kept here; the main loop calls [`pump`]
//! after each wait, which hands each window its commands and network
//! events.

pub mod cert;
pub mod keymapdlg;
pub mod keys;
pub mod loadmap;
pub mod pdf;
pub mod props;
pub mod start;
pub mod window;
pub mod worker;

use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;

use fltk::prelude::*;
use qsa_config::FileError;
use qsa_config::qsd::SessionFile;
use qsa_msg::id;
use qsa_ui::{MessageBox, msgdetails};

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
    s.place(Some(parent));
    cascade(&s);
    s.window.clone().show();
    s.connect(None);
    SESSIONS.with(|v| v.borrow_mut().push(s));
}

/// Opens a session window for the session file at `path`, and connects if
/// the file says to. Every open makes a new window, so the same file can be
/// open more than once. Problems are reported to the user here; false if
/// no window was opened.
pub fn open_file(state: &Shared, path: &Path, parent: Option<&dyn WindowExt>) -> bool {
    let path = std::path::absolute(path).unwrap_or_else(|_| path.to_owned());
    let mut doc = match SessionFile::load(&path) {
        Ok(d) => d,
        Err(e) => {
            let m = e.message(&path);
            crate::log!("{m}");
            if let FileError::Io(io) = &e
                && io.kind() == std::io::ErrorKind::NotFound
            {
                state.borrow_mut().forget_recent(&path);
            }
            msgdetails::show_message(parent, &m);
            return false;
        }
    };
    let configured = state.borrow().systems.contains(&doc.system.name);
    if !configured && !create_system(state, &mut doc, parent) {
        return false;
    }
    let s = SessionWindow::build_file(state, doc, Some(path.clone()), None);
    s.place(parent);
    cascade(&s);
    s.window.clone().show();
    if s.auto_connect() {
        s.connect(None);
    }
    state.borrow_mut().note_recent(&path);
    SESSIONS.with(|v| v.borrow_mut().push(s));
    true
}

/// `QAD2003`: offers to create the system a session file names, filled in
/// from the file's host, Telnet port and TLS setting. True once it exists.
fn create_system(state: &Shared, doc: &mut SessionFile, parent: Option<&dyn WindowExt>) -> bool {
    let m = qsa_msg::message(id::SYSTEM_NOT_CONFIGURED, &[&doc.system.name]);
    crate::log!("{m}");
    loop {
        let mut mb = MessageBox::new(
            crate::PRODUCT,
            &format!("{}\n\nMessage ID: {}", m.text, m.id),
        );
        if let Some(g) = msgdetails::glyph_for(&m) {
            mb = mb.glyph(g);
        }
        let choice = mb
            .buttons(
                &["Create system from session file", "Cancel", "Details..."],
                0,
                1,
            )
            .run(parent);
        match choice {
            0 => break,
            2 => msgdetails::show_details(parent, &m),
            _ => return false,
        }
    }
    let template = doc.system.to_system();
    let created = {
        let st = state.borrow();
        crate::properties::create(parent, &st.systems, &template)
    };
    let Some(sys) = created else { return false };
    let name = sys.name.clone();
    let mut st = state.borrow_mut();
    if let Err(e) = st.systems.add(sys) {
        crate::log!("system {name} not added: {e}");
        return false;
    }
    if let Err(e) = st.save_systems() {
        crate::log!("systems not saved: {e}");
    }
    crate::log!("{}", qsa_msg::message(id::SYSTEM_ADDED, &[&name]));
    doc.system.name = name;
    true
}

/// Moves a new window down and right while another session window has the
/// same corner, so windows opened from one file do not hide each other.
fn cascade(s: &SessionWindow) {
    let others = windows();
    let mut w = s.window.clone();
    for _ in 0..others.len() {
        let (x, y) = (w.x(), w.y());
        if !others
            .iter()
            .any(|o| o.window.x() == x && o.window.y() == y)
        {
            break;
        }
        w.resize(x + 24, y + 24, w.w(), w.h());
    }
}

/// The open session windows, in the order they were opened.
pub fn windows() -> Vec<Rc<SessionWindow>> {
    SESSIONS.with(|v| {
        v.borrow()
            .iter()
            .filter(|s| s.window.shown())
            .cloned()
            .collect()
    })
}

/// Adds a window built elsewhere (tests) to the ones [`pump`] serves.
pub fn adopt(s: Rc<SessionWindow>) {
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
