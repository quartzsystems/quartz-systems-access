// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! Macros for 5250 display sessions.
//!
//! - [`player`]: running a macro, step by step.
//! - [`recorder`]: recording one from what is typed.
//! - [`editor`]: the Macro editor.
//!
//! A `.qsm` file opened on its own (a double-click) asks which open session
//! to run it in, or opens it in the Macro editor.

pub mod editor;
pub mod player;
pub mod recorder;

use std::path::Path;

use fltk::app;
use fltk::prelude::*;
use fltk::window::Window;
use qsa_config::qsm::MacroFile;
use qsa_ui::controls::{self, Switch};
use qsa_ui::listtable::{Cell, ListTable};
use qsa_ui::propdialog::center_on;
use qsa_ui::theme::{self, colors};

use crate::app::Shared;

pub const RUN_W: i32 = 420;
pub const RUN_H: i32 = 300;

/// The Run Macro dialog's widgets.
pub struct RunDialog {
    pub window: Window,
    pub list: ListTable,
}

/// Builds the Run Macro dialog: `sessions` are (title, system).
pub fn build_run(name: &str, sessions: &[(String, String)]) -> (RunDialog, app::Receiver<u8>) {
    let (tx, rx) = app::channel::<u8>();
    let m = 10;
    let bh = theme::BUTTON_H;
    let bw = theme::BUTTON_W;
    let mut window = Window::new(0, 0, RUN_W, RUN_H, None);
    window.set_label("Run Macro");
    window.set_color(colors::FACE);
    window.make_modal(true);
    let text = if sessions.is_empty() {
        format!("No session is open to run {name} in. Open a session first, or edit the macro.")
    } else {
        format!("Run {name} in:")
    };
    let mut l = controls::label(m, m, RUN_W - 2 * m, 34, &text);
    l.set_align(fltk::enums::Align::Left | fltk::enums::Align::Inside | fltk::enums::Align::Wrap);
    let mut list = ListTable::new(
        m,
        m + 40,
        RUN_W - 2 * m,
        RUN_H - 3 * m - bh - 40,
        &[("Session", 170), ("System", RUN_W - 2 * m - 170 - 20)],
    );
    let mut edit = controls::button(m, RUN_H - m - bh, bw, bh, "Edit...");
    edit.emit(tx, 2);
    let mut run = controls::default_button(
        RUN_W - 2 * m - 2 * bw + m / 2,
        RUN_H - m - bh,
        bw,
        bh,
        "Run",
    );
    run.emit(tx, 1);
    let mut cancel = controls::button(RUN_W - m - bw, RUN_H - m - bh, bw, bh, "Cancel");
    cancel.emit(tx, 0);
    window.end();
    window.set_callback(move |_| tx.send(0));
    list.set_rows(
        sessions
            .iter()
            .map(|(t, s)| vec![Cell::from(t.as_str()), Cell::from(s.as_str())])
            .collect(),
    );
    list.select(if sessions.is_empty() { None } else { Some(0) });
    Switch::for_button(&mut run).set_enabled(!sessions.is_empty());
    list.on_activate(move |sel| {
        if sel.is_some() {
            tx.send(1);
        }
    });
    (RunDialog { window, list }, rx)
}

/// A `.qsm` file opened on its own: asks which open session to run it in,
/// or opens it in the Macro editor.
pub fn open_file(_state: &Shared, path: &Path, parent: Option<&dyn WindowExt>) {
    let m = match MacroFile::load(path) {
        Ok(m) => m,
        Err(e) => {
            let msg = e.message(path);
            crate::log!("{msg}");
            qsa_ui::msgdetails::show_message(parent, &msg);
            return;
        }
    };
    let open = crate::session::windows();
    let rows: Vec<(String, String)> = open.iter().map(|s| (s.title(), s.system_name())).collect();
    let (mut d, rx) = build_run(&m.display_name(Some(path)), &rows);
    center_on(&mut d.window, parent);
    d.window.show();
    while d.window.shown() {
        app::wait();
        match rx.recv() {
            Some(1) => {
                if let Some(s) = d.list.selected().and_then(|i| open.get(i)) {
                    d.window.hide();
                    s.window.clone().show();
                    s.play(&m, path);
                }
            }
            Some(2) => {
                d.window.hide();
                editor::open(path, parent);
            }
            Some(_) => d.window.hide(),
            None => {}
        }
    }
}
