// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! The 5250 Printer window: menu bar, toolbar, the session's state, the
//! Received Output list and a status bar.
//!
//! The network and the output run on a worker thread ([`super::worker`]);
//! this window shows what happened and acts on the list. A session that
//! drops connects again by itself, after 5 seconds, then twice as long each
//! time up to 5 minutes, so that it can run unattended for days.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant};

use fltk::app;
use fltk::enums::{Align, Key, Shortcut};
use fltk::frame::Frame;
use fltk::menu::{MenuBar, MenuFlag, MenuItem};
use fltk::prelude::*;
use fltk::window::Window;
use qsa_config::AddressLookup;
use qsa_config::qsp::{self, PrinterFile, Target as OutTarget};
use qsa_config::settings::Geometry;
use qsa_msg::{Message, id};
use qsa_net::{TlsError, TlsOptions};
use qsa_ui::chan::{self, MenuEmit};
use qsa_ui::glyph::{self, Glyph, Size};
use qsa_ui::listtable::{Cell, ListTable};
use qsa_ui::sampler::{MENU_H, menu_bar};
use qsa_ui::theme::{self, colors};
use qsa_ui::{MessageBox, Status, StatusBar, Toolbar, msgdetails};
use tn5250::negotiate::{Mode, Outcome};
use tn5250::printer::{FormFeed, Options as PrinterOptions, Paper as SourcePaper};
use tn5250::session::Target;
use tn5250::telnet::{CONNECT_TIMEOUT, ConnectError, Security};

use super::spool::{self, JobRecord, JobStatus, Spooler};
use super::worker::{FromNet, PrinterWorker, ToNet};
use crate::app::Shared;
use crate::session::worker::Ended;
use crate::trust;
use crate::{files, shell};

pub const W: i32 = 680;
pub const H: i32 = 420;

/// Height of the strip that shows the session's state.
const INFO_H: i32 = 26;

/// The first wait before connecting again, and the longest.
pub const RETRY_FIRST: Duration = Duration::from_secs(5);
pub const RETRY_MAX: Duration = Duration::from_secs(300);

/// What menus, toolbar and the list ask for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Cmd {
    Save,
    SaveAs,
    Settings,
    OpenFolder,
    TestPage,
    Close,
    Open,
    ShowInFolder,
    PrintAgain,
    Delete,
    Connect,
    Disconnect,
    About,
    /// Help > Help Topics.
    HelpTopics,
}

/// The session's state, as the status area shows it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    /// Never connected.
    Idle,
    Connecting,
    /// Connected; waiting for output.
    Ready,
    /// A job is arriving.
    Receiving,
    /// A job is being rendered or printed.
    Printing,
    /// Output failed; the system was told the printer is not ready.
    Error,
    Disconnected,
    /// Waiting to connect again.
    Reconnecting,
}

impl State {
    pub fn text(self) -> &'static str {
        match self {
            State::Idle | State::Disconnected => "Disconnected",
            State::Connecting => "Connecting",
            State::Ready => "Ready",
            State::Receiving => "Receiving",
            State::Printing => "Printing",
            State::Error => "Error",
            State::Reconnecting => "Reconnecting",
        }
    }

    fn status(self) -> Status {
        match self {
            State::Ready | State::Receiving | State::Printing => Status::Active,
            State::Error | State::Disconnected | State::Idle => Status::Error,
            State::Connecting | State::Reconnecting => Status::Neutral,
        }
    }
}

struct St {
    doc: PrinterFile,
    file: Option<PathBuf>,
    worker: Option<PrinterWorker>,
    state: State,
    security: Security,
    jobs: Vec<JobRecord>,
    /// The next wait before connecting again, and when that is due.
    backoff: Duration,
    retry_at: Option<Instant>,
    /// The system was told the printer is not ready.
    not_ready: bool,
    /// Renders the test page and prints jobs again, on this thread.
    spooler: Spooler,
}

/// One printer session window.
pub struct PrinterWindow {
    pub window: Window,
    menu: MenuBar,
    table: ListTable,
    status: StatusBar,
    system_label: Frame,
    device_label: Frame,
    state_label: Frame,
    lock: Frame,
    rx: chan::Receiver<Cmd>,
    state: Shared,
    st: RefCell<St>,
}

/// `<title> - 5250 Printer`.
fn window_title(doc: &PrinterFile, file: Option<&Path>) -> String {
    format!("{} - 5250 Printer", doc.display_title(file))
}

/// `12 KB` and the like.
fn size_text(bytes: u64) -> String {
    match bytes {
        0..1024 => format!("{bytes} bytes"),
        1024..1_048_576 => format!("{} KB", bytes.div_ceil(1024)),
        _ => format!("{:.1} MB", bytes as f64 / 1_048_576.0),
    }
}

fn target_text(t: OutTarget) -> &'static str {
    match t {
        OutTarget::Pdf => "PDF",
        OutTarget::Printer => "Printer",
        OutTarget::Both => "PDF and printer",
    }
}

/// A Received Output row.
fn row(r: &JobRecord) -> Vec<Cell> {
    let status = match r.status {
        JobStatus::Done => Status::Neutral,
        JobStatus::Failed => Status::Error,
        JobStatus::Discarded => Status::Neutral,
    };
    let output = r
        .pdf
        .as_ref()
        .and_then(|p| p.file_name())
        .map(|n| n.to_string_lossy().into_owned())
        .or_else(|| r.printer.clone())
        .unwrap_or_default();
    vec![
        crate::localtime::format(r.time).into(),
        r.pages.to_string().into(),
        size_text(r.bytes).into(),
        target_text(r.target).into(),
        Cell {
            text: r.status_text(),
            status,
        },
        output.into(),
    ]
}

/// Printer session options from the session file.
fn printer_options(doc: &PrinterFile) -> PrinterOptions {
    let (msgq, lib) = doc.message_queue();
    let mut o = PrinterOptions::new(&doc.printer.device_name, doc.printer.ccsid);
    o.message_queue = msgq;
    o.message_queue_library = lib;
    o.font = doc.printer.font;
    o.form_feed = FormFeed::Continuous;
    o.paper_source_1 = match doc.output.paper {
        qsp::Paper::Letter => SourcePaper::Letter,
        qsp::Paper::A4 => SourcePaper::A4,
        qsp::Paper::Legal => SourcePaper::Legal,
    };
    o
}

impl PrinterWindow {
    /// Builds the window for a session without connecting.
    pub fn build(state: &Shared, doc: PrinterFile, file: Option<PathBuf>) -> Rc<PrinterWindow> {
        let (tx, rx) = chan::channel::<Cmd>();
        let (w, h) = doc
            .session
            .window
            .map_or((W, H), |g| (g.w.max(480), g.h.max(300)));
        let mut window = Window::new(0, 0, w, h, None);
        window.set_label(&window_title(&doc, file.as_deref()));
        window.set_icon(Some(qsa_ui::glyph::app_icon()));
        window.set_color(colors::FACE);
        window.size_range(480, 300, 0, 0);

        let mut menu = menu_bar(0, 0, w);
        let mut add = |path: &str, sc: Shortcut, c: Cmd, flag: MenuFlag| {
            menu.add_emit_to(path, sc, flag, tx, c);
        };
        let n = MenuFlag::Normal;
        add("&File/&Save Session", Shortcut::Ctrl | 's', Cmd::Save, n);
        add(
            "&File/Save Session &As...",
            Shortcut::None,
            Cmd::SaveAs,
            MenuFlag::MenuDivider,
        );
        add("&File/Se&ttings...", Shortcut::None, Cmd::Settings, n);
        add(
            "&File/Open Output &Folder",
            Shortcut::None,
            Cmd::OpenFolder,
            n,
        );
        add(
            "&File/Print &Test Page",
            Shortcut::None,
            Cmd::TestPage,
            MenuFlag::MenuDivider,
        );
        add("&File/&Close", Shortcut::Alt | Key::F4, Cmd::Close, n);
        add("&Edit/&Open Output", Shortcut::None, Cmd::Open, n);
        add(
            "&Edit/&Show in Folder",
            Shortcut::None,
            Cmd::ShowInFolder,
            n,
        );
        add("&Edit/&Print Again", Shortcut::None, Cmd::PrintAgain, n);
        add(
            "&Edit/&Delete",
            Shortcut::None | Key::Delete,
            Cmd::Delete,
            n,
        );
        add("&Communication/&Connect", Shortcut::None, Cmd::Connect, n);
        add(
            "&Communication/&Disconnect",
            Shortcut::None,
            Cmd::Disconnect,
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

        let mut tb = Toolbar::new(0, MENU_H, w);
        for (g, tip, c) in [
            (Glyph::Connect, "Connect", Cmd::Connect),
            (Glyph::Disconnect, "Disconnect", Cmd::Disconnect),
        ] {
            tb.add(g, tip, move || tx.send(c));
        }
        tb.separator();
        for (g, tip, c) in [
            (Glyph::Folder, "Open Output Folder", Cmd::OpenFolder),
            (Glyph::System, "Settings", Cmd::Settings),
            (Glyph::Printer, "Print Test Page", Cmd::TestPage),
        ] {
            tb.add(g, tip, move || tx.send(c));
        }
        tb.end();

        // The session's state.
        let y = MENU_H + Toolbar::HEIGHT;
        let info = fltk::group::Group::new(0, y, w, INFO_H, None);
        let label = |x: i32, w: i32| {
            let mut f = Frame::new(x, y + 3, w, INFO_H - 6, None);
            f.set_align(Align::Left | Align::Inside);
            f.set_label_size(theme::UI_SIZE);
            f
        };
        let system_label = label(8, 230);
        let device_label = label(244, 160);
        let state_label = label(410, w - 410 - 30);
        let mut lock = Frame::new(w - 26, y + 5, 16, 16, None);
        lock.set_image(Some(glyph::image(Glyph::Lock, Size::Small)));
        lock.set_tooltip("The connection is encrypted with TLS");
        lock.hide();
        info.end();
        info.resizable(&state_label);

        let mut caption = Frame::new(8, y + INFO_H, 200, 18, "Received output");
        caption.set_align(Align::Left | Align::Inside);
        caption.set_label_size(theme::UI_SIZE);
        let top = y + INFO_H + 18;
        let mut table = ListTable::new(
            6,
            top,
            w - 12,
            h - top - StatusBar::HEIGHT - 4,
            &[
                ("Time", 118),
                ("Pages", 50),
                ("Size", 70),
                ("Target", 110),
                ("Status", 130),
                ("Output file", w - 12 - 118 - 50 - 70 - 110 - 130 - 20),
            ],
        );
        table.on_activate(move |row| {
            if row.is_some() {
                tx.send(Cmd::Open);
            }
        });
        table.on_context(move |row| {
            if row.is_none() {
                return;
            }
            let menu = MenuItem::new(&["Open", "Show in Folder", "Print Again", "Delete"]);
            if let Some(item) = menu.popup(app::event_x(), app::event_y()) {
                let c = match item.label().as_deref() {
                    Some("Open") => Cmd::Open,
                    Some("Show in Folder") => Cmd::ShowInFolder,
                    Some("Print Again") => Cmd::PrintAgain,
                    Some("Delete") => Cmd::Delete,
                    _ => return,
                };
                tx.send(c);
            }
        });
        let status = StatusBar::new(0, h - StatusBar::HEIGHT, w);
        window.resizable(table.table());
        window.end();
        window.set_callback(move |_| tx.send(Cmd::Close));

        let spooler = Spooler::new(spool::Output::from_file(&doc));
        let pw = Rc::new(PrinterWindow {
            window,
            menu,
            table,
            status,
            system_label,
            device_label,
            state_label,
            lock,
            rx,
            state: state.clone(),
            st: RefCell::new(St {
                doc,
                file,
                worker: None,
                state: State::Idle,
                security: Security::Plain,
                jobs: Vec::new(),
                backoff: RETRY_FIRST,
                retry_at: None,
                not_ready: false,
                spooler,
            }),
        });
        pw.recover();
        pw.refresh();
        pw
    }

    /// Centres the window on `parent`, or on the main screen.
    pub fn place(&self, parent: Option<&dyn WindowExt>) {
        let mut w = self.window.clone();
        qsa_ui::propdialog::center_on(&mut w, parent);
    }

    pub fn auto_connect(&self) -> bool {
        self.st.borrow().doc.session.auto_connect
    }

    pub fn state(&self) -> State {
        self.st.borrow().state
    }

    /// The rows of Received Output, as shown.
    pub fn jobs(&self) -> Vec<JobRecord> {
        self.st.borrow().jobs.clone()
    }

    /// Shows `jobs` and a state without a connection (screenshots).
    pub fn show_as(&self, state: State, tls: bool, jobs: Vec<JobRecord>) {
        {
            let mut st = self.st.borrow_mut();
            st.state = state;
            st.security = if tls {
                Security::Tls {
                    protocol: "TLS 1.3".into(),
                    cipher: String::new(),
                }
            } else {
                Security::Plain
            };
            st.jobs = jobs;
        }
        self.fill_table();
        self.refresh();
    }

    /// Renders jobs an earlier run left in the spool.
    fn recover(&self) {
        let recovered = self.st.borrow_mut().spooler.recover();
        for r in recovered {
            self.add_job(r, false);
        }
    }

    fn refresh(&self) {
        let st = self.st.borrow();
        let doc = &st.doc;
        let mut s = self.system_label.clone();
        s.set_label(&format!("System: {}", doc.system.name));
        let mut d = self.device_label.clone();
        d.set_label(&format!("Device: {}", doc.printer.device_name));
        let mut l = self.state_label.clone();
        let text = match (st.state, st.retry_at) {
            (State::Reconnecting, Some(at)) => format!(
                "State: Reconnecting in {} s",
                at.saturating_duration_since(Instant::now())
                    .as_secs()
                    .max(1)
            ),
            (s, _) => format!("State: {}", s.text()),
        };
        l.set_label(&text);
        l.set_label_color(match st.state.status() {
            Status::Error => colors::STATUS_ERROR,
            _ => colors::TEXT,
        });
        let mut lock = self.lock.clone();
        let tls = matches!(st.security, Security::Tls { .. })
            && matches!(
                st.state,
                State::Ready | State::Receiving | State::Printing | State::Error
            );
        if tls {
            lock.show();
        } else {
            lock.hide();
        }
        let mut bar = self.status.clone();
        bar.set_status(
            &format!("{}: {}", doc.printer.device_name, st.state.text()),
            st.state.status(),
        );
        let connected = st.worker.is_some();
        let menu = self.menu.clone();
        for (path, on) in [
            ("&Communication/&Connect", !connected),
            (
                "&Communication/&Disconnect",
                connected || st.retry_at.is_some(),
            ),
        ] {
            if let Some(mut item) = menu.find_item(path) {
                if on {
                    item.activate();
                } else {
                    item.deactivate();
                }
            }
        }
        drop(st);
        self.window.clone().redraw();
    }

    fn report(&self, m: &Message, modal: bool) {
        crate::log!("{m}");
        let mut bar = self.status.clone();
        bar.set_message(&m.to_string());
        if modal {
            msgdetails::show_message(Some(&self.window), m);
        }
    }

    /// Connects (again).
    pub fn connect(&self) {
        self.stop_worker();
        let mut st = self.st.borrow_mut();
        st.retry_at = None;
        let doc = st.doc.clone();
        if doc.printer.device_name.trim().is_empty() {
            drop(st);
            self.report(&qsa_msg::message(id::PRINTER_DEVICE_REQUIRED, &[]), true);
            return;
        }
        let Some(system) = self.state.borrow().systems.get(&doc.system.name).cloned() else {
            drop(st);
            self.report(
                &qsa_msg::message(id::HOST_NOT_FOUND, &[&doc.system.name]),
                true,
            );
            return;
        };
        let host = if system.address_lookup == AddressLookup::Never && !system.fixed_ip.is_empty() {
            system.fixed_ip.clone()
        } else {
            system.name.clone()
        };
        let tls = doc.system.tls;
        let port = if tls {
            system.ports.telnet_tls
        } else {
            system.ports.telnet
        };
        let tls_options = tls.then(|| TlsOptions {
            trust: trust::for_system(&system),
            system_roots: true,
            timeout: CONNECT_TIMEOUT,
            tls13_only: false,
        });
        // --record applies to the next session opened, of either kind.
        let record = self.state.borrow_mut().record_next.take();
        st.worker = Some(PrinterWorker::spawn(
            Target {
                host,
                port,
                tls: tls_options,
            },
            printer_options(&doc),
            spool::Output::from_file(&doc),
            record,
        ));
        st.state = State::Connecting;
        st.security = Security::Plain;
        drop(st);
        self.report(
            &qsa_msg::message(id::CONNECTING, &[&doc.system.name, &port.to_string()]),
            false,
        );
        self.refresh();
    }

    fn stop_worker(&self) {
        let mut st = self.st.borrow_mut();
        if let Some(mut w) = st.worker.take() {
            w.close();
        }
    }

    /// Disconnects and stops connecting again.
    pub fn disconnect(&self) {
        self.stop_worker();
        let mut st = self.st.borrow_mut();
        st.retry_at = None;
        st.backoff = RETRY_FIRST;
        if st.state != State::Idle {
            st.state = State::Disconnected;
        }
        drop(st);
        self.refresh();
    }

    /// Connects again after the current wait, which then doubles.
    fn retry_later(&self) {
        let (device, wait) = {
            let mut st = self.st.borrow_mut();
            let wait = st.backoff;
            st.backoff = (wait * 2).min(RETRY_MAX);
            st.retry_at = Some(Instant::now() + wait);
            st.state = State::Reconnecting;
            (st.doc.printer.device_name.clone(), wait)
        };
        self.report(
            &qsa_msg::message(
                id::PRINTER_RECONNECTING,
                &[&device, &wait.as_secs().to_string()],
            ),
            false,
        );
        // Wake the event loop when it is due, and each second for the
        // countdown.
        for s in 1..=wait.as_secs() {
            app::add_timeout3(s as f64, |_| app::awake());
        }
    }

    fn add_job(&self, r: JobRecord, idle: bool) {
        let device = self.st.borrow().doc.printer.device_name.clone();
        for m in r.messages(&device) {
            let modal = false;
            self.report(&m, modal);
        }
        if idle {
            self.report(&qsa_msg::message(id::JOB_IDLE_CLOSED, &[]), false);
        }
        let open = {
            let st = self.st.borrow();
            st.doc.output.open_pdf_when_done && r.status == JobStatus::Done
        };
        if open && let Some(p) = &r.pdf {
            shell::open(p);
        }
        self.st.borrow_mut().jobs.push(r);
        self.fill_table();
        let n = self.table.len();
        if n > 0 {
            self.table.clone().select(Some(n - 1));
        }
    }

    fn fill_table(&self) {
        let rows: Vec<Vec<Cell>> = self.st.borrow().jobs.iter().map(row).collect();
        self.table.clone().set_rows(rows);
    }

    fn selected_job(&self) -> Option<(usize, JobRecord)> {
        let i = self.table.selected()?;
        self.st.borrow().jobs.get(i).cloned().map(|r| (i, r))
    }

    /// Gives the window its waiting work. False once it is closed.
    pub fn pump(&self) -> bool {
        while let Some(c) = self.rx.recv() {
            self.command(c);
        }
        loop {
            let ev = {
                let st = self.st.borrow();
                st.worker.as_ref().and_then(|w| w.rx.try_recv().ok())
            };
            let Some(ev) = ev else { break };
            self.net(ev);
        }
        let due = {
            let st = self.st.borrow();
            st.retry_at.map(|at| Instant::now() >= at)
        };
        match due {
            Some(true) => self.connect(),
            Some(false) => self.refresh(),
            None => {}
        }
        self.window.shown()
    }

    fn command(&self, c: Cmd) {
        match c {
            Cmd::Save => self.save(false),
            Cmd::SaveAs => self.save(true),
            Cmd::Settings => self.settings(),
            Cmd::OpenFolder => {
                let folder = self.st.borrow().doc.pdf_folder();
                if let Some(dir) = folder {
                    let _ = std::fs::create_dir_all(&dir);
                    shell::open(&dir);
                }
            }
            Cmd::TestPage => self.test_page(),
            Cmd::Close => {
                self.disconnect();
                self.window.clone().hide();
            }
            Cmd::Open => {
                if let Some((_, r)) = self.selected_job()
                    && let Some(p) = r.pdf.as_ref().or(r.text.as_ref())
                {
                    shell::open(p);
                }
            }
            Cmd::ShowInFolder => {
                if let Some((_, r)) = self.selected_job() {
                    match r.pdf.as_ref().or(r.text.as_ref()) {
                        Some(p) => shell::show_in_folder(p),
                        None => {
                            if let Some(d) = self.st.borrow().doc.pdf_folder() {
                                shell::open(&d);
                            }
                        }
                    }
                }
            }
            Cmd::PrintAgain => self.print_again(),
            Cmd::Delete => self.delete(),
            Cmd::Connect => self.connect(),
            Cmd::Disconnect => {
                let system = self.st.borrow().doc.system.name.clone();
                self.disconnect();
                self.report(&qsa_msg::message(id::DISCONNECTED, &[&system]), false);
            }
            Cmd::About => crate::about::show(Some(&self.window)),
            Cmd::HelpTopics => crate::help::show(crate::help::topic::PRINTER_5250),
        }
    }

    /// The test page, rendered here to the session's targets; the system is
    /// not involved.
    fn test_page(&self) {
        {
            let mut st = self.st.borrow_mut();
            st.state = if st.worker.is_some() {
                State::Printing
            } else {
                st.state
            };
        }
        self.refresh();
        let rec = {
            let mut st = self.st.borrow_mut();
            let mut r = st.spooler.render(&scs::sample::test_page(), "Test page");
            r.title = "Test page".into();
            r
        };
        self.after_output(&rec);
        self.add_job(rec, false);
    }

    /// Puts the state back after local output, and tells the system the
    /// printer is ready again if it was told otherwise and this worked.
    fn after_output(&self, rec: &JobRecord) {
        let mut st = self.st.borrow_mut();
        if rec.status == JobStatus::Failed {
            if st.worker.is_some() {
                st.state = State::Error;
            }
            return;
        }
        if st.worker.is_some() {
            st.state = State::Ready;
        }
        if st.not_ready {
            st.not_ready = false;
            if let Some(w) = &st.worker {
                let _ = w.tx.send(ToNet::Ready);
            }
        }
    }

    fn print_again(&self) {
        let Some((_, rec)) = self.selected_job() else {
            return;
        };
        let result = self.st.borrow_mut().spooler.print_again(&rec);
        let device = self.st.borrow().doc.printer.device_name.clone();
        match result {
            Ok(name) => {
                self.report(
                    &qsa_msg::message(
                        id::OUTPUT_PRINTED,
                        &[&device, &name, &rec.pages.to_string()],
                    ),
                    false,
                );
                let mut ok = rec.clone();
                ok.status = JobStatus::Done;
                ok.problems.clear();
                self.after_output(&ok);
            }
            Err(p) => self.report(&p.message(), true),
        }
        self.refresh();
    }

    fn delete(&self) {
        let Some((i, rec)) = self.selected_job() else {
            return;
        };
        let files: Vec<&PathBuf> = rec.pdf.iter().chain(rec.text.iter()).collect();
        if !files.is_empty() {
            let names: Vec<String> = files.iter().map(|p| p.display().to_string()).collect();
            let choice = MessageBox::new(
                "Delete Output",
                &format!("Delete the output of this job?\n\n{}", names.join("\n")),
            )
            .buttons(&["Delete", "Cancel"], 1, 1)
            .run(Some(&self.window));
            if choice != 0 {
                return;
            }
        }
        for p in files.into_iter().chain(rec.spool.iter()) {
            if let Err(e) = std::fs::remove_file(p)
                && e.kind() != std::io::ErrorKind::NotFound
            {
                crate::log!("{}: {e}", p.display());
            }
        }
        self.st.borrow_mut().jobs.remove(i);
        self.fill_table();
    }

    fn settings(&self) {
        let doc = self.st.borrow().doc.clone();
        let Some(new) = super::settings::run(Some(&self.window), &doc) else {
            return;
        };
        let reconnect = new.printer.device_name != doc.printer.device_name
            || new.message_queue() != doc.message_queue();
        {
            let mut st = self.st.borrow_mut();
            let out = spool::Output::from_file(&new);
            st.spooler.set_output(out.clone());
            // New settings may have cleared what made the printer not ready.
            let ready = std::mem::take(&mut st.not_ready);
            if let Some(w) = &st.worker {
                let _ = w.tx.send(ToNet::Output(out));
                if ready {
                    let _ = w.tx.send(ToNet::Ready);
                }
            }
            st.doc = new;
        }
        let title = {
            let st = self.st.borrow();
            window_title(&st.doc, st.file.as_deref())
        };
        self.window.clone().set_label(&title);
        if reconnect && self.st.borrow().worker.is_some() {
            self.status.clone().set_message(
                "The device and message queue take effect when the session connects again.",
            );
        }
        self.refresh();
    }

    fn save(&self, ask: bool) {
        let (doc, file) = {
            let mut st = self.st.borrow_mut();
            st.doc.session.window = Some(Geometry {
                x: self.window.x(),
                y: self.window.y(),
                w: self.window.w(),
                h: self.window.h(),
            });
            (st.doc.clone(), st.file.clone())
        };
        let path = match file {
            Some(p) if !ask => p,
            _ => {
                let name = format!("{}.{}", doc.printer.device_name, qsp::EXTENSION);
                let Some(p) = files::choose_save_in(
                    &files::sessions_dir(),
                    "Save Session As",
                    files::PRINTER_PATTERN,
                    &name,
                ) else {
                    return;
                };
                if p.extension().is_none() {
                    p.with_extension(qsp::EXTENSION)
                } else {
                    p
                }
            }
        };
        match doc.save(&path) {
            Ok(()) => {
                {
                    let mut st = self.st.borrow_mut();
                    st.file = Some(path.clone());
                }
                {
                    let mut app_state = self.state.borrow_mut();
                    app_state.note_recent(&path);
                }
                self.window
                    .clone()
                    .set_label(&window_title(&doc, Some(&path)));
                self.report(
                    &qsa_msg::message(id::SESSION_SAVED, &[&path.display().to_string()]),
                    false,
                );
            }
            Err(e) => {
                crate::log!("{}: {e}", path.display());
                self.report(
                    &qsa_msg::message(
                        id::SESSION_FILE_UNREADABLE,
                        &[&path.display().to_string(), &e.to_string()],
                    ),
                    true,
                );
            }
        }
    }

    fn net(&self, ev: FromNet) {
        let (system, device) = {
            let st = self.st.borrow();
            (
                st.doc.system.name.clone(),
                st.doc.printer.device_name.clone(),
            )
        };
        match ev {
            FromNet::Connected(sec) => {
                self.st.borrow_mut().security = sec;
            }
            FromNet::Ready(Mode::Basic) => {
                // Without the enhanced protocol there is no device name to
                // give, so no printer.
                self.disconnect();
                self.report(&qsa_msg::message(id::BASIC_5250, &[]), true);
            }
            FromNet::Ready(Mode::Enhanced) => {}
            FromNet::Startup(s) => self.startup(&s, &system, &device),
            FromNet::Receiving => self.st.borrow_mut().state = State::Receiving,
            FromNet::Job { record, idle } => {
                {
                    let mut st = self.st.borrow_mut();
                    if record.status == JobStatus::Failed {
                        // A refused record leaves the session as it was:
                        // its row and message say what happened.
                        if record.problems.iter().any(|p| p.not_ready()) {
                            st.state = State::Error;
                            st.not_ready = true;
                        }
                    } else if st.state != State::Error {
                        st.state = State::Ready;
                    }
                }
                self.add_job(record, idle);
            }
            FromNet::SpoolFailed(why) => {
                let dir = spool::spool_dir(&device);
                crate::log!("spool file in {}: {why}", dir.display());
                {
                    let mut st = self.st.borrow_mut();
                    st.state = State::Error;
                    st.not_ready = true;
                }
                self.report(
                    &qsa_msg::message(
                        id::OUTPUT_FOLDER_NOT_WRITABLE,
                        &[&dir.display().to_string()],
                    ),
                    false,
                );
            }
            FromNet::ProtocolError(e) => {
                self.report(&qsa_msg::message(id::PROTOCOL_ERROR, &[&system, &e]), false);
            }
            FromNet::Ended(end) => self.ended(end, &system),
        }
        self.refresh();
    }

    fn startup(&self, s: &tn5250::negotiate::StartupResponse, system: &str, device: &str) {
        let (msgq, lib) = self.st.borrow().doc.message_queue();
        let stop = |m: Message| {
            self.disconnect();
            self.report(&m, true);
        };
        match (s.code.as_str(), s.outcome()) {
            ("8930", _) => stop(qsa_msg::message(
                id::MESSAGE_QUEUE_NOT_FOUND,
                &[&msgq, &lib, system, &s.code],
            )),
            ("8903", _) => stop(qsa_msg::message(
                id::NOT_A_PRINTER_DEVICE,
                &[device, system, &s.code, s.meaning()],
            )),
            (_, Outcome::Started) => {
                {
                    let mut st = self.st.borrow_mut();
                    st.state = State::Ready;
                    st.backoff = RETRY_FIRST;
                }
                self.report(
                    &qsa_msg::message(id::PRINTER_CONNECTED, &[device, system]),
                    false,
                );
            }
            (_, Outcome::NotValid) => stop(qsa_msg::message(
                id::DEVICE_NOT_VALID,
                &[device, &s.code, s.meaning()],
            )),
            (_, Outcome::InUse) => {
                // Another session may hold the device; try again later.
                self.stop_worker();
                self.report(
                    &qsa_msg::message(id::DEVICE_IN_USE, &[device, &s.code, s.meaning()]),
                    false,
                );
                self.retry_later();
            }
            (_, Outcome::Failed) => {
                self.stop_worker();
                let why = format!("start-up response {} ({})", s.code, s.meaning());
                self.report(
                    &qsa_msg::message(id::PROTOCOL_ERROR, &[system, &why]),
                    false,
                );
                self.retry_later();
            }
        }
    }

    fn ended(&self, end: Ended, system: &str) {
        let user = matches!(end, Ended::ByUser);
        let stopped = {
            let mut st = self.st.borrow_mut();
            st.worker = None;
            let stopped = st.state == State::Disconnected || st.retry_at.is_some();
            if !stopped {
                st.state = State::Disconnected;
            }
            stopped
        };
        if user || stopped {
            return;
        }
        let msg = |mid: &str| qsa_msg::message(mid, &[system]);
        match end {
            Ended::ByUser => {}
            Ended::Closed => self.report(&msg(id::SESSION_ENDED), false),
            Ended::Io(e) => {
                crate::log!("printer session to {system}: {e}");
                self.report(&msg(id::SESSION_ENDED), false);
            }
            Ended::Connect(ConnectError::NotFound(_)) => {
                self.report(&msg(id::HOST_NOT_FOUND), false)
            }
            Ended::Connect(ConnectError::Refused(_)) => {
                self.report(&msg(id::CONNECTION_REFUSED), false)
            }
            Ended::Connect(ConnectError::TimedOut(_)) => {
                self.report(&msg(id::CONNECT_TIMED_OUT), false)
            }
            Ended::Connect(ConnectError::Io { source, .. }) => {
                crate::log!("connecting to {system}: {source}");
                self.report(&msg(id::CONNECTION_REFUSED), false);
            }
            Ended::Connect(ConnectError::Tls(TlsError::Handshake(why))) => {
                // TLS will fail the same way again: no retry.
                self.report(&qsa_msg::message(id::TLS_FAILED, &[system, &why]), true);
                return;
            }
            Ended::Connect(ConnectError::Tls(TlsError::Untrusted {
                reason,
                certificate,
                info,
            })) => {
                if trust::ask_and_trust(
                    Some(&self.window),
                    &self.state,
                    system,
                    &reason,
                    &info,
                    &certificate,
                    |m| self.report(m, false),
                ) {
                    self.connect();
                }
                return;
            }
        }
        self.retry_later();
    }
}
