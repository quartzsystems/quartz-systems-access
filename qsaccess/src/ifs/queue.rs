// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! The Integrated File System window's transfer queue.
//!
//! - One connection of the window's own, attached to the session when the
//!   first transfer starts, runs the queue one file at a time on a thread
//!   of its own; a window costs at most one of the session's 16
//!   connections. When `Attach` answers `Busy` (`TCP1A59`) the queue runs
//!   on the shared connection instead, one request at a time, and
//!   `QAD8030` is shown once.
//! - The panel under the list: Direction, Name, Size, Progress, Speed and
//!   Status, with Cancel, Retry, Remove Finished and Clear.
//! - A batch's options ([`options_dialog`]): binary or text, the CCSID of
//!   files made on the system, line endings, and what to do with a file
//!   already there; asked about one, [`existing_prompt`] offers Replace,
//!   Skip, Replace All, Skip All and, for a download, Keep Both.
//! - A folder becomes a job per file, its folders made on the other side.
//! - Retry goes on from the last part the system confirmed: a binary
//!   transfer either way and a text download (`QAD8023`); a text upload
//!   starts again.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

use fltk::app;
use fltk::button::{Button, CheckButton, RadioRoundButton};
use fltk::enums::{Align, FrameType};
use fltk::group::Group;
use fltk::menu::Choice as Combo;
use fltk::misc::InputChoice;
use fltk::prelude::*;
use fltk::window::Window;
use qap_client::ifs::{self, Files};
use qap_client::proto::Op;
use qap_client::{Attached, Error, Pending, Wire};
use qsa_msg::id;
use qsa_transfer::files::transfer::{plan_download, plan_upload};
use qsa_transfer::files::{Choice, Conflict, Direction, Job, Options, Outcome, Policy};
use qsa_ui::listtable::{Cell, ListTable};
use qsa_ui::{Status, controls, theme};

use super::model;
use crate::qap::{self, Handle};

/// The panel's height.
pub const PANEL_H: i32 = 170;

/// The panel's columns.
pub const COLUMNS: [(&str, i32); 6] = [
    ("Direction", 76),
    ("Name", 200),
    ("Size", 80),
    ("Progress", 66),
    ("Speed", 80),
    ("Status", 300),
];

/// The connection the queue runs on.
pub enum Conn {
    /// The window's own.
    Attached(Box<Attached>),
    /// The session's shared one, held only to send each request.
    Shared(Handle),
}

impl Files for Conn {
    fn ifs_call<B: Wire, R: Wire>(&mut self, op: Op, body: &B) -> Result<R, Error> {
        match self {
            Conn::Attached(a) => a.call(op, body),
            Conn::Shared(h) => {
                let p = qap::lock(h).request(op, body)?;
                let timeout = qap::lock(h).target().options.reply_timeout;
                p.wait(timeout)?.decode()
            }
        }
    }

    fn ifs_request<B: Wire>(&mut self, op: Op, body: &B) -> Result<Pending, Error> {
        match self {
            Conn::Attached(a) => a.request(op, body),
            Conn::Shared(h) => qap::lock(h).request(op, body),
        }
    }

    fn ifs_supports(&mut self, op: Op) -> bool {
        match self {
            Conn::Attached(a) => a.supports(op).is_ok(),
            Conn::Shared(h) => qap::lock(h).supports(op).is_ok(),
        }
    }
}

/// Work for the queue's thread.
pub enum Work {
    /// One file.
    File(Job),
    /// A folder to download into a folder here.
    Download {
        remote: String,
        into: PathBuf,
        options: Options,
    },
    /// A folder here to upload into a folder on the system.
    Upload {
        local: PathBuf,
        into: String,
        options: Options,
    },
    /// A saved request (`.qdx`), planned into files when it starts.
    Request(qsa_transfer::request::FilesRequest),
}

enum Cmd {
    Run(u64, Work),
    Answer(Choice),
}

/// What the queue's thread says.
enum Ev {
    Progress(u64, ifs::Progress),
    Ask(u64, Conflict),
    Done(u64, Job, Result<Outcome, String>),
    Planned(u64, Result<Vec<Job>, String>),
}

/// The thread that runs one piece of work at a time.
struct Worker {
    tx: mpsc::Sender<Cmd>,
    rx: mpsc::Receiver<Ev>,
    /// The piece of work running, and its cancel.
    cancel: Arc<AtomicBool>,
}

impl Worker {
    fn start(mut conn: Conn) -> Worker {
        let (tx, crx) = mpsc::channel::<Cmd>();
        let (etx, rx) = mpsc::channel::<Ev>();
        let cancel = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&cancel);
        std::thread::Builder::new()
            .name("ifs transfers".into())
            .spawn(move || {
                let wake = || app::awake();
                while let Ok(cmd) = crx.recv() {
                    let Cmd::Run(id, work) = cmd else { continue };
                    flag.store(false, Ordering::SeqCst);
                    match work {
                        Work::File(mut job) => {
                            let mut ask = |c: &Conflict| {
                                let _ = etx.send(Ev::Ask(id, c.clone()));
                                wake();
                                loop {
                                    match crx.recv() {
                                        Ok(Cmd::Answer(a)) => return a,
                                        Ok(Cmd::Run(..)) => {}
                                        Err(_) => return Choice::Skip,
                                    }
                                }
                            };
                            let mut last = Instant::now() - Duration::from_secs(1);
                            let mut progress = |p: ifs::Progress| {
                                if last.elapsed() >= Duration::from_millis(100) {
                                    last = Instant::now();
                                    let _ = etx.send(Ev::Progress(id, p));
                                    wake();
                                }
                            };
                            let r = qsa_transfer::files::transfer::run(
                                &mut conn,
                                &mut job,
                                &mut ask,
                                &mut progress,
                                &flag,
                            )
                            .map_err(|f| f.error.to_string());
                            let _ = etx.send(Ev::Done(id, job, r));
                        }
                        Work::Download {
                            remote,
                            into,
                            options,
                        } => {
                            let r = plan_download(&mut conn, &remote, &into, options)
                                .map_err(|s| stopped_text(&s));
                            let _ = etx.send(Ev::Planned(id, r));
                        }
                        Work::Upload {
                            local,
                            into,
                            options,
                        } => {
                            let r = plan_upload(&mut conn, &local, &into, options)
                                .map_err(|s| stopped_text(&s));
                            let _ = etx.send(Ev::Planned(id, r));
                        }
                        Work::Request(req) => {
                            let r = qsa_transfer::files::transfer::plan_request(&mut conn, &req)
                                .map_err(|s| stopped_text(&s));
                            let _ = etx.send(Ev::Planned(id, r));
                        }
                    }
                    wake();
                }
            })
            .expect("a thread for transfers");
        Worker { tx, rx, cancel }
    }
}

fn stopped_text(s: &qsa_transfer::files::Stopped) -> String {
    let why = match s.error.server_message() {
        Some(m) => format!("{}: {}", m.id, m.text.trim_end_matches('.')),
        None => s.error.to_string(),
    };
    format!("{}: {why}", s.at)
}

/// Where an item is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum State {
    Queued,
    Running,
    Done,
    Skipped,
    Cancelled,
    Failed(String),
}

impl State {
    fn finished(&self) -> bool {
        !matches!(self, State::Queued | State::Running)
    }
}

/// One line of the queue.
pub struct Item {
    pub id: u64,
    /// The batch it came in: Replace All and Skip All hold for it.
    pub batch: u64,
    /// The file, or the folder being planned.
    pub job: Option<Job>,
    /// A folder's work, until it is planned.
    pub work: Option<Work>,
    pub name: String,
    pub direction: Direction,
    pub state: State,
    pub size: u64,
    pub progress: ifs::Progress,
    started: Option<Instant>,
    /// Bytes moved per second, while running.
    speed: Option<f64>,
}

impl Item {
    /// Its cells in the panel.
    pub fn cells(&self) -> Vec<Cell> {
        let dir = match self.direction {
            Direction::Download => "Download",
            Direction::Upload => "Upload",
        };
        let pct = match &self.state {
            State::Done => "100 %".to_owned(),
            _ if self.size > 0 && self.job.is_some() => {
                let done = match self.direction {
                    Direction::Download => self.progress.offset,
                    Direction::Upload => self.progress.offset,
                };
                format!("{} %", (done.min(self.size) * 100 / self.size))
            }
            _ => String::new(),
        };
        let speed = match (&self.state, self.speed) {
            (State::Running, Some(s)) => speed_text(s),
            _ => String::new(),
        };
        let (status, colour) = match &self.state {
            State::Queued => ("Queued".to_owned(), Status::Neutral),
            State::Running if self.job.is_none() => {
                ("Listing the folder...".to_owned(), Status::Active)
            }
            State::Running => ("Running".to_owned(), Status::Active),
            State::Done => ("Done".to_owned(), Status::Neutral),
            State::Skipped => ("Skipped: already there".to_owned(), Status::Muted),
            State::Cancelled => ("Cancelled".to_owned(), Status::Muted),
            State::Failed(why) => (format!("Failed: {why}"), Status::Error),
        };
        let size = if self.job.is_some() {
            model::size_text(self.size)
        } else {
            String::new()
        };
        vec![
            Cell::from(dir),
            Cell::from(self.name.as_str()),
            Cell::from(size),
            Cell::from(pct),
            Cell::from(speed),
            Cell {
                text: status,
                status: colour,
            },
        ]
    }
}

/// Bytes a second, for the Speed column.
pub fn speed_text(bytes_per_s: f64) -> String {
    format!("{}/s", model::bytes_text(bytes_per_s.max(0.0) as u64))
}

/// What the queue tells its window.
pub enum Note {
    /// A message for the status bar and the log.
    Said(qsa_msg::Message),
    /// An upload finished into this folder: the list may be out of date.
    Changed(String),
}

/// The queue: its panel, its items and its thread.
pub struct Queue {
    pub group: Group,
    pub list: ListTable,
    items: RefCell<Vec<Item>>,
    order: RefCell<VecDeque<u64>>,
    worker: RefCell<Option<Worker>>,
    running: RefCell<Option<u64>>,
    next_id: RefCell<u64>,
    /// Per batch, an answer for every file still to be asked about.
    all: RefCell<Vec<(u64, Choice)>>,
    pub buttons: [Button; 4],
}

/// What the panel's buttons ask for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Act {
    Cancel,
    Retry,
    RemoveFinished,
    Clear,
}

impl Queue {
    /// The panel at `x, y`, `w` wide, its buttons sending `act`.
    pub fn new(x: i32, y: i32, w: i32, act: impl Fn(Act) + Clone + 'static) -> Queue {
        let mut group = Group::new(x, y, w, PANEL_H, None);
        group.set_frame(FrameType::FlatBox);
        group.set_color(theme::colors::FACE);
        let bh = theme::BUTTON_H;
        let mut title = fltk::frame::Frame::new(x, y + 2, 120, bh, "Transfers");
        title.set_align(Align::Left | Align::Inside);
        title.set_label_font(theme::UI_FONT_BOLD);
        title.set_label_size(theme::UI_SIZE);
        let mut bx = x + w;
        let mut buttons = Vec::new();
        for (label, a, bw) in [
            ("Clear", Act::Clear, 70),
            ("Remove Finished", Act::RemoveFinished, 118),
            ("Retry", Act::Retry, 70),
            ("Cancel", Act::Cancel, 70),
        ] {
            bx -= bw;
            let mut b = controls::button(bx, y + 2, bw, bh, label);
            let act = act.clone();
            b.set_callback(move |_| act(a));
            buttons.push(b);
            bx -= 6;
        }
        buttons.reverse();
        let ly = y + bh + 6;
        let mut list = ListTable::new(x, ly, w, PANEL_H - (ly - y), &COLUMNS);
        list.set_multi(true);
        group.resizable(list.table());
        group.end();
        let buttons: [Button; 4] = buttons.try_into().expect("four buttons");
        Queue {
            group,
            list,
            items: RefCell::new(Vec::new()),
            order: RefCell::new(VecDeque::new()),
            worker: RefCell::new(None),
            running: RefCell::new(None),
            next_id: RefCell::new(1),
            all: RefCell::new(Vec::new()),
            buttons,
        }
    }

    /// Whether anything is queued or running.
    pub fn busy(&self) -> bool {
        self.running.borrow().is_some() || !self.order.borrow().is_empty()
    }

    /// Whether the queue's thread is started.
    pub fn started(&self) -> bool {
        self.worker.borrow().is_some()
    }

    /// Starts the queue's thread on `conn`.
    pub fn start(&self, conn: Conn) {
        *self.worker.borrow_mut() = Some(Worker::start(conn));
    }

    fn id(&self) -> u64 {
        let mut n = self.next_id.borrow_mut();
        *n += 1;
        *n
    }

    /// A new batch's number.
    pub fn batch(&self) -> u64 {
        self.id()
    }

    /// Adds `work` to the queue, in `batch`.
    pub fn add(&self, batch: u64, work: Work) {
        let id = self.id();
        let (name, direction, size, job, work) = match work {
            Work::File(job) => (job.name(), job.direction, job.size, Some(job), None),
            Work::Download { ref remote, .. } => (
                format!("{} (folder)", ifs::file_name(remote)),
                Direction::Download,
                0,
                None,
                Some(work),
            ),
            Work::Upload { ref local, .. } => (
                format!(
                    "{} (folder)",
                    local.file_name().unwrap_or_default().to_string_lossy()
                ),
                Direction::Upload,
                0,
                None,
                Some(work),
            ),
            Work::Request(ref r) => (
                format!("{} (request)", ifs::file_name(&r.remote)),
                r.direction,
                0,
                None,
                Some(work),
            ),
        };
        self.items.borrow_mut().push(Item {
            id,
            batch,
            job,
            work,
            name,
            direction,
            state: State::Queued,
            size,
            progress: ifs::Progress::default(),
            started: None,
            speed: None,
        });
        self.order.borrow_mut().push_back(id);
        self.redraw();
    }

    /// Starts the next piece of work when nothing runs.
    fn next(&self) {
        if self.running.borrow().is_some() {
            return;
        }
        let worker = self.worker.borrow();
        let Some(w) = worker.as_ref() else { return };
        loop {
            let Some(id) = self.order.borrow_mut().pop_front() else {
                return;
            };
            let mut items = self.items.borrow_mut();
            let Some(item) = items.iter_mut().find(|i| i.id == id) else {
                continue;
            };
            if item.state != State::Queued {
                continue;
            }
            let work = match (item.work.take(), &item.job) {
                (Some(work), _) => work,
                (None, Some(job)) => Work::File(job.clone()),
                (None, None) => continue,
            };
            item.state = State::Running;
            item.started = Some(Instant::now());
            item.speed = None;
            if w.tx.send(Cmd::Run(id, work)).is_ok() {
                *self.running.borrow_mut() = Some(id);
            }
            return;
        }
    }

    /// Takes in what the thread said; asks about existing files with `ask`
    /// (true: the folder is this computer's). What the window should know.
    pub fn pump(&self, ask: &dyn Fn(&Conflict) -> Option<(Choice, bool)>) -> Vec<Note> {
        let mut notes = Vec::new();
        loop {
            let ev = {
                let w = self.worker.borrow();
                let Some(w) = w.as_ref() else { break };
                match w.rx.try_recv() {
                    Ok(ev) => ev,
                    Err(_) => break,
                }
            };
            match ev {
                Ev::Progress(id, p) => self.with(id, |i| {
                    i.progress = p;
                    if let Some(t) = i.started {
                        let s = t.elapsed().as_secs_f64();
                        if s > 0.2 {
                            i.speed = Some(p.bytes as f64 / s);
                        }
                    }
                }),
                Ev::Ask(id, c) => {
                    let batch = self.item_batch(id);
                    let known = self
                        .all
                        .borrow()
                        .iter()
                        .find(|(b, _)| Some(*b) == batch)
                        .map(|(_, a)| *a);
                    let answer = match known {
                        Some(a) => a,
                        None => match ask(&c) {
                            Some((a, true)) => {
                                if let Some(b) = batch {
                                    self.all.borrow_mut().push((b, a));
                                }
                                a
                            }
                            Some((a, false)) => a,
                            None => Choice::Skip,
                        },
                    };
                    if let Some(w) = self.worker.borrow().as_ref() {
                        let _ = w.tx.send(Cmd::Answer(answer));
                    }
                }
                Ev::Done(id, job, r) => {
                    *self.running.borrow_mut() = None;
                    let name = job.name();
                    let upload_dir = (job.direction == Direction::Upload)
                        .then(|| ifs::parent(&job.remote).to_owned());
                    let (state, note) = match r {
                        Ok(Outcome::Done { bytes, elapsed }) => (
                            State::Done,
                            Some(qsa_msg::message(
                                id::FILE_TRANSFER_COMPLETE,
                                &[&name, &model::bytes_text(bytes), &seconds(elapsed)],
                            )),
                        ),
                        Ok(Outcome::Skipped) => (State::Skipped, None),
                        Ok(Outcome::Cancelled(_)) => (
                            State::Cancelled,
                            Some(qsa_msg::message(id::FILE_TRANSFER_CANCELLED, &[&name])),
                        ),
                        Err(why) => {
                            let m = qsa_msg::message(id::FILE_TRANSFER_FAILED, &[&name, &why]);
                            (State::Failed(why), Some(m))
                        }
                    };
                    let done = state == State::Done;
                    self.with(id, |i| {
                        i.state = state;
                        i.name = job.name();
                        i.job = Some(job);
                        if done {
                            i.progress.offset = i.size;
                        }
                    });
                    if let Some(m) = note {
                        notes.push(Note::Said(m));
                    }
                    if let Some(d) = upload_dir {
                        notes.push(Note::Changed(d));
                    }
                }
                Ev::Planned(id, r) => {
                    *self.running.borrow_mut() = None;
                    let batch = self.item_batch(id).unwrap_or(0);
                    match r {
                        Ok(jobs) => {
                            self.items.borrow_mut().retain(|i| i.id != id);
                            let mut changed = None;
                            for j in jobs {
                                if j.direction == Direction::Upload {
                                    changed = Some(ifs::parent(&j.remote).to_owned());
                                }
                                self.add(batch, Work::File(j));
                            }
                            if let Some(d) = changed {
                                notes.push(Note::Changed(ifs::parent(&d).to_owned()));
                            }
                        }
                        Err(why) => self.with(id, |i| i.state = State::Failed(why)),
                    }
                }
            }
        }
        self.next();
        self.redraw();
        notes
    }

    fn item_batch(&self, id: u64) -> Option<u64> {
        self.items
            .borrow()
            .iter()
            .find(|i| i.id == id)
            .map(|i| i.batch)
    }

    fn with(&self, id: u64, f: impl FnOnce(&mut Item)) {
        if let Some(i) = self.items.borrow_mut().iter_mut().find(|i| i.id == id) {
            f(i);
        }
    }

    /// The panel's buttons.
    pub fn act(&self, a: Act) -> Vec<Note> {
        let selected: Vec<u64> = {
            let items = self.items.borrow();
            self.list
                .selected_rows()
                .into_iter()
                .filter_map(|r| items.get(r).map(|i| i.id))
                .collect()
        };
        let mut notes = Vec::new();
        match a {
            Act::Cancel => {
                for id in &selected {
                    self.cancel(*id);
                }
            }
            Act::Retry => {
                for id in selected {
                    let mut again = None;
                    self.with(id, |i| {
                        if matches!(i.state, State::Failed(_) | State::Cancelled) {
                            i.state = State::Queued;
                            i.speed = None;
                            if let Some(j) = &i.job {
                                if j.resumes() {
                                    let at = j.resume.map(|r| r.remote).unwrap_or(0);
                                    again = Some(qsa_msg::message(
                                        id::RESUMING,
                                        &[&i.name, &model::bytes_text(at)],
                                    ));
                                } else {
                                    i.progress = ifs::Progress::default();
                                }
                            }
                        }
                    });
                    if self
                        .items
                        .borrow()
                        .iter()
                        .any(|i| i.id == id && i.state == State::Queued)
                    {
                        self.order.borrow_mut().push_back(id);
                    }
                    if let Some(m) = again {
                        notes.push(Note::Said(m));
                    }
                }
                self.next();
            }
            Act::RemoveFinished => {
                self.items.borrow_mut().retain(|i| !i.state.finished());
            }
            Act::Clear => {
                let running = *self.running.borrow();
                if let Some(id) = running {
                    self.cancel(id);
                }
                self.order.borrow_mut().clear();
                self.items.borrow_mut().retain(|i| Some(i.id) == running);
            }
        }
        self.redraw();
        notes
    }

    /// Cancels `id`: the one running is told; a queued one never starts.
    pub fn cancel(&self, id: u64) {
        if *self.running.borrow() == Some(id) {
            if let Some(w) = self.worker.borrow().as_ref() {
                w.cancel.store(true, Ordering::SeqCst);
            }
            return;
        }
        self.order.borrow_mut().retain(|i| *i != id);
        self.with(id, |i| {
            if i.state == State::Queued {
                i.state = State::Cancelled;
            }
        });
    }

    /// Cancels everything (the window closes).
    pub fn cancel_all(&self) {
        self.order.borrow_mut().clear();
        let running = *self.running.borrow();
        if let Some(id) = running {
            self.cancel(id);
        }
    }

    fn redraw(&self) {
        let items = self.items.borrow();
        let selected = self.list.selected_rows();
        let mut list = self.list.clone();
        let cells: Vec<Vec<Cell>> = items.iter().map(Item::cells).collect();
        if cells.len() == list.len() {
            // In place, keeping the selection.
            list.set_rows(cells);
            list.select_rows(&selected);
        } else {
            list.set_rows(cells);
        }
    }

    /// The items, for screenshots: added with their states as given.
    pub fn show(&self, items: Vec<(Job, State, ifs::Progress, Option<f64>)>) {
        for (job, state, progress, speed) in items {
            let id = self.id();
            self.items.borrow_mut().push(Item {
                id,
                batch: 0,
                name: job.name(),
                direction: job.direction,
                size: job.size,
                job: Some(job),
                work: None,
                state,
                progress,
                started: None,
                speed,
            });
        }
        self.redraw();
    }
}

fn seconds(d: Duration) -> String {
    format!("{:.1} s", d.as_secs_f64())
}

/// The CCSIDs offered for files made on the system.
pub const CCSIDS: [(u16, &str); 10] = [
    (1208, "1208 UTF-8"),
    (37, "37 EBCDIC US"),
    (273, "273 EBCDIC Germany"),
    (277, "277 EBCDIC Denmark, Norway"),
    (285, "285 EBCDIC UK"),
    (297, "297 EBCDIC France"),
    (500, "500 EBCDIC International"),
    (819, "819 ISO 8859-1"),
    (1252, "1252 Windows Latin 1"),
    (939, "939 Japanese mixed"),
];

/// The policies offered, in the dialog's order.
pub const POLICIES: [(Policy, &str); 4] = [
    (Policy::Ask, "Ask"),
    (Policy::Skip, "Skip"),
    (Policy::Replace, "Replace"),
    (Policy::ReplaceIfNewer, "Replace if newer"),
];

/// What the transfer options dialog answered.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Asked {
    Ok,
    Cancel,
    SaveRequest,
    OpenRequest,
}

/// The transfer options dialog's widgets.
pub struct OptionsDialog {
    pub window: Window,
    binary: RadioRoundButton,
    text: RadioRoundButton,
    ccsid: InputChoice,
    crlf: CheckButton,
    existing: Combo,
    answer: Rc<std::cell::Cell<Option<Asked>>>,
}

/// The CCSID in `text`: its first number.
pub fn ccsid_of(text: &str) -> Option<u16> {
    text.split_whitespace()
        .next()?
        .parse()
        .ok()
        .filter(|c| *c != 0)
}

impl OptionsDialog {
    /// The dialog for `what` (the batch, for its title), showing `o`.
    pub fn build(what: &str, o: Options) -> OptionsDialog {
        let (w, h) = (440, 268);
        let mut window = Window::new(0, 0, w, h, None);
        window.set_label(&format!("Transfer Options - {what}"));
        window.set_color(theme::colors::FACE);
        window.make_modal(true);
        let m = 14;
        let fh = theme::FIELD_H;
        let mut l = controls::label(m, 12, w - 2 * m, fh, "Transfer files as:");
        l.set_align(Align::Left | Align::Inside);
        let g = Group::new(m, 34, w - 2 * m, 46, None);
        let mut binary = controls::radio(
            m + 12,
            34,
            w - 2 * m - 12,
            20,
            "Binary: the bytes as they are",
        );
        let mut text = controls::radio(
            m + 12,
            56,
            w - 2 * m - 12,
            20,
            "Text: converted by the system between its CCSID and UTF-8",
        );
        g.end();
        let mut l = controls::label(m, 90, 150, fh, "CCSID for new files:");
        l.set_align(Align::Left | Align::Inside);
        let mut ccsid = controls::editable_combo(m + 150, 90, w - 2 * m - 150, fh);
        for (_, label) in CCSIDS {
            ccsid.add(label);
        }
        let shown = CCSIDS
            .iter()
            .find(|(c, _)| *c == o.ccsid)
            .map_or_else(|| o.ccsid.to_string(), |(_, l)| (*l).to_owned());
        ccsid.set_value(&shown);
        let crlf = controls::check(
            m,
            122,
            w - 2 * m,
            20,
            "Change line endings: LF on the system, CR LF on this computer",
        );
        crlf.set_checked(o.crlf);
        let mut l = controls::label(m, 154, 150, fh, "If a file already exists:");
        l.set_align(Align::Left | Align::Inside);
        let mut existing = controls::combo(m + 150, 154, 160, fh);
        for (_, label) in POLICIES {
            existing.add_choice(label);
        }
        existing.set_value(
            POLICIES
                .iter()
                .position(|(p, _)| *p == o.existing)
                .unwrap_or(0) as i32,
        );
        let mut note = fltk::frame::Frame::new(m, 184, w - 2 * m, 36, None);
        note.set_label(
            "Text mode changes only line endings here; the characters are converted by the system.",
        );
        note.set_align(Align::Left | Align::Inside | Align::Wrap | Align::Top);
        note.set_label_font(theme::UI_FONT);
        note.set_label_size(theme::UI_SIZE);
        note.set_label_color(theme::colors::DISABLED_TEXT);
        let (bw, bh) = (theme::BUTTON_W, theme::BUTTON_H);
        let by = h - bh - 12;
        let answer = Rc::new(std::cell::Cell::new(None));
        let mut save = controls::button(m, by, 110, bh, "Save Request...");
        let mut open = controls::button(m + 116, by, 110, bh, "Open Request...");
        let mut ok = controls::default_button(w - m - 2 * bw - 6, by, bw, bh, "OK");
        let mut cancel = controls::button(w - m - bw, by, bw, bh, "Cancel");
        window.end();
        for (b, a) in [
            (&mut save, Asked::SaveRequest),
            (&mut open, Asked::OpenRequest),
            (&mut ok, Asked::Ok),
            (&mut cancel, Asked::Cancel),
        ] {
            let answer = Rc::clone(&answer);
            let mut win = window.clone();
            b.set_callback(move |_| {
                answer.set(Some(a));
                win.hide();
            });
        }
        {
            let answer = Rc::clone(&answer);
            window.set_callback(move |w| {
                answer.set(Some(Asked::Cancel));
                w.hide();
            });
        }
        // Line endings only mean something for text.
        let lines = crlf.clone();
        let set = move |text: bool| {
            let mut c = lines.clone();
            if text {
                c.activate();
            } else {
                c.deactivate();
            }
        };
        let s1 = set.clone();
        text.set_callback(move |_| s1(true));
        let s2 = set.clone();
        binary.set_callback(move |_| s2(false));
        set(o.text);
        if o.text {
            text.set_value(true);
        } else {
            binary.set_value(true);
        }
        OptionsDialog {
            window,
            binary,
            text,
            ccsid,
            crlf,
            existing,
            answer,
        }
    }

    /// The options as shown now; the CCSID as entered, `None` when it is
    /// not a number.
    pub fn options(&self) -> Option<Options> {
        Some(Options {
            text: self.text.value() && !self.binary.value(),
            ccsid: ccsid_of(&self.ccsid.value().unwrap_or_default())?,
            crlf: self.crlf.is_checked(),
            existing: POLICIES
                .get(usize::try_from(self.existing.value()).unwrap_or(0))
                .map_or(Policy::Ask, |(p, _)| *p),
        })
    }

    /// Shows the dialog over `parent` until a button is chosen.
    pub fn run(&mut self, parent: Option<&dyn WindowExt>) -> Asked {
        self.answer.set(None);
        qsa_ui::propdialog::center_on(&mut self.window, parent);
        self.window.show();
        while self.window.shown() {
            app::wait();
        }
        self.answer.get().unwrap_or(Asked::Cancel)
    }
}

/// The existing-file prompt: `QAD8010` with Replace, Skip, Replace All,
/// Skip All and, for a download, Keep Both.
pub fn existing_box(c: &Conflict) -> qsa_ui::MessageBox {
    let m = qsa_msg::message(id::ALREADY_EXISTS, &[&c.name, &c.folder]);
    qsa_ui::msgdetails::ask_message_box(&m, existing_buttons(c.keep_both))
}

fn existing_buttons(keep_both: bool) -> &'static [&'static str] {
    if keep_both {
        &["Replace", "Skip", "Replace All", "Skip All", "Keep Both"]
    } else {
        &["Replace", "Skip", "Replace All", "Skip All"]
    }
}

/// Asks about an existing file: the answer, and whether it holds for the
/// rest of the batch.
pub fn existing_prompt(parent: Option<&dyn WindowExt>, c: &Conflict) -> Option<(Choice, bool)> {
    let i = existing_box(c).run(parent);
    Some(match i {
        0 => (Choice::Replace, false),
        1 => (Choice::Skip, false),
        2 => (Choice::Replace, true),
        3 => (Choice::Skip, true),
        4 if c.keep_both => (Choice::KeepBoth, false),
        _ => return None,
    })
}

/// The window's own connection for its transfers: attached to `h`'s
/// session, or, when the session has its 16 connections, the shared one
/// (`QAD8030`, said once by the caller). `None` after another failure,
/// shown.
pub fn connect(
    parent: Option<&dyn WindowExt>,
    h: &Handle,
    system: &qsa_config::System,
) -> Option<(Conn, bool)> {
    match qap::busy(|| qap::lock(h).attach()) {
        Ok(a) => {
            crate::log!(
                "{}",
                qap::attached_message(&system.name, super::FUNCTION, &a.job().job)
            );
            Some((Conn::Attached(Box::new(a)), false))
        }
        Err(e) if qap::no_more_connections(&e) => {
            if let Some(sm) = e.server_message() {
                crate::log!("{}: {}", sm.id, sm.text);
            }
            Some((Conn::Shared(h.clone()), true))
        }
        Err(e) => {
            qap::show(parent, &qap::explain(system, &e));
            None
        }
    }
}

/// The jobs and folders for downloading `rows` of the folder shown into
/// `into`, with `options`.
pub fn downloads(rows: &[model::Row], into: &Path, options: Options) -> Vec<Work> {
    rows.iter()
        .filter_map(|r| {
            let (path, entry) = r.opens_as()?;
            if r.is_real_folder() {
                Some(Work::Download {
                    remote: path,
                    into: into.to_path_buf(),
                    options,
                })
            } else if r.is_folder() {
                // A link to a folder is not followed.
                None
            } else {
                let mut j = qsa_transfer::files::transfer::download_job(
                    ifs::parent(&path),
                    &entry,
                    into,
                    options,
                );
                // Saved under the name it has in the list.
                j.local = into.join(&r.entry.name);
                Some(Work::File(j))
            }
        })
        .collect()
}

/// The jobs and folders for uploading `paths` into the system's folder
/// `into`, with `options`.
pub fn uploads(paths: &[PathBuf], into: &str, options: Options) -> Vec<Work> {
    paths
        .iter()
        .map(|p| {
            if p.is_dir() {
                Work::Upload {
                    local: p.clone(),
                    into: into.to_owned(),
                    options,
                }
            } else {
                Work::File(qsa_transfer::files::transfer::upload_job(p, into, options))
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ccsids_and_speeds() {
        assert_eq!(ccsid_of("1208 UTF-8"), Some(1208));
        assert_eq!(ccsid_of(" 37 "), Some(37));
        assert_eq!(ccsid_of("0"), None);
        assert_eq!(ccsid_of("UTF-8"), None);
        assert_eq!(speed_text(1.5 * 1024.0 * 1024.0), "1.5 MiB/s");
        assert_eq!(existing_buttons(true).len(), 5);
        assert_eq!(existing_buttons(false).len(), 4);
    }
}
