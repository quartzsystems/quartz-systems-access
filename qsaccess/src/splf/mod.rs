// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! Printer Output: the spooled files on a system, over its QAP session
//! (QAP version 3's `Spool` service). Not to be confused with a 5250
//! printer session's Received Output, which is output this computer
//! received and keeps locally.
//!
//! - Filter row: User (`*CURRENT`, `*ALL` or a profile), Output queue
//!   (`*ALL` or one from `ListOutputQueues`, or typed), Status, Form type,
//!   User data, and Go.
//! - The list is paged by the system (`start`, `max`; rows per page from
//!   settings) with Previous and Next. A column heading sorts the page
//!   shown, which the status bar says. Status words: Ready and Printing in
//!   green, Message in red.
//! - Actions from the menu, the toolbar and a right-click: Open, Save as
//!   PDF..., Save as Text..., Print..., Properties on the current file;
//!   Hold (immediately or at the end of the page), Release, Delete (after
//!   a confirmation listing every file) and Move... on every selected file,
//!   one request each, side by side, with one summary of how each ended
//!   and every refusal's message as the system sent it.
//! - Reading runs on its own thread, a batch of pages at a time, with
//!   progress in the status bar and Cancel beside it.
//! - Open shows an SCS file in the viewer ([`viewer`]), its pages as they
//!   arrive; another kind of file in the system's PDF viewer, from the
//!   server's PDF in a temporary file.
//! - Output Queues ([`queues`]): the system's queues; a double-click opens
//!   this window on one.
//! - Refresh, and Auto-refresh (Off, 15, 30 or 60 seconds): only while the
//!   window is shown and not minimised, only the page in view, keeping the
//!   selection and the scroll position.
//! - On QAP version 6 the list keeps up as changes happen instead
//!   (`QAD9020`): the filter's output queue, or with every queue its one
//!   user (`*CURRENT` or a profile), is watched on the shared connection;
//!   a file changed on the page changes in place, and one added or gone
//!   asks for the page once. Auto-refresh is set aside while the watch
//!   works. Every user on every queue cannot be watched, and a system
//!   before version 6 has no watches: the timer refreshes the list as
//!   before (`QAD9021`), as it does when a watch fails. After a
//!   reconnect the watch is watched again and the page asked for once.
//!
//! The network never runs on the GUI thread, which never waits for the
//! shared client either: workers lock it only to send. A repeating timeout
//! polls them, so progress shows also while another window's dialog runs.

pub mod dialogs;
pub mod model;
pub mod output;
pub mod queues;
pub mod viewer;

use std::cell::{Cell as StdCell, RefCell};
use std::rc::Rc;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use fltk::app;
use fltk::button::Button;
use fltk::enums::{Align, Key, Shortcut};
use fltk::frame::Frame;
use fltk::input::Input;
use fltk::menu::{Choice, MenuBar, MenuFlag, MenuItem};
use fltk::misc::InputChoice;
use fltk::prelude::*;
use fltk::window::Window;
use qap_client::proto::Op;
use qap_client::proto::ops::spool::{
    ListOutputQueues, OutputQueue, OutputQueues, SpoolTarget, SpooledFile, SpooledFileId,
    SpooledFiles,
};
use qap_client::spool::{Action, Paging, ReadEnd};
use qap_client::{Error, Watch, WatchEvent};
use qsa_config::System;
use qsa_msg::id;
use qsa_ui::chan::{self, Emit, MenuEmit};
use qsa_ui::listtable::ListTable;
use qsa_ui::sampler::{MENU_H, menu_bar};
use qsa_ui::theme::{self, colors};
use qsa_ui::{Glyph, Status, StatusBar, Toolbar, controls, msgdetails};

use crate::app::Shared;
use crate::qap::{self, Handle, lock};

use model::{Done, Filter, Outcome};
use output::{Purpose, ReadEvent, Reading};

/// Window width and height.
pub const W: i32 = 960;
pub const H: i32 = 560;

/// How often the window looks at its workers.
const POLL: f64 = 0.1;

/// A watch's burst of changes is over this long after its last one, and
/// the page is asked for again once.
const RELIST_AFTER: Duration = Duration::from_millis(300);

/// What menus, toolbar, buttons and the list ask for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cmd {
    Go,
    Refresh,
    Previous,
    Next,
    Sort(usize),
    Open,
    SavePdf,
    SaveText,
    Print,
    Hold,
    Release,
    Delete,
    Move,
    Properties,
    OutputQueues,
    AutoRefresh,
    CancelRead,
    Close,
    About,
    /// Help > Help Topics.
    HelpTopics,
}

/// The window's widgets.
pub struct Widgets {
    pub window: Window,
    pub menu: MenuBar,
    pub user: InputChoice,
    pub outq: InputChoice,
    pub status_filter: Choice,
    pub form_type: Input,
    pub user_data: Input,
    pub list: ListTable,
    pub paging: Frame,
    pub previous: Button,
    pub next: Button,
    pub auto_refresh: Choice,
    /// Sets Auto-refresh aside while a spool watch keeps the list up.
    pub auto_refresh_switch: controls::Switch,
    pub cancel_read: Button,
    pub status: StatusBar,
}

/// A list request in flight.
struct Listing {
    rx: mpsc::Receiver<Result<SpooledFiles, Error>>,
}

/// Actions on files in flight.
struct Acting {
    rx: mpsc::Receiver<Vec<Done>>,
}

/// What the window keeps.
struct St {
    system: System,
    handle: Option<Handle>,
    filter: Filter,
    paging: Paging,
    /// The page shown, in the order shown.
    files: Vec<SpooledFile>,
    sort: Option<(usize, bool)>,
    listing: Option<Listing>,
    /// Ask again when the request in flight is done.
    again: bool,
    last_list: Instant,
    queues: Vec<OutputQueue>,
    queues_rx: Option<mpsc::Receiver<Result<Vec<OutputQueue>, Error>>>,
    acting: Option<Acting>,
    reads: Vec<(Reading, Purpose)>,
    /// The page model's options for this system.
    scs: scs::Options,
    /// How the list keeps up with the system: a spool watch (version 6)
    /// or the auto-refresh timer.
    watch: Watching,
    /// Ask for the page again at this time: a watch's changes moved it.
    relist_at: Option<Instant>,
}

/// How a Printer Output window keeps up with the system.
enum Watching {
    /// The auto-refresh timer: no watch for this filter (every user on
    /// every queue), none on the system (before version 6), or one that
    /// failed.
    Polling,
    /// A watch of the target asked for, its outcome to come.
    Asking(SpoolTarget, mpsc::Receiver<Result<(), Error>>),
    /// The target is watched: its changes come as they happen.
    Live(SpoolTarget),
}

impl Watching {
    fn target(&self) -> Option<&SpoolTarget> {
        match self {
            Watching::Polling => None,
            Watching::Asking(t, _) | Watching::Live(t) => Some(t),
        }
    }
}

/// One Printer Output window.
pub struct PrinterOutput {
    pub w: Widgets,
    rx: chan::Receiver<Cmd>,
    state: Option<Shared>,
    st: RefCell<St>,
    /// The files a refresh is to keep selected, noted before it asks.
    selected_ids: RefCell<Vec<SpooledFileId>>,
}

thread_local! {
    static WINDOWS: RefCell<Vec<Rc<PrinterOutput>>> = const { RefCell::new(Vec::new()) };
    static PUMPING: StdCell<bool> = const { StdCell::new(false) };
    static TICKING: StdCell<bool> = const { StdCell::new(false) };
}

fn auto_refresh_label(secs: u32) -> String {
    if secs == 0 {
        "Off".into()
    } else {
        format!("{secs} seconds")
    }
}

fn build_widgets(system: &str, rows: u32, refresh_secs: u32) -> (Widgets, chan::Receiver<Cmd>) {
    let (tx, rx) = chan::channel::<Cmd>();
    let mut window = Window::new(0, 0, W, H, None);
    window.set_label(&format!("Printer Output - {system}"));
    window.set_icon(Some(qsa_ui::glyph::app_icon()));
    window.set_color(colors::FACE);
    window.size_range(640, 360, 0, 0);

    let mut menu = menu_bar(0, 0, W);
    let mut add = |path: &str, sc: Shortcut, c: Cmd, flag: MenuFlag| {
        menu.add_emit_to(path, sc, flag, tx, c);
    };
    let n = MenuFlag::Normal;
    let d = MenuFlag::MenuDivider;
    add("&File/&Open", Shortcut::Ctrl | 'o', Cmd::Open, n);
    add(
        "&File/Save as &PDF...",
        Shortcut::Ctrl | 's',
        Cmd::SavePdf,
        n,
    );
    add("&File/Save as &Text...", Shortcut::None, Cmd::SaveText, n);
    add("&File/P&rint...", Shortcut::Ctrl | 'p', Cmd::Print, d);
    add("&File/&Close", Shortcut::Alt | Key::F4, Cmd::Close, n);
    add("&Actions/&Hold...", Shortcut::None, Cmd::Hold, n);
    add("&Actions/&Release", Shortcut::None, Cmd::Release, n);
    add(
        "&Actions/&Delete...",
        Shortcut::None | Key::Delete,
        Cmd::Delete,
        n,
    );
    add("&Actions/&Move...", Shortcut::None, Cmd::Move, d);
    add(
        "&Actions/Propert&ies",
        Shortcut::Alt | Key::Enter,
        Cmd::Properties,
        n,
    );
    add("&View/&Refresh", Shortcut::None | Key::F5, Cmd::Refresh, n);
    add(
        "&View/&Output Queues...",
        Shortcut::None,
        Cmd::OutputQueues,
        n,
    );
    add(
        "&Help/&Help Topics",
        Shortcut::None | Key::F1,
        Cmd::HelpTopics,
        n,
    );
    add(
        "&Help/&About QS\\/400 Access for Desktop...",
        Shortcut::None,
        Cmd::About,
        n,
    );

    let mut tb = Toolbar::new(0, MENU_H, W);
    tb.add(Glyph::Connect, "Refresh (F5)", move || {
        tx.send(Cmd::Refresh)
    });
    tb.separator();
    for (g, tip, c) in [
        (Glyph::Table, "Open", Cmd::Open),
        (Glyph::Disk, "Save as PDF", Cmd::SavePdf),
        (Glyph::Printer, "Print", Cmd::Print),
    ] {
        tb.add(g, tip, move || tx.send(c));
    }
    tb.separator();
    for (g, tip, c) in [
        (Glyph::Lock, "Hold", Cmd::Hold),
        (Glyph::Check, "Release", Cmd::Release),
        (Glyph::Cross, "Delete", Cmd::Delete),
        (Glyph::Paste, "Move", Cmd::Move),
    ] {
        tb.add(g, tip, move || tx.send(c));
    }
    tb.separator();
    tb.add(Glyph::System, "Properties", move || {
        tx.send(Cmd::Properties)
    });
    tb.add(Glyph::Folder, "Output Queues", move || {
        tx.send(Cmd::OutputQueues)
    });
    tb.end();

    // The filter row.
    let m = 8;
    let fh = theme::FIELD_H;
    let fy = MENU_H + Toolbar::HEIGHT + 6;
    let mut x = m;
    let label = |x: &mut i32, w: i32, t: &str| {
        let mut f = controls::label(*x, fy, w, fh, t);
        f.set_align(Align::Left | Align::Inside);
        *x += w;
    };
    label(&mut x, 34, "User:");
    let mut user = controls::editable_combo(x, fy, 110, fh);
    for v in ["*CURRENT", "*ALL"] {
        user.add(v);
    }
    user.set_value("*CURRENT");
    x += 110 + 10;
    label(&mut x, 84, "Output queue:");
    let mut outq = controls::editable_combo(x, fy, 160, fh);
    outq.add("*ALL");
    outq.set_value("*ALL");
    x += 160 + 10;
    label(&mut x, 44, "Status:");
    let mut status_filter = controls::combo(x, fy, 90, fh);
    status_filter.add_choice("*ALL");
    for s in model::STATUSES {
        status_filter.add_choice(model::status_word(s));
    }
    status_filter.set_value(0);
    x += 90 + 10;
    label(&mut x, 66, "Form type:");
    let mut form_type = controls::field(x, fy, 74, fh);
    form_type.set_tooltip("Blank or *ALL for every form type.");
    x += 74 + 10;
    label(&mut x, 64, "User data:");
    let mut user_data = controls::field(x, fy, 90, fh);
    user_data.set_tooltip("Blank or *ALL for any user data.");
    x += 90 + 10;
    let mut go = controls::default_button(x, fy - 1, theme::BUTTON_W, theme::BUTTON_H, "Go");
    go.emit_to(tx, Cmd::Go);
    for i in [&mut form_type, &mut user_data] {
        i.set_trigger(fltk::enums::CallbackTrigger::EnterKeyAlways);
        i.set_callback(move |_| tx.send(Cmd::Go));
    }

    let ly = fy + fh + 8;
    let ph = theme::BUTTON_H + 8;
    let lh = H - ly - ph - StatusBar::HEIGHT - 4;
    let mut list = ListTable::new(m - 2, ly, W - 2 * m + 4, lh, &model::COLUMNS);
    list.set_multi(true);
    list.on_activate(move |row| {
        if row.is_some() {
            tx.send(Cmd::Open);
        }
    });
    list.on_header_click(move |c| tx.send(Cmd::Sort(c)));
    list.on_context(move |row| {
        if row.is_none() {
            return;
        }
        let items = [
            ("Open", Cmd::Open),
            ("Save as PDF...", Cmd::SavePdf),
            ("Save as Text...", Cmd::SaveText),
            ("Print...", Cmd::Print),
            ("Hold...", Cmd::Hold),
            ("Release", Cmd::Release),
            ("Delete...", Cmd::Delete),
            ("Move...", Cmd::Move),
            ("Properties", Cmd::Properties),
        ];
        let labels: Vec<&str> = items.iter().map(|(l, _)| *l).collect();
        let menu = MenuItem::new(&labels);
        if let Some(item) = menu.popup(app::event_x(), app::event_y())
            && let Some((_, c)) = items
                .iter()
                .find(|(l, _)| item.label().as_deref() == Some(*l))
        {
            tx.send(*c);
        }
    });

    // The paging line.
    let py = ly + lh + 4;
    let bw = theme::BUTTON_W;
    let bh = theme::BUTTON_H;
    let mut previous = controls::button(m, py, bw, bh, "Previous");
    previous.emit_to(tx, Cmd::Previous);
    let mut next = controls::button(m + bw + 6, py, bw, bh, "Next");
    next.emit_to(tx, Cmd::Next);
    let mut paging = Frame::new(m + 2 * bw + 18, py, 300, bh, None);
    paging.set_align(Align::Left | Align::Inside);
    paging.set_label_size(theme::UI_SIZE);
    paging.set_label(&format!("{rows} files a page"));
    let mut cancel_read = controls::button(W - m - 3 * bw - 230, py, bw + 20, bh, "Cancel Reading");
    cancel_read.emit_to(tx, Cmd::CancelRead);
    cancel_read.hide();
    let mut ar = controls::label(W - m - 110 - 82, py, 82, bh, "Auto-refresh:");
    ar.set_align(Align::Right | Align::Inside);
    let mut auto_refresh = controls::combo(W - m - 110, py, 110, bh);
    for s in qsa_config::settings::AUTO_REFRESH {
        auto_refresh.add_choice(&auto_refresh_label(s));
    }
    let at = qsa_config::settings::AUTO_REFRESH
        .iter()
        .position(|s| *s == refresh_secs)
        .unwrap_or(2);
    auto_refresh.set_value(at as i32);
    auto_refresh.set_callback(move |_| tx.send(Cmd::AutoRefresh));
    let auto_refresh_switch = controls::Switch::for_choice(&mut auto_refresh);

    let status = StatusBar::new(0, H - StatusBar::HEIGHT, W);
    window.resizable(list.table());
    window.end();
    window.set_callback(move |_| tx.send(Cmd::Close));
    (
        Widgets {
            window,
            menu,
            user,
            outq,
            status_filter,
            form_type,
            user_data,
            list,
            paging,
            previous,
            next,
            auto_refresh,
            auto_refresh_switch,
            cancel_read,
            status,
        },
        rx,
    )
}

impl PrinterOutput {
    /// The window for `system` with `settings`, not shown and not yet
    /// listing (screenshots use it so).
    pub fn build(
        system: &System,
        settings: &qsa_config::settings::PrinterOutput,
    ) -> Rc<PrinterOutput> {
        Rc::new(PrinterOutput::new(system, settings, None))
    }

    fn new(
        system: &System,
        settings: &qsa_config::settings::PrinterOutput,
        state: Option<Shared>,
    ) -> PrinterOutput {
        let rows = settings.rows();
        let (w, rx) = build_widgets(&system.name, rows, settings.refresh_secs());
        PrinterOutput {
            w,
            rx,
            state,
            selected_ids: RefCell::new(Vec::new()),
            st: RefCell::new(St {
                system: system.clone(),
                handle: None,
                filter: Filter::default(),
                paging: Paging::new(rows),
                files: Vec::new(),
                sort: None,
                listing: None,
                again: false,
                last_list: Instant::now(),
                queues: Vec::new(),
                queues_rx: None,
                acting: None,
                reads: Vec::new(),
                scs: model::scs_options(37, settings.paper),
                watch: Watching::Polling,
                relist_at: None,
            }),
        }
    }

    /// Shows `files` as page one of `total` with `filter` and the status
    /// line a listing leaves, without a system (screenshots).
    pub fn show_files(
        &self,
        filter: &Filter,
        files: Vec<SpooledFile>,
        total: u32,
        created: &dyn Fn(i64) -> String,
    ) {
        self.set_filter(filter);
        {
            let mut st = self.st.borrow_mut();
            st.filter = filter.clone();
            st.paging.update(total);
            st.files = files;
        }
        self.fill(created);
    }

    /// The system's name.
    pub fn system(&self) -> String {
        self.st.borrow().system.name.clone()
    }

    fn set_filter(&self, f: &Filter) {
        let w = &self.w;
        w.user.clone().set_value(&f.user);
        w.outq.clone().set_value(&f.outq);
        let at = f
            .status
            .and_then(|s| model::STATUSES.iter().position(|x| *x == s))
            .map_or(0, |i| i + 1);
        w.status_filter.clone().set_value(at as i32);
        w.form_type.clone().set_value(&f.form_type);
        w.user_data.clone().set_value(&f.user_data);
    }

    fn read_filter(&self) -> Filter {
        let w = &self.w;
        let status = usize::try_from(w.status_filter.value())
            .ok()
            .and_then(|i| i.checked_sub(1))
            .and_then(|i| model::STATUSES.get(i).copied());
        Filter {
            user: w.user.value().unwrap_or_default().trim().to_owned(),
            outq: w.outq.value().unwrap_or_default().trim().to_owned(),
            status,
            form_type: w.form_type.value().trim().to_owned(),
            user_data: w.user_data.value(),
        }
    }

    /// The rows from the page kept, keeping the selection (by file) and
    /// the scroll position.
    fn fill(&self, created: &dyn Fn(i64) -> String) {
        let st = self.st.borrow();
        let mut list = self.w.list.clone();
        let mut table = list.table().clone();
        let top = table.row_position();
        let before = &*self.selected_ids.borrow();
        let rows = st.files.iter().map(|f| model::row(f, created)).collect();
        list.set_rows(rows);
        let keep: Vec<usize> = st
            .files
            .iter()
            .enumerate()
            .filter(|(_, f)| before.contains(&f.id))
            .map(|(i, _)| i)
            .collect();
        list.select_rows(&keep);
        if !st.files.is_empty() {
            table.set_row_position(top.clamp(0, st.files.len() as i32 - 1));
        }
        list.set_sort(st.sort);
        let mut paging = self.w.paging.clone();
        paging.set_label(&model::paging_line(&st.paging, st.files.len()));
        let mut p = self.w.previous.clone();
        let mut n = self.w.next.clone();
        if st.paging.has_previous() {
            p.activate();
        } else {
            p.deactivate();
        }
        if st.paging.has_next() {
            n.activate();
        } else {
            n.deactivate();
        }
        let sorted = st.sort.map(|(c, _)| model::COLUMNS[c].0);
        let line = model::list_line(st.paging.total, &st.system.name, sorted);
        drop(st);
        if self.reading().is_none() {
            self.w.status.clone().set_message(&line);
        }
        self.window().clone().redraw();
    }

    pub fn window(&self) -> &Window {
        &self.w.window
    }

    /// The files selected, in the order shown.
    fn selected(&self) -> Vec<SpooledFile> {
        let st = self.st.borrow();
        self.w
            .list
            .selected_rows()
            .into_iter()
            .filter_map(|i| st.files.get(i).cloned())
            .collect()
    }

    /// The file the single-file actions act on: the first selected.
    fn current(&self) -> Option<SpooledFile> {
        self.selected().into_iter().next()
    }

    fn report(&self, text: &str) {
        crate::log!("{}: {text}", self.system());
        self.w.status.clone().set_message(text);
    }

    fn parent(&self) -> Option<&dyn WindowExt> {
        Some(&self.w.window as &dyn WindowExt)
    }

    fn handle(&self) -> Option<Handle> {
        self.st.borrow().handle.clone()
    }

    /// Asks for the page in view, unless a request is in flight (then once
    /// it is done).
    pub fn list(&self) {
        let Some(h) = self.handle() else { return };
        let mut st = self.st.borrow_mut();
        if st.listing.is_some() {
            st.again = true;
            return;
        }
        let req = st.filter.request(st.paging.start, st.paging.per_page);
        st.last_list = Instant::now();
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(qap::call_unlocked(&h, Op::ListSpooledFiles, &req));
            app::awake();
        });
        st.listing = Some(Listing { rx });
    }

    /// Asks for every output queue, for the pickers.
    fn list_queues(&self) {
        let Some(h) = self.handle() else { return };
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let mut all = Vec::new();
            let mut req = ListOutputQueues {
                library: "*ALL".into(),
                start: 0,
                max: 500,
            };
            let r = loop {
                match qap::call_unlocked::<_, OutputQueues>(&h, Op::ListOutputQueues, &req) {
                    Ok(page) => {
                        let n = page.queues.len();
                        all.extend(page.queues);
                        if n == 0 || all.len() >= page.total as usize {
                            break Ok(all);
                        }
                        req.start = all.len() as u32;
                    }
                    Err(e) => break Err(e),
                }
            };
            let _ = tx.send(r);
            app::awake();
        });
        self.st.borrow_mut().queues_rx = Some(rx);
    }

    fn reading(&self) -> Option<String> {
        let st = self.st.borrow();
        st.reads.first().map(|(r, _)| r.line())
    }

    /// Gives the window its waiting work; false once it is closed.
    fn pump(&self) -> bool {
        while let Some(c) = self.rx.recv() {
            self.command(c);
        }
        self.take_listing();
        self.take_queues();
        self.take_actions();
        self.take_reads();
        self.take_watch();
        self.relist_when_due();
        self.auto_refresh();
        self.w.window.shown()
    }

    fn take_listing(&self) {
        let r = {
            let st = self.st.borrow();
            match st.listing.as_ref().map(|l| l.rx.try_recv()) {
                Some(Ok(r)) => r,
                Some(Err(mpsc::TryRecvError::Empty)) | None => return,
                Some(Err(mpsc::TryRecvError::Disconnected)) => {
                    Err(Error::Closed(qap_client::Closed::ByClient))
                }
            }
        };
        let again = {
            let mut st = self.st.borrow_mut();
            st.listing = None;
            std::mem::take(&mut st.again)
        };
        match r {
            Ok(page) => {
                let moved = {
                    let mut st = self.st.borrow_mut();
                    let moved = st.paging.update(page.total);
                    let mut files = page.files;
                    if let Some((c, asc)) = st.sort {
                        model::sort_page(&mut files, c, asc);
                    }
                    st.files = files;
                    moved
                };
                self.fill(&model::local_time);
                if moved || again {
                    self.list();
                }
            }
            Err(e) => {
                let sys = self.st.borrow().system.clone();
                let x = qap::explain(&sys, &e);
                self.report(&x.line());
                if again {
                    self.list();
                }
            }
        }
    }

    fn take_queues(&self) {
        let r = {
            let st = self.st.borrow();
            match st.queues_rx.as_ref().map(mpsc::Receiver::try_recv) {
                Some(Ok(r)) => r,
                _ => return,
            }
        };
        self.st.borrow_mut().queues_rx = None;
        match r {
            Ok(queues) => {
                let mut outq = self.w.outq.clone();
                let typed = outq.value().unwrap_or_default();
                outq.clear();
                outq.add("*ALL");
                for q in model::queue_choices(&queues) {
                    outq.add(&q.replace('/', "\\/"));
                }
                outq.set_value(&typed);
                self.st.borrow_mut().queues = queues;
            }
            Err(e) => crate::log!("output queues of {}: {e}", self.system()),
        }
    }

    fn refresh_secs(&self) -> u32 {
        usize::try_from(self.w.auto_refresh.value())
            .ok()
            .and_then(|i| qsa_config::settings::AUTO_REFRESH.get(i).copied())
            .unwrap_or(0)
    }

    fn auto_refresh(&self) {
        // A watch that works says what changes; the timer waits.
        if matches!(self.st.borrow().watch, Watching::Live(_)) {
            return;
        }
        let secs = self.refresh_secs();
        let w = &self.w.window;
        // Shown and not minimised.
        if secs == 0 || !w.shown() || !w.visible() {
            return;
        }
        let due = {
            let st = self.st.borrow();
            st.listing.is_none() && st.last_list.elapsed() >= Duration::from_secs(u64::from(secs))
        };
        if due {
            self.keep_selection();
            self.list();
        }
    }

    /// Notes the selected files, so a refresh keeps them selected.
    fn keep_selection(&self) {
        let ids: Vec<SpooledFileId> = self.selected().into_iter().map(|f| f.id).collect();
        *self.selected_ids.borrow_mut() = ids;
    }

    /// Watches what the filter lists (version 6), off the GUI thread, and
    /// stops watching what it listed before; or polls when the filter
    /// cannot be watched.
    fn watch_filter(&self) {
        let Some(h) = self.handle() else { return };
        let target = self.st.borrow().filter.target();
        let before = self.st.borrow().watch.target().cloned();
        if target == before {
            return;
        }
        if let Some(old) = before {
            self.unwatch(&h, old);
        }
        let Some(t) = target else {
            self.st.borrow_mut().watch = Watching::Polling;
            self.show_mode();
            return;
        };
        let (tx, rx) = mpsc::channel();
        let w = Watch::Spool(t.clone());
        std::thread::spawn(move || {
            let _ = tx.send(lock(&h).watch(w));
            app::awake();
        });
        self.st.borrow_mut().watch = Watching::Asking(t, rx);
    }

    /// Stops watching `t` unless another Printer Output window of the
    /// system watches it too.
    fn unwatch(&self, h: &Handle, t: SpoolTarget) {
        let system = self.system();
        let shared = WINDOWS.with(|v| {
            v.borrow().iter().any(|p| {
                !std::ptr::eq(&**p, self)
                    && p.w.window.shown()
                    && p.system().eq_ignore_ascii_case(&system)
                    && p.st.borrow().watch.target() == Some(&t)
            })
        });
        if shared {
            return;
        }
        let h = h.clone();
        std::thread::spawn(move || {
            if let Err(e) = lock(&h).unwatch(&Watch::Spool(t)) {
                crate::log!("{system}: no longer watching spooled files: {e}");
            }
        });
    }

    fn take_watch(&self) {
        let r = {
            let st = self.st.borrow();
            match &st.watch {
                Watching::Asking(t, rx) => match rx.try_recv() {
                    Ok(r) => Some((t.clone(), r)),
                    Err(mpsc::TryRecvError::Empty) => None,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        Some((t.clone(), Err(Error::Closed(qap_client::Closed::ByClient))))
                    }
                },
                _ => None,
            }
        };
        let Some((t, r)) = r else { return };
        match r {
            Ok(()) => {
                self.st.borrow_mut().watch = Watching::Live(t);
                // What changed while the watch was asked for.
                self.keep_selection();
                self.list();
            }
            Err(e) => {
                // Before version 6 this is no failure: the list polls, as
                // it always did.
                if !matches!(e, Error::Unsupported { .. }) {
                    self.watch_failed(&t, &e.to_string());
                }
                self.st.borrow_mut().watch = Watching::Polling;
            }
        }
        self.show_mode();
    }

    /// `QAD9011` for a spool watch of this window's.
    fn watch_failed(&self, t: &SpoolTarget, why: &str) {
        let me = self.me();
        let name = crate::messages::watching::watch_name(&Watch::Spool(t.clone()), &me);
        let m = qsa_msg::message(
            id::WATCH_STOPPED,
            &[&name, &self.system(), why.trim_end_matches('.')],
        );
        crate::log!("{m}");
    }

    /// The signed-on profile, for `*CURRENT`.
    fn me(&self) -> String {
        self.state
            .as_ref()
            .and_then(|s| crate::qap::signed_on(s, &self.system()))
            .unwrap_or_default()
    }

    /// Says how the list keeps up: `QAD9020` while watched, with
    /// Auto-refresh set aside; `QAD9021` while the timer refreshes it.
    fn show_mode(&self) {
        let live = matches!(self.st.borrow().watch, Watching::Live(_));
        let mut ar = self.w.auto_refresh.clone();
        self.w.auto_refresh_switch.set_enabled(!live);
        let m = if live {
            Some(qsa_msg::message(id::SPOOL_LIVE, &[]))
        } else {
            let secs = self.refresh_secs();
            (secs > 0).then(|| qsa_msg::message(id::SPOOL_POLLING, &[&secs.to_string()]))
        };
        ar.set_tooltip(&m.as_ref().map(|m| m.text.clone()).unwrap_or_default());
        if let Some(m) = m {
            self.report(&m.to_string());
        }
    }

    /// Shows the window as a watch keeps it, without a system
    /// (screenshots).
    pub fn show_live(&self, target: SpoolTarget) {
        self.st.borrow_mut().watch = Watching::Live(target);
        self.show_mode();
    }

    /// Asks for the page again once a burst of changes is over.
    fn relist_when_due(&self) {
        let due = self
            .st
            .borrow()
            .relist_at
            .is_some_and(|at| Instant::now() >= at);
        if due {
            self.st.borrow_mut().relist_at = None;
            self.keep_selection();
            self.list();
        }
    }

    /// What the system's watches reported, for this window.
    fn watch_event(&self, e: &WatchEvent) {
        let target = self.st.borrow().watch.target().cloned();
        let live = matches!(self.st.borrow().watch, Watching::Live(_));
        match e {
            WatchEvent::SpoolChanged(c) if live && Some(&c.target) == target.as_ref() => {
                let me = self.me();
                self.keep_selection();
                let effect = {
                    let mut st = self.st.borrow_mut();
                    let St {
                        filter,
                        files,
                        paging,
                        ..
                    } = &mut *st;
                    model::apply(filter, &me, files, &mut paging.total, c)
                };
                match effect {
                    model::Effect::Nothing => {}
                    model::Effect::Shown => self.fill(&model::local_time),
                    model::Effect::Relist => {
                        self.fill(&model::local_time);
                        // One list for a burst of changes.
                        let mut st = self.st.borrow_mut();
                        st.relist_at.get_or_insert(Instant::now() + RELIST_AFTER);
                    }
                }
            }
            // Watched again after a reconnect: what changed meanwhile was
            // not reported, so the page is asked for once.
            WatchEvent::SpoolRewatched(t) if live && Some(t) == target.as_ref() => {
                self.keep_selection();
                self.list();
            }
            WatchEvent::Failed {
                watch: Watch::Spool(t),
                failure,
            } if Some(t) == target.as_ref() => {
                self.watch_failed(t, &failure.text);
                self.st.borrow_mut().watch = Watching::Polling;
                self.show_mode();
            }
            _ => {}
        }
    }

    /// The window is closing: its watch goes unless another window shares
    /// it.
    fn closing(&self) {
        let t = self.st.borrow().watch.target().cloned();
        if let (Some(t), Some(h)) = (t, self.handle()) {
            self.st.borrow_mut().watch = Watching::Polling;
            self.unwatch(&h, t);
        }
    }

    fn command(&self, c: Cmd) {
        match c {
            Cmd::Go => {
                let f = self.read_filter();
                {
                    let mut st = self.st.borrow_mut();
                    st.filter = f;
                    st.paging.start = 0;
                    st.sort = None;
                }
                self.selected_ids.borrow_mut().clear();
                self.list();
                self.watch_filter();
            }
            Cmd::Refresh => {
                self.keep_selection();
                self.list();
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
                    self.selected_ids.borrow_mut().clear();
                    self.w.list.table().clone().set_row_position(0);
                    self.list();
                }
            }
            Cmd::Sort(col) => {
                self.keep_selection();
                {
                    let mut st = self.st.borrow_mut();
                    let asc = match st.sort {
                        Some((c, asc)) if c == col => !asc,
                        _ => true,
                    };
                    st.sort = Some((col, asc));
                    model::sort_page(&mut st.files, col, asc);
                }
                self.fill(&model::local_time);
            }
            Cmd::Open => self.read(Purpose::Open),
            Cmd::SavePdf => self.save(true),
            Cmd::SaveText => self.save(false),
            Cmd::Print => self.print(),
            Cmd::Hold => {
                let files = self.selected();
                if files.is_empty() {
                    return;
                }
                if let Some(immediately) = dialogs::ask_hold(self.parent(), &files) {
                    self.act(files, |f| Action::Hold {
                        id: f.id.clone(),
                        immediately,
                    });
                }
            }
            Cmd::Release => {
                let files = self.selected();
                self.act(files, |f| Action::Release(f.id.clone()));
            }
            Cmd::Delete => {
                let files = self.selected();
                if !files.is_empty() && dialogs::ask_delete(self.parent(), &files) {
                    self.act(files, |f| Action::Delete(f.id.clone()));
                }
            }
            Cmd::Move => {
                let files = self.selected();
                if files.is_empty() {
                    return;
                }
                let (queues, current) = {
                    let st = self.st.borrow();
                    let current = model::object_text(&files[0].outq);
                    (st.queues.clone(), current)
                };
                if let Some((outq, first)) =
                    dialogs::ask_move(self.parent(), &files, &queues, &current)
                {
                    self.act(files, |f| Action::Move {
                        id: f.id.clone(),
                        outq: outq.clone(),
                        first,
                    });
                }
            }
            Cmd::Properties => {
                if let Some(f) = self.current() {
                    dialogs::show_properties(self.parent(), &f);
                }
            }
            Cmd::OutputQueues => {
                if let Some(state) = &self.state {
                    let system = self.system();
                    queues::open(self.parent(), state, &system);
                }
            }
            Cmd::AutoRefresh => {
                let secs = usize::try_from(self.w.auto_refresh.value())
                    .ok()
                    .and_then(|i| qsa_config::settings::AUTO_REFRESH.get(i).copied())
                    .unwrap_or(0);
                if let Some(state) = &self.state {
                    let mut s = state.borrow_mut();
                    s.settings.printer_output.auto_refresh_secs = secs;
                    if let Err(e) = s.save_settings() {
                        crate::log!("settings not saved: {e}");
                    }
                }
                self.st.borrow_mut().last_list = Instant::now();
                self.show_mode();
            }
            Cmd::CancelRead => {
                for (r, _) in &self.st.borrow().reads {
                    r.cancel();
                }
            }
            Cmd::Close => {
                for (r, _) in &self.st.borrow().reads {
                    r.cancel();
                }
                self.w.window.clone().hide();
            }
            Cmd::About => crate::about::show(self.parent()),
            Cmd::HelpTopics => crate::help::show(crate::help::topic::PRINTER_OUTPUT),
        }
    }

    /// Runs `make` for each of `files`, one request each, side by side.
    fn act(&self, files: Vec<SpooledFile>, make: impl Fn(&SpooledFile) -> Action) {
        let Some(h) = self.handle() else { return };
        if files.is_empty() || self.st.borrow().acting.is_some() {
            return;
        }
        let work: Vec<(SpooledFile, Action)> = files
            .into_iter()
            .map(|f| {
                let a = make(&f);
                (f, a)
            })
            .collect();
        let sys = self.st.borrow().system.clone();
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let sent: Vec<(SpooledFile, Action, Result<qap_client::Pending, Error>)> = work
                .into_iter()
                .map(|(f, a)| {
                    let p = lock(&h).spool_action(&a);
                    (f, a, p)
                })
                .collect();
            let timeout = lock(&h).target().options.reply_timeout;
            let done = sent
                .into_iter()
                .map(|(file, action, p)| {
                    let r = p.and_then(|p| p.wait(timeout)).and_then(Action::outcome);
                    let outcome = match r {
                        Ok(()) => Outcome::Done,
                        Err(e) => {
                            let x = qap::explain(&sys, &e);
                            Outcome::Refused {
                                line: x.line(),
                                server: x.server,
                            }
                        }
                    };
                    Done {
                        file,
                        action,
                        outcome,
                    }
                })
                .collect();
            let _ = tx.send(done);
            app::awake();
        });
        self.st.borrow_mut().acting = Some(Acting { rx });
    }

    fn take_actions(&self) {
        let done = {
            let st = self.st.borrow();
            match st.acting.as_ref().map(|a| a.rx.try_recv()) {
                Some(Ok(d)) => d,
                _ => return,
            }
        };
        self.st.borrow_mut().acting = None;
        for d in &done {
            match &d.outcome {
                Outcome::Done => crate::log!("{}", model::done_message(&d.action)),
                Outcome::Refused { line, .. } => crate::log!("{}: {line}", d.file.id.file),
            }
        }
        match &done[..] {
            [] => {}
            [one] => match &one.outcome {
                Outcome::Done => self.report(&model::done_message(&one.action).to_string()),
                Outcome::Refused { server, line } => {
                    self.report(line);
                    match server {
                        Some(m) => {
                            msgdetails::show_system_message(self.parent(), None, &qap::shown(m))
                        }
                        None => msgdetails::show_message(
                            self.parent(),
                            &qsa_msg::message(id::UNEXPECTED_REPLY, &["QAP", &self.system(), line]),
                        ),
                    }
                }
            },
            many => {
                self.report(&model::summary_heading(many).0);
                dialogs::show_summary(self.parent(), many);
            }
        }
        self.keep_selection();
        self.list();
    }

    /// Starts reading the current file for `purpose`.
    fn read(&self, purpose: Purpose) {
        let Some(f) = self.current() else { return };
        let Some(h) = self.handle() else { return };
        let format = purpose.format(&f);
        match Reading::start(&h, f, format) {
            Ok(r)
                if purpose == Purpose::Open
                    && format == qap_client::proto::ops::spool::ReadFormat::Scs =>
            {
                let (sys, options) = {
                    let st = self.st.borrow();
                    (st.system.clone(), st.scs.clone())
                };
                viewer::open(self.parent(), &sys, options, r);
            }
            Ok(r) => {
                self.report(&r.line());
                self.st.borrow_mut().reads.push((r, purpose));
                self.w.cancel_read.clone().show();
            }
            Err(e) => self.report(&format!("Reading not started: {e}")),
        }
    }

    fn save(&self, pdf: bool) {
        let Some(f) = self.current() else { return };
        let (title, pattern, ext) = if pdf {
            ("Save as PDF", "PDF Files (*.pdf)\tAll Files (*)", "pdf")
        } else {
            ("Save as Text", "Text Files (*.txt)\tAll Files (*)", "txt")
        };
        let Some(path) = crate::files::choose_save(title, pattern, &model::save_name(&f.id, ext))
        else {
            return;
        };
        let path = crate::files::with_extension(path, ext);
        self.read(if pdf {
            Purpose::SavePdf(path)
        } else {
            Purpose::SaveText(path)
        });
    }

    fn print(&self) {
        let Some(f) = self.current() else { return };
        let (all, default) = match output::printers() {
            Ok(p) => p,
            Err(why) => {
                let m = qsa_msg::message(id::SYSTEM_PRINTING_UNAVAILABLE, &[&why]);
                self.report(&m.to_string());
                msgdetails::show_message(self.parent(), &m);
                return;
            }
        };
        if all.is_empty() {
            let m = qsa_msg::message(
                id::SYSTEM_PRINTING_UNAVAILABLE,
                &["no printer is set up on this computer"],
            );
            self.report(&m.to_string());
            msgdetails::show_message(self.parent(), &m);
            return;
        }
        if let Some(p) = dialogs::ask_print(self.parent(), &f, &all, default.as_deref()) {
            self.read(Purpose::Print(p));
        }
    }

    fn take_reads(&self) {
        let mut finished = Vec::new();
        {
            let mut st = self.st.borrow_mut();
            let mut i = 0;
            while i < st.reads.len() {
                let events = st.reads[i].0.poll();
                let end = events.into_iter().find_map(|e| match e {
                    ReadEvent::Done(r) => Some(r),
                    ReadEvent::Progress(_) => None,
                });
                match end {
                    Some(r) => {
                        let (reading, purpose) = st.reads.remove(i);
                        finished.push((reading, purpose, r));
                    }
                    None => i += 1,
                }
            }
        }
        if let Some(line) = self.reading() {
            self.w.status.clone().set_message(&line);
        } else {
            self.w.cancel_read.clone().hide();
        }
        for (reading, purpose, r) in finished {
            self.finish_read(&reading, &purpose, r);
        }
    }

    fn finish_read(&self, r: &Reading, purpose: &Purpose, end: Result<ReadEnd, Error>) {
        let (sys, options) = {
            let st = self.st.borrow();
            (st.system.clone(), st.scs.clone())
        };
        match end {
            Ok(ReadEnd::Done { .. }) => match output::finish(r, purpose, &options) {
                Ok(Some(m)) => self.report(&m.to_string()),
                Ok(None) => self.report(&format!("{} read.", r.file.id.file)),
                Err(why) => {
                    let m = match purpose {
                        Purpose::Print(p) => qsa_msg::message(id::PRINTER_ERROR, &[p, &why]),
                        _ => qsa_msg::message(id::OUTPUT_FOLDER_NOT_WRITABLE, &[&why]),
                    };
                    self.report(&m.to_string());
                    msgdetails::show_message(self.parent(), &m);
                }
            },
            Ok(ReadEnd::Cancelled(_)) => {
                let m = qsa_msg::message(id::SPOOL_READ_CANCELLED, &[&r.file.id.file]);
                self.report(&m.to_string());
            }
            Err(e) => {
                let x = output::explain_read(&sys, &r.file, r.format, &e);
                self.report(&x.line());
                qap::show(self.parent(), &x);
            }
        }
    }
}

/// Opens Printer Output for `system`, signing on first when this program
/// has no session on it; on `outq` when given (from Output Queues). An
/// open window for the system comes to the front instead.
pub fn open(
    parent: Option<&dyn WindowExt>,
    state: &Shared,
    system: &str,
    outq: Option<&qap_client::proto::types::ObjectName>,
) {
    let existing = WINDOWS.with(|v| {
        v.borrow()
            .iter()
            .find(|p| p.w.window.shown() && p.system().eq_ignore_ascii_case(system))
            .cloned()
    });
    if let Some(p) = existing {
        if let Some(q) = outq {
            p.set_filter(&Filter::on_queue(q));
            p.command(Cmd::Go);
        }
        p.w.window.clone().show();
        return;
    }
    let Some(sys) = state.borrow().systems.get(system).cloned() else {
        return;
    };
    let Some(h) = qap::session(parent, state, &sys.name) else {
        return;
    };
    // Version 3's Spool service, or QAD5001.
    let unsupported = h.try_lock().ok().and_then(|mut c| {
        let has = c.version().unwrap_or(1);
        c.supports(Op::ListSpooledFiles).err().map(|_| has)
    });
    if let Some(has) = unsupported {
        let m = qap::version_needed("Printer Output", &sys.name, Op::ListSpooledFiles, has);
        crate::log!("{m}");
        msgdetails::show_message(parent, &m);
        return;
    }
    let settings = state.borrow().settings.printer_output.clone();
    let ccsid = h
        .try_lock()
        .ok()
        .and_then(|c| c.hello().map(|h| h.system_ccsid))
        .unwrap_or(37);
    let p = Rc::new(PrinterOutput::new(&sys, &settings, Some(state.clone())));
    {
        let mut st = p.st.borrow_mut();
        st.handle = Some(h);
        st.scs = model::scs_options(ccsid, settings.paper);
        if let Some(q) = outq {
            st.filter = Filter::on_queue(q);
        }
    }
    let f = p.st.borrow().filter.clone();
    p.set_filter(&f);
    let mut win = p.w.window.clone();
    qsa_ui::propdialog::center_on(&mut win, parent);
    qsa_ui::propdialog::show_placed(&mut win);
    p.w.status.clone().set_status(
        &format!(
            "{}: {}",
            sys.name,
            qap::signed_on(state, &sys.name).unwrap_or_default()
        ),
        Status::Active,
    );
    p.list();
    p.watch_filter();
    p.list_queues();
    WINDOWS.with(|v| v.borrow_mut().push(p));
    tick();
}

/// The repeating look at every window's workers.
fn tick() {
    if TICKING.with(|t| t.replace(true)) {
        return;
    }
    app::add_timeout3(POLL, |h| {
        pump();
        if any_open() {
            app::repeat_timeout3(POLL, h);
        } else {
            TICKING.with(|t| t.set(false));
        }
    });
}

/// Gives every Printer Output window its waiting work and forgets the
/// closed ones. Not again while it runs (a dialog it shows waits inside).
pub fn pump() {
    if PUMPING.with(|p| p.replace(true)) {
        return;
    }
    let windows: Vec<Rc<PrinterOutput>> = WINDOWS.with(|v| v.borrow().clone());
    let open: Vec<bool> = windows.iter().map(|p| p.pump()).collect();
    for (p, open) in windows.iter().zip(&open) {
        if !open {
            p.closing();
        }
    }
    queues::pump();
    viewer::pump();
    WINDOWS.with(|v| {
        let mut v = v.borrow_mut();
        let mut i = 0;
        v.retain(|_| {
            let keep = open.get(i).copied().unwrap_or(true);
            i += 1;
            keep
        });
    });
    PUMPING.with(|p| p.set(false));
}

/// What `system`'s watches reported, for its Printer Output windows.
pub fn watch_events(system: &str, events: &[WatchEvent]) {
    let windows: Vec<Rc<PrinterOutput>> = WINDOWS.with(|v| {
        v.borrow()
            .iter()
            .filter(|p| p.w.window.shown() && p.system().eq_ignore_ascii_case(system))
            .cloned()
            .collect()
    });
    for p in windows {
        for e in events {
            p.watch_event(e);
        }
    }
}

/// Whether any Printer Output or Output Queues window is open.
pub fn any_open() -> bool {
    WINDOWS.with(|v| v.borrow().iter().any(|p| p.w.window.shown()))
        || queues::any_open()
        || viewer::any_open()
}
