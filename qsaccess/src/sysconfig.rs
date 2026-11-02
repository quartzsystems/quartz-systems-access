// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! The System Configurations dialog: the list of systems, with New, Edit,
//! Delete, Verify, Import, Export and Close.

use qsa_ui::chan::{self, Emit};
use std::path::Path;

use fltk::app;
use fltk::button::Button;
use fltk::prelude::*;
use fltk::window::Window;
use qsa_config::{System, qsc};
use qsa_msg::{Message, id};
use qsa_ui::controls::{self, Switch};
use qsa_ui::listtable::{Cell, ListTable};
use qsa_ui::propdialog::center_on;
use qsa_ui::statusbar::{Status, StatusLine};
use qsa_ui::theme::{self, colors};
use qsa_ui::{Glyph, MessageBox};

use crate::app::Shared;
use crate::files;
use crate::mainwin::Ui;
use crate::properties;

/// Dialog width.
pub const W: i32 = 620;
/// Dialog height.
pub const H: i32 = 360;

/// Columns of the list: title and width.
pub const COLUMNS: [(&str, i32); 4] = [
    ("Name", 170),
    ("Description", 220),
    ("TLS", 50),
    ("Default user", 54 + 46),
];

/// What the dialog's buttons ask for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Request {
    New,
    Edit,
    Delete,
    Verify,
    Import,
    Export,
    Close,
}

/// The dialog's widgets.
pub struct ListDialog {
    pub window: Window,
    pub list: ListTable,
    pub status: StatusLine,
    edit: Switch,
    delete: Switch,
    verify: Switch,
    export: Switch,
}

/// The rows for `systems`.
pub fn rows(systems: &[System]) -> Vec<Vec<Cell>> {
    systems
        .iter()
        .map(|s| {
            vec![
                Cell::from(s.name.as_str()),
                Cell::from(s.description.as_str()),
                Cell::from(if s.use_tls { "Yes" } else { "No" }),
                Cell::from(s.default_user.as_str()),
            ]
        })
        .collect()
}

/// Builds the dialog. It is not shown. Button presses arrive on the receiver.
fn build_with(state: &Shared) -> (ListDialog, chan::Receiver<Request>) {
    let (tx, rx) = chan::channel::<Request>();
    let m = 10;
    let bw = theme::BUTTON_W;
    let bh = theme::BUTTON_H;
    let mut window = Window::new(0, 0, W, H, None);
    window.set_label("System Configurations");
    window.set_color(colors::FACE);
    window.make_modal(true);

    let lw = W - 3 * m - bw;
    let lh = H - 2 * m - bh - 8;
    let mut list = ListTable::new(m, m, lw, lh, &COLUMNS);
    let bx = m + lw + m;
    let mut y = m;
    let button = |label: &str, r: Request, y: i32| -> Button {
        let mut b = controls::button(bx, y, bw, bh, label);
        b.emit_to(tx, r);
        b
    };
    button("New...", Request::New, y);
    y += bh + 6;
    let mut edit = button("Edit...", Request::Edit, y);
    y += bh + 6;
    let mut delete = button("Delete", Request::Delete, y);
    y += bh + 6;
    let mut verify = button("Verify...", Request::Verify, y);
    y += bh + 18;
    button("Import...", Request::Import, y);
    y += bh + 6;
    let mut export = button("Export...", Request::Export, y);

    let status = StatusLine::new(m, H - m - bh, W - 3 * m - bw, bh);
    let mut close = controls::default_button(bx, H - m - bh, bw, bh, "Close");
    close.emit_to(tx, Request::Close);
    window.end();
    window.set_callback(move |_| tx.send(Request::Close));

    let edit = Switch::for_button(&mut edit);
    let delete = Switch::for_button(&mut delete);
    let verify = Switch::for_button(&mut verify);
    let export = Switch::for_button(&mut export);
    {
        let (e, d, v) = (edit.clone(), delete.clone(), verify.clone());
        list.on_select(move |sel| {
            e.set_enabled(sel.is_some());
            d.set_enabled(sel.is_some());
            v.set_enabled(sel.is_some());
        });
    }
    list.on_activate(move |sel| {
        if sel.is_some() {
            tx.send(Request::Edit);
        }
    });

    let mut d = ListDialog {
        window,
        list,
        status,
        edit,
        delete,
        verify,
        export,
    };
    d.fill(state, None);
    (d, rx)
}

/// Builds the dialog without showing it (for screenshots).
pub fn build(state: &Shared) -> ListDialog {
    build_with(state).0
}

impl ListDialog {
    /// Refills the list, selecting `select` (by name) if given.
    fn fill(&mut self, state: &Shared, select: Option<&str>) {
        let st = state.borrow();
        self.list.set_rows(rows(st.systems.as_slice()));
        let index = select.and_then(|n| {
            st.systems
                .iter()
                .position(|s| s.name.eq_ignore_ascii_case(n))
        });
        let index = index.or(if st.systems.is_empty() { None } else { Some(0) });
        drop(st);
        self.list.select(index);
        self.edit.set_enabled(index.is_some());
        self.delete.set_enabled(index.is_some());
        self.verify.set_enabled(index.is_some());
        self.export.set_enabled(!state.borrow().systems.is_empty());
    }

    fn selected(&self, state: &Shared) -> Option<System> {
        let i = self.list.selected()?;
        state.borrow().systems.as_slice().get(i).cloned()
    }

    /// Shows a message in the dialog's status line and the main window's.
    fn report(&mut self, ui: &Ui, m: &Message) {
        let status = if m.kind() == qsa_msg::Kind::Error {
            Status::Error
        } else {
            Status::Neutral
        };
        self.status.set(&m.to_string(), status);
        ui.report(m);
    }

    /// Shows an error that has no catalogue message.
    fn error(&mut self, text: &str) {
        self.status.set(text, Status::Error);
        MessageBox::new("System Configurations", text)
            .glyph(Glyph::Cross)
            .run(Some(&self.window));
    }
}

/// Saves `systems.toml`, reporting a failure in the dialog.
fn save(d: &mut ListDialog, state: &Shared) -> bool {
    let result = state.borrow().save_systems();
    match result {
        Ok(()) => true,
        Err(e) => {
            crate::log!("systems not saved: {e}");
            d.error(&format!(
                "The system configurations could not be saved: {e}"
            ));
            false
        }
    }
}

/// Opens System Configurations over the main window and runs it until closed.
pub fn open(ui: &Ui) {
    let state = ui.state.clone();
    let (mut d, rx) = build_with(&state);
    center_on(&mut d.window, Some(&ui.window));
    d.window.show();
    while d.window.shown() {
        app::wait();
        let Some(request) = rx.recv() else { continue };
        match request {
            Request::Close => d.window.hide(),
            Request::New => new_system(&mut d, ui),
            Request::Edit => edit_system(&mut d, ui),
            Request::Delete => delete_system(&mut d, ui),
            Request::Verify => {
                if let Some(sys) = d.selected(&state) {
                    crate::verify::open(Some(&d.window), &state, &sys.name);
                }
            }
            Request::Import => import(&mut d, ui),
            Request::Export => export(&mut d, ui),
        }
    }
    ui.refresh_systems();
}

fn new_system(d: &mut ListDialog, ui: &Ui) {
    let state = &ui.state;
    let edited = {
        let st = state.borrow();
        properties::edit(Some(&d.window), &st.systems, None)
    };
    let Some(sys) = edited else { return };
    let name = sys.name.clone();
    if let Err(e) = state.borrow_mut().systems.add(sys) {
        d.error(&e.to_string());
        return;
    }
    if save(d, state) {
        d.report(ui, &qsa_msg::message(id::SYSTEM_ADDED, &[&name]));
    }
    d.fill(state, Some(&name));
    ui.refresh_systems();
}

fn edit_system(d: &mut ListDialog, ui: &Ui) {
    let state = &ui.state;
    let Some(original) = d.selected(state) else {
        return;
    };
    let edited = {
        let st = state.borrow();
        properties::edit(Some(&d.window), &st.systems, Some(&original))
    };
    let Some(sys) = edited else { return };
    let name = sys.name.clone();
    if let Err(e) = state.borrow_mut().systems.change(&original.name, sys) {
        d.error(&e.to_string());
        return;
    }
    if save(d, state) {
        d.report(ui, &qsa_msg::message(id::SYSTEM_CHANGED, &[&name]));
    }
    d.fill(state, Some(&name));
    ui.refresh_systems();
}

fn delete_system(d: &mut ListDialog, ui: &Ui) {
    let state = &ui.state;
    let Some(sys) = d.selected(state) else { return };
    let answer = MessageBox::new(
        "Delete System Configuration",
        &format!("Delete the system configuration {}?", sys.name),
    )
    .buttons(&["Delete", "Cancel"], 1, 1)
    .run(Some(&d.window));
    if answer != 0 {
        return;
    }
    state.borrow_mut().systems.remove(&sys.name);
    if save(d, state) {
        d.report(ui, &qsa_msg::message(id::SYSTEM_DELETED, &[&sys.name]));
    }
    d.fill(state, None);
    ui.refresh_systems();
}

const QSC_PATTERN: &str = "System configurations (*.qsc)\tAll Files (*)";

fn import(d: &mut ListDialog, ui: &Ui) {
    let Some(path) = files::choose_open("Import System Configurations", QSC_PATTERN) else {
        return;
    };
    let incoming = match qsc::import(&path) {
        Ok(v) => v,
        Err(e) => {
            d.error(&format!("{} could not be imported. {e}", path.display()));
            return;
        }
    };
    let window = d.window.clone();
    let report = {
        let mut st = ui.state.borrow_mut();
        qsc::merge(&mut st.systems, incoming, |existing, _| {
            MessageBox::new(
                "Import System Configurations",
                &format!(
                    "A system configuration named {} already exists.\n\n\
                     Replace it with the one being imported?",
                    existing.name
                ),
            )
            .buttons(&["Replace", "Keep"], 1, 1)
            .run(Some(&window))
                == 0
        })
    };
    let count = report.imported().to_string();
    if save(d, &ui.state) {
        d.report(ui, &qsa_msg::message(id::SYSTEMS_IMPORTED, &[&count]));
    }
    let first = report.added.first().or(report.replaced.first()).cloned();
    d.fill(&ui.state, first.as_deref());
    ui.refresh_systems();
}

fn export(d: &mut ListDialog, ui: &Ui) {
    let Some(mut path) =
        files::choose_save("Export System Configurations", QSC_PATTERN, "systems.qsc")
    else {
        return;
    };
    if path.extension().is_none() {
        path.set_extension(qsc::EXTENSION);
    }
    if path.exists() && !confirm_replace(d, &path) {
        return;
    }
    let systems = ui.state.borrow().systems.as_slice().to_vec();
    match qsc::export(&path, &systems) {
        Ok(()) => {
            let (n, p) = (systems.len().to_string(), path.display().to_string());
            d.report(ui, &qsa_msg::message(id::SYSTEMS_EXPORTED, &[&n, &p]));
        }
        Err(e) => d.error(&format!("{} could not be written: {e}", path.display())),
    }
}

fn confirm_replace(d: &ListDialog, path: &Path) -> bool {
    MessageBox::new(
        "Export System Configurations",
        &format!("{} already exists.\n\nReplace it?", path.display()),
    )
    .buttons(&["Replace", "Cancel"], 1, 1)
    .run(Some(&d.window))
        == 0
}
