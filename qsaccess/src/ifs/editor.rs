// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! The Integrated File System's text editor (Edit on a text file), and
//! Open, which hands a read-only copy to this computer's default
//! application.
//!
//! - The file is read on the session's shared connection with `as_ccsid`
//!   1208, so the system converts it from its own CCSID, and shown in the
//!   system-output font with the line and column on the status bar.
//! - Save looks at the file first (`GetInfo`): when it changed on the system
//!   since it was opened, `QAD8011` offers Overwrite, Reload or Cancel. The
//!   text then goes back as UTF-8 (`data_ccsid` 1208) with `Replace`, then
//!   `Append` for the rest, and the file keeps its CCSID.
//! - Save As writes to a new path in a CCSID chosen for it; Reload reads
//!   the system's file again; Find and Replace work on the text shown.
//! - A file over 8 MiB, or a binary one (CCSID 65535, or NUL bytes in its
//!   first 8 KiB), opens read-only with `QAD8013`, a binary one read as
//!   stored. Over 64 MiB the editor refuses with `QAD8012` and offers
//!   Download.

use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use fltk::app;
use fltk::enums::{Align, Color, FrameType, Key, Shortcut};
use fltk::frame::Frame;
use fltk::menu::MenuFlag;
use fltk::prelude::*;
use fltk::text::{TextBuffer, TextEditor};
use fltk::window::Window;
use qap_client::ifs::{self, Files, ReadOptions, WriteOptions};
use qap_client::proto::Op;
use qap_client::proto::ops::ifs::{Entry, EntryKind, WriteMode};
use qap_client::{Client, Error, Status as QapStatus};
use qsa_config::System;
use qsa_msg::{Message, id};
use qsa_ui::chan::{self, MenuEmit};
use qsa_ui::inputbox::InputBox;
use qsa_ui::sampler::{MENU_H, menu_bar};
use qsa_ui::statusbar::StatusBar;
use qsa_ui::theme::{self, colors};
use qsa_ui::{MessageBox, Status, controls, msgdetails};

use crate::qap::{self, Handle};

/// Over this, a file opens read-only.
pub const READ_ONLY_OVER: u64 = 8 << 20;
/// Over this, the editor does not open a file.
pub const EDIT_MAX: u64 = 64 << 20;
/// The bytes looked at for NULs.
pub const SNIFF: u64 = 8 * 1024;

/// Window width and height.
pub const W: i32 = 860;
pub const H: i32 = 620;

/// What reading a file for the editor found.
#[derive(Debug)]
pub enum Loaded {
    /// The text, the file's entry, and why it is read-only, if it is.
    Text {
        text: String,
        entry: Entry,
        read_only: Option<Message>,
    },
    /// Too large to edit: `QAD8012`.
    TooLarge(Message),
}

/// Whether a file is binary: tagged 65535, or NUL bytes in `head`.
pub fn is_binary(ccsid: u16, head: &[u8]) -> bool {
    ccsid == ifs::BINARY || head.contains(&0)
}

/// The whole of `path`, as stored or converted to `as_ccsid`.
fn read_all<F: Files + ?Sized>(
    f: &mut F,
    path: &str,
    as_ccsid: Option<u16>,
) -> Result<Vec<u8>, Error> {
    let mut out = Vec::new();
    ifs::read(
        |r| f.ifs_request(Op::ReadFile, r),
        path,
        ReadOptions {
            as_ccsid,
            ..ReadOptions::default()
        },
        &mut out,
        |_| {},
        &AtomicBool::new(false),
    )?;
    Ok(out)
}

/// Reads `path` for the editor: a link opens as its target.
pub fn load<F: Files + ?Sized>(f: &mut F, path: &str) -> Result<Loaded, Error> {
    let mut entry = f.get_info(path)?;
    if entry.kind == EntryKind::Symlink
        && let Some(t) = entry.target.clone()
    {
        let target = if t.starts_with('/') {
            t
        } else {
            ifs::join(ifs::parent(path), &t)
        };
        entry = f.get_info(&target)?;
    }
    let name = ifs::file_name(path).to_owned();
    if entry.size > EDIT_MAX {
        return Ok(Loaded::TooLarge(qsa_msg::message(
            id::TOO_LARGE_TO_EDIT,
            &[&name],
        )));
    }
    let head = if entry.ccsid == ifs::BINARY {
        Vec::new()
    } else {
        f.read_file(path, 0, SNIFF, None)?.data
    };
    let not_text = qsa_msg::message(id::NOT_TEXT, &[&name]);
    if is_binary(entry.ccsid, &head) {
        // As stored, as a hex dump: a binary file converts to nothing.
        let raw = read_all(f, path, None)?;
        return Ok(Loaded::Text {
            text: hex_dump(&raw),
            entry,
            read_only: Some(not_text),
        });
    }
    let raw = read_all(f, path, Some(ifs::UTF8))?;
    // A NUL past the bytes looked at: shown, never saved back without it.
    let nul = raw.contains(&0);
    let text = String::from_utf8_lossy(&raw).replace('\0', "\u{2400}");
    Ok(Loaded::Text {
        text,
        read_only: (entry.size > READ_ONLY_OVER || nul).then_some(not_text),
        entry,
    })
}

/// `data` as a hex dump: the offset, 16 bytes in hex, and those that are
/// printable ASCII.
pub fn hex_dump(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len() * 4 + 80);
    for (i, row) in data.chunks(16).enumerate() {
        out.push_str(&format!("{:08X}  ", i * 16));
        for j in 0..16 {
            match row.get(j) {
                Some(b) => out.push_str(&format!("{b:02X} ")),
                None => out.push_str("   "),
            }
            if j == 7 {
                out.push(' ');
            }
        }
        out.push(' ');
        out.extend(row.iter().map(|&b| {
            if (0x20..0x7F).contains(&b) {
                b as char
            } else {
                '.'
            }
        }));
        out.push('\n');
    }
    out
}

/// Writes `text` to `path` as UTF-8 for the system to convert: `first` the
/// first request's mode (`Replace`, or `Create` for a new file), `ccsid`
/// that of a file made. The file's entry afterwards.
pub fn save<F: Files + ?Sized>(
    f: &mut F,
    path: &str,
    text: &str,
    first: WriteMode,
    ccsid: u16,
) -> Result<Entry, Error> {
    ifs::write(
        |w| f.ifs_request(Op::WriteFile, w),
        path,
        &mut text.as_bytes(),
        WriteOptions {
            data_ccsid: Some(ifs::UTF8),
            ccsid,
            first,
            ..WriteOptions::default()
        },
        |_| {},
        &AtomicBool::new(false),
    )?;
    f.get_info(path)
}

/// The 1-based line and column of byte `pos` in `buf`, the column in
/// characters.
pub fn line_column(buf: &TextBuffer, pos: i32) -> (i32, i32) {
    let start = buf.line_start(pos);
    let line = buf.count_lines(0, pos) + 1;
    let col = buf.text_range(start, pos).map_or(0, |t| t.chars().count()) as i32 + 1;
    (line, col)
}

/// What the editor's controls ask for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Act {
    Save,
    SaveAs,
    Reload,
    Close,
    Undo,
    Cut,
    Copy,
    Paste,
    SelectAll,
    Find,
    FindNext,
    Replace,
}

/// One editor window.
pub struct Editor {
    pub window: Window,
    pub editor: TextEditor,
    pub buf: TextBuffer,
    pub status: StatusBar,
    /// Why the file is read-only, shown above the text.
    pub notice: Frame,
    rx: chan::Receiver<Act>,
    sys: System,
    h: Option<Handle>,
    path: RefCell<String>,
    entry: RefCell<Entry>,
    read_only: bool,
    dirty: Rc<Cell<bool>>,
    /// The last place shown on the status bar.
    shown_at: Cell<i32>,
    find: RefCell<(String, String, bool)>,
}

impl Editor {
    /// The window for `path` with `text`, `notice` saying why it is
    /// read-only; not shown, without a connection (screenshots), until
    /// [`open`] gives it one.
    pub fn build(
        sys: &System,
        path: &str,
        text: &str,
        entry: Entry,
        notice: Option<&Message>,
    ) -> Editor {
        let (tx, rx) = chan::channel::<Act>();
        let mut window = Window::new(0, 0, W, H, None);
        window.set_color(colors::FACE);
        window.set_icon(Some(qsa_ui::glyph::app_icon()));
        window.size_range(400, 300, 0, 0);
        let mut menu = menu_bar(0, 0, W);
        let read_only = notice.is_some();
        {
            let n = MenuFlag::Normal;
            let d = MenuFlag::MenuDivider;
            let ro = if read_only { MenuFlag::Inactive } else { n };
            let mut add = |p: &str, sc: Shortcut, a: Act, f: MenuFlag| {
                menu.add_emit_to(p, sc, f, tx, a);
            };
            add("&File/&Save", Shortcut::Ctrl | 's', Act::Save, ro);
            add(
                "&File/Save &As...",
                Shortcut::Ctrl | Shortcut::Shift | 's',
                Act::SaveAs,
                d,
            );
            add("&File/&Reload", Shortcut::None, Act::Reload, d);
            add("&File/&Close", Shortcut::Ctrl | 'w', Act::Close, n);
            add("&Edit/&Undo", Shortcut::Ctrl | 'z', Act::Undo, d);
            add("&Edit/Cu&t", Shortcut::Ctrl | 'x', Act::Cut, n);
            add("&Edit/&Copy", Shortcut::Ctrl | 'c', Act::Copy, n);
            add("&Edit/&Paste", Shortcut::Ctrl | 'v', Act::Paste, d);
            add("&Edit/Select &All", Shortcut::Ctrl | 'a', Act::SelectAll, d);
            add("&Edit/&Find...", Shortcut::Ctrl | 'f', Act::Find, n);
            add(
                "&Edit/Find &Next",
                Shortcut::None | Key::F3,
                Act::FindNext,
                n,
            );
            add("&Edit/R&eplace...", Shortcut::Ctrl | 'h', Act::Replace, ro);
        }
        let nh = if read_only { 26 } else { 0 };
        let mut notice_frame = Frame::new(0, MENU_H, W, nh, None);
        notice_frame.set_frame(FrameType::FlatBox);
        notice_frame.set_color(Color::from_rgb(0xFF, 0xF4, 0xCE));
        notice_frame.set_align(Align::Left | Align::Inside);
        notice_frame.set_label_font(theme::UI_FONT);
        notice_frame.set_label_size(theme::UI_SIZE);
        if let Some(m) = notice {
            notice_frame.set_label(&format!("  {m}"));
        } else {
            notice_frame.hide();
        }
        let mut buf = TextBuffer::default();
        buf.set_text(text);
        let mut editor =
            TextEditor::new(0, MENU_H + nh, W, H - MENU_H - nh - StatusBar::HEIGHT, None);
        editor.set_buffer(Some(buf.clone()));
        editor.set_text_font(theme::MONO_FONT);
        editor.set_text_size(theme::MONO_SIZE);
        editor.set_frame(FrameType::BorderBox);
        editor.set_linenumber_width(0);
        if read_only {
            // Shown, selectable and copied from, never changed.
            editor.handle(|_, ev| {
                ev == fltk::enums::Event::KeyDown && {
                    let k = app::event_key();
                    let ctrl = app::event_state().contains(Shortcut::Ctrl);
                    !(ctrl
                        || matches!(
                            k,
                            Key::Left
                                | Key::Right
                                | Key::Up
                                | Key::Down
                                | Key::Home
                                | Key::End
                                | Key::PageUp
                                | Key::PageDown
                        ))
                }
            });
        }
        let status = StatusBar::new(0, H - StatusBar::HEIGHT, W);
        window.resizable(&editor);
        window.end();
        window.set_callback(move |_| tx.send(Act::Close));
        let dirty = Rc::new(Cell::new(false));
        {
            let d = Rc::clone(&dirty);
            buf.add_modify_callback(move |_, inserted, deleted, _, _| {
                if inserted > 0 || deleted > 0 {
                    d.set(true);
                }
            });
        }
        let e = Editor {
            window,
            editor,
            buf,
            status,
            notice: notice_frame,
            rx,
            sys: sys.clone(),
            h: None,
            path: RefCell::new(path.to_owned()),
            entry: RefCell::new(entry),
            read_only,
            dirty,
            shown_at: Cell::new(-1),
            find: RefCell::new((String::new(), String::new(), false)),
        };
        e.title();
        e.place();
        e
    }

    fn parent(&self) -> Option<&dyn WindowExt> {
        Some(&self.window as &dyn WindowExt)
    }

    fn title(&self) {
        let p = self.path.borrow();
        let star = if self.dirty.get() { "*" } else { "" };
        let ro = if self.read_only { " (read-only)" } else { "" };
        self.window.clone().set_label(&format!(
            "{star}{p}{ro} - {} - {}",
            super::FUNCTION,
            self.sys.name
        ));
    }

    /// The line and column on the status bar.
    fn place(&self) {
        let pos = self.editor.insert_position();
        if pos == self.shown_at.get() {
            return;
        }
        self.shown_at.set(pos);
        let (l, c) = line_column(&self.buf, pos);
        let e = self.entry.borrow();
        self.status.clone().set_status(
            &format!("Ln {l}, Col {c}    CCSID {}", e.ccsid),
            Status::Neutral,
        );
    }

    fn report(&self, text: &str) {
        self.status.clone().set_message(text);
    }

    fn call<T: Send>(&self, f: impl FnOnce(&mut Client) -> T + Send) -> Option<T> {
        let h = self.h.as_ref()?;
        Some(qap::busy(|| f(&mut qap::lock(h))))
    }

    fn failed(&self, e: &Error) {
        let x = qap::explain(&self.sys, e);
        self.report(&x.line());
        qap::show(self.parent(), &x);
    }

    fn pump(&self) -> bool {
        while let Some(a) = self.rx.recv() {
            self.act(a);
            if !self.window.shown() {
                return false;
            }
        }
        self.place();
        let star = self.window.label().starts_with('*');
        if star != self.dirty.get() {
            self.title();
        }
        self.window.shown()
    }

    fn act(&self, a: Act) {
        let ed = self.editor.clone();
        match a {
            Act::Save => {
                self.save();
            }
            Act::SaveAs => {
                self.save_as();
            }
            Act::Reload => {
                if self.dirty.get() && !self.ask_discard() {
                    return;
                }
                self.reload();
            }
            Act::Close => {
                if self.dirty.get() && !self.read_only {
                    let path = self.path.borrow().clone();
                    match MessageBox::new(
                        "Edit",
                        &format!("Save the changes to {path} on the system?"),
                    )
                    .buttons(&["Save", "Don't Save", "Cancel"], 0, 2)
                    .run(self.parent())
                    {
                        0 if !self.save() => return,
                        0 | 1 => {}
                        _ => return,
                    }
                }
                self.window.clone().hide();
            }
            Act::Undo => {
                ed.undo();
            }
            Act::Cut => ed.cut(),
            Act::Copy => ed.copy(),
            Act::Paste => ed.paste(),
            Act::SelectAll => {
                let n = self.buf.length();
                self.buf.clone().select(0, n);
            }
            Act::Find => self.find_dialog(false),
            Act::Replace => self.find_dialog(true),
            Act::FindNext => {
                if !self.find_next() {
                    self.find_dialog(false);
                }
            }
        }
    }

    fn ask_discard(&self) -> bool {
        MessageBox::new(
            "Edit",
            "Your changes are not saved. Read the system's file again and lose them?",
        )
        .buttons(&["Reload", "Cancel"], 1, 1)
        .run(self.parent())
            == 0
    }

    /// Reads the system's file again.
    fn reload(&self) {
        let path = self.path.borrow().clone();
        match self.call(move |c| load(c, &path)) {
            Some(Ok(Loaded::Text { text, entry, .. })) => {
                let pos = self.editor.insert_position();
                self.buf.clone().set_text(&text);
                self.editor
                    .clone()
                    .set_insert_position(pos.min(self.buf.length()));
                *self.entry.borrow_mut() = entry;
                self.dirty.set(false);
                self.title();
                self.report("Read again from the system.");
            }
            Some(Ok(Loaded::TooLarge(m))) => {
                self.report(&m.to_string());
                msgdetails::show_message(self.parent(), &m);
            }
            Some(Err(e)) => self.failed(&e),
            None => {}
        }
    }

    /// Saves to the file it came from; true when saved.
    fn save(&self) -> bool {
        if self.read_only {
            return false;
        }
        let path = self.path.borrow().clone();
        let opened = self.entry.borrow().modified;
        let p = path.clone();
        let now = match self.call(move |c| c.get_info(&p)) {
            Some(Ok(e)) => Some(e),
            Some(Err(e)) if e.status() == Some(QapStatus::NotFound) => None,
            Some(Err(e)) => {
                self.failed(&e);
                return false;
            }
            None => return false,
        };
        if now.as_ref().is_some_and(|e| e.modified != opened) {
            let m = qsa_msg::message(id::CHANGED_ON_SYSTEM, &[&path]);
            crate::log!("{m}");
            match changed_prompt(&m).run(self.parent()) {
                0 => {}
                1 => {
                    self.reload();
                    return false;
                }
                _ => return false,
            }
        }
        let text = self.buf.text();
        let ccsid = self.entry.borrow().ccsid;
        let mode = if now.is_some() {
            WriteMode::Replace
        } else {
            WriteMode::Create
        };
        let p = path.clone();
        match self.call(move |c| save(c, &p, &text, mode, ccsid)) {
            Some(Ok(e)) => {
                let size = e.size;
                *self.entry.borrow_mut() = e;
                self.dirty.set(false);
                self.title();
                self.report(&format!(
                    "Saved {path}: {} in CCSID {ccsid}.",
                    super::model::bytes_text(size)
                ));
                true
            }
            Some(Err(e)) => {
                self.failed(&e);
                false
            }
            None => false,
        }
    }

    /// Saves to a new path, in a CCSID chosen for it; the editor goes on
    /// with that file.
    fn save_as(&self) -> bool {
        let cur = self.path.borrow().clone();
        let Some(path) = InputBox::new("Save As", "Path on the system:")
            .value(&cur)
            .run(self.parent())
        else {
            return false;
        };
        let path = super::normal(path.trim());
        let ccsid_now = self.entry.borrow().ccsid;
        let default = if ccsid_now == ifs::BINARY {
            ifs::UTF8
        } else {
            ccsid_now
        };
        let Some(ccsid) = InputBox::new("Save As", "CCSID of the new file (1208 for UTF-8):")
            .value(&default.to_string())
            .run(self.parent())
            .and_then(|s| super::queue::ccsid_of(&s))
        else {
            return false;
        };
        let text = self.buf.text();
        let (p, t) = (path.clone(), text.clone());
        let r = match self.call(move |c| save(c, &p, &t, WriteMode::Create, ccsid)) {
            Some(Err(e)) if e.status() == Some(QapStatus::Exists) => {
                let m = qsa_msg::message(
                    id::ALREADY_EXISTS,
                    &[ifs::file_name(&path), ifs::parent(&path)],
                );
                if msgdetails::ask_message(self.parent(), &m, &["Replace", "Cancel"]) != 0 {
                    return false;
                }
                let p = path.clone();
                self.call(move |c| save(c, &p, &text, WriteMode::Replace, ccsid))
            }
            r => r,
        };
        match r {
            Some(Ok(e)) => {
                *self.path.borrow_mut() = path.clone();
                *self.entry.borrow_mut() = e;
                self.dirty.set(false);
                self.shown_at.set(-1);
                self.title();
                self.report(&format!("Saved as {path}."));
                true
            }
            Some(Err(e)) => {
                self.failed(&e);
                false
            }
            None => false,
        }
    }

    /// Selects the next match of what Find looks for, from the cursor,
    /// round to the start; false when there is none.
    fn find_next(&self) -> bool {
        let (what, _, case) = self.find.borrow().clone();
        if what.is_empty() {
            return false;
        }
        let mut buf = self.buf.clone();
        let mut ed = self.editor.clone();
        let from = ed.insert_position();
        let hit = buf
            .search_forward(from, &what, case)
            .or_else(|| buf.search_forward(0, &what, case));
        match hit {
            Some(at) => {
                let end = at + what.len() as i32;
                buf.select(at, end);
                ed.set_insert_position(end);
                ed.show_insert_position();
                true
            }
            None => {
                self.report(&format!("\"{what}\" is not in the text."));
                true
            }
        }
    }

    /// Find, or Find and Replace.
    fn find_dialog(&self, replace: bool) {
        let (what, with, case) = self.find.borrow().clone();
        let (w, h) = (420, if replace { 150 } else { 118 });
        let mut win = Window::new(0, 0, w, h, None);
        win.set_label(if replace { "Replace" } else { "Find" });
        win.set_color(colors::FACE);
        win.make_modal(true);
        let fh = theme::FIELD_H;
        let l = controls::label(12, 12, 90, fh, "Find what:");
        let _ = l;
        let mut find = controls::field(102, 12, 200, fh);
        find.set_value(&what);
        let mut repl = controls::field(102, 44, 200, fh);
        repl.set_value(&with);
        if replace {
            controls::label(12, 44, 90, fh, "Replace with:");
        } else {
            repl.hide();
        }
        let case_b = controls::check(12, h - 64, 200, 20, "Match case");
        case_b.set_checked(case);
        let (bw, bh) = (100, theme::BUTTON_H);
        let chosen = Rc::new(Cell::new(0u8));
        let mut buttons = vec![("Find Next", 1u8)];
        if replace {
            buttons.push(("Replace", 2));
            buttons.push(("Replace All", 3));
        }
        buttons.push(("Close", 9));
        for (i, (label, n)) in buttons.iter().enumerate() {
            let mut b = if i == 0 {
                controls::default_button(w - bw - 12, 12 + i as i32 * (bh + 4), bw, bh, label)
            } else {
                controls::button(w - bw - 12, 12 + i as i32 * (bh + 4), bw, bh, label)
            };
            let c = Rc::clone(&chosen);
            let n = *n;
            b.set_callback(move |_| c.set(n));
        }
        win.end();
        {
            let c = Rc::clone(&chosen);
            win.set_callback(move |_| c.set(9));
        }
        qsa_ui::propdialog::center_on(&mut win, self.parent());
        win.show();
        loop {
            chosen.set(0);
            while chosen.get() == 0 && win.shown() {
                app::wait();
            }
            *self.find.borrow_mut() = (find.value(), repl.value(), case_b.is_checked());
            match chosen.get() {
                1 => {
                    self.find_next();
                }
                2 => {
                    self.replace_one();
                }
                3 => {
                    let n = self.replace_all();
                    self.report(&format!("{n} replaced."));
                }
                _ => break,
            }
        }
        win.hide();
    }

    /// Replaces the selection when it is a match, then finds the next.
    fn replace_one(&self) {
        let (what, with, case) = self.find.borrow().clone();
        let mut buf = self.buf.clone();
        if let Some((s, e)) = buf.selection_position() {
            let sel = buf.text_range(s, e).unwrap_or_default();
            let same = if case {
                sel == what
            } else {
                sel.to_lowercase() == what.to_lowercase()
            };
            if same {
                buf.replace(s, e, &with);
                self.editor
                    .clone()
                    .set_insert_position(s + with.len() as i32);
            }
        }
        self.find_next();
    }

    /// Replaces every match; how many.
    fn replace_all(&self) -> usize {
        let (what, with, case) = self.find.borrow().clone();
        if what.is_empty() {
            return 0;
        }
        let mut buf = self.buf.clone();
        let mut at = 0;
        let mut n = 0;
        while let Some(hit) = buf.search_forward(at, &what, case) {
            buf.replace(hit, hit + what.len() as i32, &with);
            at = hit + with.len() as i32;
            n += 1;
        }
        n
    }
}

/// `QAD8011` with Overwrite, Reload and Cancel.
pub fn changed_prompt(m: &Message) -> MessageBox {
    msgdetails::ask_message_box(m, &["Overwrite", "Reload", "Cancel"])
}

thread_local! {
    static EDITORS: RefCell<Vec<Rc<Editor>>> = const { RefCell::new(Vec::new()) };
}

/// What an Edit asked for that the window does: download a file too
/// large to edit.
pub enum Asked {
    Nothing,
    Download,
}

/// Edit: opens `path` in an editor, reading it on `h`'s shared connection.
pub fn open(parent: Option<&dyn WindowExt>, h: &Handle, sys: &System, path: &str) -> Asked {
    let p = path.to_owned();
    let loaded = qap::busy(|| load(&mut *qap::lock(h), &p));
    let (text, entry, notice) = match loaded {
        Ok(Loaded::Text {
            text,
            entry,
            read_only,
        }) => (text, entry, read_only),
        Ok(Loaded::TooLarge(m)) => {
            crate::log!("{m}");
            return match msgdetails::ask_message(parent, &m, &["Download...", "Cancel"]) {
                0 => Asked::Download,
                _ => Asked::Nothing,
            };
        }
        Err(e) => {
            qap::show(parent, &qap::explain(sys, &e));
            return Asked::Nothing;
        }
    };
    if let Some(m) = &notice {
        crate::log!("{m}");
    }
    let mut e = Editor::build(sys, path, &text, entry, notice.as_ref());
    e.h = Some(h.clone());
    let e = Rc::new(e);
    let mut win = e.window.clone();
    qsa_ui::propdialog::center_on(&mut win, parent);
    qsa_ui::propdialog::show_placed(&mut win);
    let _ = e.editor.clone().take_focus();
    EDITORS.with(|v| v.borrow_mut().push(e));
    tick();
    Asked::Nothing
}

fn tick() {
    thread_local! { static TICKING: Cell<bool> = const { Cell::new(false) }; }
    if TICKING.with(|t| t.replace(true)) {
        return;
    }
    app::add_timeout3(0.05, |h| {
        let editors: Vec<Rc<Editor>> = EDITORS.with(|v| v.borrow().clone());
        let open: Vec<bool> = editors.iter().map(|e| e.pump()).collect();
        EDITORS.with(|v| {
            let mut i = 0;
            v.borrow_mut().retain(|_| {
                let keep = open.get(i).copied().unwrap_or(true);
                i += 1;
                keep
            });
        });
        if EDITORS.with(|v| !v.borrow().is_empty()) {
            app::repeat_timeout3(0.05, h);
        } else {
            TICKING.with(|t| t.set(false));
        }
    });
}

/// Whether any editor is open.
pub fn any_open() -> bool {
    EDITORS.with(|v| v.borrow().iter().any(|e| e.window.shown()))
}

/// Open: a read-only copy of `path` in this process's temporary folder,
/// handed to the desktop's default application; removed when the program
/// ends. The copy's path.
pub fn open_copy(h: &Handle, path: &str) -> Result<PathBuf, String> {
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let dir = crate::splf::output::temporary_dir()
        .join("ifs")
        .join(SEQ.fetch_add(1, Ordering::SeqCst).to_string());
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let local = dir.join(ifs::file_name(path));
    let p = path.to_owned();
    let l = local.clone();
    qap::busy(move || {
        let mut file = std::fs::File::create(&l).map_err(|e| e.to_string())?;
        let mut c = qap::lock(h);
        ifs::read(
            |r| c.request(Op::ReadFile, r),
            &p,
            ReadOptions::default(),
            &mut file,
            |_| {},
            &AtomicBool::new(false),
        )
        .map_err(|e| match e.server_message() {
            Some(m) => format!("{}: {}", m.id, m.text),
            None => e.to_string(),
        })?;
        drop(file);
        let mut perm = std::fs::metadata(&l)
            .map_err(|e| e.to_string())?
            .permissions();
        perm.set_readonly(true);
        std::fs::set_permissions(&l, perm).map_err(|e| e.to_string())
    })?;
    crate::shell::open(&local);
    Ok(local)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn binary_files_and_places() {
        assert!(is_binary(65535, b"text"));
        assert!(is_binary(1208, b"a\0b"));
        assert!(!is_binary(37, b"\xC1\xC2"));
        assert_eq!(
            hex_dump(&[0x00, 0x0E, 0xFF, 0x0F, 0x41, 0x80, 0xC1]),
            "00000000  00 0E FF 0F 41 80 C1                              ....A..\n"
        );
        assert_eq!(
            hex_dump(&[b'a'; 17]).lines().nth(1).unwrap(),
            "00000010  61                                                a"
        );
        let mut b = TextBuffer::default();
        b.set_text("ab\nc\u{e4}d\n");
        assert_eq!(line_column(&b, 0), (1, 1));
        assert_eq!(line_column(&b, 3), (2, 1));
        // After c and the two bytes of ä: column 3 in characters.
        assert_eq!(line_column(&b, 6), (2, 3));
    }
}
