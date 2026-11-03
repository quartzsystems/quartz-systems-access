// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! Integrated File System: a system's files and folders, from the main
//! window's General group, over the session's shared connection (QAP
//! version 5's `Ifs` service, which runs beside commands and statements on
//! the system).
//!
//! - Left, a folder tree read as it is opened, from the profile's home
//!   directory (the sign-on reply's) and `/`. Right, the folder's entries:
//!   Name, Type, Size, Modified, Owner and CCSID, 500 a page with the next
//!   page loaded as the list scrolls, a name pattern (`*` and `?`), sorted
//!   by a column within the entries loaded.
//! - A path field with Go, and Back, Forward, Up and Refresh.
//! - A symbolic link shows its target and opens as it; a broken link (its
//!   target's `GetInfo` is `NotFound`) is red.
//! - New Folder, Rename, Copy and Cut with Paste into the folder shown
//!   (the system's `Copy`, a folder copied an entry at a time; `Rename` for
//!   a cut), and Delete: files and links after a confirmation listing
//!   them, a folder with everything in it after one giving the counts,
//!   depth-first, stopping at the first refusal with the system's message
//!   (`QAD8006`).
//! - Download... and Upload... (and files dragged from the desktop's file
//!   manager onto the list) go to the transfer queue ([`queue`]), on a
//!   connection of the window's own.
//! - The system's messages are shown as it sent them; a folder the profile
//!   may not read shows its message in the list.
//! - Before QAP version 5 the window does not open: `QAD5001`.

pub mod model;
pub mod queue;

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;

use fltk::app;
use fltk::enums::{Align, CallbackTrigger, Key, Shortcut};
use fltk::frame::Frame;
use fltk::input::Input;
use fltk::menu::{MenuBar, MenuFlag, MenuItem};
use fltk::prelude::*;
use fltk::tree::{Tree, TreeItem, TreeReason, TreeSelect};
use fltk::window::Window;
use qap_client::ifs::{Listing, file_name, join, parent};
use qap_client::proto::Op;
use qap_client::proto::ops::ifs::{Entry, EntryKind};
use qap_client::{Error, Files, Status as QapStatus};
use qsa_config::System;
use qsa_msg::id;
use qsa_ui::chan::{self, Emit, MenuEmit};
use qsa_ui::inputbox::InputBox;
use qsa_ui::listtable::ListTable;
use qsa_ui::sampler::{MENU_H, menu_bar};
use qsa_ui::statusbar::StatusBar;
use qsa_ui::theme::{self, colors};
use qsa_ui::{Glyph, MessageBox, Status, Toolbar, controls, msgdetails};

use crate::app::Shared;
use crate::qap::{self, Handle};
use model::Row;

/// Window width and height.
pub const W: i32 = 960;
pub const H: i32 = 640;

/// The function's name where the user sees it.
pub const FUNCTION: &str = "Integrated File System";

/// Rows from the end of those loaded at which the next page is read.
const MORE_AT: usize = 40;

/// What the controls ask for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Act {
    Go,
    Back,
    Forward,
    Up,
    Home,
    Refresh,
    Tree,
    Open,
    NewFolder,
    Rename,
    Copy,
    Cut,
    Paste,
    Delete,
    SelectAll,
    Sort(usize),
    Download,
    Upload,
    /// Files dropped on the list.
    Dropped,
    /// Show or hide the transfer queue.
    Transfers,
    Queue(queue::Act),
    Close,
}

/// The window's widgets.
pub struct Widgets {
    pub window: Window,
    pub menu: MenuBar,
    pub path: Input,
    pub pattern: Input,
    pub tree: Tree,
    pub list: ListTable,
    /// A folder's refusal, shown in the list area.
    pub note: Frame,
    pub status: StatusBar,
    pub queue: queue::Queue,
    back: fltk::button::Button,
    forward: fltk::button::Button,
    /// Paths dropped on the list, until taken.
    dropped: Rc<RefCell<Vec<PathBuf>>>,
}

/// A label as a tree path part: its `/` and `\` kept.
fn part(label: &str) -> String {
    label.replace('\\', "\\\\").replace('/', "\\/")
}

/// The actions a right-click in the list offers, with what they ask for.
const CONTEXT: [(&str, Act); 9] = [
    ("Open", Act::Open),
    ("Download...", Act::Download),
    ("Upload...", Act::Upload),
    ("Rename...", Act::Rename),
    ("Copy", Act::Copy),
    ("Cut", Act::Cut),
    ("Paste", Act::Paste),
    ("Delete", Act::Delete),
    ("New Folder...", Act::NewFolder),
];

fn build_widgets(system: &str) -> (Widgets, chan::Receiver<Act>) {
    let (tx, rx) = chan::channel::<Act>();
    let mut window = Window::new(0, 0, W, H, None);
    window.set_label(&format!("{FUNCTION} - {system}"));
    window.set_icon(Some(qsa_ui::glyph::app_icon()));
    window.set_color(colors::FACE);
    window.size_range(640, 400, 0, 0);
    let mut menu = menu_bar(0, 0, W);
    {
        let mut add = |p: &str, sc: Shortcut, a: Act, f: MenuFlag| {
            menu.add_emit_to(p, sc, f, tx, a);
        };
        let n = MenuFlag::Normal;
        let d = MenuFlag::MenuDivider;
        add("&File/&Open", Shortcut::None, Act::Open, d);
        add("&File/&Download...", Shortcut::Ctrl | 'd', Act::Download, n);
        add("&File/&Upload...", Shortcut::Ctrl | 'u', Act::Upload, d);
        add(
            "&File/New &Folder...",
            Shortcut::Ctrl | Shortcut::Shift | 'n',
            Act::NewFolder,
            d,
        );
        add("&File/&Close", Shortcut::Alt | Key::F4, Act::Close, n);
        add("&Edit/Cu&t", Shortcut::Ctrl | 'x', Act::Cut, n);
        add("&Edit/&Copy", Shortcut::Ctrl | 'c', Act::Copy, n);
        add("&Edit/&Paste", Shortcut::Ctrl | 'v', Act::Paste, d);
        add("&Edit/Re&name...", Shortcut::None | Key::F2, Act::Rename, n);
        add(
            "&Edit/&Delete",
            Shortcut::None | Key::Delete,
            Act::Delete,
            d,
        );
        add("&Edit/Select &All", Shortcut::Ctrl | 'a', Act::SelectAll, n);
        add("&View/&Refresh", Shortcut::None | Key::F5, Act::Refresh, d);
        add("&View/&Transfers", Shortcut::Ctrl | 't', Act::Transfers, n);
        add("&Go/&Back", Shortcut::Alt | Key::Left, Act::Back, n);
        add("&Go/&Forward", Shortcut::Alt | Key::Right, Act::Forward, n);
        add("&Go/&Up", Shortcut::Alt | Key::Up, Act::Up, d);
        add("&Go/&Home Folder", Shortcut::Alt | Key::Home, Act::Home, n);
    }
    let mut tb = Toolbar::new(0, MENU_H, W);
    for (g, tip, a) in [
        (Glyph::Up, "Up one folder", Act::Up),
        (Glyph::Folder, "New Folder", Act::NewFolder),
    ] {
        tb.add(g, tip, move || tx.send(a));
    }
    tb.separator();
    for (g, tip, a) in [
        (Glyph::Download, "Download...", Act::Download),
        (Glyph::Upload, "Upload...", Act::Upload),
    ] {
        tb.add(g, tip, move || tx.send(a));
    }
    tb.separator();
    for (g, tip, a) in [
        (Glyph::Copy, "Copy", Act::Copy),
        (Glyph::Paste, "Paste", Act::Paste),
        (Glyph::Cross, "Delete", Act::Delete),
    ] {
        tb.add(g, tip, move || tx.send(a));
    }
    tb.end();
    let m = 8;
    let fh = theme::FIELD_H;
    let (bw, bh) = (theme::BUTTON_W, theme::BUTTON_H);
    let fy = MENU_H + Toolbar::HEIGHT + 6;
    let mut back = controls::button(m, fy - 1, 52, bh, "Back");
    back.emit_to(tx, Act::Back);
    let mut forward = controls::button(m + 56, fy - 1, 62, bh, "Forward");
    forward.emit_to(tx, Act::Forward);
    let mut l = controls::label(m + 126, fy, 36, fh, "Path:");
    l.set_align(Align::Left | Align::Inside);
    let pw = W - (m + 162) - bw - 8 - 182;
    let mut path = controls::field(m + 162, fy, pw, fh);
    path.set_trigger(CallbackTrigger::EnterKeyAlways);
    path.set_callback(move |_| tx.send(Act::Go));
    let mut go = controls::default_button(m + 162 + pw + 4, fy - 1, bw - 20, bh, "Go");
    go.emit_to(tx, Act::Go);
    let px = m + 162 + pw + bw - 10;
    let mut l = controls::label(px, fy, 44, fh, "Show:");
    l.set_align(Align::Left | Align::Inside);
    let mut pattern = controls::field(px + 44, fy, W - px - 44 - m, fh);
    pattern.set_value("*");
    pattern.set_tooltip("Names to list: * for any characters, ? for one. Press Enter.");
    pattern.set_trigger(CallbackTrigger::EnterKeyAlways);
    pattern.set_callback(move |_| tx.send(Act::Refresh));
    let ty = fy + fh + 8;
    let th = H - ty - StatusBar::HEIGHT - 6;
    let tw = 240;
    let mut tree = Tree::new(m, ty, tw, th, None);
    tree.set_select_mode(TreeSelect::Single);
    tree.set_item_label_font(theme::UI_FONT);
    tree.set_item_label_size(theme::UI_SIZE);
    tree.set_show_root(false);
    tree.set_callback(move |_| tx.send(Act::Tree));
    let lx = m + tw + 6;
    let mut list = ListTable::new(lx, ty, W - lx - m, th, &model::COLUMNS);
    list.set_multi(true);
    list.on_activate(move |_| tx.send(Act::Open));
    list.on_header_click(move |c| tx.send(Act::Sort(c)));
    list.on_context(move |_| {
        let labels: Vec<&str> = CONTEXT.iter().map(|(l, _)| *l).collect();
        if let Some(chosen) = MenuItem::new(&labels).popup(app::event_x(), app::event_y())
            && let Some((_, a)) = CONTEXT
                .iter()
                .find(|(l, _)| chosen.label().as_deref() == Some(*l))
        {
            tx.send(*a);
        }
    });
    let mut note = Frame::new(
        lx + 1,
        ty + ListTable::HEADER_H + 1,
        W - lx - m - 2,
        th - ListTable::HEADER_H - 2,
        None,
    );
    note.set_align(Align::Center | Align::Inside | Align::Wrap);
    note.set_label_font(theme::UI_FONT);
    note.set_label_size(theme::UI_SIZE);
    note.set_label_color(colors::TEXT);
    note.hide();
    // Files from the desktop: uploaded to the folder shown.
    let dropped = Rc::new(RefCell::new(Vec::new()));
    {
        let d = Rc::clone(&dropped);
        list.on_drop(move |paths| {
            d.borrow_mut().extend(paths);
            tx.send(Act::Dropped);
        });
    }
    let queue = queue::Queue::new(lx, ty + th - queue::PANEL_H, W - lx - m, move |a| {
        tx.send(Act::Queue(a));
    });
    queue.group.clone().hide();
    let status = StatusBar::new(0, H - StatusBar::HEIGHT, W);
    window.resizable(list.table());
    window.end();
    window.set_callback(move |_| tx.send(Act::Close));
    let w = Widgets {
        window,
        menu,
        path,
        pattern,
        tree,
        list,
        note,
        status,
        queue,
        back,
        forward,
        dropped,
    };
    (w, rx)
}

impl Widgets {
    /// Shows or hides the transfer queue under the list, the list taking
    /// the room it leaves.
    fn show_queue(&self, on: bool) {
        let mut g = self.queue.group.clone();
        if on == g.visible() {
            return;
        }
        let t = self.list.table();
        let (x, y, w) = (t.x(), t.y(), t.w());
        let bottom = self.window.h() - StatusBar::HEIGHT - 6;
        let list_h = if on {
            bottom - y - queue::PANEL_H - 6
        } else {
            bottom - y
        };
        self.list.table().clone().resize(x, y, w, list_h);
        let mut n = self.note.clone();
        n.resize(n.x(), n.y(), n.w(), list_h - ListTable::HEADER_H - 2);
        g.resize(x, bottom - queue::PANEL_H, w, queue::PANEL_H);
        if on {
            g.show();
        } else {
            g.hide();
        }
        self.window.clone().redraw();
    }
}

/// Entries copied or cut, for Paste in this window or another of the same
/// system.
#[derive(Clone, Debug)]
struct Clip {
    system: String,
    /// Each entry's path and whether it is a folder (not a link to one).
    entries: Vec<(String, bool)>,
    cut: bool,
}

thread_local! {
    static CLIP: RefCell<Option<Clip>> = const { RefCell::new(None) };
}

/// The Integrated File System window.
pub struct Ifs {
    pub w: Widgets,
    rx: chan::Receiver<Act>,
    sys: System,
    h: Option<Handle>,
    state: Option<Shared>,
    home: String,
    /// The folder shown.
    cur: RefCell<String>,
    listing: RefCell<Option<Listing>>,
    rows: RefCell<Vec<Row>>,
    sort: Cell<(usize, bool)>,
    back: RefCell<Vec<String>>,
    forward: RefCell<Vec<String>>,
    /// Tree items' paths, by their path in the tree.
    folders: RefCell<HashMap<String, String>>,
    /// A call is under way: nothing else starts meanwhile.
    busy: Cell<bool>,
    /// The last batch's transfer options, offered for the next.
    options: Cell<qsa_transfer::files::Options>,
    /// `QAD8030` was shown: the queue runs on the shared connection.
    shared_said: Cell<bool>,
    /// A folder whose list an upload changed, refreshed when nothing runs.
    stale: RefCell<Option<String>>,
}

impl Ifs {
    fn new(w: Widgets, rx: chan::Receiver<Act>, sys: &System, home: &str) -> Ifs {
        let s = Ifs {
            w,
            rx,
            sys: sys.clone(),
            h: None,
            state: None,
            home: if home.is_empty() {
                "/".into()
            } else {
                home.to_owned()
            },
            cur: RefCell::new(String::new()),
            listing: RefCell::new(None),
            rows: RefCell::new(Vec::new()),
            sort: Cell::new((0, true)),
            back: RefCell::new(Vec::new()),
            forward: RefCell::new(Vec::new()),
            folders: RefCell::default(),
            busy: Cell::new(false),
            options: Cell::new(qsa_transfer::files::Options::default()),
            shared_said: Cell::new(false),
            stale: RefCell::new(None),
        };
        let mut tree = s.w.tree.clone();
        for p in [s.home.clone(), "/".to_owned()] {
            if s.folders.borrow().values().any(|v| *v == p) {
                continue;
            }
            if let Some(item) = tree.add(&part(&p)) {
                s.folders.borrow_mut().insert(s.key(&item), p.clone());
                s.placeholder(&item);
            }
        }
        s
    }

    /// The window for `system` showing `rows` of the folder `path`, or the
    /// folder's refusal `note`, not shown and without a connection: for
    /// screenshots.
    pub fn build(
        system: &System,
        home: &str,
        path: &str,
        rows: Vec<Row>,
        total: u32,
        note: Option<&str>,
    ) -> Ifs {
        let (w, rx) = build_widgets(&system.name);
        let s = Ifs::new(w, rx, system, home);
        *s.cur.borrow_mut() = path.to_owned();
        s.w.path.clone().set_value(path);
        let mut l = Listing::new(path, "*");
        l.next = u32::try_from(rows.len()).unwrap_or(u32::MAX);
        l.total = Some(total);
        *s.listing.borrow_mut() = Some(l);
        match note {
            Some(n) => {
                s.show_note(n);
                s.report(n);
            }
            None => {
                *s.rows.borrow_mut() = rows;
                s.show_rows();
                s.listed(total);
            }
        }
        s.nav_buttons();
        s
    }

    /// Shows the transfer queue with `items` as given: for screenshots.
    pub fn show_transfers(
        &self,
        items: Vec<(
            qsa_transfer::files::Job,
            queue::State,
            qap_client::ifs::Progress,
            Option<f64>,
        )>,
    ) {
        self.w.queue.show(items);
        self.w.show_queue(true);
    }

    fn parent_window(&self) -> Option<&dyn WindowExt> {
        Some(&self.w.window as &dyn WindowExt)
    }

    fn report(&self, text: &str) {
        self.w.status.clone().set_message(text);
    }

    fn said(&self, m: &qsa_msg::Message) {
        crate::log!("{m}");
        self.report(&m.to_string());
    }

    fn failed(&self, e: &Error) {
        let x = qap::explain(&self.sys, e);
        self.report(&x.line());
        qap::show(self.parent_window(), &x);
    }

    /// Runs `f` on the shared connection off the GUI thread.
    fn call<T: Send>(&self, f: impl FnOnce(&mut qap_client::Client) -> T + Send) -> Option<T> {
        let h = self.h.as_ref()?;
        self.busy.set(true);
        let mut win = self.w.window.clone();
        win.set_cursor(fltk::enums::Cursor::Wait);
        let r = qap::busy(|| f(&mut qap::lock(h)));
        win.set_cursor(fltk::enums::Cursor::Default);
        self.busy.set(false);
        Some(r)
    }

    // The tree.

    fn key(&self, item: &TreeItem) -> String {
        self.w.tree.item_pathname(item).unwrap_or_default()
    }

    fn placeholder(&self, item: &TreeItem) {
        let mut tree = self.w.tree.clone();
        let k = self.key(item);
        tree.add(&format!("{k}/..."));
        item.clone().close();
    }

    fn unread(&self, item: &TreeItem) -> bool {
        item.child(0)
            .and_then(|c| c.label())
            .is_some_and(|l| l == "...")
    }

    fn tree_event(&self) {
        let tree = &self.w.tree;
        let Some(item) = tree.callback_item() else {
            return;
        };
        let Some(path) = self.folders.borrow().get(&self.key(&item)).cloned() else {
            return;
        };
        match tree.callback_reason() {
            TreeReason::Opened if self.unread(&item) => self.fill_tree(&item, &path),
            TreeReason::Selected => self.go_to(&path, true),
            _ => {}
        }
    }

    /// The folders in `path`, under `item`.
    fn fill_tree(&self, item: &TreeItem, path: &str) {
        let p = path.to_owned();
        let Some(r) = self.call(move |c| c.all_entries(&p, "")) else {
            return;
        };
        let mut tree = self.w.tree.clone();
        tree.clear_children(item);
        match r {
            Ok(list) => {
                let k = self.key(item);
                for e in list.iter().filter(|e| e.kind == EntryKind::Directory) {
                    if let Some(child) = tree.add(&format!("{k}/{}", part(&e.name))) {
                        self.folders
                            .borrow_mut()
                            .insert(self.key(&child), join(path, &e.name));
                        self.placeholder(&child);
                    }
                }
            }
            Err(e) => self.report(&qap::explain(&self.sys, &e).line()),
        }
        tree.redraw();
    }

    // The list.

    /// Shows the folder `path` from its first page; `record` keeps the
    /// folder left in Back.
    fn go_to(&self, path: &str, record: bool) {
        let path = normal(path);
        let pattern = match self.w.pattern.value().trim() {
            "" => "*".to_owned(),
            p => p.to_owned(),
        };
        let (p, pat) = (path.clone(), pattern.clone());
        let Some(r) = self.call(move |c| {
            let mut l = Listing::new(&p, &pat);
            let page = l.next_page(c)?.unwrap_or_default();
            let rows: Vec<Row> = page.into_iter().map(|e| Row::of(c, &p, e)).collect();
            Ok::<_, Error>((l, rows))
        }) else {
            return;
        };
        let prev = self.cur.replace(path.clone());
        if record && !prev.is_empty() && prev != path {
            self.back.borrow_mut().push(prev);
            self.forward.borrow_mut().clear();
        }
        self.w.path.clone().set_value(&path);
        match r {
            Ok((l, rows)) => {
                let total = l.total.unwrap_or(0);
                *self.listing.borrow_mut() = Some(l);
                *self.rows.borrow_mut() = rows;
                self.show_rows();
                self.listed(total);
            }
            Err(e) => {
                // The system's refusal, in the list area.
                *self.listing.borrow_mut() = None;
                self.rows.borrow_mut().clear();
                let x = qap::explain(&self.sys, &e);
                crate::log!("{}: {}", path, x.line());
                self.report(&x.line());
                self.show_note(&x.line());
            }
        }
        self.nav_buttons();
    }

    fn listed(&self, total: u32) {
        let cur = self.cur.borrow().clone();
        self.said(&qsa_msg::message(
            id::ENTRIES_LISTED,
            &[&model::grouped(u64::from(total)), &cur],
        ));
    }

    /// The rows loaded, sorted, in the list.
    fn show_rows(&self) {
        let (col, asc) = self.sort.get();
        model::sort(&mut self.rows.borrow_mut(), col, asc);
        let rows = self.rows.borrow();
        let mut list = self.w.list.clone();
        list.set_rows(rows.iter().map(Row::cells).collect());
        list.set_icons(rows.iter().map(|r| Some(r.glyph())).collect());
        list.set_sort(Some((col, asc)));
        self.w.note.clone().hide();
    }

    fn show_note(&self, text: &str) {
        self.w.list.clone().set_rows(Vec::new());
        let mut n = self.w.note.clone();
        n.set_label(text);
        n.show();
        n.redraw();
    }

    /// The next page, when the list has scrolled near the end of those
    /// loaded.
    fn more(&self) {
        let wanted = self.listing.borrow().as_ref().is_some_and(|l| !l.done())
            && self.w.list.near_end(MORE_AT);
        if !wanted {
            return;
        }
        let Some(mut l) = self.listing.borrow().clone() else {
            return;
        };
        let Some(r) = self.call(move |c| {
            let page = l.next_page(c)?.unwrap_or_default();
            let dir = l.path.clone();
            let rows: Vec<Row> = page.into_iter().map(|e| Row::of(c, &dir, e)).collect();
            Ok::<_, Error>((l, rows))
        }) else {
            return;
        };
        match r {
            Ok((l, rows)) => {
                *self.listing.borrow_mut() = Some(l);
                let top = self.w.list.table().top_row();
                let selected = self.w.list.selected_rows();
                self.rows.borrow_mut().extend(rows);
                self.show_rows();
                let mut list = self.w.list.clone();
                list.select_rows(&selected);
                let mut t = list.table().clone();
                t.set_top_row(top);
                let loaded = self.rows.borrow().len();
                let total = self
                    .listing
                    .borrow()
                    .as_ref()
                    .and_then(|l| l.total)
                    .unwrap_or(0);
                self.report(&format!(
                    "{} of {} entries loaded.",
                    model::grouped(loaded as u64),
                    model::grouped(u64::from(total))
                ));
            }
            Err(e) => {
                // No more pages this time; Refresh tries again.
                if let Some(l) = self.listing.borrow_mut().as_mut() {
                    l.total = Some(l.next);
                }
                self.report(&qap::explain(&self.sys, &e).line());
            }
        }
    }

    fn nav_buttons(&self) {
        for (b, on) in [
            (&self.w.back, !self.back.borrow().is_empty()),
            (&self.w.forward, !self.forward.borrow().is_empty()),
        ] {
            let mut b = b.clone();
            if on {
                b.activate();
            } else {
                b.deactivate();
            }
        }
    }

    /// The rows selected.
    fn selected(&self) -> Vec<Row> {
        let rows = self.rows.borrow();
        self.w
            .list
            .selected_rows()
            .into_iter()
            .filter_map(|i| rows.get(i).cloned())
            .collect()
    }

    fn pump(&self) -> bool {
        if self.busy.get() {
            return self.w.window.shown();
        }
        while let Some(a) = self.rx.recv() {
            self.act(a);
            if !self.w.window.shown() {
                return false;
            }
        }
        let parent = self.parent_window();
        let notes = self.w.queue.pump(&|c| queue::existing_prompt(parent, c));
        self.notes(notes);
        if self.h.is_some() {
            if !self.w.queue.busy()
                && let Some(d) = self.stale.borrow_mut().take()
                && d == *self.cur.borrow()
            {
                self.go_to(&d, false);
            }
            self.more();
        }
        self.w.window.shown()
    }

    /// What the queue said: on the status line and in the log; a folder
    /// shown that an upload changed is listed again once nothing runs.
    fn notes(&self, notes: Vec<queue::Note>) {
        for n in notes {
            match n {
                queue::Note::Said(m) => self.said(&m),
                queue::Note::Changed(d) => *self.stale.borrow_mut() = Some(d),
            }
        }
    }

    /// Closes the window, asking first when transfers are running.
    fn close(&self) {
        if self.w.queue.busy() {
            let choice = MessageBox::new(
                FUNCTION,
                "Transfers are still running or queued. Cancel them and close the window?",
            )
            .buttons(&["Cancel Transfers", "Keep Open"], 1, 1)
            .run(self.parent_window());
            if choice != 0 {
                return;
            }
            self.w.queue.cancel_all();
        }
        self.w.window.clone().hide();
    }

    /// The options for a batch, from the dialog; `None` when cancelled.
    fn ask_options(&self, what: &str) -> Option<qsa_transfer::files::Options> {
        let mut d = queue::OptionsDialog::build(what, self.options.get());
        loop {
            match d.run(self.parent_window()) {
                queue::Asked::Ok => match d.options() {
                    Some(o) => {
                        self.options.set(o);
                        return Some(o);
                    }
                    None => self.report("Enter a CCSID for new files, such as 1208."),
                },
                queue::Asked::Cancel => return None,
                queue::Asked::SaveRequest | queue::Asked::OpenRequest => {
                    self.report("Requests are saved from the transfer options of a later step.");
                }
            }
        }
    }

    /// Starts the queue's connection when it has none; false when it
    /// could not.
    fn queue_ready(&self) -> bool {
        if self.w.queue.started() {
            return true;
        }
        let Some(h) = &self.h else { return false };
        let Some((conn, shared)) = queue::connect(self.parent_window(), h, &self.sys) else {
            return false;
        };
        self.w.queue.start(conn);
        if shared && !self.shared_said.replace(true) {
            let m = qsa_msg::message(id::SHARED_FOR_TRANSFERS, &[]);
            self.said(&m);
            msgdetails::show_message(self.parent_window(), &m);
        }
        true
    }

    /// Queues `works` as one batch and shows the queue.
    fn enqueue(&self, works: Vec<queue::Work>) {
        if works.is_empty() || !self.queue_ready() {
            return;
        }
        let batch = self.w.queue.batch();
        let n = works.len();
        for w in works {
            self.w.queue.add(batch, w);
        }
        self.w.show_queue(true);
        self.report(&format!(
            "{n} {} queued.",
            if n == 1 { "transfer" } else { "transfers" }
        ));
    }

    fn download(&self) {
        let rows = self.selected();
        if rows.is_empty() {
            self.report("Select the files and folders to download.");
            return;
        }
        let Some(into) =
            crate::files::choose_folder(&crate::files::downloads_dir(), "Download To Folder")
        else {
            return;
        };
        let what = match &rows[..] {
            [one] => one.entry.name.clone(),
            _ => format!("{} entries", rows.len()),
        };
        let Some(options) = self.ask_options(&format!("Download {what}")) else {
            return;
        };
        self.enqueue(queue::downloads(&rows, &into, options));
    }

    fn upload(&self, paths: Vec<PathBuf>) {
        if paths.is_empty() {
            return;
        }
        let cur = self.cur.borrow().clone();
        let what = match &paths[..] {
            [one] => one
                .file_name()
                .map_or_else(String::new, |n| n.to_string_lossy().into_owned()),
            _ => format!("{} files", paths.len()),
        };
        let Some(options) = self.ask_options(&format!("Upload {what} to {cur}")) else {
            return;
        };
        self.enqueue(queue::uploads(&paths, &cur, options));
    }

    fn act(&self, a: Act) {
        let cur = self.cur.borrow().clone();
        match a {
            Act::Close => self.close(),
            Act::Download => self.download(),
            Act::Upload => {
                let paths = crate::files::choose_open_many(
                    &crate::files::downloads_dir(),
                    &format!("Upload To {cur}"),
                );
                self.upload(paths);
            }
            Act::Dropped => {
                let paths = std::mem::take(&mut *self.w.dropped.borrow_mut());
                self.upload(paths);
            }
            Act::Transfers => {
                let on = !self.w.queue.group.visible();
                self.w.show_queue(on);
            }
            Act::Queue(q) => {
                let notes = self.w.queue.act(q);
                self.notes(notes);
            }
            Act::Go => {
                let p = self.w.path.value();
                if p.starts_with('/') {
                    self.go_to(&p, true);
                } else {
                    self.go_to(&join(&cur, &p), true);
                }
            }
            Act::Refresh => self.go_to(&cur, false),
            Act::Up => {
                if cur != "/" {
                    self.go_to(parent(&cur), true);
                }
            }
            Act::Home => self.go_to(&self.home.clone(), true),
            Act::Back => {
                let Some(p) = self.back.borrow_mut().pop() else {
                    return;
                };
                self.forward.borrow_mut().push(cur);
                self.go_to(&p, false);
            }
            Act::Forward => {
                let Some(p) = self.forward.borrow_mut().pop() else {
                    return;
                };
                self.back.borrow_mut().push(cur);
                self.go_to(&p, false);
            }
            Act::Tree => self.tree_event(),
            Act::Sort(c) => {
                let (col, asc) = self.sort.get();
                self.sort.set((c, if col == c { !asc } else { true }));
                self.show_rows();
            }
            Act::SelectAll => {
                let n = self.w.list.len();
                self.w.list.clone().select_rows(&(0..n).collect::<Vec<_>>());
            }
            Act::Open => self.open(),
            Act::NewFolder => self.new_folder(&cur),
            Act::Rename => self.rename(&cur),
            Act::Copy | Act::Cut => self.clip(a == Act::Cut),
            Act::Paste => self.paste(&cur),
            Act::Delete => self.delete(),
        }
    }

    /// Opens the entry selected: a folder (or a link to one) is shown.
    fn open(&self) {
        let [row] = &self.selected()[..] else {
            return;
        };
        if row.broken() {
            self.report(&format!(
                "{} is a link to {}, which is not there.",
                row.entry.name,
                row.name_text()
            ));
            return;
        }
        let Some((path, entry)) = row.opens_as() else {
            self.report(&format!(
                "The target of {} cannot be looked at.",
                row.entry.name
            ));
            return;
        };
        if entry.kind == EntryKind::Directory {
            self.go_to(&path, true);
        }
    }

    fn new_folder(&self, cur: &str) {
        let Some(name) = InputBox::new("New Folder", &format!("Name of the new folder in {cur}:"))
            .run(self.parent_window())
        else {
            return;
        };
        let name = name.trim().to_owned();
        if name.is_empty() || name.contains('/') {
            return;
        }
        let path = join(cur, &name);
        let p = path.clone();
        match self.call(move |c| c.create_directory(&p, false)) {
            Some(Ok(())) => {
                self.go_to(cur, false);
                self.said(&qsa_msg::message(id::FOLDER_CREATED, &[&path]));
            }
            Some(Err(e)) => self.failed(&e),
            None => {}
        }
    }

    fn rename(&self, cur: &str) {
        let [row] = &self.selected()[..] else {
            self.report("Select one entry to rename.");
            return;
        };
        let Some(name) = InputBox::new("Rename", &format!("New name for {}:", row.entry.name))
            .value(&row.entry.name)
            .run(self.parent_window())
        else {
            return;
        };
        let name = name.trim().to_owned();
        if name.is_empty() || name == row.entry.name || name.contains('/') {
            return;
        }
        let (from, to) = (row.path.clone(), join(cur, &name));
        let (f, t) = (from.clone(), to.clone());
        match self.call(move |c| c.rename(&f, &t, false)) {
            Some(Ok(())) => {
                self.go_to(cur, false);
                self.said(&qsa_msg::message(id::RENAMED, &[&from, &to]));
            }
            Some(Err(e)) => self.failed(&e),
            None => {}
        }
    }

    fn clip(&self, cut: bool) {
        let rows = self.selected();
        if rows.is_empty() {
            return;
        }
        let n = rows.len();
        CLIP.with(|c| {
            *c.borrow_mut() = Some(Clip {
                system: self.sys.name.clone(),
                entries: rows
                    .iter()
                    .map(|r| (r.path.clone(), r.is_real_folder()))
                    .collect(),
                cut,
            });
        });
        self.report(&format!(
            "{n} {} {}; open a folder and Paste.",
            if n == 1 { "entry" } else { "entries" },
            if cut { "cut" } else { "copied" }
        ));
    }

    /// Asks what to do with `name`, already in `dir`: replace (true), skip
    /// (false), or `None` to stop.
    fn ask_existing(&self, name: &str, dir: &str) -> Option<bool> {
        let m = qsa_msg::message(id::ALREADY_EXISTS, &[name, dir]);
        match msgdetails::ask_message(self.parent_window(), &m, &["Replace", "Skip", "Cancel"]) {
            0 => Some(true),
            1 => Some(false),
            _ => None,
        }
    }

    fn paste(&self, cur: &str) {
        let Some(clip) = CLIP.with(|c| c.borrow().clone()) else {
            return;
        };
        if !clip.system.eq_ignore_ascii_case(&self.sys.name) {
            self.report(&format!(
                "What was copied is on {}, not {}.",
                clip.system, self.sys.name
            ));
            return;
        }
        let mut last = None;
        for (from, folder) in &clip.entries {
            let to = join(cur, file_name(from));
            if *from == to && clip.cut {
                continue;
            }
            if to.starts_with(&format!("{from}/")) {
                self.report(&format!("{from} cannot be put inside itself."));
                break;
            }
            let mut replace = false;
            loop {
                let (f, t, folder) = (from.clone(), to.clone(), *folder);
                let cut = clip.cut;
                let r = self.call(move |c| -> Result<(), qsa_transfer::files::Stopped> {
                    let stop = |at: &str, error: Error| qsa_transfer::files::Stopped {
                        at: at.to_owned(),
                        error: Box::new(error),
                    };
                    if cut {
                        c.rename(&f, &t, replace).map_err(|e| stop(&f, e))
                    } else if folder {
                        qsa_transfer::files::copy_tree(c, &f, &t, replace, &mut |_| {})
                    } else {
                        c.copy(&f, &t, replace).map_err(|e| stop(&f, e))
                    }
                });
                match r {
                    Some(Ok(())) => {
                        last = Some(if clip.cut {
                            qsa_msg::message(id::RENAMED, &[from, &to])
                        } else {
                            qsa_msg::message(id::COPIED, &[from, cur])
                        });
                        break;
                    }
                    Some(Err(s)) if s.error.status() == Some(QapStatus::Exists) && !replace => {
                        match self.ask_existing(file_name(&to), cur) {
                            Some(true) => replace = true,
                            Some(false) => break,
                            None => {
                                self.go_to(cur, false);
                                return;
                            }
                        }
                    }
                    Some(Err(s)) => {
                        self.go_to(cur, false);
                        self.failed(&s.error);
                        return;
                    }
                    None => return,
                }
            }
        }
        if clip.cut {
            CLIP.with(|c| *c.borrow_mut() = None);
        }
        self.go_to(cur, false);
        if let Some(m) = last {
            self.said(&m);
        }
    }

    fn delete(&self) {
        let rows = self.selected();
        if rows.is_empty() {
            return;
        }
        let folders: Vec<&Row> = rows.iter().filter(|r| r.is_real_folder()).collect();
        let mut text = String::new();
        if folders.is_empty() {
            text.push_str(&delete_files_text(&rows));
        } else {
            // The counts first, so the confirmation says what goes.
            let paths: Vec<String> = folders.iter().map(|r| r.path.clone()).collect();
            let Some(counted) = self.call(move |c| {
                paths
                    .iter()
                    .map(|p| qsa_transfer::files::count(c, p).map(|n| (p.clone(), n)))
                    .collect::<Result<Vec<_>, _>>()
            }) else {
                return;
            };
            let counted = match counted {
                Ok(c) => c,
                Err(s) => {
                    self.failed(&s.error);
                    return;
                }
            };
            text = delete_folders_text(&rows, &counted);
        }
        let choice = MessageBox::new("Confirm Delete", &text)
            .glyph(Glyph::Cross)
            .buttons(&["Delete", "Cancel"], 1, 1)
            .run(self.parent_window());
        if choice != 0 {
            return;
        }
        let cur = self.cur.borrow().clone();
        let mut deleted = 0usize;
        for r in &rows {
            let (p, folder) = (r.path.clone(), r.is_real_folder());
            let res = self.call(move |c| {
                if folder {
                    qsa_transfer::files::delete_tree(c, &p, &mut |_| {})
                } else {
                    c.delete(&p).map_err(|e| qsa_transfer::files::Stopped {
                        at: p.clone(),
                        error: Box::new(e),
                    })
                }
            });
            match res {
                Some(Ok(())) => deleted += 1,
                Some(Err(s)) => {
                    self.go_to(&cur, false);
                    let sm = s.error.server_message().map_or_else(
                        || s.error.to_string(),
                        |m| format!("{}: {}", m.id, m.text.trim_end_matches('.')),
                    );
                    let m = qsa_msg::message(id::DELETE_STOPPED, &[&r.path, &s.at, &sm]);
                    self.said(&m);
                    match s.error.server_message() {
                        Some(sm) => msgdetails::show_system_message(
                            self.parent_window(),
                            Some(&m),
                            &qap::shown(sm),
                        ),
                        None => msgdetails::show_message(self.parent_window(), &m),
                    }
                    return;
                }
                None => return,
            }
        }
        self.go_to(&cur, false);
        match &rows[..] {
            [one] => self.said(&qsa_msg::message(id::DELETED, &[&one.path])),
            _ => self.report(&format!("{deleted} entries deleted.")),
        }
    }
}

/// `path` with no trailing `/` (but `/` itself) and no empty parts.
pub fn normal(path: &str) -> String {
    let parts: Vec<&str> = path
        .split('/')
        .filter(|p| !p.is_empty() && *p != ".")
        .collect();
    let mut out: Vec<&str> = Vec::new();
    for p in parts {
        if p == ".." {
            out.pop();
        } else {
            out.push(p);
        }
    }
    format!("/{}", out.join("/"))
}

/// The confirmation for deleting files and links: each named.
pub fn delete_files_text(rows: &[Row]) -> String {
    const SHOWN: usize = 12;
    let mut t = match rows {
        [one] => format!("Delete {} from the system?\n", one.entry.name),
        _ => format!("Delete these {} entries from the system?\n", rows.len()),
    };
    if rows.len() > 1 {
        for r in rows.iter().take(SHOWN) {
            t.push_str(&format!("\n{}", r.entry.name));
        }
        if rows.len() > SHOWN {
            t.push_str(&format!("\n... and {} more", rows.len() - SHOWN));
        }
    }
    t
}

/// The confirmation for deleting folders with their contents: the counts,
/// then the other entries.
pub fn delete_folders_text(
    rows: &[Row],
    counted: &[(String, qsa_transfer::files::Counts)],
) -> String {
    let plural = |n: u64, one: &str, many: &str| {
        format!("{} {}", model::grouped(n), if n == 1 { one } else { many })
    };
    let mut t = String::from("Delete from the system, with everything in them:\n");
    for (p, c) in counted {
        t.push_str(&format!(
            "\n{} ({} and {}, {})",
            file_name(p),
            plural(c.files, "file", "files"),
            plural(c.folders, "folder", "folders"),
            model::bytes_text(c.bytes)
        ));
    }
    let others: Vec<&Row> = rows.iter().filter(|r| !r.is_real_folder()).collect();
    if !others.is_empty() {
        t.push_str(&format!(
            "\n\nand {}:",
            plural(others.len() as u64, "other entry", "other entries")
        ));
        for r in others.iter().take(8) {
            t.push_str(&format!("\n{}", r.entry.name));
        }
        if others.len() > 8 {
            t.push_str(&format!("\n... and {} more", others.len() - 8));
        }
    }
    t.push_str("\n\nA delete that the system refuses part way stops there; what was deleted before stays deleted.");
    t
}

thread_local! {
    static WINDOWS: RefCell<Vec<Rc<Ifs>>> = const { RefCell::new(Vec::new()) };
}

/// Why the Integrated File System is not available for `system`, if it is
/// not: `QAD5001` when its QAP version has no `Ifs` service. A version not
/// known does not stop it: opening it finds out.
pub fn blocked(system: &str, version: Option<u16>) -> Option<qsa_msg::Message> {
    let op = Op::ListDirectory;
    match version {
        Some(v) if v < op.since() => Some(qap::version_needed(FUNCTION, system, op, v)),
        _ => None,
    }
}

/// Opens the Integrated File System window for `system`, signing on first
/// when this program has no session on it. Before QAP version 5:
/// `QAD5001`.
pub fn open(parent: Option<&dyn WindowExt>, state: &Shared, system: &str) {
    let Some(sys) = state.borrow().systems.get(system).cloned() else {
        return;
    };
    let Some(h) = qap::session(parent, state, &sys.name) else {
        return;
    };
    let (refused, home) = match h.try_lock() {
        Ok(mut c) => {
            let has = c.version().unwrap_or(1);
            let refused = c.supports(Op::ListDirectory).err().map(|_| has);
            let home = c
                .profile()
                .map(|p| p.home_directory.clone())
                .unwrap_or_default();
            (refused, home)
        }
        Err(_) => (None, String::new()),
    };
    if let Some(has) = refused {
        let m = qap::version_needed(FUNCTION, &sys.name, Op::ListDirectory, has);
        crate::log!("{m}");
        msgdetails::show_message(parent, &m);
        return;
    }
    let (w, rx) = build_widgets(&sys.name);
    let mut s = Ifs::new(w, rx, &sys, &home);
    s.h = Some(h);
    s.state = Some(state.clone());
    let s = Rc::new(s);
    let mut win = s.w.window.clone();
    qsa_ui::propdialog::center_on(&mut win, parent);
    qsa_ui::propdialog::show_placed(&mut win);
    let profile = qap::signed_on(state, &sys.name).unwrap_or_default();
    s.w.status
        .clone()
        .set_status(&format!("{}: {profile}", sys.name), Status::Active);
    let home = s.home.clone();
    s.go_to(&home, false);
    s.nav_buttons();
    WINDOWS.with(|v| v.borrow_mut().push(s));
    tick();
}

fn tick() {
    thread_local! { static TICKING: Cell<bool> = const { Cell::new(false) }; }
    if TICKING.with(|t| t.replace(true)) {
        return;
    }
    app::add_timeout3(0.05, |h| {
        let windows: Vec<Rc<Ifs>> = WINDOWS.with(|v| v.borrow().clone());
        let open: Vec<bool> = windows.iter().map(|w| w.pump()).collect();
        WINDOWS.with(|v| {
            let mut i = 0;
            v.borrow_mut().retain(|_| {
                let keep = open.get(i).copied().unwrap_or(true);
                i += 1;
                keep
            });
        });
        if WINDOWS.with(|v| !v.borrow().is_empty()) {
            app::repeat_timeout3(0.05, h);
        } else {
            TICKING.with(|t| t.set(false));
        }
    });
}

/// Whether any of these windows is open.
pub fn any_open() -> bool {
    WINDOWS.with(|v| v.borrow().iter().any(|w| w.w.window.shown()))
}

/// An entry for screenshots and tests.
pub fn entry(name: &str, kind: EntryKind, size: u64, ccsid: u16) -> Entry {
    Entry {
        name: name.into(),
        kind,
        size,
        modified: 1_793_000_000_000,
        owner: "BOB".into(),
        ccsid,
        target: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_are_normal() {
        assert_eq!(normal("/home/BOB/"), "/home/BOB");
        assert_eq!(normal("//home//BOB/./x/../y"), "/home/BOB/y");
        assert_eq!(normal("/.."), "/");
        assert_eq!(normal(""), "/");
        assert_eq!(part("/home/BOB"), "\\/home\\/BOB");
    }

    #[test]
    fn delete_confirmations() {
        let row = |n: &str, k| Row {
            path: format!("/home/BOB/{n}"),
            entry: entry(n, k, 10, 1208),
            link: None,
        };
        let one = delete_files_text(&[row("a.txt", EntryKind::File)]);
        assert_eq!(one, "Delete a.txt from the system?\n");
        let many: Vec<Row> = (0..15)
            .map(|i| row(&format!("f{i}.txt"), EntryKind::File))
            .collect();
        let t = delete_files_text(&many);
        assert!(t.starts_with("Delete these 15 entries"));
        assert!(t.contains("f11.txt") && !t.contains("f12.txt"));
        assert!(t.ends_with("... and 3 more"));
        let rows = [
            row("tree", EntryKind::Directory),
            row("x.txt", EntryKind::File),
        ];
        let counts = qsa_transfer::files::Counts {
            files: 7,
            folders: 4,
            bytes: 300,
        };
        let t = delete_folders_text(&rows, &[("/home/BOB/tree".into(), counts)]);
        assert!(t.contains("tree (7 files and 4 folders, 300 bytes)"), "{t}");
        assert!(t.contains("and 1 other entry:\nx.txt"), "{t}");
    }
}
