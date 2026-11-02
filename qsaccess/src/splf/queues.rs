// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! Output Queues: a system's output queues from `ListOutputQueues`, by
//! library (`*ALL`, `*LIBL` or a name), paged as the file list is. A
//! double-click opens Printer Output on that queue.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::mpsc;

use fltk::app;
use fltk::button::Button;
use fltk::enums::{Align, CallbackTrigger};
use fltk::frame::Frame;
use fltk::misc::InputChoice;
use fltk::prelude::*;
use fltk::window::Window;
use qap_client::Error;
use qap_client::proto::Op;
use qap_client::proto::ops::spool::{ListOutputQueues, OutputQueue, OutputQueues};
use qap_client::spool::Paging;
use qsa_config::System;
use qsa_ui::chan::{self, Emit};
use qsa_ui::listtable::{Cell, ListTable};
use qsa_ui::theme::{self, colors};
use qsa_ui::{Status, StatusBar, controls};

use crate::app::Shared;
use crate::qap::{self, Handle};

/// Window width and height.
pub const W: i32 = 640;
pub const H: i32 = 420;

/// The columns: title and width.
pub const COLUMNS: [(&str, i32); 6] = [
    ("Queue", 100),
    ("Library", 100),
    ("Text", 240),
    ("Files", 60),
    ("Writer", 80),
    ("Held", 40),
];

/// One row.
pub fn row(q: &OutputQueue) -> Vec<Cell> {
    vec![
        q.outq.name.as_str().into(),
        q.outq.library.as_str().into(),
        q.text.as_str().into(),
        q.files.to_string().into(),
        q.writer.as_str().into(),
        Cell {
            text: if q.held { "Yes" } else { "No" }.into(),
            status: if q.held {
                Status::Error
            } else {
                Status::Neutral
            },
        },
    ]
}

/// The status bar's line: `3 output queues in *ALL on SYS.`
pub fn line(total: u32, library: &str, system: &str) -> String {
    let what = if total == 1 {
        "output queue"
    } else {
        "output queues"
    };
    format!("{total} {what} in {library} on {system}.")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Cmd {
    Go,
    Previous,
    Next,
    Open,
    Close,
}

/// The window's widgets.
pub struct QueuesWindow {
    pub window: Window,
    pub library: InputChoice,
    pub list: ListTable,
    pub paging: Frame,
    previous: Button,
    next: Button,
    pub status: StatusBar,
    rx: chan::Receiver<Cmd>,
    st: RefCell<St>,
}

struct St {
    system: System,
    state: Option<Shared>,
    handle: Option<Handle>,
    library: String,
    paging: Paging,
    queues: Vec<OutputQueue>,
    listing: Option<mpsc::Receiver<Result<OutputQueues, Error>>>,
}

thread_local! {
    static WINDOWS: RefCell<Vec<Rc<QueuesWindow>>> = const { RefCell::new(Vec::new()) };
}

impl QueuesWindow {
    /// The window for `system`, not shown and not listing (screenshots).
    pub fn build(system: &System, rows: u32) -> Rc<QueuesWindow> {
        let (tx, rx) = chan::channel::<Cmd>();
        let mut window = Window::new(0, 0, W, H, None);
        window.set_label(&format!("Output Queues - {}", system.name));
        window.set_icon(Some(qsa_ui::glyph::app_icon()));
        window.set_color(colors::FACE);
        window.size_range(420, 260, 0, 0);
        let (m, fh) = (8, theme::FIELD_H);
        let (bw, bh) = (theme::BUTTON_W, theme::BUTTON_H);
        let mut l = controls::label(m, m, 50, fh, "Library:");
        l.set_align(Align::Left | Align::Inside);
        let mut library = controls::editable_combo(m + 50, m, 160, fh);
        for v in ["*ALL", "*LIBL"] {
            library.add(v);
        }
        library.set_value("*ALL");
        library.input().set_trigger(CallbackTrigger::EnterKeyAlways);
        library.input().set_callback(move |_| tx.send(Cmd::Go));
        let mut go = controls::default_button(m + 50 + 170, m - 1, bw, bh, "Go");
        go.emit_to(tx, Cmd::Go);
        let ly = m + fh + 8;
        let lh = H - ly - bh - 8 - StatusBar::HEIGHT - 4;
        let mut list = ListTable::new(m - 2, ly, W - 2 * m + 4, lh, &COLUMNS);
        list.on_activate(move |r| {
            if r.is_some() {
                tx.send(Cmd::Open);
            }
        });
        list.table()
            .clone()
            .set_tooltip("Double-click a queue for its spooled files.");
        let py = ly + lh + 4;
        let mut previous = controls::button(m, py, bw, bh, "Previous");
        previous.emit_to(tx, Cmd::Previous);
        let mut next = controls::button(m + bw + 6, py, bw, bh, "Next");
        next.emit_to(tx, Cmd::Next);
        let mut paging = Frame::new(m + 2 * bw + 18, py, 300, bh, None);
        paging.set_align(Align::Left | Align::Inside);
        paging.set_label_size(theme::UI_SIZE);
        let status = StatusBar::new(0, H - StatusBar::HEIGHT, W);
        window.resizable(list.table());
        window.end();
        window.set_callback(move |_| tx.send(Cmd::Close));
        Rc::new(QueuesWindow {
            window,
            library,
            list,
            paging,
            previous,
            next,
            status,
            rx,
            st: RefCell::new(St {
                system: system.clone(),
                state: None,
                handle: None,
                library: "*ALL".into(),
                paging: Paging::new(rows),
                queues: Vec::new(),
                listing: None,
            }),
        })
    }

    /// Shows `queues` as page one of `total` in `library` (screenshots).
    pub fn show_queues(&self, library: &str, queues: Vec<OutputQueue>, total: u32) {
        self.library.clone().set_value(library);
        {
            let mut st = self.st.borrow_mut();
            st.library = library.to_owned();
            st.paging.update(total);
            st.queues = queues;
        }
        self.fill();
    }

    fn fill(&self) {
        let st = self.st.borrow();
        self.list
            .clone()
            .set_rows(st.queues.iter().map(row).collect());
        let (page, pages) = st.paging.page();
        let text = match st.paging.shown(st.queues.len()) {
            Some((a, b)) => format!(
                "Queues {a}-{b} of {}, page {page} of {pages}",
                st.paging.total
            ),
            None => "No output queues".into(),
        };
        self.paging.clone().set_label(&text);
        for (b, on) in [
            (&self.previous, st.paging.has_previous()),
            (&self.next, st.paging.has_next()),
        ] {
            let mut b = b.clone();
            if on {
                b.activate();
            } else {
                b.deactivate();
            }
        }
        let l = line(st.paging.total, &st.library, &st.system.name);
        drop(st);
        self.status.clone().set_message(&l);
        self.window.clone().redraw();
    }

    fn list_page(&self) {
        let mut st = self.st.borrow_mut();
        let Some(h) = st.handle.clone() else { return };
        if st.listing.is_some() {
            return;
        }
        let req = ListOutputQueues {
            library: st.library.clone(),
            start: st.paging.start,
            max: st.paging.per_page,
        };
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let r = (|| {
                let (p, t) = {
                    let mut c = qap::lock(&h);
                    let t = c.target().options.reply_timeout;
                    (c.request(Op::ListOutputQueues, &req)?, t)
                };
                p.wait(t)?.decode::<OutputQueues>()
            })();
            let _ = tx.send(r);
            app::awake();
        });
        st.listing = Some(rx);
    }

    fn pump(&self) -> bool {
        while let Some(c) = self.rx.recv() {
            match c {
                Cmd::Go => {
                    let lib = self
                        .library
                        .value()
                        .unwrap_or_default()
                        .trim()
                        .to_ascii_uppercase();
                    {
                        let mut st = self.st.borrow_mut();
                        st.library = if lib.is_empty() { "*ALL".into() } else { lib };
                        st.paging.start = 0;
                    }
                    self.list_page();
                }
                Cmd::Previous | Cmd::Next => {
                    let moved = {
                        let mut st = self.st.borrow_mut();
                        if c == Cmd::Next {
                            st.paging.next_page()
                        } else {
                            st.paging.previous_page()
                        }
                    };
                    if moved {
                        self.list_page();
                    }
                }
                Cmd::Open => {
                    let (state, system, q) = {
                        let st = self.st.borrow();
                        let q = self.list.selected().and_then(|i| st.queues.get(i).cloned());
                        (st.state.clone(), st.system.name.clone(), q)
                    };
                    if let (Some(state), Some(q)) = (state, q) {
                        let parent = Some(&self.window as &dyn WindowExt);
                        super::open(parent, &state, &system, Some(&q.outq));
                    }
                }
                Cmd::Close => self.window.clone().hide(),
            }
        }
        let r = {
            let st = self.st.borrow();
            match st.listing.as_ref().map(mpsc::Receiver::try_recv) {
                Some(Ok(r)) => Some(r),
                _ => None,
            }
        };
        if let Some(r) = r {
            self.st.borrow_mut().listing = None;
            match r {
                Ok(page) => {
                    let moved = {
                        let mut st = self.st.borrow_mut();
                        let moved = st.paging.update(page.total);
                        st.queues = page.queues;
                        moved
                    };
                    self.fill();
                    if moved {
                        self.list_page();
                    }
                }
                Err(e) => {
                    let sys = self.st.borrow().system.clone();
                    let x = qap::explain(&sys, &e);
                    crate::log!("{}", x.line());
                    self.status.clone().set_message(&x.line());
                    if x.server.is_some() {
                        qap::show(Some(&self.window), &x);
                    }
                }
            }
        }
        self.window.shown()
    }
}

/// Opens Output Queues for `system` (whose session Printer Output has),
/// or brings an open one to the front.
pub fn open(parent: Option<&dyn WindowExt>, state: &Shared, system: &str) {
    let existing = WINDOWS.with(|v| {
        v.borrow()
            .iter()
            .find(|q| q.window.shown() && q.st.borrow().system.name.eq_ignore_ascii_case(system))
            .cloned()
    });
    if let Some(q) = existing {
        q.window.clone().show();
        return;
    }
    let Some(sys) = state.borrow().systems.get(system).cloned() else {
        return;
    };
    let Some(h) = qap::session(parent, state, &sys.name) else {
        return;
    };
    let rows = state.borrow().settings.printer_output.rows();
    let q = QueuesWindow::build(&sys, rows);
    {
        let mut st = q.st.borrow_mut();
        st.state = Some(state.clone());
        st.handle = Some(h);
    }
    let mut win = q.window.clone();
    qsa_ui::propdialog::center_on(&mut win, parent);
    qsa_ui::propdialog::show_placed(&mut win);
    q.list_page();
    WINDOWS.with(|v| v.borrow_mut().push(q));
    super::tick();
}

/// Gives every Output Queues window its waiting work; forgets closed ones.
pub fn pump() {
    let windows: Vec<Rc<QueuesWindow>> = WINDOWS.with(|v| v.borrow().clone());
    let open: Vec<bool> = windows.iter().map(|q| q.pump()).collect();
    WINDOWS.with(|v| {
        let mut i = 0;
        v.borrow_mut().retain(|_| {
            let keep = open.get(i).copied().unwrap_or(true);
            i += 1;
            keep
        });
    });
}

/// Whether any Output Queues window is open.
pub fn any_open() -> bool {
    WINDOWS.with(|v| v.borrow().iter().any(|q| q.window.shown()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::splf::model;

    #[test]
    fn rows_and_line() {
        let q = OutputQueue {
            outq: model::object_named("QGPL/PRT01").unwrap(),
            text: "Printer PRT01".into(),
            files: 2,
            writer: "PRT01".into(),
            held: true,
        };
        let r = row(&q);
        let t: Vec<&str> = r.iter().map(|c| c.text.as_str()).collect();
        assert_eq!(t, ["PRT01", "QGPL", "Printer PRT01", "2", "PRT01", "Yes"]);
        assert_eq!(r[5].status, Status::Error);
        assert_eq!(line(3, "*ALL", "SYS"), "3 output queues in *ALL on SYS.");
        assert_eq!(line(1, "QGPL", "SYS"), "1 output queue in QGPL on SYS.");
    }
}
