// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! The Macro editor: the macro's name, description and screen size, and a
//! table of its steps with Add, Edit, Remove, Move Up and Move Down. A
//! step that waits for text can take its row and column from a click on a
//! live session's screen.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use fltk::app;
use fltk::button::{Button, CheckButton};
use fltk::input::{Input, IntInput};
use fltk::menu::Choice;
use fltk::prelude::*;
use fltk::window::Window;
use qsa_config::keymap::FUNCTIONS;
use qsa_config::qsm::{Action, DEFAULT_TIMEOUT_MS, MacroFile, Step};
use qsa_config::settings::ScreenSize;
use qsa_ui::controls::{self, Switch};
use qsa_ui::listtable::{Cell, ListTable};
use qsa_ui::propdialog::center_on;
use qsa_ui::statusbar::{Status, StatusLine};
use qsa_ui::theme::{self, colors};
use qsa_ui::{Glyph, MessageBox};

use crate::files;

/// Window width.
pub const W: i32 = 560;
/// Window height.
pub const H: i32 = 440;

/// The rows of the step table.
pub fn rows(m: &MacroFile) -> Vec<Vec<Cell>> {
    m.steps
        .iter()
        .enumerate()
        .map(|(i, s)| {
            vec![
                Cell::from((i + 1).to_string()),
                Cell::from(s.action.describe()),
            ]
        })
        .collect()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Req {
    Add,
    Edit,
    Remove,
    Up,
    Down,
    Save,
    SaveAs,
    Close,
}

/// One Macro editor window.
pub struct Editor {
    pub window: Window,
    pub list: ListTable,
    name: Input,
    description: Input,
    size: Choice,
    status: StatusLine,
    rx: app::Receiver<Req>,
    file: RefCell<Option<PathBuf>>,
    doc: RefCell<MacroFile>,
    one: Vec<Switch>,
}

thread_local! {
    static EDITORS: RefCell<Vec<Rc<Editor>>> = const { RefCell::new(Vec::new()) };
}

fn title_for(m: &MacroFile, file: Option<&Path>) -> String {
    format!("Macro Editor - {}", m.display_name(file))
}

/// Builds an editor for `m` without showing it.
pub fn build(m: MacroFile, file: Option<PathBuf>) -> Rc<Editor> {
    let (tx, rx) = app::channel::<Req>();
    let mm = 10;
    let fh = theme::FIELD_H;
    let bw = theme::BUTTON_W + 10;
    let bh = theme::BUTTON_H;
    let mut window = Window::new(0, 0, W, H, None);
    window.set_label(&title_for(&m, file.as_deref()));
    window.set_color(colors::FACE);
    window.size_range(460, 320, 0, 0);
    let lw = 86;
    let fw = W - 2 * mm - lw;
    controls::label(mm, mm, lw, fh, "Name:");
    let mut name = controls::field(mm + lw, mm, fw, fh);
    name.set_value(&m.info.name);
    controls::label(mm, mm + fh + 6, lw, fh, "Description:");
    let mut description = controls::field(mm + lw, mm + fh + 6, fw, fh);
    description.set_value(&m.info.description);
    controls::label(mm, mm + 2 * (fh + 6), lw, fh, "Screen size:");
    let mut size = controls::combo(mm + lw, mm + 2 * (fh + 6), 110, fh);
    size.add_choice("24 x 80|27 x 132");
    size.set_value(i32::from(m.info.screen_size == ScreenSize::S27x132));

    let top = mm + 3 * (fh + 6) + 4;
    let listw = W - 3 * mm - bw;
    let listh = H - top - mm - bh - 8;
    let mut list = ListTable::new(
        mm,
        top,
        listw,
        listh,
        &[("#", 34), ("Step", listw - 34 - 20)],
    );
    let bx = mm + listw + mm;
    let mut buttons = fltk::group::Group::new(bx, top, bw, listh, None);
    let mut y = top;
    let mut button = |label: &str, r: Req, gap: i32| -> Button {
        let mut b = controls::button(bx, y, bw, bh, label);
        b.emit(tx, r);
        y += bh + gap;
        b
    };
    button("Add...", Req::Add, 6);
    let mut edit = button("Edit...", Req::Edit, 6);
    let mut remove = button("Remove", Req::Remove, 18);
    let mut up = button("Move Up", Req::Up, 6);
    let mut down = button("Move Down", Req::Down, 6);
    buttons.end();
    buttons.make_resizable(false);
    let status = StatusLine::new(mm, H - mm - bh, W - 4 * mm - 3 * bw, bh);
    let mut save = controls::default_button(W - 3 * bw - 2 * 6 - mm, H - mm - bh, bw, bh, "Save");
    save.emit(tx, Req::Save);
    let mut save_as = controls::button(W - 2 * bw - 6 - mm, H - mm - bh, bw, bh, "Save As...");
    save_as.emit(tx, Req::SaveAs);
    let mut close = controls::button(W - bw - mm, H - mm - bh, bw, bh, "Close");
    close.emit(tx, Req::Close);
    window.end();
    window.resizable(list.table());
    window.set_callback(move |_| tx.send(Req::Close));
    list.on_activate(move |sel| {
        if sel.is_some() {
            tx.send(Req::Edit);
        }
    });
    let one = vec![
        Switch::for_button(&mut edit),
        Switch::for_button(&mut remove),
        Switch::for_button(&mut up),
        Switch::for_button(&mut down),
    ];
    {
        let one = one.clone();
        list.on_select(move |sel| {
            for s in &one {
                s.set_enabled(sel.is_some());
            }
        });
    }
    list.set_rows(rows(&m));
    list.select(if m.steps.is_empty() { None } else { Some(0) });
    for s in &one {
        s.set_enabled(!m.steps.is_empty());
    }
    Rc::new(Editor {
        window,
        list,
        name,
        description,
        size,
        status,
        rx,
        file: RefCell::new(file),
        doc: RefCell::new(m),
        one,
    })
}

/// Opens a macro file in a new editor window.
pub fn open(path: &Path, parent: Option<&dyn WindowExt>) {
    match MacroFile::load(path) {
        Ok(m) => show(m, Some(path.to_owned()), parent),
        Err(e) => {
            let m = e.message(path);
            crate::log!("{m}");
            qsa_ui::msgdetails::show_message(parent, &m);
        }
    }
}

/// Shows an editor for `m`.
pub fn show(m: MacroFile, file: Option<PathBuf>, parent: Option<&dyn WindowExt>) {
    let e = build(m, file);
    let mut w = e.window.clone();
    center_on(&mut w, parent);
    w.show();
    EDITORS.with(|v| v.borrow_mut().push(e));
}

/// Handles every editor's requests; forgets closed ones. Called by the main
/// loop after each wait.
pub fn pump() {
    let all: Vec<Rc<Editor>> = EDITORS.with(|v| v.borrow().clone());
    for e in &all {
        while let Some(r) = e.rx.recv() {
            e.handle(r);
        }
    }
    EDITORS.with(|v| v.borrow_mut().retain(|e| e.window.shown()));
}

/// Whether any editor is open.
pub fn any_open() -> bool {
    EDITORS.with(|v| v.borrow().iter().any(|e| e.window.shown()))
}

impl Editor {
    fn current(&self) -> MacroFile {
        let mut m = self.doc.borrow().clone();
        m.info.name = self.name.value().trim().to_owned();
        m.info.description = self.description.value().trim().to_owned();
        m.info.screen_size = if self.size.value() == 1 {
            ScreenSize::S27x132
        } else {
            ScreenSize::S24x80
        };
        m
    }

    fn refill(&self, select: Option<usize>) {
        let m = self.doc.borrow().clone();
        let mut list = self.list.clone();
        list.set_rows(rows(&m));
        let sel = select.filter(|&i| i < m.steps.len());
        list.select(sel);
        for s in &self.one {
            s.set_enabled(sel.is_some());
        }
    }

    fn handle(&self, r: Req) {
        let sel = self.list.selected();
        match r {
            Req::Add => {
                let size = self.current().info.screen_size;
                let blank = Step::new(Action::WaitUnlock {
                    timeout_ms: DEFAULT_TIMEOUT_MS,
                });
                if let Some(step) = edit_step(&self.window, "Add Step", &blank, size) {
                    let at = sel.map_or(self.doc.borrow().steps.len(), |i| i + 1);
                    self.doc.borrow_mut().steps.insert(at, step);
                    self.refill(Some(at));
                }
            }
            Req::Edit => {
                let Some(i) = sel else { return };
                let size = self.current().info.screen_size;
                let old = self.doc.borrow().steps[i].clone();
                if let Some(step) = edit_step(&self.window, "Edit Step", &old, size) {
                    self.doc.borrow_mut().steps[i] = step;
                    self.refill(Some(i));
                }
            }
            Req::Remove => {
                let Some(i) = sel else { return };
                self.doc.borrow_mut().steps.remove(i);
                let n = self.doc.borrow().steps.len();
                self.refill(if n == 0 { None } else { Some(i.min(n - 1)) });
            }
            Req::Up => {
                if let Some(i) = sel.filter(|&i| i > 0) {
                    self.doc.borrow_mut().steps.swap(i - 1, i);
                    self.refill(Some(i - 1));
                }
            }
            Req::Down => {
                let n = self.doc.borrow().steps.len();
                if let Some(i) = sel.filter(|&i| i + 1 < n) {
                    self.doc.borrow_mut().steps.swap(i, i + 1);
                    self.refill(Some(i + 1));
                }
            }
            Req::Save => {
                let file = self.file.borrow().clone();
                match file {
                    Some(p) => self.save_to(&p),
                    None => self.save_as(),
                }
            }
            Req::SaveAs => self.save_as(),
            Req::Close => self.window.clone().hide(),
        }
    }

    fn save_as(&self) {
        let m = self.current();
        let name = files::file_name_for(&m.display_name(None), qsa_config::qsm::EXTENSION);
        let dir = self
            .file
            .borrow()
            .as_deref()
            .and_then(Path::parent)
            .map(Path::to_owned)
            .unwrap_or_else(files::sessions_dir);
        let Some(p) = files::choose_save_in(&dir, "Save Macro As", files::MACRO_PATTERN, &name)
        else {
            return;
        };
        self.save_to(&files::with_extension(p, qsa_config::qsm::EXTENSION));
    }

    fn save_to(&self, path: &Path) {
        let m = self.current();
        for (i, s) in m.steps.iter().enumerate() {
            if let Err(e) = MacroFile::check_step(s, m.info.screen_size) {
                self.status
                    .clone()
                    .set(&format!("Step {}: {e}.", i + 1), Status::Error);
                self.list.clone().select(Some(i));
                return;
            }
        }
        match m.save(path) {
            Ok(()) => {
                *self.doc.borrow_mut() = m.clone();
                *self.file.borrow_mut() = Some(path.to_owned());
                self.window.clone().set_label(&title_for(&m, Some(path)));
                self.status
                    .clone()
                    .set(&format!("Saved to {}.", path.display()), Status::Neutral);
                crate::log!("macro saved to {}", path.display());
            }
            Err(e) => {
                MessageBox::new(
                    "Macro Editor",
                    &format!("{} could not be written: {e}", path.display()),
                )
                .glyph(Glyph::Cross)
                .run(Some(&self.window));
            }
        }
    }
}

/// The kinds of step, in the order the Step dialog lists them.
const KINDS: [&str; 6] = [
    "Type text",
    "Press a key",
    "Wait for the keyboard",
    "Wait for text on the screen",
    "Pause",
    "Ask for a value",
];

fn kind_of(a: &Action) -> usize {
    match a {
        Action::Type { .. } => 0,
        Action::Key { .. } => 1,
        Action::WaitUnlock { .. } => 2,
        Action::WaitText { .. } => 3,
        Action::Pause { .. } => 4,
        Action::Prompt { .. } => 5,
    }
}

/// The Step dialog's fields.
#[derive(Clone)]
pub struct StepForm {
    kind: Choice,
    text: Input,
    key: Choice,
    row: IntInput,
    col: IntInput,
    ms: IntInput,
    hidden: CheckButton,
    switches: Vec<(Switch, &'static [usize])>,
    pick: Switch,
}

fn int_field(x: i32, y: i32, w: i32) -> IntInput {
    let mut i = IntInput::new(x, y, w, theme::FIELD_H, None);
    i.set_frame(fltk::enums::FrameType::DownBox);
    i.set_color(colors::WINDOW);
    i.set_text_size(theme::UI_SIZE);
    i.set_selection_color(colors::SELECTION);
    i
}

impl StepForm {
    fn show_kind(&self) {
        let k = self.kind.value().max(0) as usize;
        for (s, kinds) in &self.switches {
            s.set_enabled(kinds.contains(&k));
        }
        self.pick.set_enabled(k == 3);
    }

    /// The step entered, or what is wrong with it.
    pub fn read(&self, extra: &qsa_config::doc::Extra) -> Result<Step, String> {
        let num = |i: &IntInput, what: &str| -> Result<u32, String> {
            i.value()
                .trim()
                .parse::<u32>()
                .map_err(|_| format!("Enter a whole number for {what}."))
        };
        let action = match self.kind.value() {
            0 => Action::Type {
                text: self.text.value(),
            },
            1 => Action::Key {
                key: FUNCTIONS
                    .get(self.key.value().max(0) as usize)
                    .map_or("Enter", |(n, _)| n)
                    .to_owned(),
            },
            2 => Action::WaitUnlock {
                timeout_ms: num(&self.ms, "the time")?,
            },
            3 => Action::WaitText {
                row: num(&self.row, "the row")?.min(255) as u8,
                col: num(&self.col, "the column")?.min(255) as u8,
                text: self.text.value(),
                timeout_ms: num(&self.ms, "the time")?,
            },
            4 => Action::Pause {
                ms: num(&self.ms, "the time")?,
            },
            _ => Action::Prompt {
                label: self.text.value().trim().to_owned(),
                hidden: self.hidden.is_checked(),
            },
        };
        Ok(Step {
            action,
            extra: extra.clone(),
        })
    }
}

/// Builds the Step dialog for `step` without showing it.
pub fn build_step(title: &str, step: &Step) -> (Window, StepForm, app::Receiver<u8>) {
    let (tx, rx) = app::channel::<u8>();
    let (w, h) = (430, 270);
    let m = 12;
    let fh = theme::FIELD_H;
    let lw = 104;
    let fw = w - 2 * m - lw;
    let mut win = Window::new(0, 0, w, h, None);
    win.set_label(title);
    win.set_color(colors::FACE);
    win.make_modal(true);
    let mut y = m;
    let mut row = |label: &str| {
        controls::label(m, y, lw, fh, label);
        let at = y;
        y += fh + 8;
        at
    };
    let at = row("Step:");
    let mut kind = controls::combo(m + lw, at, fw, fh);
    for k in KINDS {
        kind.add_choice(k);
    }
    kind.set_value(kind_of(&step.action) as i32);
    let at = row("Text or label:");
    let mut text = controls::field(m + lw, at, fw, fh);
    let at = row("Key:");
    let mut key = controls::combo(m + lw, at, fw, fh);
    for (_, label) in FUNCTIONS {
        key.add_choice(&label.replace('/', "\\/"));
    }
    key.set_value(0);
    let at = row("Row, column:");
    let mut r = int_field(m + lw, at, 50);
    let mut c = int_field(m + lw + 56, at, 50);
    let mut pick = controls::button(m + lw + 116, at, fw - 116, fh, "Pick from Screen...");
    pick.emit(tx, 2);
    let at = row("Time (ms):");
    let mut ms = int_field(m + lw, at, 90);
    let at = row("");
    let mut hidden = controls::check(m + lw, at, fw, fh, "Hide what is typed");
    hidden.set_checked(true);
    match &step.action {
        Action::Type { text: t } => text.set_value(t),
        Action::Key { key: k } => {
            let i = FUNCTIONS.iter().position(|(n, _)| n == k).unwrap_or(0);
            key.set_value(i as i32);
        }
        Action::WaitUnlock { timeout_ms } => ms.set_value(&timeout_ms.to_string()),
        Action::WaitText {
            row: rr,
            col: cc,
            text: t,
            timeout_ms,
        } => {
            r.set_value(&rr.to_string());
            c.set_value(&cc.to_string());
            text.set_value(t);
            ms.set_value(&timeout_ms.to_string());
        }
        Action::Pause { ms: p } => ms.set_value(&p.to_string()),
        Action::Prompt { label, hidden: hd } => {
            text.set_value(label);
            hidden.set_checked(*hd);
        }
    }
    if ms.value().is_empty() {
        ms.set_value(&DEFAULT_TIMEOUT_MS.to_string());
    }
    let bw = theme::BUTTON_W;
    let bh = theme::BUTTON_H;
    let mut ok = controls::default_button(w - m - 2 * bw - 6, h - m - bh, bw, bh, "OK");
    ok.emit(tx, 1);
    let mut cancel = controls::button(w - m - bw, h - m - bh, bw, bh, "Cancel");
    cancel.emit(tx, 0);
    win.end();
    win.set_callback(move |_| tx.send(0));
    let form = StepForm {
        switches: vec![
            (Switch::for_input(&mut text), &[0, 3, 5]),
            (Switch::for_choice(&mut key), &[1]),
            (Switch::for_input(&mut r), &[3]),
            (Switch::for_input(&mut c), &[3]),
            (Switch::for_input(&mut ms), &[2, 3, 4]),
            (Switch::for_button(&mut hidden), &[5]),
        ],
        pick: Switch::for_button(&mut pick),
        kind: kind.clone(),
        text,
        key,
        row: r,
        col: c,
        ms,
        hidden,
    };
    form.show_kind();
    {
        let f = form.clone();
        kind.set_callback(move |_| f.show_kind());
    }
    (win, form, rx)
}

/// Runs the Step dialog over `parent`. `Some` with the step on OK.
fn edit_step(parent: &Window, title: &str, step: &Step, size: ScreenSize) -> Option<Step> {
    let (mut win, form, rx) = build_step(title, step);
    center_on(&mut win, Some(parent));
    win.show();
    while win.shown() {
        app::wait();
        match rx.recv() {
            Some(1) => match form.read(&step.extra).and_then(|s| {
                MacroFile::check_step(&s, size).map(|()| s).map_err(|e| {
                    let mut e = e;
                    if let Some(c) = e.get_mut(0..1) {
                        c.make_ascii_uppercase();
                    }
                    format!("{e}.")
                })
            }) {
                Ok(s) => {
                    win.hide();
                    return Some(s);
                }
                Err(e) => {
                    MessageBox::new(title, &e)
                        .glyph(Glyph::Cross)
                        .run(Some(&win));
                }
            },
            Some(2) => {
                win.hide();
                if let Some((r, c, word)) = crate::session::pick_position(Some(parent)) {
                    let mut f = form.clone();
                    f.row.set_value(&r.to_string());
                    f.col.set_value(&c.to_string());
                    if f.text.value().is_empty() {
                        f.text.set_value(&word);
                    }
                }
                win.show();
            }
            Some(_) => win.hide(),
            None => {}
        }
    }
    None
}
