// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! The Keyboard Map dialog: every key and the 5250 function it performs,
//! with Change... and Reset to defaults. Functions without a key are listed
//! too, as `(none)`, so they can be given one.

use std::cell::RefCell;
use std::rc::Rc;

use fltk::app;
use fltk::enums::{Align, Event, FrameType};
use fltk::frame::Frame;
use fltk::prelude::*;
use fltk::window::Window;
use qsa_config::KeyMap;
use qsa_config::keymap::{FUNCTIONS, function_label};
use qsa_ui::controls::{self, Switch};
use qsa_ui::listtable::{Cell, ListTable};
use qsa_ui::propdialog::center_on;
use qsa_ui::theme::{self, colors};

use super::keys::key_name;

pub const W: i32 = 470;
pub const H: i32 = 400;

const COLUMNS: [(&str, i32); 2] = [("Key", 170), ("5250 function", 196)];

/// One row: the key (if any) and the function.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub key: Option<String>,
    pub function: String,
}

/// The rows for a map: by function in [`FUNCTIONS`] order, each key of a
/// function on its own row, a function with no key as one `(none)` row.
pub fn rows(map: &KeyMap) -> Vec<Row> {
    let mut out = Vec::new();
    for (f, _) in FUNCTIONS {
        let keys = map.keys_for(f);
        if keys.is_empty() {
            out.push(Row {
                key: None,
                function: (*f).to_owned(),
            });
        }
        for k in keys {
            out.push(Row {
                key: Some(k.to_owned()),
                function: (*f).to_owned(),
            });
        }
    }
    out
}

fn cells(rows: &[Row]) -> Vec<Vec<Cell>> {
    rows.iter()
        .map(|r| {
            vec![
                Cell::from(r.key.as_deref().unwrap_or("(none)")),
                Cell::from(function_label(&r.function)),
            ]
        })
        .collect()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Req {
    Change,
    Reset,
    Ok,
    Cancel,
}

/// The dialog's widgets.
pub struct KeymapDialog {
    pub window: Window,
    pub list: ListTable,
    map: Rc<RefCell<KeyMap>>,
    rows: Vec<Row>,
    change: Switch,
}

impl KeymapDialog {
    fn fill(&mut self, select: Option<usize>) {
        self.rows = rows(&self.map.borrow());
        self.list.set_rows(cells(&self.rows));
        let sel = select.filter(|&i| i < self.rows.len()).or(Some(0));
        self.list.select(sel);
        self.change.set_enabled(sel.is_some());
    }
}

fn build_with(title: &str, map: &KeyMap) -> (KeymapDialog, app::Receiver<Req>) {
    let (tx, rx) = app::channel::<Req>();
    let m = 10;
    let (bw, bh) = (theme::BUTTON_W + 30, theme::BUTTON_H);
    let mut window = Window::new(0, 0, W, H, None);
    window.set_label(title);
    window.set_color(colors::FACE);
    window.make_modal(true);
    let lw = W - 3 * m - bw;
    let lh = H - 3 * m - bh;
    let mut list = ListTable::new(m, m, lw, lh, &COLUMNS);
    let bx = m + lw + m;
    let mut change = controls::button(bx, m, bw, bh, "Change...");
    change.emit(tx, Req::Change);
    let mut reset = controls::button(bx, m + bh + 6, bw, bh, "Reset to defaults");
    reset.emit(tx, Req::Reset);
    let mut ok = controls::default_button(
        W - 2 * m - 2 * theme::BUTTON_W,
        H - m - bh,
        theme::BUTTON_W,
        bh,
        "OK",
    );
    ok.emit(tx, Req::Ok);
    let mut cancel = controls::button(
        W - m - theme::BUTTON_W,
        H - m - bh,
        theme::BUTTON_W,
        bh,
        "Cancel",
    );
    cancel.emit(tx, Req::Cancel);
    window.end();
    window.set_callback(move |_| tx.send(Req::Cancel));
    let change = Switch::for_button(&mut change);
    {
        let c = change.clone();
        list.on_select(move |sel| c.set_enabled(sel.is_some()));
    }
    list.on_activate(move |sel| {
        if sel.is_some() {
            tx.send(Req::Change);
        }
    });
    let mut d = KeymapDialog {
        window,
        list,
        map: Rc::new(RefCell::new(map.clone())),
        rows: Vec::new(),
        change,
    };
    d.fill(None);
    (d, rx)
}

/// Builds the dialog without showing it (for screenshots).
pub fn build(title: &str, map: &KeyMap) -> KeymapDialog {
    build_with(title, map).0
}

/// Runs the dialog over `parent`. `Some` with the new map on OK.
pub fn run(parent: Option<&dyn WindowExt>, title: &str, map: &KeyMap) -> Option<KeyMap> {
    let (mut d, rx) = build_with(title, map);
    center_on(&mut d.window, parent);
    d.window.show();
    let mut result = None;
    while d.window.shown() {
        app::wait();
        let Some(req) = rx.recv() else { continue };
        match req {
            Req::Ok => {
                result = Some(d.map.borrow().clone());
                d.window.hide();
            }
            Req::Cancel => d.window.hide(),
            Req::Reset => {
                *d.map.borrow_mut() = KeyMap::defaults();
                d.fill(d.list.selected());
            }
            Req::Change => {
                let Some(i) = d.list.selected() else { continue };
                let row = d.rows[i].clone();
                let label = function_label(&row.function).to_owned();
                match capture(&d.window, &label) {
                    Captured::Key(k) => {
                        let mut map = d.map.borrow_mut();
                        if let Some(old) = &row.key {
                            map.keys.remove(old);
                        }
                        map.assign(&k, &row.function);
                    }
                    Captured::Clear => {
                        if let Some(old) = &row.key {
                            d.map.borrow_mut().keys.remove(old);
                        }
                    }
                    Captured::Cancel => continue,
                }
                let at = rows(&d.map.borrow())
                    .iter()
                    .position(|r| r.function == row.function);
                d.fill(at);
            }
        }
    }
    result
}

enum Captured {
    Key(String),
    Clear,
    Cancel,
}

/// Asks for a key: the next key pressed (Right Ctrl on release) is shown;
/// OK takes it.
fn capture(parent: &Window, function: &str) -> Captured {
    let (w, h) = (340, 150);
    let m = 10;
    let bh = theme::BUTTON_H;
    let mut win = Window::new(0, 0, w, h, "Change Key");
    win.set_color(colors::FACE);
    win.make_modal(true);
    controls::label(
        m,
        m,
        w - 2 * m,
        20,
        &format!("Press the key for {function}."),
    );
    let mut shown = Frame::new(m, m + 28, w - 2 * m, 28, "(none)");
    shown.set_frame(FrameType::DownBox);
    shown.set_color(colors::WINDOW);
    shown.set_align(Align::Center | Align::Inside);
    let (tx, rx) = app::channel::<u8>();
    let bw = theme::BUTTON_W;
    let mut none = controls::button(m, h - m - bh, bw, bh, "No key");
    none.emit(tx, 2);
    let mut ok = controls::default_button(w - 2 * m - 2 * bw, h - m - bh, bw, bh, "OK");
    ok.emit(tx, 1);
    let mut cancel = controls::button(w - m - bw, h - m - bh, bw, bh, "Cancel");
    cancel.emit(tx, 0);
    win.end();
    win.set_callback(move |_| tx.send(0));
    let key: Rc<RefCell<Option<String>>> = Rc::default();
    let right_ctrl = Rc::new(RefCell::new(false));
    {
        let (key, rc, mut shown) = (key.clone(), right_ctrl.clone(), shown.clone());
        win.handle(move |_, ev| {
            let (k, st) = (app::event_key(), app::event_state());
            let name = match ev {
                Event::KeyDown | Event::Shortcut if k == fltk::enums::Key::ControlR => {
                    *rc.borrow_mut() = true;
                    return true;
                }
                Event::KeyUp if k == fltk::enums::Key::ControlR => {
                    if !std::mem::take(&mut *rc.borrow_mut()) {
                        return true;
                    }
                    Some("Right Ctrl".to_owned())
                }
                Event::KeyDown | Event::Shortcut => {
                    *rc.borrow_mut() = false;
                    key_name(k, st)
                }
                _ => return false,
            };
            if let Some(n) = name {
                shown.set_label(&n);
                shown.redraw();
                *key.borrow_mut() = Some(n);
            }
            true
        });
    }
    center_on(&mut win, Some(parent));
    win.show();
    let _ = win.take_focus();
    loop {
        if !win.shown() {
            return Captured::Cancel;
        }
        app::wait();
        match rx.recv() {
            Some(1) => {
                win.hide();
                return match key.borrow().clone() {
                    Some(k) => Captured::Key(k),
                    None => Captured::Cancel,
                };
            }
            Some(2) => {
                win.hide();
                return Captured::Clear;
            }
            Some(_) => {
                win.hide();
                return Captured::Cancel;
            }
            None => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_function_has_a_row_and_keys_have_theirs() {
        let map = KeyMap::defaults();
        let r = rows(&map);
        for (f, _) in FUNCTIONS {
            assert!(r.iter().any(|row| row.function == *f), "{f}");
        }
        let help = r.iter().find(|row| row.function == "Help").unwrap();
        assert_eq!(help.key, None, "Help has no default key");
        let fe = r.iter().find(|row| row.function == "FieldExit").unwrap();
        assert_eq!(fe.key.as_deref(), Some("Right Ctrl"));
        assert_eq!(
            r.iter().filter(|row| row.key.is_some()).count(),
            map.keys.len()
        );
    }
}
