// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! A small modal box that asks for one line of text, with OK and Cancel.
//! A hidden box shows the text as it is typed as dots (for passwords).

use std::cell::Cell;
use std::rc::Rc;

use fltk::app;
use fltk::enums::Align;
use fltk::frame::Frame;
use fltk::input::{Input, SecretInput};
use fltk::prelude::*;
use fltk::window::Window;

use crate::controls;
use crate::propdialog::center_on;
use crate::theme::{self, colors};

/// An input box under construction.
pub struct InputBox {
    title: String,
    prompt: String,
    value: String,
    hidden: bool,
}

/// The text field: plain or hidden.
pub enum Field {
    Plain(Input),
    Hidden(SecretInput),
}

impl Field {
    /// The text in the field.
    pub fn value(&self) -> String {
        match self {
            Field::Plain(i) => i.value(),
            Field::Hidden(i) => i.value(),
        }
    }
}

impl InputBox {
    /// A box titled `title` showing `prompt` above the field.
    pub fn new(title: &str, prompt: &str) -> InputBox {
        InputBox {
            title: title.to_owned(),
            prompt: prompt.to_owned(),
            value: String::new(),
            hidden: false,
        }
    }

    /// The text the field starts with (selected, so typing replaces it).
    pub fn value(mut self, v: &str) -> InputBox {
        self.value = v.to_owned();
        self
    }

    /// Shows dots instead of what is typed.
    pub fn hidden(mut self, on: bool) -> InputBox {
        self.hidden = on;
        self
    }

    /// Builds the window without showing it. The cell is set to true for
    /// OK and false for Cancel.
    pub fn build(&self) -> (Window, Field, Rc<Cell<Option<bool>>>) {
        let (w, h) = (360, 124);
        let m = 12;
        let bw = theme::BUTTON_W;
        let bh = theme::BUTTON_H;
        let mut win = Window::new(0, 0, w, h, None);
        win.set_label(&self.title);
        win.set_color(colors::FACE);
        win.make_modal(true);
        let mut text = Frame::new(m, m, w - 2 * m, 20, None);
        text.set_label(&self.prompt);
        text.set_align(Align::Left | Align::Inside);
        text.set_label_size(theme::UI_SIZE);
        let fy = m + 24;
        let field = if self.hidden {
            let mut i = controls::secret_field(m, fy, w - 2 * m, theme::FIELD_H);
            i.set_value(&self.value);
            Field::Hidden(i)
        } else {
            let mut i = controls::field(m, fy, w - 2 * m, theme::FIELD_H);
            i.set_value(&self.value);
            let _ = i.set_position(self.value.len() as i32);
            let _ = i.set_mark(0);
            Field::Plain(i)
        };
        let answer = Rc::new(Cell::new(None));
        let mut ok = controls::default_button(w - m - 2 * bw - 6, h - m - bh, bw, bh, "OK");
        let a = answer.clone();
        ok.set_callback(move |_| a.set(Some(true)));
        let mut cancel = controls::button(w - m - bw, h - m - bh, bw, bh, "Cancel");
        let a = answer.clone();
        cancel.set_callback(move |_| a.set(Some(false)));
        win.end();
        let a = answer.clone();
        win.set_callback(move |_| a.set(Some(false)));
        (win, field, answer)
    }

    /// Shows the box centred on `parent` and waits. `Some` with the text on
    /// OK.
    pub fn run(&self, parent: Option<&dyn WindowExt>) -> Option<String> {
        let (mut win, field, answer) = self.build();
        center_on(&mut win, parent);
        crate::propdialog::show_placed(&mut win);
        match &field {
            Field::Plain(i) => {
                let _ = i.clone().take_focus();
            }
            Field::Hidden(i) => {
                let _ = i.clone().take_focus();
            }
        }
        while win.shown() {
            app::wait();
            if let Some(ok) = answer.take() {
                win.hide();
                return ok.then(|| field.value());
            }
        }
        None
    }
}
