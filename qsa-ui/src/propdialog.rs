// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! The modal property dialog: tabs above, a status line and OK / Cancel /
//! Help at the bottom right.

use std::cell::Cell;
use std::rc::Rc;

use fltk::app;
use fltk::button::Button;
use fltk::prelude::*;
use fltk::window::Window;

use crate::controls;
use crate::statusbar::{Status, StatusLine};
use crate::tabs::Tabs;
use crate::theme::{self, colors};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Request {
    Ok,
    Cancel,
}

/// A modal tabbed property dialog.
pub struct PropertyDialog {
    window: Window,
    tabs: Tabs,
    status: StatusLine,
    ok: Button,
    cancel: Button,
    help: Button,
    request: Rc<Cell<Option<Request>>>,
}

impl PropertyDialog {
    /// Margin round the dialog content.
    pub const MARGIN: i32 = 8;

    /// A dialog of the given size. Add pages through [`PropertyDialog::tabs`].
    pub fn new(title: &str, w: i32, h: i32) -> PropertyDialog {
        let m = Self::MARGIN;
        let bh = theme::BUTTON_H;
        let bw = theme::BUTTON_W;
        let mut window = Window::new(0, 0, w, h, None);
        window.set_label(title);
        window.set_color(colors::FACE);
        window.make_modal(true);
        let tabs = Tabs::new(m, m, w - 2 * m, h - 3 * m - bh);
        let by = h - m - bh;
        let help_x = w - m - bw;
        let cancel_x = help_x - 6 - bw;
        let ok_x = cancel_x - 6 - bw;
        let status = StatusLine::new(m, by, ok_x - 2 * m, bh);
        let mut ok = controls::default_button(ok_x, by, bw, bh, "OK");
        let mut cancel = controls::button(cancel_x, by, bw, bh, "Cancel");
        let help = controls::button(help_x, by, bw, bh, "Help");
        window.end();

        let request = Rc::new(Cell::new(None));
        let r = request.clone();
        ok.set_callback(move |_| r.set(Some(Request::Ok)));
        let r = request.clone();
        cancel.set_callback(move |_| r.set(Some(Request::Cancel)));
        let r = request.clone();
        // Close box and Escape.
        window.set_callback(move |_| r.set(Some(Request::Cancel)));

        PropertyDialog {
            window,
            tabs,
            status,
            ok,
            cancel,
            help,
            request,
        }
    }

    /// The tab set, for adding pages.
    pub fn tabs(&mut self) -> &mut Tabs {
        &mut self.tabs
    }

    /// The dialog window.
    pub fn window(&self) -> &Window {
        &self.window
    }

    /// The OK button.
    pub fn ok_button(&self) -> &Button {
        &self.ok
    }

    /// The Cancel button.
    pub fn cancel_button(&self) -> &Button {
        &self.cancel
    }

    /// Sets what Help does.
    pub fn on_help(&mut self, mut cb: impl FnMut() + 'static) {
        self.help.set_callback(move |_| cb());
    }

    /// Shows a validation error in the status line.
    pub fn set_error(&mut self, text: &str) {
        self.status.set(text, Status::Error);
    }

    /// Shows plain text in the status line.
    pub fn set_info(&mut self, text: &str) {
        self.status.set(text, Status::Neutral);
    }

    /// The text in the status line.
    pub fn status_text(&self) -> String {
        self.status.text()
    }

    /// Shows the dialog centred on `parent` (or the screen) and runs it until
    /// OK passes `validate`, or the dialog is cancelled. Returns true for OK.
    /// A failed validation is shown in the status line; the dialog stays open.
    pub fn run(
        &mut self,
        parent: Option<&dyn WindowExt>,
        mut validate: impl FnMut() -> Result<(), String>,
    ) -> bool {
        self.place(parent);
        show_placed(&mut self.window);
        while self.window.shown() {
            app::wait();
            match self.request.take() {
                Some(Request::Cancel) => {
                    self.window.hide();
                    return false;
                }
                Some(Request::Ok) => match validate() {
                    Ok(()) => {
                        self.window.hide();
                        return true;
                    }
                    Err(e) => self.set_error(&e),
                },
                None => {}
            }
        }
        false
    }

    /// Centres the dialog on `parent`, or on the screen.
    pub fn place(&mut self, parent: Option<&dyn WindowExt>) {
        center_on(&mut self.window, parent);
    }
}

/// Centres `win` on `parent`, or on the screen.
pub fn center_on(win: &mut Window, parent: Option<&dyn WindowExt>) {
    match parent {
        Some(p) if p.shown() => {
            let x = p.x() + (p.w() - win.w()) / 2;
            let y = p.y() + (p.h() - win.h()) / 2;
            win.set_pos(x.max(0), y.max(0));
        }
        _ => {
            let (sx, sy, sw, sh) = work_area();
            win.set_pos(
                sx + ((sw - win.w()) / 2).max(0),
                sy + ((sh - win.h()) / 2).max(0),
            );
        }
    }
}

/// Shows a window where it was placed. On Windows a new window can come up
/// somewhere else (under the pointer, on another monitor); it is moved
/// back as soon as it exists.
pub fn show_placed(win: &mut Window) {
    let (x, y) = (win.x(), win.y());
    win.show();
    if (win.x(), win.y()) != (x, y) {
        win.set_pos(x, y);
    }
}

/// The usable part (without the taskbar or panels) of the main monitor:
/// x, y, width, height. The main monitor is the one at the origin of the
/// desktop, wherever the pointer is.
pub fn work_area() -> (i32, i32, i32, i32) {
    app::screen_work_area(app::screen_num(0, 0))
}
