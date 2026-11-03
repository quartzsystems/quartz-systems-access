// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! Run SQL: SQL statements on a system, over a connection of the window's
//! own (QAP version 4's `Sql` service), from the main window's Database
//! group.
//!
//! - The script editor is in the system-output font; the status bar shows
//!   the cursor's line and column. Statements are split on `;` outside
//!   strings, delimited identifiers and comments ([`script`]). Run
//!   Statement runs the one at the cursor (Ctrl+Enter), Run Selected the
//!   selection's, Run All every one (F5), stopping at the first failure
//!   when Stop at First Error is on.
//! - The options row: the default schema (from `ListSchemas`, or typed),
//!   SQL or System naming, the commit level (None by default), Commit and
//!   Rollback (only under a commit level), Run, Cancel and Clear Results.
//!   While changes are uncommitted the level, naming and schema cannot
//!   change (the system says `InUse`): changing them first asks Commit,
//!   Roll Back or Cancel, as closing the window does (`QAD7021`).
//! - Results: a tab per result set, its column headers the SQL names with
//!   the label and type as their tooltip; rows 500 at a time, with Fetch
//!   More and Fetch All; NULL a grey `-`. Copy Cell and Copy Row copy
//!   tab-separated text. A result cut short (cancelled, the connection
//!   lost, the session ended) keeps its rows, marked Incomplete in its tab
//!   and the status bar (`QAD7011`), still to copy or export.
//! - Messages: every statement's message as the system sent it, with its
//!   ID, SQLSTATE and SQLCODE, the rows changed or fetched, and the time
//!   it took; a double-click on one opens its second-level text.
//! - File: New, Open, Save and Save As for `.sql` files, and the recent
//!   ones; Run > History lists the last 100 statements run on the system.
//! - The window's connection is attached to the session, its title naming
//!   the system and its job (`QAD7001`). Closing the window closes the
//!   connection; it never signs off. A lost connection keeps what was shown
//!   and says the statement must be run again (`QAD7010`): the next run
//!   attaches anew, and nothing runs again by itself. When the session
//!   ends, the window says so with the system's message and offers to sign
//!   on again.
//! - Before QAP version 4 the window does not open: `QAD5001`.
//!
//! The connection's work runs on its own thread ([`worker`]); a repeating
//! timeout takes its events into the window, which never waits on the
//! network.

pub mod script;
pub mod worker;

use std::cell::{Cell as StdCell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::Instant;

use fltk::app;
use fltk::button::Button;
use fltk::enums::{Align, Event, Key, Shortcut};
use fltk::frame::Frame;
use fltk::menu::{Choice, MenuBar, MenuFlag, MenuItem};
use fltk::misc::InputChoice;
use fltk::prelude::*;
use fltk::text::{TextBuffer, TextEditor};
use fltk::window::Window;
use qap_client::proto::Op;
use qap_client::proto::ops::sql::{Column, CommitLevel, Naming, SetOptions, SqlValue};
use qap_client::proto::types::JobId;
use qsa_config::System;
use qsa_msg::id;
use qsa_sql::{BitData, DateStyle, Style};
use qsa_ui::chan::{self, Emit, MenuEmit};
use qsa_ui::listtable::{Cell, ListTable};
use qsa_ui::sampler::{MENU_H, menu_bar};
use qsa_ui::statusbar::{Panel, StatusBar};
use qsa_ui::theme::{self, colors};
use qsa_ui::{Status, Tabs, controls, msgdetails};

use crate::app::Shared;
use crate::qap;

use worker::{Cmd, Detail, Ev, Line, Standing};

/// Window width and height.
pub const W: i32 = 960;
pub const H: i32 = 660;

/// Rows a result fetches at a time.
pub const FETCH_ROWS: u32 = 500;

/// Rows Schemas' View data fetches first.
pub const VIEW_ROWS: u32 = 1000;

/// The Messages tab's columns: title and width.
pub const MESSAGE_COLUMNS: [(&str, i32); 8] = [
    ("#", 34),
    ("Line", 40),
    ("ID", 70),
    ("SQLSTATE", 70),
    ("SQLCODE", 64),
    ("Rows", 64),
    ("Elapsed", 64),
    ("Text", 460),
];

/// The commit levels offered, in the protocol's order.
pub const COMMIT_LEVELS: [(&str, CommitLevel); 5] = [
    ("None", CommitLevel::None),
    ("Read uncommitted", CommitLevel::ReadUncommitted),
    ("Read committed", CommitLevel::ReadCommitted),
    ("Repeatable read", CommitLevel::RepeatableRead),
    ("Serializable", CommitLevel::Serializable),
];

/// The namings offered.
pub const NAMINGS: [(&str, Naming); 2] = [("SQL", Naming::Sql), ("System", Naming::System)];

/// What the window's controls ask for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Act {
    New,
    Open,
    Save,
    SaveAs,
    Recent(usize),
    Close,
    RunStatement,
    RunSelected,
    RunAll,
    StopOnError,
    History,
    Cancel,
    Commit,
    Rollback,
    CommitLevel,
    Naming,
    Schema,
    Clear,
    FetchMore,
    FetchAll,
    CopyCell,
    CopyRow,
    Details,
    BitData,
    Export,
}

/// A result as a screenshot shows it: its columns, rows and how it stands.
pub type ShownResult = (Vec<Column>, Vec<Vec<SqlValue>>, Standing);

/// A result tab's widgets and rows.
struct ResultTab {
    list: ListTable,
    columns: Vec<Column>,
    /// The rows as the system sent them, for copy and export.
    rows: Vec<Vec<SqlValue>>,
    standing: Standing,
    more: Button,
    all: Button,
    info: Frame,
    /// Its page in the tabs.
    page: usize,
}

/// The window's widgets.
pub struct Widgets {
    pub window: Window,
    menu: MenuBar,
    pub editor: TextEditor,
    pub buffer: TextBuffer,
    pub schema: InputChoice,
    pub naming: Choice,
    pub commit_level: Choice,
    commit: Button,
    rollback: Button,
    run: Button,
    cancel: Button,
    clear: Button,
    pub tabs: Tabs,
    pub messages: ListTable,
    pub message: Panel,
    pub position: Panel,
    pub status: Panel,
}

/// What the window keeps.
struct State {
    system: System,
    job: Option<JobId>,
    profile: String,
    file: Option<PathBuf>,
    /// The script's text when last opened or saved.
    saved: String,
    results: Vec<ResultTab>,
    lines: Vec<Line>,
    running: bool,
    started: Option<Instant>,
    cmd: Option<mpsc::Sender<Cmd>>,
    events: Option<mpsc::Receiver<Ev>>,
    cancel: Arc<AtomicBool>,
    /// The options the connection has.
    options: SetOptions,
    uncommitted: bool,
    ended: bool,
    style: Style,
    /// The job's CCSID, for CCSID 65535 data shown as text.
    job_ccsid: u16,
    position: (i32, i32),
    stop_on_error: bool,
    /// Schemas for the combo, from a thread on the shared connection.
    schemas: Option<mpsc::Receiver<Vec<String>>>,
}

/// One Run SQL window.
pub struct RunSql {
    pub w: Widgets,
    tx: chan::Sender<Act>,
    rx: chan::Receiver<Act>,
    st: RefCell<State>,
    state: Option<Shared>,
}

thread_local! {
    static WINDOWS: RefCell<Vec<Rc<RunSql>>> = const { RefCell::new(Vec::new()) };
    static TICKING: StdCell<bool> = const { StdCell::new(false) };
    static PUMPING: StdCell<bool> = const { StdCell::new(false) };
}

/// How often the windows look at their workers, in seconds.
const POLL: f64 = 0.05;

/// A menu label with `/`, `&` and `\` kept as they are.
fn menu_text(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('/', "\\/")
        .replace('&', "&&")
}

fn build_widgets(
    system: &str,
    stop_on_error: bool,
) -> (Widgets, chan::Sender<Act>, chan::Receiver<Act>) {
    let (tx, rx) = chan::channel::<Act>();
    let mut window = Window::new(0, 0, W, H, None);
    window.set_label(&qap::window_title("Run SQL", system, None));
    window.set_icon(Some(qsa_ui::glyph::app_icon()));
    window.set_color(colors::FACE);
    window.size_range(640, 420, 0, 0);

    let mut menu = menu_bar(0, 0, W);
    {
        let mut add = |path: &str, sc: Shortcut, a: Act, flag: MenuFlag| {
            menu.add_emit_to(path, sc, flag, tx, a);
        };
        let n = MenuFlag::Normal;
        let d = MenuFlag::MenuDivider;
        add("&File/&New", Shortcut::Ctrl | 'n', Act::New, n);
        add("&File/&Open...", Shortcut::Ctrl | 'o', Act::Open, n);
        add("&File/&Save", Shortcut::Ctrl | 's', Act::Save, n);
        add("&File/Save &As...", Shortcut::None, Act::SaveAs, d);
        add("&File/&Close", Shortcut::Alt | Key::F4, Act::Close, n);
        add(
            "&Edit/Copy &Cell",
            Shortcut::Ctrl | Shortcut::Shift | 'c',
            Act::CopyCell,
            n,
        );
        add(
            "&Edit/Copy &Row",
            Shortcut::Ctrl | Shortcut::Shift | 'r',
            Act::CopyRow,
            n,
        );
        add(
            "&Run/Run &Statement",
            Shortcut::Ctrl | Key::Enter,
            Act::RunStatement,
            n,
        );
        add(
            "&Run/Run S&elected",
            Shortcut::Ctrl | Shortcut::Shift | Key::Enter,
            Act::RunSelected,
            n,
        );
        add("&Run/Run &All", Shortcut::None | Key::F5, Act::RunAll, d);
        let stop = if stop_on_error {
            MenuFlag::Toggle | MenuFlag::Value
        } else {
            MenuFlag::Toggle
        };
        add(
            "&Run/Stop at &First Error",
            Shortcut::None,
            Act::StopOnError,
            stop | d,
        );
        add("&Run/&Cancel", Shortcut::None, Act::Cancel, d);
        add("&Run/&History...", Shortcut::Ctrl | 'h', Act::History, n);
        add("Re&sults/Fetch &More", Shortcut::None, Act::FetchMore, n);
        add("Re&sults/Fetch &All", Shortcut::None, Act::FetchAll, d);
        add("Re&sults/&Export...", Shortcut::Ctrl | 'e', Act::Export, d);
        add("Re&sults/&Clear Results", Shortcut::None, Act::Clear, n);
        add(
            "&Options/Show CCSID 65535 Data as &Text",
            Shortcut::None,
            Act::BitData,
            MenuFlag::Toggle,
        );
    }

    // The options row.
    let m = 8;
    let fh = theme::FIELD_H;
    let (bw, bh) = (theme::BUTTON_W, theme::BUTTON_H);
    let oy = MENU_H + 6;
    let mut run = controls::default_button(m, oy - 1, bw, bh, "Run");
    run.set_tooltip("Ctrl+Enter runs the statement at the cursor; F5 runs them all.");
    run.emit_to(tx, Act::RunStatement);
    let mut cancel = controls::button(m + bw + 4, oy - 1, bw, bh, "Cancel");
    cancel.emit_to(tx, Act::Cancel);
    let mut x = m + 2 * bw + 18;
    let label = |x: &mut i32, w: i32, t: &str| {
        let mut f = controls::label(*x, oy, w, fh, t);
        f.set_align(Align::Left | Align::Inside);
        *x += w;
    };
    label(&mut x, 50, "Schema:");
    let mut schema = controls::editable_combo(x, oy, 120, fh);
    schema.set_tooltip("The schema unqualified names are in; blank for the default.");
    schema.set_trigger(
        fltk::enums::CallbackTrigger::EnterKeyAlways | fltk::enums::CallbackTrigger::Changed,
    );
    schema.set_callback(move |c| {
        // Typing waits for Enter; a choice from the list goes at once.
        if app::event() != Event::KeyDown || app::event_key() == Key::Enter {
            let _ = c;
            tx.send(Act::Schema);
        }
    });
    x += 120 + 10;
    label(&mut x, 50, "Naming:");
    let mut naming = controls::combo(x, oy, 74, fh);
    for (t, _) in NAMINGS {
        naming.add_choice(t);
    }
    naming.set_value(0);
    naming.emit_to(tx, Act::Naming);
    x += 74 + 10;
    label(&mut x, 80, "Commit level:");
    let mut commit_level = controls::combo(x, oy, 120, fh);
    for (t, _) in COMMIT_LEVELS {
        commit_level.add_choice(t);
    }
    commit_level.set_value(0);
    commit_level.emit_to(tx, Act::CommitLevel);
    x += 120 + 8;
    let mut commit = controls::button(x, oy - 1, bw, bh, "Commit");
    commit.emit_to(tx, Act::Commit);
    x += bw + 4;
    let mut rollback = controls::button(x, oy - 1, bw, bh, "Rollback");
    rollback.emit_to(tx, Act::Rollback);
    let mut clear = controls::button(W - m - bw - 16, oy - 1, bw + 16, bh, "Clear Results");
    clear.emit_to(tx, Act::Clear);

    // The editor.
    let ey = oy + bh + 6;
    let eh = 190;
    let buffer = TextBuffer::default();
    let mut editor = TextEditor::new(m, ey, W - 2 * m, eh, None);
    editor.set_buffer(buffer.clone());
    editor.set_text_font(theme::MONO_FONT);
    editor.set_text_size(theme::MONO_SIZE);
    editor.set_cursor_style(fltk::text::Cursor::Simple);
    editor.set_frame(fltk::enums::FrameType::DownBox);
    editor.set_scrollbar_size(16);

    // Results and messages.
    let ty = ey + eh + 8;
    let th = H - ty - StatusBar::HEIGHT - 4;
    let mut tabs = Tabs::new(m, ty, W - 2 * m, th);
    let mut messages = None;
    tabs.add_page("Messages", |r| {
        let mut l = ListTable::new(r.x, r.y, r.w, r.h, &MESSAGE_COLUMNS);
        l.on_activate(move |row| {
            if row.is_some() {
                tx.send(Act::Details);
            }
        });
        messages = Some(l);
    });
    let messages = messages.expect("the Messages page is built");

    // The status bar: a message, the cursor's place, and the system.
    let sy = H - StatusBar::HEIGHT;
    let mut bar = fltk::group::Group::new(0, sy, W, StatusBar::HEIGHT, None);
    bar.set_frame(fltk::enums::FrameType::FlatBox);
    bar.set_color(colors::FACE);
    let (pw, sw) = (110, 260);
    let message = Panel::new(2, sy + 3, W - pw - sw - 8, StatusBar::HEIGHT - 5);
    let mut position = Panel::new(W - pw - sw - 4, sy + 3, pw, StatusBar::HEIGHT - 5);
    position.set("Ln 1, Col 1", qsa_ui::Status::Neutral);
    let status = Panel::new(W - sw - 2, sy + 3, sw, StatusBar::HEIGHT - 5);
    bar.end();
    bar.resizable(message.widget());

    window.resizable(tabs.group());
    window.end();
    window.set_callback(move |_| tx.send(Act::Close));
    let w = Widgets {
        window,
        menu,
        editor,
        buffer,
        schema,
        naming,
        commit_level,
        commit,
        rollback,
        run,
        cancel,
        clear,
        tabs,
        messages,
        message,
        position,
        status,
    };
    (w, tx, rx)
}

/// The commit level at `i` in [`COMMIT_LEVELS`].
fn level_at(i: i32) -> CommitLevel {
    usize::try_from(i)
        .ok()
        .and_then(|i| COMMIT_LEVELS.get(i))
        .map_or(CommitLevel::None, |(_, l)| *l)
}

fn level_index(l: CommitLevel) -> i32 {
    COMMIT_LEVELS
        .iter()
        .position(|(_, x)| *x == l)
        .map_or(0, |i| i as i32)
}

fn naming_index(n: Naming) -> i32 {
    NAMINGS
        .iter()
        .position(|(_, x)| *x == n)
        .map_or(0, |i| i as i32)
}

/// A message line as table cells.
pub fn message_row(l: &Line) -> Vec<Cell> {
    let status = if l.failed {
        Status::Error
    } else {
        Status::Neutral
    };
    let num = |n: usize| if n == 0 { String::new() } else { n.to_string() };
    vec![
        Cell::from(num(l.statement)),
        Cell::from(num(l.script_line)),
        Cell {
            text: l.id.clone(),
            status,
        },
        Cell::from(l.sqlstate.as_str()),
        Cell::from(l.sqlcode.map(|c| c.to_string()).unwrap_or_default()),
        Cell::from(l.rows.map(|r| r.to_string()).unwrap_or_default()),
        Cell::from(elapsed(l.elapsed)),
        Cell::from(l.text.as_str()),
    ]
}

/// A duration as Elapsed shows it: `0.042 s`.
pub fn elapsed(d: std::time::Duration) -> String {
    format!("{:.3} s", d.as_secs_f64())
}

/// A row of values as grid cells: NULL is a grey `-`.
pub fn value_cells(row: &[SqlValue], columns: &[Column], style: &Style) -> Vec<Cell> {
    row.iter()
        .zip(columns)
        .map(|(v, c)| match qsa_sql::value::display(v, c, style) {
            Some(t) => Cell::from(t),
            None => Cell {
                text: "-".into(),
                status: Status::Muted,
            },
        })
        .collect()
}

/// A column header's tooltip: its label and type.
pub fn header_tip(c: &Column) -> String {
    let label = c.label.split_whitespace().collect::<Vec<_>>().join(" ");
    let null = if c.nullable { "" } else { ", not null" };
    format!("{label}\n{}{null}", qsa_sql::value::type_name(c))
}

/// A column's width in the grid, from its name and length.
fn column_width(c: &Column) -> i32 {
    let chars = match qsa_sql::value::kind(c) {
        qsa_sql::Kind::Timestamp => 20 + i32::from(c.scale),
        qsa_sql::Kind::Date => 10,
        qsa_sql::Kind::Time => 8,
        qsa_sql::Kind::Boolean => 5,
        qsa_sql::Kind::Integer { .. } => 11,
        _ => i32::try_from(c.length).unwrap_or(24).clamp(4, 24),
    };
    let name = i32::try_from(c.name.chars().count()).unwrap_or(10);
    (chars.max(name) * 7 + 16).clamp(56, 320)
}

/// The info line under a result.
pub fn result_info(rows: usize, standing: &Standing) -> String {
    let n = rows.to_string();
    match standing {
        Standing::More => format!("{n} rows; more to fetch."),
        Standing::Complete => format!("{n} rows."),
        Standing::Incomplete(why) => format!("{n} rows. Incomplete ({why})."),
    }
}

/// A result's tab title.
pub fn result_title(index: usize, standing: &Standing) -> String {
    match standing {
        Standing::Incomplete(why) => format!("Result {} - Incomplete ({why})", index + 1),
        _ => format!("Result {}", index + 1),
    }
}

impl RunSql {
    fn new(system: &System, state: Option<Shared>) -> RunSql {
        let stop = state
            .as_ref()
            .is_some_and(|s| s.borrow().settings.run_sql.stop_on_error);
        let (w, tx, rx) = build_widgets(&system.name, stop);
        let st = State {
            system: system.clone(),
            job: None,
            profile: String::new(),
            file: None,
            saved: String::new(),
            results: Vec::new(),
            lines: Vec::new(),
            running: false,
            started: None,
            cmd: None,
            events: None,
            cancel: Arc::new(AtomicBool::new(false)),
            options: SetOptions::default(),
            uncommitted: false,
            ended: false,
            style: Style::default(),
            job_ccsid: 37,
            position: (0, 0),
            stop_on_error: stop,
            schemas: None,
        };
        let r = RunSql {
            w,
            tx,
            rx,
            st: RefCell::new(st),
            state,
        };
        r.refresh_recent();
        r.buttons();
        r
    }

    /// The window for `system` showing `script`, `lines` and the results
    /// given (columns and rows, and how each stands), not shown and with
    /// no connection: for screenshots.
    pub fn build(
        system: &System,
        job: Option<&JobId>,
        script: &str,
        lines: &[Line],
        results: &[ShownResult],
        options: &SetOptions,
        uncommitted: bool,
    ) -> RunSql {
        let r = RunSql::new(system, None);
        {
            let mut st = r.st.borrow_mut();
            st.job = job.cloned();
            st.options = options.clone();
            st.uncommitted = uncommitted;
            st.style = Style {
                dates: DateStyle::job("*MDY", "/", ":"),
                ..Style::default()
            };
        }
        let mut w = r.w.window.clone();
        w.set_label(&qap::window_title("Run SQL", &system.name, job));
        r.w.buffer.clone().set_text(script);
        for l in lines {
            r.add_line(l.clone());
        }
        for (i, (columns, rows, standing)) in results.iter().enumerate() {
            r.add_result(i, columns.clone());
            r.add_rows(i, rows.clone());
            r.set_standing(i, standing.clone());
        }
        r.show_options();
        r.buttons();
        r.w.status
            .clone()
            .set(&format!("{}: {}", system.name, "BOB"), Status::Active);
        r
    }

    pub fn window(&self) -> &Window {
        &self.w.window
    }

    fn parent(&self) -> Option<&dyn WindowExt> {
        Some(&self.w.window as &dyn WindowExt)
    }

    fn report(&self, text: &str, status: Status) {
        self.w.message.clone().set(text, status);
    }

    /// Enables what the state allows.
    fn buttons(&self) {
        let st = self.st.borrow();
        let connected = st.cmd.is_some() && !st.ended;
        let under_control = st.options.commit != CommitLevel::None;
        let idle = !st.running;
        let set = |b: &Button, on: bool| {
            let mut b = b.clone();
            if on {
                b.activate();
            } else {
                b.deactivate();
            }
        };
        let preview = st.cmd.is_none() && !st.ended;
        set(
            &self.w.commit,
            (connected || preview) && under_control && idle,
        );
        set(
            &self.w.rollback,
            (connected || preview) && under_control && idle,
        );
        set(&self.w.run, (connected || preview) && idle);
        set(&self.w.cancel, st.running);
        set(&self.w.clear, idle);
        for r in &st.results {
            let more = idle && r.standing == Standing::More;
            set(&r.more, more);
            set(&r.all, more);
        }
    }

    /// The options row as the connection has them.
    fn show_options(&self) {
        let st = self.st.borrow();
        self.w
            .commit_level
            .clone()
            .set_value(level_index(st.options.commit));
        self.w
            .naming
            .clone()
            .set_value(naming_index(st.options.naming));
        self.w
            .schema
            .clone()
            .set_value(st.options.default_schema.as_deref().unwrap_or(""));
    }

    fn send(&self, c: Cmd) -> bool {
        let st = self.st.borrow();
        match &st.cmd {
            Some(tx) if !st.ended => tx.send(c).is_ok(),
            _ => false,
        }
    }

    fn add_line(&self, l: Line) {
        let mut m = self.w.messages.clone();
        m.append_rows(vec![message_row(&l)]);
        self.st.borrow_mut().lines.push(l);
    }

    fn add_result(&self, index: usize, columns: Vec<Column>) {
        let titles: Vec<(String, i32)> = columns
            .iter()
            .map(|c| (c.name.clone(), column_width(c)))
            .collect();
        let cols: Vec<(&str, i32)> = titles.iter().map(|(t, w)| (t.as_str(), *w)).collect();
        let tips: Vec<Option<String>> = columns.iter().map(|c| Some(header_tip(c))).collect();
        let tx = self.tx();
        let mut parts = None;
        let mut tabs = self.w.tabs.clone();
        let page = tabs.len();
        tabs.add_page(&result_title(index, &Standing::More), |r| {
            let bh = theme::BUTTON_H;
            let mut list = ListTable::new(r.x, r.y, r.w, r.h - bh - 6, &cols);
            list.set_header_tooltips(&tips);
            list.on_context(move |row| {
                if row.is_none() {
                    return;
                }
                let items = [("Copy Cell", Act::CopyCell), ("Copy Row", Act::CopyRow)];
                let labels: Vec<&str> = items.iter().map(|(l, _)| *l).collect();
                if let Some(item) = MenuItem::new(&labels).popup(app::event_x(), app::event_y())
                    && let Some((_, a)) = items
                        .iter()
                        .find(|(l, _)| item.label().as_deref() == Some(*l))
                {
                    tx.send(*a);
                }
            });
            let by = r.y + r.h - bh;
            let mut more = controls::button(r.x, by, theme::BUTTON_W + 10, bh, "Fetch More");
            more.emit_to(tx, Act::FetchMore);
            let mut all = controls::button(
                r.x + theme::BUTTON_W + 16,
                by,
                theme::BUTTON_W,
                bh,
                "Fetch All",
            );
            all.emit_to(tx, Act::FetchAll);
            let mut info = Frame::new(
                r.x + 2 * theme::BUTTON_W + 30,
                by,
                r.w - 2 * theme::BUTTON_W - 30,
                bh,
                None,
            );
            info.set_align(Align::Left | Align::Inside);
            info.set_label_size(theme::UI_SIZE);
            parts = Some((list, more, all, info));
        });
        let (list, more, all, info) = parts.expect("the result page is built");
        tabs.select(page);
        let mut st = self.st.borrow_mut();
        debug_assert_eq!(st.results.len(), index, "results arrive in order");
        st.results.push(ResultTab {
            list,
            columns,
            rows: Vec::new(),
            standing: Standing::More,
            more,
            all,
            info,
            page,
        });
    }

    fn add_rows(&self, index: usize, rows: Vec<Vec<SqlValue>>) {
        let mut st = self.st.borrow_mut();
        let style = st.style;
        let Some(r) = st.results.get_mut(index) else {
            return;
        };
        let cells: Vec<Vec<Cell>> = rows
            .iter()
            .map(|row| value_cells(row, &r.columns, &style))
            .collect();
        r.list.append_rows(cells);
        r.rows.extend(rows);
        let info = result_info(r.rows.len(), &r.standing);
        r.info.set_label(&info);
    }

    fn set_standing(&self, index: usize, standing: Standing) {
        let (title, page, incomplete) = {
            let mut st = self.st.borrow_mut();
            let Some(r) = st.results.get_mut(index) else {
                return;
            };
            r.standing = standing.clone();
            r.info.set_label(&result_info(r.rows.len(), &standing));
            r.info.redraw();
            (
                result_title(index, &standing),
                r.page,
                matches!(standing, Standing::Incomplete(_)),
            )
        };
        self.w.tabs.clone().set_title(page, &title);
        if let Standing::Incomplete(why) = &standing
            && incomplete
        {
            let m = qsa_msg::message(id::RESULT_INCOMPLETE, &[why]);
            self.report(&m.to_string(), Status::Error);
        }
        self.buttons();
    }

    fn tx(&self) -> chan::Sender<Act> {
        self.tx
    }

    /// The menu's Recent items, from the settings.
    fn refresh_recent(&self) {
        let Some(state) = &self.state else { return };
        let recent = state.borrow().settings.run_sql.recent.clone();
        let mut menu = self.w.menu.clone();
        while let Some(i) = (0..menu.size()).find(|&i| {
            menu.at(i)
                .and_then(|m| menu.item_pathname(Some(&m)).ok())
                .is_some_and(|p| p.starts_with("&File/Recent &Scripts/"))
        }) {
            menu.remove(i);
        }
        let at = menu.find_index("&File/&Close");
        for (n, p) in recent
            .iter()
            .enumerate()
            .take(qsa_config::settings::RECENT_MAX)
        {
            let label = format!(
                "&File/Recent &Scripts/&{} {}",
                n + 1,
                menu_text(&p.display().to_string())
            );
            let tx = self.tx;
            menu.insert(at, &label, Shortcut::None, MenuFlag::Normal, move |_| {
                tx.send(Act::Recent(n))
            });
        }
    }

    /// Takes in what the controls asked for and what the worker said;
    /// false once the window is closed and its connection with it.
    fn pump(&self) -> bool {
        while let Some(a) = self.rx.recv() {
            self.act(a);
        }
        self.drain();
        self.track_cursor();
        self.take_schemas();
        self.w.window.shown()
    }

    /// The editor's cursor, as line and column in the status bar.
    fn track_cursor(&self) {
        let pos = self.w.editor.insert_position();
        let buf = &self.w.buffer;
        let line = buf.count_lines(0, pos) + 1;
        let start = buf.line_start(pos);
        let col = buf
            .text_range(start, pos)
            .map_or(0, |t| t.chars().count() as i32)
            + 1;
        let mut st = self.st.borrow_mut();
        if st.position != (line, col) {
            st.position = (line, col);
            self.w
                .position
                .clone()
                .set(&format!("Ln {line}, Col {col}"), Status::Neutral);
        }
    }

    fn take_schemas(&self) {
        let got = {
            let st = self.st.borrow();
            st.schemas.as_ref().and_then(|rx| rx.try_recv().ok())
        };
        if let Some(names) = got {
            self.st.borrow_mut().schemas = None;
            let mut c = self.w.schema.clone();
            let keep = c.value().unwrap_or_default();
            c.clear();
            for n in names {
                c.add(&menu_text(&n));
            }
            c.set_value(&keep);
        }
    }

    /// The worker's events.
    fn drain(&self) {
        loop {
            let ev = {
                let st = self.st.borrow();
                match st.events.as_ref().map(mpsc::Receiver::try_recv) {
                    Some(Ok(ev)) => ev,
                    _ => return,
                }
            };
            self.event(ev);
        }
    }

    fn event(&self, ev: Ev) {
        match ev {
            Ev::Started { index, of, line } => {
                self.report(
                    &format!("Running statement {index} of {of} (line {line})..."),
                    Status::Neutral,
                );
            }
            Ev::Result { result, columns } => self.add_result(result, columns),
            Ev::Rows { result, rows } => self.add_rows(result, rows),
            Ev::Standing { result, standing } => self.set_standing(result, standing),
            Ev::Message(l) => {
                if l.failed {
                    self.report(&format!("{}: {}", l.id, l.text), Status::Error);
                }
                self.add_line(l);
            }
            Ev::Done { cancelled } => {
                let took = self.st.borrow_mut().started.take().map(|t| t.elapsed());
                self.st.borrow_mut().running = false;
                if cancelled {
                    self.report("Cancelled.", Status::Neutral);
                } else if self.w.message.text().starts_with("Running") {
                    let failed = self
                        .st
                        .borrow()
                        .lines
                        .iter()
                        .rev()
                        .take_while(|l| l.statement > 0)
                        .any(|l| l.failed);
                    let t = took.map(elapsed).unwrap_or_default();
                    let text = if failed {
                        format!("Ended with an error after {t}.")
                    } else {
                        format!("Done in {t}.")
                    };
                    self.report(
                        &text,
                        if failed {
                            Status::Error
                        } else {
                            Status::Neutral
                        },
                    );
                }
                self.buttons();
            }
            Ev::Options(Ok(o)) => {
                self.st.borrow_mut().options = o;
                self.show_options();
                self.buttons();
            }
            Ev::Options(Err(l)) => {
                self.report(&format!("{}: {}", l.id, l.text), Status::Error);
                self.add_line(l);
                self.show_options();
            }
            Ev::Uncommitted(u) => {
                let changed = {
                    let mut st = self.st.borrow_mut();
                    std::mem::replace(&mut st.uncommitted, u) != u
                };
                if changed {
                    self.buttons();
                }
            }
            Ev::Lost => {
                let sys = self.st.borrow().system.name.clone();
                let m = qsa_msg::message(id::STATEMENT_LOST, &[&sys]);
                crate::log!("{m}");
                self.report(&m.to_string(), Status::Error);
                self.add_line(Line {
                    statement: 0,
                    script_line: 0,
                    verb: String::new(),
                    id: m.id.clone(),
                    sqlstate: String::new(),
                    sqlcode: None,
                    text: m.text.clone(),
                    rows: None,
                    elapsed: std::time::Duration::ZERO,
                    failed: true,
                    detail: Detail::Program(m),
                });
                self.st.borrow_mut().uncommitted = false;
                self.buttons();
            }
            Ev::Ended(why) => self.session_ended(&why),
            Ev::Closed => {}
        }
    }

    fn session_ended(&self, why: &qap_client::Ended) {
        let sys = {
            let mut st = self.st.borrow_mut();
            st.ended = true;
            st.running = false;
            st.uncommitted = false;
            st.system.clone()
        };
        self.buttons();
        let x = qap::ended(&sys, why);
        crate::log!("{}", x.line());
        self.report(&x.line(), Status::Error);
        let mut status = self.w.status.clone();
        status.set(&format!("{}: Session ended", sys.name), Status::Error);
        let Some(state) = self.state.clone() else {
            return;
        };
        state.borrow_mut().qap.forget_ended(&sys.name);
        if qap::offer_sign_on(self.parent(), &x) {
            self.connect(&state);
        }
    }

    /// Attaches a connection for the window and starts its worker; false
    /// when the user cancelled the sign-on or the system refused.
    fn connect(&self, state: &Shared) -> bool {
        let sys = self.st.borrow().system.clone();
        let Some(h) = qap::session(self.parent(), state, &sys.name) else {
            return false;
        };
        let refused = h.try_lock().ok().and_then(|mut c| {
            let has = c.version().unwrap_or(1);
            c.supports(Op::Prepare).err().map(|_| has)
        });
        if let Some(has) = refused {
            let m = qap::version_needed("Run SQL", &sys.name, Op::Prepare, has);
            crate::log!("{m}");
            msgdetails::show_message(self.parent(), &m);
            return false;
        }
        let Some(a) = qap::attach(self.parent(), &h, &sys, "Run SQL") else {
            return false;
        };
        let job = a.job().job.clone();
        let profile = a.profile().clone();
        let ccsid = match profile.ccsid {
            0 | 65535 => a.hello().system_ccsid,
            c => c,
        };
        let q = qap_client::SqlConnection::new(a);
        let (ctx, crx) = mpsc::channel();
        let (etx, erx) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let w = worker::Worker::new(
            q,
            sys.clone(),
            crx,
            etx,
            Arc::clone(&cancel),
            Box::new(app::awake),
        );
        if let Err(e) = std::thread::Builder::new()
            .name(format!("run sql {}", sys.name))
            .spawn(move || w.run())
        {
            crate::log!("Run SQL worker not started: {e}");
            return false;
        }
        let bits = state.borrow().settings.run_sql.bit_data_as_text(&sys.name);
        {
            let mut st = self.st.borrow_mut();
            st.cmd = Some(ctx);
            st.events = Some(erx);
            st.cancel = cancel;
            st.job = Some(job.clone());
            st.profile.clone_from(&profile.profile);
            st.ended = false;
            st.options = SetOptions::default();
            st.uncommitted = false;
            st.job_ccsid = ccsid;
            st.style = Style {
                dates: DateStyle::job(
                    &profile.date_format,
                    &profile.date_separator,
                    &profile.time_separator,
                ),
                point: '.',
                bit_data: if bits {
                    BitData::Text { ccsid }
                } else {
                    BitData::Hex
                },
            };
        }
        self.bit_data_menu(bits, ccsid);
        let mut win = self.w.window.clone();
        win.set_label(&qap::window_title("Run SQL", &sys.name, Some(&job)));
        let m = qap::attached_message(&sys.name, "Run SQL", &job);
        self.report(&m.to_string(), Status::Neutral);
        self.w.status.clone().set(
            &format!("{}: {}", sys.name, profile.profile),
            Status::Active,
        );
        self.show_options();
        self.buttons();
        self.load_schemas(&h);
        true
    }

    /// Fills the schema combo from `ListSchemas` on the shared connection,
    /// on a thread of its own.
    fn load_schemas(&self, h: &qap::Handle) {
        let (tx, rx) = mpsc::channel();
        self.st.borrow_mut().schemas = Some(rx);
        let h = Arc::clone(h);
        std::thread::spawn(move || {
            if let Ok(all) = qap::lock(&h).all_schemas("%") {
                let _ = tx.send(all.into_iter().map(|s| s.name).collect());
                app::awake();
            }
        });
    }

    /// The Options menu's CCSID 65535 item, with the job's CCSID.
    fn bit_data_menu(&self, on: bool, ccsid: u16) {
        let menu = self.w.menu.clone();
        let idx = (0..menu.size()).find(|&i| {
            menu.at(i)
                .and_then(|m| m.label())
                .is_some_and(|l| l.starts_with("Show CCSID 65535"))
        });
        if let Some(i) = idx
            && let Some(mut item) = menu.at(i)
        {
            item.set_label(&format!("Show CCSID 65535 Data as &Text (CCSID {ccsid})"));
            if on {
                item.set();
            } else {
                item.clear();
            }
        }
    }
}

impl RunSql {
    fn act(&self, a: Act) {
        match a {
            Act::Close => self.close(),
            Act::New => {
                if self.keep_script() {
                    self.w.buffer.clone().set_text("");
                    let mut st = self.st.borrow_mut();
                    st.file = None;
                    st.saved.clear();
                }
            }
            Act::Open => {
                if self.keep_script()
                    && let Some(p) = crate::files::choose_open(
                        "Open SQL Script",
                        "SQL Scripts (*.sql)\tAll Files (*)",
                    )
                {
                    self.open_file(&p);
                }
            }
            Act::Recent(n) => {
                let p = self
                    .state
                    .as_ref()
                    .and_then(|s| s.borrow().settings.run_sql.recent.get(n).cloned());
                if let Some(p) = p
                    && self.keep_script()
                {
                    self.open_file(&p);
                }
            }
            Act::Save => {
                let file = self.st.borrow().file.clone();
                match file {
                    Some(p) => {
                        self.save_file(&p);
                    }
                    None => {
                        self.save_as();
                    }
                }
            }
            Act::SaveAs => {
                self.save_as();
            }
            Act::RunStatement => self.run(Which::AtCursor),
            Act::RunSelected => self.run(Which::Selected),
            Act::RunAll => self.run(Which::All),
            Act::StopOnError => {
                let on = !self.st.borrow().stop_on_error;
                self.st.borrow_mut().stop_on_error = on;
                if let Some(state) = &self.state {
                    let mut s = state.borrow_mut();
                    s.settings.run_sql.stop_on_error = on;
                    if let Err(e) = s.save_settings() {
                        crate::log!("settings not saved: {e}");
                    }
                }
            }
            Act::History => self.history(),
            Act::Cancel => {
                let running = self.st.borrow().running;
                if running {
                    self.st.borrow().cancel.store(true, Ordering::SeqCst);
                    self.report("Cancelling...", Status::Neutral);
                }
            }
            Act::Commit => {
                if self.idle() {
                    self.send(Cmd::Commit);
                }
            }
            Act::Rollback => {
                if self.idle() {
                    self.send(Cmd::Rollback);
                }
            }
            Act::CommitLevel | Act::Naming | Act::Schema => self.change_options(),
            Act::Clear => {
                if self.idle() {
                    self.send(Cmd::ClearResults);
                    self.clear_results();
                }
            }
            Act::FetchMore | Act::FetchAll => self.fetch(a == Act::FetchAll),
            Act::CopyCell | Act::CopyRow => self.copy(a == Act::CopyRow),
            Act::Details => self.details(),
            Act::BitData => self.toggle_bit_data(),
            Act::Export => self.export(),
        }
    }

    fn idle(&self) -> bool {
        !self.st.borrow().running
    }

    fn run(&self, which: Which) {
        if !self.idle() {
            return;
        }
        let text = self.w.buffer.text();
        let statements = match which {
            Which::AtCursor => {
                let at = usize::try_from(self.w.editor.insert_position()).unwrap_or(0);
                script::at(&text, at).into_iter().collect()
            }
            Which::Selected => match self.w.buffer.selection_position() {
                Some((a, b)) if a != b => script::selected(
                    &text,
                    usize::try_from(a).unwrap_or(0),
                    usize::try_from(b).unwrap_or(0),
                ),
                _ => {
                    self.report("Select the statements to run.", Status::Error);
                    return;
                }
            },
            Which::All => script::split(&text),
        };
        self.run_statements(statements, FETCH_ROWS);
    }

    /// Runs `statements`, the first `first_rows` of each result fetched.
    fn run_statements(&self, statements: Vec<script::Statement>, first_rows: u32) {
        if statements.is_empty() {
            self.report("There is no statement to run.", Status::Error);
            return;
        }
        let (sys, ended) = {
            let st = self.st.borrow();
            (st.system.name.clone(), st.ended || st.cmd.is_none())
        };
        // The next run after an ended session attaches anew.
        if ended {
            let Some(state) = self.state.clone() else {
                return;
            };
            if !self.connect(&state) {
                return;
            }
        }
        if let Some(state) = &self.state {
            let mut s = state.borrow_mut();
            let mut kept = false;
            for st in &statements {
                kept |= s.settings.run_sql.add(&sys, &st.text);
            }
            if kept && let Err(e) = s.save_settings() {
                crate::log!("settings not saved: {e}");
            }
        }
        for st in &statements {
            crate::log!(
                "Run SQL on {sys}: {} at line {}",
                script::verb(&st.text),
                st.line
            );
        }
        let stop = self.st.borrow().stop_on_error;
        let sent = self.send(Cmd::Run {
            statements,
            stop_on_error: stop,
            first_rows,
        });
        if sent {
            let mut st = self.st.borrow_mut();
            st.running = true;
            st.started = Some(Instant::now());
            drop(st);
            self.buttons();
        }
    }

    fn fetch(&self, all: bool) {
        if !self.idle() {
            return;
        }
        let result = {
            let st = self.st.borrow();
            let page = self.w.tabs.current();
            st.results
                .iter()
                .position(|r| r.page == page && r.standing == Standing::More)
        };
        let Some(result) = result else {
            return;
        };
        if self.send(Cmd::Fetch { result, all }) {
            let mut st = self.st.borrow_mut();
            st.running = true;
            st.started = Some(Instant::now());
            drop(st);
            self.report("Fetching...", Status::Neutral);
            self.buttons();
        }
    }

    /// The result on the tab shown.
    fn shown_result(&self) -> Option<usize> {
        let page = self.w.tabs.current();
        self.st.borrow().results.iter().position(|r| r.page == page)
    }

    /// Copies the selected cell or row as tab-separated text, as shown
    /// (NULL as nothing).
    fn copy(&self, row: bool) {
        let Some(i) = self.shown_result() else {
            return;
        };
        let (out, incomplete) = {
            let st = self.st.borrow();
            let r = &st.results[i];
            let Some(sel) = r.list.selected() else {
                return;
            };
            let Some(values) = r.rows.get(sel) else {
                return;
            };
            let text = |v: &SqlValue, c: &Column| {
                qsa_sql::value::display(v, c, &st.style).unwrap_or_default()
            };
            let out = if row {
                values
                    .iter()
                    .zip(&r.columns)
                    .map(|(v, c)| text(v, c))
                    .collect::<Vec<_>>()
                    .join("\t")
            } else {
                let col = r.list.clicked_column().unwrap_or(0);
                match (values.get(col), r.columns.get(col)) {
                    (Some(v), Some(c)) => text(v, c),
                    _ => return,
                }
            };
            (out, matches!(r.standing, Standing::Incomplete(_)))
        };
        app::copy(&out);
        let what = if row { "Row" } else { "Cell" };
        let note = if incomplete {
            " from an incomplete result"
        } else {
            ""
        };
        self.report(&format!("{what} copied{note}."), Status::Neutral);
    }

    /// The selected message's second-level text.
    fn details(&self) {
        let Some(i) = self.w.messages.selected() else {
            return;
        };
        let detail = self.st.borrow().lines.get(i).map(|l| l.detail.clone());
        match detail {
            Some(Detail::System(m)) => {
                msgdetails::show_system_message(self.parent(), None, &qap::shown(&m));
            }
            Some(Detail::Program(m)) => msgdetails::show_details(self.parent(), &m),
            _ => {}
        }
    }

    /// Every result tab goes, and the messages with them.
    fn clear_results(&self) {
        self.w.tabs.clone().truncate(1);
        self.w.messages.clone().set_rows(Vec::new());
        let mut st = self.st.borrow_mut();
        st.results.clear();
        st.lines.clear();
        drop(st);
        self.report("", Status::Neutral);
        self.buttons();
    }

    fn toggle_bit_data(&self) {
        let (sys, ccsid, on) = {
            let mut st = self.st.borrow_mut();
            let on = st.style.bit_data == BitData::Hex;
            let ccsid = st.job_ccsid;
            st.style.bit_data = if on {
                BitData::Text { ccsid }
            } else {
                BitData::Hex
            };
            (st.system.name.clone(), ccsid, on)
        };
        if let Some(state) = &self.state {
            let mut s = state.borrow_mut();
            s.settings.run_sql.set_bit_data_as_text(&sys, on);
            if let Err(e) = s.save_settings() {
                crate::log!("settings not saved: {e}");
            }
        }
        self.bit_data_menu(on, ccsid);
        // Every result again, in the new form.
        let st = self.st.borrow();
        for r in &st.results {
            let cells = r
                .rows
                .iter()
                .map(|row| value_cells(row, &r.columns, &st.style))
                .collect();
            r.list.clone().set_rows(cells);
        }
    }

    /// The options row as it stands now.
    fn options_shown(&self) -> SetOptions {
        let schema = self
            .w
            .schema
            .value()
            .unwrap_or_default()
            .trim()
            .to_ascii_uppercase();
        SetOptions {
            commit: level_at(self.w.commit_level.value()),
            naming: NAMINGS
                .get(usize::try_from(self.w.naming.value()).unwrap_or(0))
                .map_or(Naming::Sql, |(_, n)| *n),
            default_schema: (!schema.is_empty()).then_some(schema),
        }
    }

    /// The options row changed: settle uncommitted changes first, then
    /// set the new options; the row goes back if the user cancels.
    fn change_options(&self) {
        if !self.idle() {
            self.show_options();
            return;
        }
        let wanted = self.options_shown();
        if wanted == self.st.borrow().options {
            return;
        }
        if self.st.borrow().uncommitted {
            match self.ask_uncommitted() {
                Some(true) => {
                    self.send(Cmd::Commit);
                }
                Some(false) => {
                    self.send(Cmd::Rollback);
                }
                None => {
                    self.show_options();
                    return;
                }
            }
        }
        if !self.send(Cmd::SetOptions(wanted.clone())) {
            // Not connected: the options shown are what the next
            // connection gets.
            self.st.borrow_mut().options = wanted;
            self.buttons();
        }
    }

    /// `QAD7021`: Commit, Roll Back or Cancel. `Some(true)` to commit,
    /// `Some(false)` to roll back, `None` to cancel.
    fn ask_uncommitted(&self) -> Option<bool> {
        let what = self.w.window.label();
        let m = qsa_msg::message(id::UNCOMMITTED_CHANGES, &[&what]);
        match msgdetails::ask_message(self.parent(), &m, &["Commit", "Roll Back", "Cancel"]) {
            0 => Some(true),
            1 => Some(false),
            _ => None,
        }
    }

    fn close(&self) {
        let settle = if self.st.borrow().uncommitted {
            match self.ask_uncommitted() {
                Some(c) => Some(c),
                None => return,
            }
        } else {
            None
        };
        if !self.keep_script() {
            return;
        }
        let running = self.st.borrow().running;
        if running {
            self.st.borrow().cancel.store(true, Ordering::SeqCst);
        }
        self.send(Cmd::Close(settle));
        let mut st = self.st.borrow_mut();
        st.cmd = None;
        st.events = None;
        drop(st);
        self.w.window.clone().hide();
    }

    /// Whether the script may be replaced or closed: unchanged, saved now,
    /// or the user does not want it kept.
    fn keep_script(&self) -> bool {
        let text = self.w.buffer.text();
        let (changed, name) = {
            let st = self.st.borrow();
            let name = st.file.as_ref().and_then(|p| p.file_name()).map_or_else(
                || "the script".to_owned(),
                |n| n.to_string_lossy().into_owned(),
            );
            (text != st.saved && !text.trim().is_empty(), name)
        };
        if !changed {
            return true;
        }
        let q = format!("Save changes to {name}?");
        let mb = qsa_ui::MessageBox::new("Run SQL", &q)
            .glyph(qsa_ui::Glyph::Disk)
            .buttons(&["Save", "Don't Save", "Cancel"], 0, 2);
        match mb.run(self.parent()) {
            0 => {
                let file = self.st.borrow().file.clone();
                match file {
                    Some(p) => self.save_file(&p),
                    None => self.save_as(),
                }
            }
            1 => true,
            _ => false,
        }
    }

    fn open_file(&self, p: &Path) {
        match std::fs::read(p) {
            Ok(bytes) => {
                let b = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(&bytes);
                let text = String::from_utf8_lossy(b).into_owned();
                self.w.buffer.clone().set_text(&text);
                let mut st = self.st.borrow_mut();
                st.file = Some(p.to_owned());
                st.saved = text;
                drop(st);
                self.remember(p);
                self.report(&format!("Opened {}.", p.display()), Status::Neutral);
            }
            Err(e) => self.report(
                &format!("{} cannot be read: {e}", p.display()),
                Status::Error,
            ),
        }
    }

    fn save_as(&self) -> bool {
        let name = self
            .st
            .borrow()
            .file
            .as_ref()
            .and_then(|p| p.file_name())
            .map_or_else(
                || "script.sql".to_owned(),
                |n| n.to_string_lossy().into_owned(),
            );
        let Some(mut p) =
            crate::files::choose_save("Save SQL Script", "SQL Scripts (*.sql)", &name)
        else {
            return false;
        };
        if p.extension().is_none() {
            p.set_extension("sql");
        }
        self.save_file(&p)
    }

    fn save_file(&self, p: &Path) -> bool {
        let text = self.w.buffer.text();
        match std::fs::write(p, &text) {
            Ok(()) => {
                let mut st = self.st.borrow_mut();
                st.file = Some(p.to_owned());
                st.saved = text;
                drop(st);
                self.remember(p);
                self.report(&format!("Saved {}.", p.display()), Status::Neutral);
                true
            }
            Err(e) => {
                self.report(
                    &format!("{} cannot be written: {e}", p.display()),
                    Status::Error,
                );
                false
            }
        }
    }

    fn remember(&self, p: &Path) {
        if let Some(state) = &self.state {
            let mut s = state.borrow_mut();
            s.settings.run_sql.add_recent(p);
            if let Err(e) = s.save_settings() {
                crate::log!("settings not saved: {e}");
            }
        }
        self.refresh_recent();
    }

    /// Run > History: the system's last statements, newest first; the one
    /// chosen goes into the script at the cursor.
    fn history(&self) {
        let Some(state) = &self.state else {
            return;
        };
        let sys = self.st.borrow().system.name.clone();
        let items: Vec<String> = state
            .borrow()
            .settings
            .run_sql
            .history(&sys)
            .iter()
            .rev()
            .cloned()
            .collect();
        if let Some(s) = history_dialog(self.parent(), &sys, &items) {
            let mut e = self.w.editor.clone();
            e.insert(&format!("{s};\n"));
            let _ = e.take_focus();
        }
    }
}

/// Which statements a run takes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Which {
    AtCursor,
    Selected,
    All,
}

/// The history list: a statement to insert, or `None`.
fn history_dialog(
    parent: Option<&dyn WindowExt>,
    system: &str,
    items: &[String],
) -> Option<String> {
    let (w, h) = (640, 400);
    let mut win = Window::new(0, 0, w, h, None);
    win.set_label(&format!("History - {system}"));
    win.set_color(colors::FACE);
    win.make_modal(true);
    let m = 10;
    let (bw, bh) = (theme::BUTTON_W, theme::BUTTON_H);
    let mut list = ListTable::new(
        m,
        m,
        w - 2 * m,
        h - 3 * m - bh,
        &[("Statement", w - 2 * m - 20)],
    );
    list.set_rows(
        items
            .iter()
            .map(|s| vec![Cell::from(s.replace('\n', " "))])
            .collect(),
    );
    let chosen = Rc::new(StdCell::new(None::<usize>));
    let mut insert = controls::default_button(w - m - 2 * bw - 6, h - m - bh, bw, bh, "Insert");
    let mut close = controls::button(w - m - bw, h - m - bh, bw, bh, "Close");
    win.end();
    {
        let (c, l, mut wn) = (chosen.clone(), list.clone(), win.clone());
        insert.set_callback(move |_| {
            c.set(l.selected());
            wn.hide();
        });
    }
    {
        let (c, mut wn) = (chosen.clone(), win.clone());
        list.on_activate(move |row| {
            c.set(row);
            wn.hide();
        });
    }
    {
        let mut wn = win.clone();
        close.set_callback(move |_| wn.hide());
    }
    qsa_ui::propdialog::center_on(&mut win, parent);
    qsa_ui::propdialog::show_placed(&mut win);
    while win.shown() {
        app::wait();
    }
    chosen.get().and_then(|i| items.get(i).cloned())
}

/// Opens a Run SQL window for `system`, signing on first when this program
/// has no session on it. With `sql` the script holds it, and with `run` it
/// runs at once, fetching `first_rows` first (Schemas' View data).
pub fn open(
    parent: Option<&dyn WindowExt>,
    state: &Shared,
    system: &str,
    sql: Option<&str>,
    run: bool,
    first_rows: u32,
) {
    let Some(sys) = state.borrow().systems.get(system).cloned() else {
        return;
    };
    let r = Rc::new(RunSql::new(&sys, Some(state.clone())));
    if !r.connect(state) {
        return;
    }
    if let Some(s) = sql {
        r.w.buffer.clone().set_text(s);
    }
    let mut win = r.w.window.clone();
    qsa_ui::propdialog::center_on(&mut win, parent);
    qsa_ui::propdialog::show_placed(&mut win);
    let _ = r.w.editor.clone().take_focus();
    if run && let Some(s) = sql {
        r.run_statements(script::split(s), first_rows.max(1));
    }
    WINDOWS.with(|v| v.borrow_mut().push(r));
    tick();
}

/// Why Run SQL, Schemas or Data Transfer (`function`) is not available
/// for `system`, if it is not: `QAD5001` when its QAP version has no `Sql`
/// service. A version not known does not stop it: opening finds out.
pub fn sql_blocked(function: &str, system: &str, version: Option<u16>) -> Option<qsa_msg::Message> {
    let op = Op::Prepare;
    match version {
        Some(v) if v < op.since() => Some(qap::version_needed(function, system, op, v)),
        _ => None,
    }
}

/// The repeating look at every window's worker.
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

/// Gives every Run SQL window its waiting work and forgets the closed
/// ones. Not again while it runs (a dialog it shows waits inside).
pub fn pump() {
    if PUMPING.with(|p| p.replace(true)) {
        return;
    }
    let windows: Vec<Rc<RunSql>> = WINDOWS.with(|v| v.borrow().clone());
    let open: Vec<bool> = windows.iter().map(|w| w.pump()).collect();
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

/// Whether any Run SQL window is open.
pub fn any_open() -> bool {
    WINDOWS.with(|v| v.borrow().iter().any(|w| w.w.window.shown()))
}

impl RunSql {
    /// Results > Export: the result shown, its rows as fetched, through
    /// Data Transfer's writers in the format the file's extension names
    /// (CSV when it names none). An incomplete result is exported as it
    /// is, labelled incomplete.
    fn export(&self) {
        let Some(i) = self.shown_result() else {
            self.report("Show a result to export.", Status::Error);
            return;
        };
        let (incomplete, rows) = {
            let st = self.st.borrow();
            let r = &st.results[i];
            (matches!(r.standing, Standing::Incomplete(_)), r.rows.len())
        };
        let name = if incomplete {
            format!("result-{}-incomplete.csv", i + 1)
        } else {
            format!("result-{}.csv", i + 1)
        };
        let pattern = "CSV (*.csv)\tTSV (*.tsv)\tExcel workbook (*.xlsx)\tOpenDocument spreadsheet (*.ods)\tFixed width (*.txt)";
        let Some(path) = crate::files::choose_save("Export Result", pattern, &name) else {
            return;
        };
        let format = qsa_transfer::Format::from_path(&path).unwrap_or_default();
        let opts = qsa_transfer::FileOptions {
            format,
            sheet: format!("Result {}", i + 1),
            bit_data_as_text: self.st.borrow().style.bit_data != BitData::Hex,
            ..qsa_transfer::FileOptions::default()
        };
        let r = {
            let st = self.st.borrow();
            let res = &st.results[i];
            let dates = st.style.dates;
            let ccsid = st.job_ccsid;
            qsa_transfer::writer::open(&path, &res.columns, &opts, dates, ccsid, false).and_then(
                |mut sink| {
                    for row in &res.rows {
                        sink.row(row)?;
                    }
                    sink.finish()
                },
            )
        };
        match r {
            Ok(()) => {
                let note = if incomplete {
                    " (an incomplete result)"
                } else {
                    ""
                };
                self.report(
                    &format!("{rows} rows exported to {}{note}.", path.display()),
                    if incomplete {
                        Status::Error
                    } else {
                        Status::Neutral
                    },
                );
            }
            Err(e) => self.report(
                &format!("{} cannot be written: {e}", path.display()),
                Status::Error,
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn titles_info_and_tooltips() {
        assert_eq!(result_title(0, &Standing::More), "Result 1");
        assert_eq!(
            result_title(1, &Standing::Incomplete("cancelled".into())),
            "Result 2 - Incomplete (cancelled)"
        );
        assert_eq!(
            result_info(500, &Standing::More),
            "500 rows; more to fetch."
        );
        assert_eq!(result_info(12, &Standing::Complete), "12 rows.");
        assert_eq!(
            result_info(3, &Standing::Incomplete("the connection was lost".into())),
            "3 rows. Incomplete (the connection was lost)."
        );
        let c = Column {
            name: "COST".into(),
            label: "Purchase            cost".into(),
            sql_type: "DECIMAL".into(),
            length: 11,
            scale: 2,
            nullable: true,
            ccsid: 0,
        };
        assert_eq!(header_tip(&c), "Purchase cost\nDECIMAL(11,2)");
        assert_eq!(menu_text("C:/a&b\\c.sql"), "C:\\/a&&b\\\\c.sql");
        assert_eq!(level_at(2), CommitLevel::ReadCommitted);
        assert_eq!(level_index(CommitLevel::Serializable), 4);
        assert_eq!(elapsed(std::time::Duration::from_millis(42)), "0.042 s");
        assert_eq!(sql_blocked("Run SQL", "S", Some(3)).unwrap().id, "QAD5001");
        assert!(sql_blocked("Run SQL", "S", Some(4)).is_none());
        assert!(sql_blocked("Run SQL", "S", None).is_none());
    }

    #[test]
    fn nulls_are_a_grey_dash() {
        let c = Column {
            name: "N".into(),
            sql_type: "INTEGER".into(),
            nullable: true,
            ..Column::default()
        };
        let cells = value_cells(
            &[SqlValue::Null, SqlValue::Integer { value: 7 }],
            &[c.clone(), c],
            &Style::default(),
        );
        assert_eq!(cells[0].text, "-");
        assert_eq!(cells[0].status, Status::Muted);
        assert_eq!(cells[1].text, "7");
    }
}
