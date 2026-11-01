// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! 5250 printer sessions.
//!
//! - [`start`]: the Start 5250 Printer dialog; [`settings`]: a session's
//!   Settings dialog.
//! - [`window`]: the printer window; [`worker`]: its network thread.
//! - [`spool`]: received jobs, spooled to disk and rendered.
//!
//! Every open printer window is kept here; the main loop calls [`pump`]
//! after each wait.

pub mod settings;
pub mod spool;
pub mod start;
pub mod window;
pub mod worker;

use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;

use fltk::prelude::*;
use qsa_config::FileError;
use qsa_config::qsp::PrinterFile;
use qsa_ui::msgdetails;

use crate::app::Shared;
use window::PrinterWindow;

thread_local! {
    static PRINTERS: RefCell<Vec<Rc<PrinterWindow>>> = const { RefCell::new(Vec::new()) };
}

/// Asks for the options, then opens a printer window and connects.
pub fn start(parent: &fltk::window::Window, state: &Shared, system: Option<&str>) {
    let Some(doc) = start::run(Some(parent), state, system) else {
        return;
    };
    let p = PrinterWindow::build(state, doc, None);
    show(&p, Some(parent));
    p.connect();
    PRINTERS.with(|v| v.borrow_mut().push(p));
}

/// Places and shows a new window, clear of other printer windows.
fn show(p: &PrinterWindow, parent: Option<&dyn WindowExt>) {
    p.place(parent);
    let others = windows();
    let mut w = p.window.clone();
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
    qsa_ui::propdialog::show_placed(&mut w);
}

/// Opens a printer window for the `.qsp` file at `path`, and connects if
/// the file says to. Problems are reported here; false if no window
/// opened.
pub fn open_file(state: &Shared, path: &Path, parent: Option<&dyn WindowExt>) -> bool {
    let path = std::path::absolute(path).unwrap_or_else(|_| path.to_owned());
    let mut doc = match PrinterFile::load(&path) {
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
    if !configured && !crate::session::create_system(state, &mut doc.system, parent) {
        return false;
    }
    let p = PrinterWindow::build(state, doc, Some(path.clone()));
    show(&p, parent);
    if p.auto_connect() {
        p.connect();
    }
    {
        let mut st = state.borrow_mut();
        st.settings.mark_used(&path, crate::localtime::now());
        st.note_recent(&path);
    }
    PRINTERS.with(|v| v.borrow_mut().push(p));
    true
}

/// The open printer windows, in the order they were opened.
pub fn windows() -> Vec<Rc<PrinterWindow>> {
    PRINTERS.with(|v| {
        v.borrow()
            .iter()
            .filter(|p| p.window.shown())
            .cloned()
            .collect()
    })
}

/// Adds a window built elsewhere (tests) to the ones [`pump`] serves.
pub fn adopt(p: Rc<PrinterWindow>) {
    PRINTERS.with(|v| v.borrow_mut().push(p));
}

/// Gives every printer window its waiting work; forgets closed ones.
pub fn pump() {
    let all: Vec<Rc<PrinterWindow>> = PRINTERS.with(|v| v.borrow().clone());
    let mut closed = Vec::new();
    for p in &all {
        if !p.pump() {
            closed.push(Rc::as_ptr(p));
        }
    }
    if !closed.is_empty() {
        PRINTERS.with(|v| v.borrow_mut().retain(|p| !closed.contains(&Rc::as_ptr(p))));
    }
}

/// Disconnects and closes every printer window.
pub fn close_all() {
    for p in windows() {
        p.disconnect();
        p.window.clone().hide();
    }
}

/// Whether any printer window is open.
pub fn any_open() -> bool {
    PRINTERS.with(|v| v.borrow().iter().any(|p| p.window.shown()))
}
