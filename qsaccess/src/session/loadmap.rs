// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! Load Keyboard Map: where a `.qkm` file opened on its own goes. It is
//! copied into an open session (kept when that session is saved), or made
//! the map new sessions start from.

use qsa_ui::chan::{self, Emit};
use std::path::Path;
use std::rc::Rc;

use fltk::app;
use fltk::prelude::*;
use fltk::window::Window;
use qsa_config::qkm::KeymapFile;
use qsa_config::qsd::KeymapOverrides;
use qsa_ui::listtable::{Cell, ListTable};
use qsa_ui::propdialog::center_on;
use qsa_ui::theme::{self, colors};
use qsa_ui::{controls, msgdetails};

use super::window::SessionWindow;
use crate::app::Shared;

pub const W: i32 = 420;
pub const H: i32 = 300;

/// Where the map goes.
#[derive(Clone)]
pub enum Target {
    Session(Rc<SessionWindow>),
    NewSessions,
}

/// The dialog's widgets.
pub struct LoadMapDialog {
    pub window: Window,
    pub list: ListTable,
}

/// The rows: each open session, then new sessions.
pub fn rows(sessions: &[(String, String)]) -> Vec<Vec<Cell>> {
    let mut out: Vec<Vec<Cell>> = sessions
        .iter()
        .map(|(t, s)| vec![Cell::from(t.as_str()), Cell::from(s.as_str())])
        .collect();
    out.push(vec![
        Cell::from("New sessions"),
        Cell::from("(the map new sessions start with)"),
    ]);
    out
}

fn build_with(file: &Path, sessions: &[(String, String)]) -> (LoadMapDialog, chan::Receiver<bool>) {
    let (tx, rx) = chan::channel::<bool>();
    let m = 10;
    let bh = theme::BUTTON_H;
    let bw = theme::BUTTON_W;
    let mut window = Window::new(0, 0, W, H, None);
    window.set_label("Load Keyboard Map");
    window.set_color(colors::FACE);
    window.make_modal(true);
    let name = file.file_name().map_or_else(
        || file.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    );
    let mut l = controls::label(
        m,
        m,
        W - 2 * m,
        34,
        &format!("Copy the keyboard map in {name} into:"),
    );
    l.set_align(fltk::enums::Align::Left | fltk::enums::Align::Inside | fltk::enums::Align::Wrap);
    let mut list = ListTable::new(
        m,
        m + 40,
        W - 2 * m,
        H - 3 * m - bh - 40,
        &[("Session", 170), ("System", W - 2 * m - 170 - 20)],
    );
    let mut ok = controls::default_button(W - 2 * m - 2 * bw + m / 2, H - m - bh, bw, bh, "OK");
    ok.emit_to(tx, true);
    let mut cancel = controls::button(W - m - bw, H - m - bh, bw, bh, "Cancel");
    cancel.emit_to(tx, false);
    window.end();
    window.set_callback(move |_| tx.send(false));
    list.set_rows(rows(sessions));
    list.select(Some(0));
    list.on_activate(move |sel| {
        if sel.is_some() {
            tx.send(true);
        }
    });
    (LoadMapDialog { window, list }, rx)
}

/// Builds the dialog without showing it (for screenshots).
pub fn build(file: &Path, sessions: &[(String, String)]) -> LoadMapDialog {
    build_with(file, sessions).0
}

/// Reads `path` and asks where its map goes, then puts it there.
pub fn run(state: &Shared, path: &Path, parent: Option<&dyn WindowExt>) {
    let file = match KeymapFile::load(path) {
        Ok(f) => f,
        Err(e) => {
            let m = e.message(path);
            crate::log!("{m}");
            msgdetails::show_message(parent, &m);
            return;
        }
    };
    let open = super::windows();
    let rows_in: Vec<(String, String)> =
        open.iter().map(|s| (s.title(), s.system_name())).collect();
    let (mut d, rx) = build_with(path, &rows_in);
    center_on(&mut d.window, parent);
    d.window.show();
    let mut target = None;
    while d.window.shown() {
        app::wait();
        match rx.recv() {
            Some(true) => {
                if let Some(i) = d.list.selected() {
                    target = Some(match open.get(i) {
                        Some(s) => Target::Session(s.clone()),
                        None => Target::NewSessions,
                    });
                    d.window.hide();
                }
            }
            Some(false) => d.window.hide(),
            None => {}
        }
    }
    match target {
        Some(Target::Session(s)) => {
            s.load_keymap(file.map, path);
            s.window.clone().show();
        }
        Some(Target::NewSessions) => {
            let mut st = state.borrow_mut();
            st.settings.new_session_keymap = Some(KeymapOverrides::from_map(&file.map));
            if let Err(e) = st.save_settings() {
                crate::log!("settings not saved: {e}");
            }
            drop(st);
            let m = qsa_msg::message(qsa_msg::id::KEYMAP_LOADED, &[&path.display().to_string()]);
            crate::log!("{m}");
            msgdetails::show_message(parent, &m);
        }
        None => {}
    }
}
