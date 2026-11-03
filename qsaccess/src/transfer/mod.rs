// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! Data Transfer: rows between the system's tables and files on this
//! computer ([`qsa_transfer`]), from the task pane's General group.
//!
//! - [`download`]: from the system to a file.
//! - [`upload`]: from a file to the system.
//! - Every run has a connection of its own, attached to the session, and
//!   closes it when done; it runs on a thread of its own with a progress
//!   dialog and Cancel, and ends with a summary (`QAD7030`, `QAD7031`,
//!   `QAD7032`).
//! - Before QAP version 4 the windows do not open: `QAD5001`.

pub mod download;
pub mod upload;

use std::cell::Cell as StdCell;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::Duration;

use fltk::app;
use fltk::frame::Frame;
use fltk::prelude::*;
use fltk::window::Window;
use qap_client::proto::Op;
use qsa_config::System;
use qsa_msg::{Message, id};
use qsa_transfer::Progress;
use qsa_ui::listtable::{Cell, ListTable};
use qsa_ui::theme::{self, colors};
use qsa_ui::{controls, msgdetails};

use crate::app::Shared;
use crate::qap::{self, Handle};

/// A duration as a summary says it: `0.4 s`, `2 min 05 s`.
pub fn took(d: Duration) -> String {
    let s = d.as_secs_f64();
    if s < 60.0 {
        format!("{s:.1} s")
    } else {
        let whole = d.as_secs();
        format!("{} min {:02} s", whole / 60, whole % 60)
    }
}

/// `QAD7030`: transfer `name` moved `rows` rows in `elapsed`.
pub fn complete(name: &str, rows: u64, elapsed: Duration) -> Message {
    qsa_msg::message(
        id::TRANSFER_COMPLETE,
        &[name, &rows.to_string(), &took(elapsed)],
    )
}

/// `QAD7032`: transfer `name` was cancelled.
pub fn cancelled(name: &str) -> Message {
    qsa_msg::message(id::TRANSFER_CANCELLED, &[name])
}

/// `QAD7031`: transfer `name` failed at `row`, line `line` of `file`.
pub fn failed(name: &str, row: u64, line: u64, file: &str, why: &str) -> Message {
    qsa_msg::message(
        id::TRANSFER_FAILED,
        &[
            name,
            &row.to_string(),
            &line.to_string(),
            file,
            why.trim_end_matches('.'),
        ],
    )
}

/// The session on `system` for a transfer, signing on first if there is
/// none; `None` (shown) when the user cancels or the system's QAP version
/// has no `Sql` service (`QAD5001`).
pub fn session(
    parent: Option<&dyn WindowExt>,
    state: &Shared,
    system: &str,
) -> Option<(System, Handle)> {
    let sys = state.borrow().systems.get(system).cloned()?;
    let h = qap::session(parent, state, &sys.name)?;
    let refused = h.try_lock().ok().and_then(|mut c| {
        let has = c.version().unwrap_or(1);
        c.supports(Op::Prepare).err().map(|_| has)
    });
    if let Some(has) = refused {
        let m = qap::version_needed("Data Transfer", &sys.name, Op::Prepare, has);
        crate::log!("{m}");
        msgdetails::show_message(parent, &m);
        return None;
    }
    Some((sys, h))
}

/// A run's connection: attached to the session, with the job's date
/// style and CCSID.
pub struct RunConnection {
    pub q: qap_client::SqlConnection,
    pub dates: qsa_sql::DateStyle,
    pub ccsid: u16,
}

/// Attaches a connection for a run (`QAD7003` when the session has its 16).
pub fn attach(parent: Option<&dyn WindowExt>, h: &Handle, sys: &System) -> Option<RunConnection> {
    let a = qap::attach(parent, h, sys, "Data Transfer")?;
    let p = a.profile().clone();
    let ccsid = match p.ccsid {
        0 | 65535 => a.hello().system_ccsid,
        c => c,
    };
    Some(RunConnection {
        q: qap_client::SqlConnection::new(a),
        dates: qsa_sql::DateStyle::job(&p.date_format, &p.date_separator, &p.time_separator),
        ccsid,
    })
}

/// What the progress dialog hears from the run's thread.
enum Msg<T> {
    Progress(Progress),
    Done(T),
}

/// The progress dialog's widgets, for screenshots.
pub struct ProgressDialog {
    pub window: Window,
    pub rows: Frame,
    pub cancel: fltk::button::Button,
}

/// The progress dialog for transfer `name`, not shown.
pub fn progress_dialog(name: &str) -> ProgressDialog {
    let (w, h) = (380, 120);
    let mut window = Window::new(0, 0, w, h, None);
    window.set_label(&format!("Transferring {name}"));
    window.set_color(colors::FACE);
    let m = 12;
    let mut what = Frame::new(m, m, w - 2 * m, theme::FIELD_H, None);
    what.set_label(&format!("Transfer {name}"));
    what.set_align(fltk::enums::Align::Left | fltk::enums::Align::Inside);
    what.set_label_size(theme::UI_SIZE);
    let mut rows = Frame::new(m, m + theme::FIELD_H + 4, w - 2 * m, theme::FIELD_H, None);
    rows.set_label("Starting...");
    rows.set_align(fltk::enums::Align::Left | fltk::enums::Align::Inside);
    rows.set_label_size(theme::UI_SIZE);
    let cancel = controls::button(
        w - m - theme::BUTTON_W,
        h - m - theme::BUTTON_H,
        theme::BUTTON_W,
        theme::BUTTON_H,
        "Cancel",
    );
    window.end();
    window.make_modal(true);
    ProgressDialog {
        window,
        rows,
        cancel,
    }
}

/// The progress line: `1,500 rows, 2.5 s`.
pub fn progress_text(p: Progress) -> String {
    format!("{} rows, {}", p.rows, took(p.elapsed))
}

/// Runs `work` on a thread of its own with the progress dialog for
/// `name`; Cancel (or closing the dialog) sets the flag `work` gets. Other
/// windows go on meanwhile. What `work` returned.
pub fn with_progress<T: Send + 'static>(
    parent: Option<&dyn WindowExt>,
    name: &str,
    work: impl FnOnce(&dyn Fn(Progress), &AtomicBool) -> T + Send + 'static,
) -> T {
    let (tx, rx) = mpsc::channel::<Msg<T>>();
    let cancel = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&cancel);
    let ptx = tx.clone();
    std::thread::spawn(move || {
        let report = move |p: Progress| {
            let _ = ptx.send(Msg::Progress(p));
            app::awake();
        };
        let r = work(&report, &flag);
        let _ = tx.send(Msg::Done(r));
        app::awake();
    });
    let mut d = progress_dialog(name);
    {
        let c = Arc::clone(&cancel);
        let mut rows = d.rows.clone();
        d.cancel.set_callback(move |b| {
            c.store(true, Ordering::SeqCst);
            b.deactivate();
            rows.set_label("Cancelling...");
        });
        let c = Arc::clone(&cancel);
        d.window
            .set_callback(move |_| c.store(true, Ordering::SeqCst));
    }
    qsa_ui::propdialog::center_on(&mut d.window, parent);
    qsa_ui::propdialog::show_placed(&mut d.window);
    loop {
        let _ = app::wait_for(0.05);
        loop {
            match rx.try_recv() {
                Ok(Msg::Progress(p)) => {
                    if !cancel.load(Ordering::SeqCst) {
                        d.rows.set_label(&progress_text(p));
                        d.rows.redraw();
                    }
                }
                Ok(Msg::Done(r)) => {
                    d.window.hide();
                    return r;
                }
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    panic!("the transfer thread ended without an answer")
                }
            }
        }
    }
}

/// Picks a schema and a table from the system's catalog, on the shared
/// connection; `None` when the user cancels.
pub fn pick_table(
    parent: Option<&dyn WindowExt>,
    h: &Handle,
    title: &str,
) -> Option<(String, String)> {
    let schemas = match qap::busy(|| qap::lock(h).all_schemas("%")) {
        Ok(s) => s,
        Err(e) => {
            crate::log!("schemas: {e}");
            return None;
        }
    };
    let (w, hh) = (640, 420);
    let mut win = Window::new(0, 0, w, hh, None);
    win.set_label(title);
    win.set_color(colors::FACE);
    win.make_modal(true);
    let m = 10;
    let (bw, bh) = (theme::BUTTON_W, theme::BUTTON_H);
    let lh = hh - 3 * m - bh;
    let mut left = ListTable::new(m, m, 240, lh, &[("Schema", 100), ("Text", 120)]);
    left.set_rows(
        schemas
            .iter()
            .map(|s| vec![Cell::from(s.name.as_str()), Cell::from(s.text.as_str())])
            .collect(),
    );
    let mut right = ListTable::new(
        m + 250,
        m,
        w - 2 * m - 250,
        lh,
        &[("Table", 120), ("Kind", 90), ("Text", 150)],
    );
    let mut ok = controls::default_button(w - m - 2 * bw - 6, hh - m - bh, bw, bh, "OK");
    let mut cancel = controls::button(w - m - bw, hh - m - bh, bw, bh, "Cancel");
    win.end();
    let tables: Rc<std::cell::RefCell<Vec<qap_client::proto::ops::sql::Table>>> = Rc::default();
    {
        let (h, t, mut r, names) = (
            Arc::clone(h),
            tables.clone(),
            right.clone(),
            schemas.clone(),
        );
        left.on_select(move |row| {
            let Some(s) = row.and_then(|i| names.get(i)) else {
                return;
            };
            let schema = s.name.clone();
            let got = qap::busy(|| qap::lock(&h).all_tables(&schema, "%", &[])).unwrap_or_default();
            r.set_rows(
                got.iter()
                    .map(|t| {
                        vec![
                            Cell::from(t.name.as_str()),
                            Cell::from(kind_name(t.kind)),
                            Cell::from(t.text.as_str()),
                        ]
                    })
                    .collect(),
            );
            *t.borrow_mut() = got;
        });
    }
    let chosen = Rc::new(StdCell::new(false));
    {
        let (c, mut wn) = (chosen.clone(), win.clone());
        ok.set_callback(move |_| {
            c.set(true);
            wn.hide();
        });
        let (c, mut wn) = (chosen.clone(), win.clone());
        right.on_activate(move |row| {
            if row.is_some() {
                c.set(true);
                wn.hide();
            }
        });
        let mut wn = win.clone();
        cancel.set_callback(move |_| wn.hide());
    }
    qsa_ui::propdialog::center_on(&mut win, parent);
    qsa_ui::propdialog::show_placed(&mut win);
    while win.shown() {
        app::wait();
    }
    if !chosen.get() {
        return None;
    }
    let t = tables.borrow();
    right
        .selected()
        .and_then(|i| t.get(i))
        .map(|t| (t.schema.clone(), t.name.clone()))
}

/// A catalog entry's kind where the user sees it.
pub fn kind_name(k: qap_client::proto::ops::sql::TableKind) -> &'static str {
    use qap_client::proto::ops::sql::TableKind;
    match k {
        TableKind::Table => "Table",
        TableKind::View => "View",
        TableKind::Alias => "Alias",
        TableKind::PhysicalFile => "Physical file",
        TableKind::LogicalFile => "Logical file",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summaries() {
        assert_eq!(took(Duration::from_millis(400)), "0.4 s");
        assert_eq!(took(Duration::from_secs(125)), "2 min 05 s");
        assert_eq!(
            complete("assets.csv", 12, Duration::from_millis(400)).text,
            "Transfer assets.csv complete: 12 rows in 0.4 s."
        );
        assert_eq!(
            failed("up.csv", 7, 8, "up.csv", "SQL0803: Duplicate key.").text,
            "Transfer up.csv failed at row 7 (line 8 of up.csv): SQL0803: Duplicate key."
        );
        assert_eq!(cancelled("x").id, "QAD7032");
        assert_eq!(
            progress_text(Progress {
                rows: 1500,
                elapsed: Duration::from_millis(2500)
            }),
            "1500 rows, 2.5 s"
        );
    }
}
