// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! Printer Output's dialogs: the hold choice, Move, the delete
//! confirmation listing every file, Properties, the printer for Print, and
//! the summary of actions on several files. Each `build_*` makes the
//! window without showing it (for screenshots); each `ask_*` runs it.

use fltk::app;
use fltk::button::CheckButton;
use fltk::enums::Align;
use fltk::frame::Frame;
use fltk::menu::Choice;
use fltk::misc::InputChoice;
use fltk::prelude::*;
use fltk::window::Window;
use qap_client::proto::ops::spool::{OutputQueue, SpooledFile};
use qap_client::proto::types::ObjectName;
use qsa_ui::chan::{self, Emit};
use qsa_ui::listtable::{Cell, ListTable};
use qsa_ui::propdialog::{center_on, show_placed};
use qsa_ui::statusbar::{Status, StatusLine};
use qsa_ui::theme::{self, colors};
use qsa_ui::{Glyph, MessageBox, msgdetails};

use super::model::{self, Done, Outcome};

/// `QSYSPRT` for one file, `3 spooled files` for more.
fn what(files: &[SpooledFile]) -> String {
    match files {
        [f] => format!("spooled file {}", f.id.file),
        _ => format!("{} spooled files", files.len()),
    }
}

/// A modal window of `w` by `h` titled `title`.
fn dialog(title: &str, w: i32, h: i32) -> Window {
    let mut win = Window::new(0, 0, w, h, None);
    win.set_label(title);
    win.set_color(colors::FACE);
    win.make_modal(true);
    win
}

fn text(x: i32, y: i32, w: i32, h: i32, s: &str) -> Frame {
    let mut f = Frame::new(x, y, w, h, None);
    f.set_label(s);
    f.set_align(Align::Left | Align::Top | Align::Inside | Align::Wrap);
    f.set_label_size(theme::UI_SIZE);
    f
}

/// Shows `win` over `parent` and waits until `answer` gives something or
/// the window closes (`None`).
fn run<T>(
    mut win: Window,
    parent: Option<&dyn WindowExt>,
    mut answer: impl FnMut() -> Option<Option<T>>,
) -> Option<T> {
    center_on(&mut win, parent);
    show_placed(&mut win);
    while win.shown() {
        app::wait();
        if let Some(a) = answer() {
            win.hide();
            return a;
        }
    }
    None
}

/// The hold choice: immediately, or at the end of the page.
pub fn hold_box(files: &[SpooledFile]) -> MessageBox {
    MessageBox::new(
        "Hold",
        &format!(
            "Hold {}?\n\nA file being printed stops printing immediately, or at the end of the page it is printing.",
            what(files)
        ),
    )
    .buttons(&["Immediately", "At End of Page", "Cancel"], 1, 2)
}

/// Asks how to hold `files`: `Some(true)` immediately, `Some(false)` at
/// the end of the page.
pub fn ask_hold(parent: Option<&dyn WindowExt>, files: &[SpooledFile]) -> Option<bool> {
    match hold_box(files).run(parent) {
        0 => Some(true),
        1 => Some(false),
        _ => None,
    }
}

/// The Move dialog's widgets.
pub struct MoveDialog {
    pub window: Window,
    pub queue: InputChoice,
    pub top: CheckButton,
    pub status: StatusLine,
    rx: chan::Receiver<bool>,
}

/// Move: an output queue, picked from `queues` or typed, and whether to
/// put the files at its top. `current` is filled in.
pub fn build_move(files: &[SpooledFile], queues: &[OutputQueue], current: &str) -> MoveDialog {
    let (tx, rx) = chan::channel::<bool>();
    let (w, h, m) = (420, 150, 12);
    let (bw, bh, fh) = (theme::BUTTON_W, theme::BUTTON_H, theme::FIELD_H);
    let win = dialog("Move", w, h);
    text(
        m,
        m,
        w - 2 * m,
        18,
        &format!("Move {} to output queue:", what(files)),
    );
    let mut queue = qsa_ui::controls::editable_combo(m, m + 22, w - 2 * m, fh);
    for q in model::queue_choices(queues) {
        queue.add(&q.replace('/', "\\/"));
    }
    queue.set_value(current);
    queue.set_tooltip("LIBRARY/QUEUE; QUEUE alone is looked for in the library list.");
    let top = qsa_ui::controls::check(m, m + 22 + fh + 8, w - 2 * m, 20, "To the top of the queue");
    let status = StatusLine::new(m, h - m - bh, w - 4 * m - 2 * bw, bh);
    let mut ok = qsa_ui::controls::default_button(w - 2 * m - 2 * bw + 6, h - m - bh, bw, bh, "OK");
    ok.emit_to(tx, true);
    let mut cancel = qsa_ui::controls::button(w - m - bw, h - m - bh, bw, bh, "Cancel");
    cancel.emit_to(tx, false);
    win.end();
    let mut win = win;
    win.set_callback(move |_| tx.send(false));
    MoveDialog {
        window: win,
        queue,
        top,
        status,
        rx,
    }
}

/// Asks where to move `files`: the queue and whether to its top.
pub fn ask_move(
    parent: Option<&dyn WindowExt>,
    files: &[SpooledFile],
    queues: &[OutputQueue],
    current: &str,
) -> Option<(ObjectName, bool)> {
    let mut d = build_move(files, queues, current);
    let win = d.window.clone();
    run(win, parent, || match d.rx.recv()? {
        false => Some(None),
        true => match model::object_named(&d.queue.value().unwrap_or_default()) {
            Some(q) => Some(Some((q, d.top.is_checked()))),
            None => {
                d.status.set("Type or pick an output queue.", Status::Error);
                None
            }
        },
    })
}

/// The delete confirmation, listing every file.
pub struct DeleteDialog {
    pub window: Window,
    rx: chan::Receiver<bool>,
}

/// The delete confirmation's columns.
pub const DELETE_COLUMNS: [(&str, i32); 5] = [
    ("File", 92),
    ("Number", 56),
    ("Job", 180),
    ("User", 80),
    ("Output queue", 110),
];

pub fn build_delete(files: &[SpooledFile]) -> DeleteDialog {
    let (tx, rx) = chan::channel::<bool>();
    let (w, m) = (560, 12);
    let rows = files.len().clamp(1, 10) as i32;
    let lh = ListTable::HEADER_H + rows * 18 + 4;
    let h = m + 36 + 8 + lh + m + theme::BUTTON_H + m;
    let (bw, bh) = (theme::BUTTON_W, theme::BUTTON_H);
    let mut win = dialog("Delete", w, h);
    let mut icon = Frame::new(m, m, 32, 32, None);
    icon.set_image(Some(qsa_ui::glyph::image(
        Glyph::Cross,
        qsa_ui::glyph::Size::Large,
    )));
    let message = match files {
        [f] => format!(
            "Delete spooled file {} from the system? It cannot be brought back.",
            f.id.file
        ),
        _ => format!(
            "Delete these {} spooled files from the system? They cannot be brought back.",
            files.len()
        ),
    };
    text(m + 44, m, w - 2 * m - 44, 36, &message);
    let mut list = ListTable::new(m, m + 44, w - 2 * m, lh, &DELETE_COLUMNS);
    list.set_rows(
        files
            .iter()
            .map(|f| {
                vec![
                    Cell::from(f.id.file.as_str()),
                    f.id.number.to_string().into(),
                    model::job_text(&f.id.job).into(),
                    f.user.as_str().into(),
                    model::object_text(&f.outq).into(),
                ]
            })
            .collect(),
    );
    let mut delete = qsa_ui::controls::button(w - 2 * m - 2 * bw + 6, h - m - bh, bw, bh, "Delete");
    delete.emit_to(tx, true);
    let mut cancel = qsa_ui::controls::default_button(w - m - bw, h - m - bh, bw, bh, "Cancel");
    cancel.emit_to(tx, false);
    win.end();
    win.set_callback(move |_| tx.send(false));
    DeleteDialog { window: win, rx }
}

/// Asks before deleting `files`; true to delete.
pub fn ask_delete(parent: Option<&dyn WindowExt>, files: &[SpooledFile]) -> bool {
    let d = build_delete(files);
    run(d.window.clone(), parent, || d.rx.recv().map(Some)).unwrap_or(false)
}

/// Properties: every field of the file.
pub fn build_properties(
    f: &SpooledFile,
    created: &dyn Fn(i64) -> String,
) -> (Window, chan::Receiver<bool>) {
    let (tx, rx) = chan::channel::<bool>();
    let fields = model::properties(f, created);
    let (w, m, rh) = (400, 12, 20);
    let h = m + fields.len() as i32 * rh + m + theme::BUTTON_H + m;
    let mut win = dialog(&format!("Properties - {}", f.id.file), w, h);
    for (i, (label, value)) in fields.iter().enumerate() {
        let y = m + i as i32 * rh;
        text(m, y, 120, rh, &format!("{label}:"));
        let mut v = text(m + 124, y, w - 2 * m - 124, rh, value);
        v.set_align(Align::Left | Align::Inside | Align::Clip);
    }
    let (bw, bh) = (theme::BUTTON_W, theme::BUTTON_H);
    let mut ok = qsa_ui::controls::default_button(w - m - bw, h - m - bh, bw, bh, "OK");
    ok.emit_to(tx, true);
    win.end();
    win.set_callback(move |_| tx.send(true));
    (win, rx)
}

pub fn show_properties(parent: Option<&dyn WindowExt>, f: &SpooledFile) {
    let (win, rx) = build_properties(f, &model::local_time);
    run::<()>(win, parent, || rx.recv().map(|_| None));
}

/// Print: which printer of this computer.
pub struct PrintDialog {
    pub window: Window,
    pub printer: Choice,
    rx: chan::Receiver<bool>,
}

pub fn build_print(f: &SpooledFile, printers: &[String], default: Option<&str>) -> PrintDialog {
    let (tx, rx) = chan::channel::<bool>();
    let (w, h, m) = (420, 110, 12);
    let (bw, bh, fh) = (theme::BUTTON_W, theme::BUTTON_H, theme::FIELD_H);
    let mut win = dialog("Print", w, h);
    text(
        m,
        m,
        w - 2 * m,
        18,
        &format!("Print spooled file {} on:", f.id.file),
    );
    let mut printer = qsa_ui::controls::combo(m, m + 22, w - 2 * m, fh);
    for p in printers {
        printer.add_choice(&p.replace('/', "\\/"));
    }
    let at = default
        .and_then(|d| printers.iter().position(|p| p == d))
        .unwrap_or(0);
    printer.set_value(at as i32);
    let mut ok =
        qsa_ui::controls::default_button(w - 2 * m - 2 * bw + 6, h - m - bh, bw, bh, "Print");
    ok.emit_to(tx, true);
    let mut cancel = qsa_ui::controls::button(w - m - bw, h - m - bh, bw, bh, "Cancel");
    cancel.emit_to(tx, false);
    win.end();
    win.set_callback(move |_| tx.send(false));
    PrintDialog {
        window: win,
        printer,
        rx,
    }
}

/// Asks which printer to print `f` on.
pub fn ask_print(
    parent: Option<&dyn WindowExt>,
    f: &SpooledFile,
    printers: &[String],
    default: Option<&str>,
) -> Option<String> {
    let d = build_print(f, printers, default);
    let i = |c: &Choice| usize::try_from(c.value()).ok();
    run(d.window.clone(), parent, || match d.rx.recv()? {
        false => Some(None),
        true => Some(i(&d.printer).and_then(|i| printers.get(i).cloned())),
    })
}

/// The summary of actions on several files.
pub struct SummaryWindow {
    pub window: Window,
    pub list: ListTable,
    rx: chan::Receiver<Option<usize>>,
}

pub fn build_summary(done: &[Done]) -> SummaryWindow {
    let (tx, rx) = chan::channel::<Option<usize>>();
    let (w, m) = (660, 12);
    let rows = done.len().clamp(3, 12) as i32;
    let lh = ListTable::HEADER_H + rows * 18 + 4;
    let h = m + 22 + lh + m + theme::BUTTON_H + m;
    let name = done
        .first()
        .map_or("Actions", |d| model::action_name(&d.action));
    let mut win = dialog(&format!("{name} - {} spooled files", done.len()), w, h);
    let (heading, status) = model::summary_heading(done);
    let mut head = text(m, m, w - 2 * m, 18, &heading);
    head.set_label_color(status.color());
    let mut list = ListTable::new(m, m + 22, w - 2 * m, lh, &model::SUMMARY_COLUMNS);
    list.set_rows(model::summary_rows(done));
    list.on_activate(move |row| {
        if row.is_some() {
            tx.send(row);
        }
    });
    list.table()
        .clone()
        .set_tooltip("Double-click a refusal for the system's second-level text.");
    let (bw, bh) = (theme::BUTTON_W, theme::BUTTON_H);
    let mut close = qsa_ui::controls::default_button(w - m - bw, h - m - bh, bw, bh, "Close");
    close.emit_to(tx, None);
    win.end();
    win.set_callback(move |_| tx.send(None));
    SummaryWindow {
        window: win,
        list,
        rx,
    }
}

/// Shows the summary until closed; a refusal's row opens the system's
/// message as it sent it.
pub fn show_summary(parent: Option<&dyn WindowExt>, done: &[Done]) {
    let d = build_summary(done);
    let win = d.window.clone();
    run::<()>(win.clone(), parent, || match d.rx.recv()? {
        None => Some(None),
        Some(row) => {
            if let Some(Done {
                outcome:
                    Outcome::Refused {
                        server: Some(m), ..
                    },
                ..
            }) = done.get(row)
            {
                msgdetails::show_system_message(Some(&win), None, &crate::qap::shown(m));
            }
            None
        }
    });
}
