// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! A small modal message box with a row of buttons at the bottom right.

use std::cell::Cell;
use std::rc::Rc;

use fltk::app;
use fltk::draw;
use fltk::enums::{Align, FrameType};
use fltk::frame::Frame;
use fltk::prelude::*;
use fltk::window::Window;

use crate::controls;
use crate::glyph::{self, Glyph, Size};
use crate::propdialog::center_on;
use crate::theme::{self, colors};

/// A message box under construction.
pub struct MessageBox {
    title: String,
    text: String,
    glyph: Option<Glyph>,
    buttons: Vec<String>,
    default: usize,
    cancel: usize,
}

impl MessageBox {
    /// A message box with an OK button.
    pub fn new(title: &str, text: &str) -> MessageBox {
        MessageBox {
            title: title.to_owned(),
            text: text.to_owned(),
            glyph: None,
            buttons: vec!["OK".to_owned()],
            default: 0,
            cancel: 0,
        }
    }

    /// Shows a 32×32 glyph to the left of the text.
    pub fn glyph(mut self, g: Glyph) -> MessageBox {
        self.glyph = Some(g);
        self
    }

    /// Sets the buttons, left to right; `default` is pressed by Enter and
    /// `cancel` is the answer when the box is closed or Escape is pressed.
    pub fn buttons(mut self, labels: &[&str], default: usize, cancel: usize) -> MessageBox {
        self.buttons = labels.iter().map(|s| (*s).to_owned()).collect();
        self.default = default.min(self.buttons.len() - 1);
        self.cancel = cancel.min(self.buttons.len() - 1);
        self
    }

    /// Builds the window without showing it. The cell receives the index of
    /// the button pressed.
    pub fn build(&self) -> (Window, Rc<Cell<Option<usize>>>) {
        let m = 12;
        let bw = theme::BUTTON_W;
        let bh = theme::BUTTON_H;
        let text_x = if self.glyph.is_some() { m + 32 + 12 } else { m };
        draw::set_font(theme::UI_FONT, theme::UI_SIZE);
        let wrap = 360;
        let (tw, th) = draw::wrap_measure(&self.text, wrap, false);
        let n = self.buttons.len() as i32;
        let buttons_w = n * bw + (n - 1) * 6;
        let w = (text_x + tw.max(180) + m).max(buttons_w + 2 * m);
        let body_h = th.max(32);
        let h = m + body_h + m + bh + m;

        let mut win = Window::new(0, 0, w, h, None);
        win.set_label(&self.title);
        win.set_color(colors::FACE);
        win.make_modal(true);
        if let Some(g) = self.glyph {
            let mut icon = Frame::new(m, m, 32, 32, None);
            icon.set_frame(FrameType::NoBox);
            icon.set_image(Some(glyph::image(g, Size::Large)));
        }
        let mut text = Frame::new(text_x, m, w - text_x - m, body_h, None);
        text.set_label(&self.text);
        text.set_align(Align::Left | Align::Top | Align::Inside | Align::Wrap);
        text.set_label_size(theme::UI_SIZE);

        let answer = Rc::new(Cell::new(None));
        let mut x = w - m - buttons_w;
        for (i, label) in self.buttons.iter().enumerate() {
            let mut b = if i == self.default {
                controls::default_button(x, h - m - bh, bw, bh, label)
            } else {
                controls::button(x, h - m - bh, bw, bh, label)
            };
            let a = answer.clone();
            b.set_callback(move |_| a.set(Some(i)));
            if i == self.default {
                let _ = b.take_focus();
            }
            x += bw + 6;
        }
        win.end();
        let a = answer.clone();
        let cancel = self.cancel;
        win.set_callback(move |_| a.set(Some(cancel)));
        (win, answer)
    }

    /// Shows the box centred on `parent` and waits for an answer.
    pub fn run(&self, parent: Option<&dyn WindowExt>) -> usize {
        let (mut win, answer) = self.build();
        center_on(&mut win, parent);
        win.show();
        while win.shown() {
            app::wait();
            if let Some(i) = answer.take() {
                win.hide();
                return i;
            }
        }
        self.cancel
    }
}
