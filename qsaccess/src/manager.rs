// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! Session Manager: the session files in the Sessions folder and any other
//! folders the user adds, with New, Start, Edit, Copy, Rename, Delete, Add
//! folder, Remove folder and Refresh.
//!
//! - It is a window of the one `qsaccess` process; each session it starts
//!   opens in a window of its own.
//! - Several rows can be selected; Start opens them all. A double-click
//!   starts one.
//! - A click on a column header sorts by it; another click reverses.
//! - The folders are looked at every five seconds, and the list follows
//!   files added, changed or removed by anything else.

use qsa_ui::chan::{self, Emit};
use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant, SystemTime};

use fltk::app;
use fltk::button::Button;
use fltk::prelude::*;
use fltk::window::Window;
use qsa_config::qsd::{self, SessionFile};
use qsa_config::settings::same_path;
use qsa_msg::{Message, id};
use qsa_ui::controls::{self, Switch};
use qsa_ui::listtable::{Cell as LCell, ListTable};
use qsa_ui::propdialog::center_on;
use qsa_ui::statusbar::{Status, StatusLine};
use qsa_ui::theme::{self, colors};
use qsa_ui::{Glyph, InputBox, MessageBox};

use crate::app::Shared;
use crate::session::props;
use crate::{files, localtime};

/// Window width.
pub const W: i32 = 860;
/// Window height.
pub const H: i32 = 440;
/// How often the folders are looked at.
pub const POLL: Duration = Duration::from_secs(5);

/// Column titles and widths.
pub const COLUMNS: [(&str, i32); 6] = [
    ("Title", 170),
    ("System", 130),
    ("Screen size", 78),
    ("Device name", 86),
    ("Last used", 112),
    ("Location", 140),
];

/// One session file in a listed folder.
#[derive(Clone, Debug)]
pub struct Entry {
    pub path: PathBuf,
    pub folder: PathBuf,
    /// The file, or why it could not be read.
    pub file: Result<SessionFile, String>,
    pub last_used: Option<i64>,
}

impl Entry {
    /// The title shown: the file's, else its name.
    pub fn title(&self) -> String {
        match &self.file {
            Ok(f) => f.display_title(Some(&self.path)),
            Err(_) => self
                .path
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default(),
        }
    }

    /// The text of each column.
    pub fn columns(&self) -> [String; 6] {
        let (system, size, device) = match &self.file {
            Ok(f) => (
                f.system.name.clone(),
                f.session.screen_size.label().to_owned(),
                f.session.device_name.clone(),
            ),
            Err(_) => ("(cannot be read)".to_owned(), String::new(), String::new()),
        };
        [
            self.title(),
            system,
            size,
            device,
            self.last_used.map(localtime::format).unwrap_or_default(),
            self.folder.display().to_string(),
        ]
    }

    fn cells(&self) -> Vec<LCell> {
        let mut out: Vec<LCell> = self.columns().into_iter().map(LCell::from).collect();
        if self.file.is_err() {
            out[1].status = Status::Error;
        }
        out
    }
}

/// The folders listed: the Sessions folder first, then the ones added.
pub fn folders(state: &Shared) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = qsa_config::store::default_sessions_dir()
        .into_iter()
        .collect();
    for d in &state.borrow().settings.session_folders {
        if !out.iter().any(|o| same_path(o, d)) {
            out.push(d.clone());
        }
    }
    out
}

/// Reads every session file in `folders`.
pub fn scan(folders: &[PathBuf], state: &Shared) -> Vec<Entry> {
    let st = state.borrow();
    let mut out = Vec::new();
    for folder in folders {
        for path in qsd::files_in(folder) {
            out.push(Entry {
                file: SessionFile::load(&path).map_err(|e| e.to_string()),
                last_used: st.settings.last_used(&path),
                folder: folder.clone(),
                path,
            });
        }
    }
    out
}

/// What the folders hold, to notice changes: each file, its size and when
/// it was changed.
type Signature = Vec<(PathBuf, u64, Option<SystemTime>)>;

fn signature(folders: &[PathBuf]) -> Signature {
    folders
        .iter()
        .flat_map(|d| qsd::files_in(d))
        .map(|p| {
            let meta = std::fs::metadata(&p).ok();
            let len = meta.as_ref().map_or(0, |m| m.len());
            let modified = meta.and_then(|m| m.modified().ok());
            (p, len, modified)
        })
        .collect()
}

/// Sorts by column `col`, ascending or not. Text sorts without regard to
/// case; Last used sorts by time, sessions never used last.
pub fn sort(entries: &mut [Entry], col: usize, ascending: bool) {
    use std::cmp::Ordering;
    let order = |o: Ordering| if ascending { o } else { o.reverse() };
    entries.sort_by(|a, b| {
        let o = if col == 4 {
            match (a.last_used, b.last_used) {
                (Some(x), Some(y)) => order(x.cmp(&y)),
                (Some(_), None) => Ordering::Less,
                (None, Some(_)) => Ordering::Greater,
                (None, None) => Ordering::Equal,
            }
        } else {
            let (x, y) = (&a.columns()[col], &b.columns()[col]);
            order(x.to_lowercase().cmp(&y.to_lowercase()))
        };
        o.then_with(|| a.path.cmp(&b.path))
    });
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Req {
    New,
    Start,
    Edit,
    Copy,
    Rename,
    Delete,
    AddFolder,
    RemoveFolder,
    Refresh,
    Sort(usize),
    Close,
}

/// The Session Manager window.
pub struct Manager {
    pub window: Window,
    pub list: ListTable,
    status: StatusLine,
    state: Shared,
    rx: chan::Receiver<Req>,
    entries: RefCell<Vec<Entry>>,
    sort: Cell<(usize, bool)>,
    sig: RefCell<Signature>,
    polled: Cell<Instant>,
    /// Enabled with any selection; with exactly one readable; with a
    /// removable folder selected.
    any: Vec<Switch>,
    one: Vec<Switch>,
    folder: Switch,
}

thread_local! {
    static MANAGER: RefCell<Option<Rc<Manager>>> = const { RefCell::new(None) };
}

/// Builds the window without showing it (for screenshots).
pub fn build(state: &Shared) -> Rc<Manager> {
    let (tx, rx) = chan::channel::<Req>();
    let m = 10;
    let bw = theme::BUTTON_W + 14;
    let bh = theme::BUTTON_H;
    let mut window = Window::new(0, 0, W, H, None);
    window.set_label("Session Manager");
    window.set_color(colors::FACE);
    window.size_range(560, 320, 0, 0);

    let lw = W - 3 * m - bw;
    let lh = H - 2 * m - bh - 8;
    let mut list = ListTable::new(m, m, lw, lh, &COLUMNS);
    list.set_multi(true);
    let bx = m + lw + m;
    let mut buttons = fltk::group::Group::new(bx, m, bw, lh, None);
    let mut y = m;
    let mut button = |label: &str, r: Req, gap: i32| -> Button {
        let mut b = controls::button(bx, y, bw, bh, label);
        b.emit_to(tx, r);
        y += bh + gap;
        b
    };
    button("New...", Req::New, 6);
    let mut start = button("Start", Req::Start, 6);
    let mut edit = button("Edit...", Req::Edit, 6);
    let mut copy = button("Copy", Req::Copy, 6);
    let mut rename = button("Rename...", Req::Rename, 6);
    let mut delete = button("Delete", Req::Delete, 18);
    button("Add Folder...", Req::AddFolder, 6);
    let mut remove = button("Remove Folder", Req::RemoveFolder, 18);
    button("Refresh", Req::Refresh, 6);
    buttons.end();
    buttons.make_resizable(false);
    let status = StatusLine::new(m, H - m - bh, W - 3 * m - bw, bh);
    let mut close = controls::default_button(bx, H - m - bh, bw, bh, "Close");
    close.emit_to(tx, Req::Close);
    window.end();
    window.resizable(list.table());
    window.set_callback(move |_| tx.send(Req::Close));
    list.on_header_click(move |c| tx.send(Req::Sort(c)));
    list.on_activate(move |sel| {
        if sel.is_some() {
            tx.send(Req::Start);
        }
    });

    let any = vec![
        Switch::for_button(&mut start),
        Switch::for_button(&mut copy),
        Switch::for_button(&mut delete),
    ];
    let one = vec![
        Switch::for_button(&mut edit),
        Switch::for_button(&mut rename),
    ];
    let folder = Switch::for_button(&mut remove);
    let mgr = Rc::new(Manager {
        window,
        list,
        status,
        state: state.clone(),
        rx,
        entries: RefCell::new(Vec::new()),
        sort: Cell::new((0, true)),
        sig: RefCell::new(Vec::new()),
        polled: Cell::new(Instant::now()),
        any,
        one,
        folder,
    });
    {
        let me = Rc::downgrade(&mgr);
        mgr.list.clone().on_select(move |_| {
            if let Some(m) = me.upgrade() {
                m.update_buttons();
            }
        });
    }
    mgr.reload(&[]);
    mgr
}

/// Shows Session Manager, building it the first time, and raises it.
pub fn open(state: &Shared, parent: Option<&dyn WindowExt>) {
    let existing = MANAGER.with(|m| m.borrow().clone());
    let mgr = match existing {
        Some(m) => m,
        None => {
            let _ = files::sessions_dir();
            let m = build(state);
            let mut w = m.window.clone();
            center_on(&mut w, parent);
            MANAGER.with(|slot| *slot.borrow_mut() = Some(m.clone()));
            // Wakes the event loop so that [`pump`] can look at the folders.
            app::add_timeout3(POLL.as_secs_f64(), |h| {
                if shown() {
                    app::repeat_timeout3(POLL.as_secs_f64(), h);
                }
            });
            m
        }
    };
    qsa_ui::propdialog::show_placed(&mut mgr.window.clone());
}

/// Closes Session Manager if it is open.
pub fn close() {
    if let Some(m) = MANAGER.with(|m| m.borrow_mut().take()) {
        m.window.clone().hide();
    }
}

/// Whether Session Manager is open.
pub fn shown() -> bool {
    MANAGER.with(|m| m.borrow().as_ref().is_some_and(|m| m.window.shown()))
}

/// Handles Session Manager's requests, and looks at the folders when it is
/// time. Called by the main loop after each wait.
pub fn pump() {
    let Some(mgr) = MANAGER.with(|m| m.borrow().clone()) else {
        return;
    };
    while let Some(r) = mgr.rx.recv() {
        mgr.handle(r);
    }
    if !mgr.window.shown() {
        MANAGER.with(|m| *m.borrow_mut() = None);
        return;
    }
    if mgr.polled.get().elapsed() >= POLL {
        mgr.poll();
    }
}

impl Manager {
    /// The selected entries.
    fn selected(&self) -> Vec<Entry> {
        let e = self.entries.borrow();
        self.list
            .selected_rows()
            .into_iter()
            .filter_map(|i| e.get(i).cloned())
            .collect()
    }

    fn update_buttons(&self) {
        let sel = self.selected();
        for s in &self.any {
            s.set_enabled(!sel.is_empty());
        }
        let one = sel.len() == 1 && sel[0].file.is_ok();
        for s in &self.one {
            s.set_enabled(one);
        }
        let default = qsa_config::store::default_sessions_dir();
        let removable = !sel.is_empty()
            && sel
                .iter()
                .all(|e| default.as_deref().is_none_or(|d| !same_path(d, &e.folder)));
        self.folder.set_enabled(removable);
    }

    /// Reads the folders again and redraws, keeping the selection (by path)
    /// or selecting `select`.
    fn reload(&self, select: &[PathBuf]) {
        let keep: Vec<PathBuf> = if select.is_empty() {
            self.selected().into_iter().map(|e| e.path).collect()
        } else {
            select.to_vec()
        };
        let dirs = folders(&self.state);
        let entries = scan(&dirs, &self.state);
        *self.sig.borrow_mut() = signature(&dirs);
        self.polled.set(Instant::now());
        self.show(entries, &keep, dirs.len());
    }

    /// Shows `entries` (sorted here), selecting those in `keep`.
    pub fn show(&self, mut entries: Vec<Entry>, keep: &[PathBuf], folder_count: usize) {
        let (col, asc) = self.sort.get();
        sort(&mut entries, col, asc);
        let rows: Vec<Vec<LCell>> = entries.iter().map(Entry::cells).collect();
        let picks: Vec<usize> = entries
            .iter()
            .enumerate()
            .filter(|(_, e)| keep.iter().any(|k| same_path(k, &e.path)))
            .map(|(i, _)| i)
            .collect();
        let n = entries.len();
        *self.entries.borrow_mut() = entries;
        let mut list = self.list.clone();
        list.set_rows(rows);
        list.set_sort(Some((col, asc)));
        list.select_rows(&picks);
        self.update_buttons();
        let text = format!(
            "{n} session{} in {folder_count} folder{}",
            if n == 1 { "" } else { "s" },
            if folder_count == 1 { "" } else { "s" }
        );
        self.status.clone().set(&text, Status::Neutral);
    }

    /// Reloads if anything in the folders has changed.
    fn poll(&self) {
        self.polled.set(Instant::now());
        let now = signature(&folders(&self.state));
        if *self.sig.borrow() != now {
            self.reload(&[]);
        }
    }

    fn report(&self, m: &Message) {
        crate::log!("{m}");
        let s = if m.kind() == qsa_msg::Kind::Error {
            Status::Error
        } else {
            Status::Neutral
        };
        self.status.clone().set(&m.to_string(), s);
    }

    fn error(&self, text: &str) {
        crate::log!("Session Manager: {text}");
        self.status.clone().set(text, Status::Error);
        MessageBox::new("Session Manager", text)
            .glyph(Glyph::Cross)
            .run(Some(&self.window));
    }

    fn save_settings(&self) {
        if let Err(e) = self.state.borrow().save_settings() {
            crate::log!("settings not saved: {e}");
        }
    }

    fn handle(&self, r: Req) {
        match r {
            Req::New => self.new_session(),
            Req::Start => self.start(),
            Req::Edit => self.edit(),
            Req::Copy => self.copy(),
            Req::Rename => self.rename(),
            Req::Delete => self.delete(),
            Req::AddFolder => self.add_folder(),
            Req::RemoveFolder => self.remove_folder(),
            Req::Refresh => self.reload(&[]),
            Req::Sort(c) => {
                let (col, asc) = self.sort.get();
                self.sort.set((c, if col == c { !asc } else { true }));
                self.reload(&[]);
            }
            Req::Close => self.window.clone().hide(),
        }
    }

    fn start(&self) {
        let sel = self.selected();
        for e in &sel {
            crate::session::open_file(&self.state, &e.path, Some(&self.window));
        }
        if !sel.is_empty() {
            self.reload(&[]);
        }
    }

    fn new_session(&self) {
        let (system, keymap, systems) = {
            let st = self.state.borrow();
            let sys = st
                .systems
                .get(&st.settings.last_system)
                .or_else(|| st.systems.iter().next())
                .cloned();
            (
                sys,
                st.settings.new_session_keymap.clone(),
                st.systems.clone(),
            )
        };
        let Some(system) = system else {
            self.error("Add a system in System Configurations before making a session.");
            return;
        };
        let mut f = SessionFile::new(&system);
        f.keymap = keymap;
        let Some(p) = props::edit(
            Some(&self.window),
            "New Session",
            &props::Props::from_file(&f),
            &systems,
        ) else {
            return;
        };
        p.apply(&mut f, &systems);
        let name = files::file_name_for(&f.display_title(None), qsd::EXTENSION);
        let Some(path) = files::choose_save_in(
            &files::sessions_dir(),
            "Save New Session",
            files::SESSION_PATTERN,
            &name,
        ) else {
            return;
        };
        let path = files::with_extension(path, qsd::EXTENSION);
        if path.exists() && !self.confirm_replace(&path) {
            return;
        }
        self.write(&f, &path);
        self.reload(std::slice::from_ref(&path));
    }

    fn confirm_replace(&self, path: &Path) -> bool {
        MessageBox::new(
            "Session Manager",
            &format!("{} already exists. Replace it?", path.display()),
        )
        .buttons(&["Replace", "Cancel"], 1, 1)
        .run(Some(&self.window))
            == 0
    }

    /// Writes a session file and reports `QAD2004`.
    fn write(&self, f: &SessionFile, path: &Path) -> bool {
        match f.save(path) {
            Ok(()) => {
                self.report(&qsa_msg::message(
                    id::SESSION_SAVED,
                    &[&path.display().to_string()],
                ));
                true
            }
            Err(e) => {
                self.error(&format!("{} could not be written: {e}", path.display()));
                false
            }
        }
    }

    fn edit(&self) {
        let sel = self.selected();
        let [e] = sel.as_slice() else { return };
        let mut f = match &e.file {
            Ok(f) => f.clone(),
            Err(why) => {
                self.error(why);
                return;
            }
        };
        let systems = self.state.borrow().systems.clone();
        let title = format!("Session Properties - {}", e.title());
        let Some(p) = props::edit(
            Some(&self.window),
            &title,
            &props::Props::from_file(&f),
            &systems,
        ) else {
            return;
        };
        p.apply(&mut f, &systems);
        self.write(&f, &e.path);
        self.reload(&[]);
    }

    /// A free path for a copy of `path`: `name (copy).qsd`, then
    /// `name (copy 2).qsd` ...
    fn copy_path(path: &Path) -> PathBuf {
        let dir = path.parent().unwrap_or(Path::new("."));
        let stem = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        let mut n = 1;
        loop {
            let suffix = if n == 1 {
                " (copy)".to_owned()
            } else {
                format!(" (copy {n})")
            };
            let p = dir.join(format!("{stem}{suffix}.{}", qsd::EXTENSION));
            if !p.exists() {
                return p;
            }
            n += 1;
        }
    }

    fn copy(&self) {
        let mut made = Vec::new();
        for e in self.selected() {
            let Ok(mut f) = e.file.clone() else {
                self.error(&format!(
                    "{} cannot be read, so it was not copied.",
                    e.path.display()
                ));
                continue;
            };
            if !f.session.title.trim().is_empty() {
                f.session.title = format!("{} (copy)", f.session.title.trim());
            }
            let to = Manager::copy_path(&e.path);
            if self.write(&f, &to) {
                made.push(to);
            }
        }
        self.reload(&made);
    }

    fn rename(&self) {
        let sel = self.selected();
        let [e] = sel.as_slice() else { return };
        let Ok(mut f) = e.file.clone() else { return };
        let Some(title) = InputBox::new("Rename Session", "New title:")
            .value(&e.title())
            .run(Some(&self.window))
        else {
            return;
        };
        let title = title.trim().to_owned();
        if title.is_empty() {
            return;
        }
        let to = e.folder.join(files::file_name_for(&title, qsd::EXTENSION));
        if to.exists() && !same_path(&to, &e.path) {
            self.error(&format!(
                "{} already exists. Choose another title.",
                to.display()
            ));
            return;
        }
        f.session.title = title;
        if !self.write(&f, &e.path) {
            return;
        }
        if to != e.path
            && let Err(err) = std::fs::rename(&e.path, &to)
        {
            self.error(&format!("{} could not be renamed: {err}", e.path.display()));
            self.reload(&[]);
            return;
        }
        {
            let mut st = self.state.borrow_mut();
            st.settings.moved(&e.path, Some(&to));
            st.recent_gen += 1;
        }
        self.save_settings();
        self.reload(std::slice::from_ref(&to));
    }

    fn delete(&self) {
        let sel = self.selected();
        if sel.is_empty() {
            return;
        }
        let what = match sel.as_slice() {
            [one] => format!("the session file {}", one.path.display()),
            many => format!("{} session files", many.len()),
        };
        let answer = MessageBox::new(
            "Delete Session",
            &format!("Delete {what}? This cannot be undone."),
        )
        .glyph(Glyph::Cross)
        .buttons(&["Delete", "Cancel"], 1, 1)
        .run(Some(&self.window));
        if answer != 0 {
            return;
        }
        for e in &sel {
            match std::fs::remove_file(&e.path) {
                Ok(()) => {
                    crate::log!("session file {} deleted", e.path.display());
                    let mut st = self.state.borrow_mut();
                    st.settings.moved(&e.path, None);
                    st.recent_gen += 1;
                }
                Err(err) => {
                    self.error(&format!("{} could not be deleted: {err}", e.path.display()))
                }
            }
        }
        self.save_settings();
        self.reload(&[]);
    }

    fn add_folder(&self) {
        let Some(dir) = files::choose_folder(&files::sessions_dir(), "Add Folder") else {
            return;
        };
        let dir = std::path::absolute(&dir).unwrap_or(dir);
        if folders(&self.state).iter().any(|d| same_path(d, &dir)) {
            self.status.clone().set(
                &format!("{} is already listed.", dir.display()),
                Status::Neutral,
            );
            return;
        }
        self.state.borrow_mut().settings.add_session_folder(&dir);
        self.save_settings();
        self.reload(&[]);
        self.report(&qsa_msg::message(
            id::FOLDER_ADDED,
            &[&dir.display().to_string()],
        ));
    }

    fn remove_folder(&self) {
        let sel = self.selected();
        let mut removed = Vec::new();
        {
            let mut st = self.state.borrow_mut();
            for e in &sel {
                let before = st.settings.session_folders.len();
                st.settings
                    .session_folders
                    .retain(|d| !same_path(d, &e.folder));
                if st.settings.session_folders.len() != before {
                    removed.push(e.folder.clone());
                }
            }
        }
        if removed.is_empty() {
            self.status
                .clone()
                .set("The Sessions folder is always listed.", Status::Error);
            return;
        }
        self.save_settings();
        for d in &removed {
            crate::log!("folder {} removed from Session Manager", d.display());
        }
        self.reload(&[]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use qsa_config::System;

    fn entry(title: &str, sys: &str, used: Option<i64>) -> Entry {
        let mut f = SessionFile::new(&System::new(sys));
        f.session.title = title.into();
        Entry {
            path: PathBuf::from(format!("/s/{title}.qsd")),
            folder: PathBuf::from("/s"),
            file: Ok(f),
            last_used: used,
        }
    }

    #[test]
    fn sorts_by_any_column_and_puts_unused_last() {
        let mut v = vec![
            entry("b", "sys2", Some(5)),
            entry("A", "sys3", None),
            entry("c", "sys1", Some(9)),
        ];
        sort(&mut v, 0, true);
        let titles: Vec<String> = v.iter().map(Entry::title).collect();
        assert_eq!(titles, ["A", "b", "c"]);
        sort(&mut v, 0, false);
        assert_eq!(v[0].title(), "c");
        sort(&mut v, 1, true);
        assert_eq!(v[0].title(), "c");
        sort(&mut v, 4, true);
        let titles: Vec<String> = v.iter().map(Entry::title).collect();
        assert_eq!(titles, ["b", "c", "A"]);
        sort(&mut v, 4, false);
        let titles: Vec<String> = v.iter().map(Entry::title).collect();
        assert_eq!(
            titles,
            ["c", "b", "A"],
            "newest first, never used still last"
        );
    }

    #[test]
    fn copies_get_free_names() {
        let dir = std::env::temp_dir().join(format!("qsa-mgr-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("Pay.qsd");
        assert_eq!(Manager::copy_path(&p), dir.join("Pay (copy).qsd"));
        std::fs::write(dir.join("Pay (copy).qsd"), "").unwrap();
        assert_eq!(Manager::copy_path(&p), dir.join("Pay (copy 2).qsd"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn unreadable_files_are_listed() {
        let e = Entry {
            path: PathBuf::from("/s/broken.qsd"),
            folder: PathBuf::from("/s"),
            file: Err("bad".into()),
            last_used: None,
        };
        let c = e.columns();
        assert_eq!(c[0], "broken");
        assert_eq!(c[1], "(cannot be read)");
    }
}
