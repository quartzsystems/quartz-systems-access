// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! Standard controls with the house look applied, and enable/disable that
//! keeps disabled text at exactly the palette grey.

use std::cell::Cell;
use std::rc::Rc;

use fltk::button::{Button, CheckButton, RadioRoundButton};
use fltk::enums::{Align, Event, FrameType, Key, Shortcut};
use fltk::frame::Frame;
use fltk::input::Input;
use fltk::menu::Choice;
use fltk::prelude::*;

use crate::theme::{self, colors};

/// A push button: light body, thin outline, darker while pressed.
pub fn button(x: i32, y: i32, w: i32, h: i32, label: &str) -> Button {
    let mut b = Button::new(x, y, w, h, None);
    b.set_label(label);
    b.set_frame(FrameType::UpBox);
    b.set_down_frame(theme::PRESSED_BUTTON_BOX);
    b.set_color(colors::BUTTON);
    b.set_label_size(theme::UI_SIZE);
    b
}

/// The default button of a dialog: dark outline, pressed by Enter.
pub fn default_button(x: i32, y: i32, w: i32, h: i32, label: &str) -> Button {
    let mut b = button(x, y, w, h, label);
    b.set_frame(theme::DEFAULT_BUTTON_BOX);
    b.set_shortcut(Shortcut::None | Key::Enter);
    b
}

/// A left-aligned text label.
pub fn label(x: i32, y: i32, w: i32, h: i32, text: &str) -> Frame {
    let mut f = Frame::new(x, y, w, h, None);
    f.set_label(text);
    f.set_align(Align::Left | Align::Inside | Align::Clip);
    f.set_label_size(theme::UI_SIZE);
    f
}

/// A single-line text field: sunken, white.
pub fn field(x: i32, y: i32, w: i32, h: i32) -> Input {
    let mut i = Input::new(x, y, w, h, None);
    i.set_frame(FrameType::DownBox);
    i.set_color(colors::WINDOW);
    i.set_text_size(theme::UI_SIZE);
    i.set_selection_color(colors::SELECTION);
    i
}

/// A drop-down list (combo).
pub fn combo(x: i32, y: i32, w: i32, h: i32) -> Choice {
    let mut c = Choice::new(x, y, w, h, None);
    c.set_frame(FrameType::DownBox);
    c.set_down_frame(FrameType::FlatBox);
    c.set_color(colors::WINDOW);
    c.set_text_size(theme::UI_SIZE);
    c.set_selection_color(colors::SELECTION);
    c
}

/// A check box.
pub fn check(x: i32, y: i32, w: i32, h: i32, label: &str) -> CheckButton {
    let mut c = CheckButton::new(x, y, w, h, None);
    c.set_label(label);
    c.set_label_size(theme::UI_SIZE);
    c.set_selection_color(colors::TEXT);
    c
}

/// A radio button. Radios in the same group are exclusive.
pub fn radio(x: i32, y: i32, w: i32, h: i32, label: &str) -> RadioRoundButton {
    let mut r = RadioRoundButton::new(x, y, w, h, None);
    r.set_label(label);
    r.set_label_size(theme::UI_SIZE);
    r.set_selection_color(colors::TEXT);
    r.set_down_frame(FrameType::RoundDownBox);
    r.set_color(colors::WINDOW);
    r
}

/// Enables and disables one control.
///
/// FLTK's own `deactivate` blends text towards the face colour, which cannot
/// give the palette's disabled grey. A switched control stays active as far as
/// FLTK is concerned, but while disabled it swallows input, cannot take focus,
/// and draws its text in [`colors::DISABLED_TEXT`].
#[derive(Clone)]
pub struct Switch {
    enabled: Rc<Cell<bool>>,
    apply: Rc<dyn Fn(bool)>,
}

impl Switch {
    /// Installs a switch on `w`. `apply` restyles the widget for the given state.
    pub fn install<W>(w: &mut W, apply: impl Fn(&mut W, bool) + 'static) -> Switch
    where
        W: WidgetExt + WidgetBase + Clone + 'static,
    {
        let enabled = Rc::new(Cell::new(true));
        let flag = enabled.clone();
        w.super_handle_first(false);
        w.handle(move |_, ev| {
            !flag.get()
                && matches!(
                    ev,
                    Event::Push
                        | Event::Released
                        | Event::Drag
                        | Event::KeyDown
                        | Event::KeyUp
                        | Event::Shortcut
                        | Event::MouseWheel
                        | Event::Paste
                        | Event::DndEnter
                        | Event::DndDrag
                        | Event::DndRelease
                )
        });
        let target = w.clone();
        Switch {
            enabled,
            apply: Rc::new(move |on| {
                let mut target = target.clone();
                if on {
                    target.set_visible_focus();
                } else {
                    target.clear_visible_focus();
                }
                apply(&mut target, on);
                target.redraw();
            }),
        }
    }

    /// Switch for a button, check box or radio: only the label changes.
    pub fn for_button<B>(b: &mut B) -> Switch
    where
        B: ButtonExt + WidgetBase + Clone + 'static,
    {
        Switch::install(b, |b, on| {
            b.set_label_color(if on {
                colors::TEXT
            } else {
                colors::DISABLED_TEXT
            });
        })
    }

    /// Switch for a text field: disabled fields are read-only on the face colour.
    pub fn for_input<I>(i: &mut I) -> Switch
    where
        I: InputExt + WidgetBase + Clone + 'static,
    {
        Switch::install(i, |i, on| {
            i.set_readonly(!on);
            i.set_color(if on { colors::WINDOW } else { colors::FACE });
            i.set_text_color(if on {
                colors::TEXT
            } else {
                colors::DISABLED_TEXT
            });
            i.set_label_color(if on {
                colors::TEXT
            } else {
                colors::DISABLED_TEXT
            });
        })
    }

    /// Switch for a combo.
    pub fn for_choice(c: &mut Choice) -> Switch {
        Switch::install(c, |c, on| {
            c.set_color(if on { colors::WINDOW } else { colors::FACE });
            c.set_text_color(if on {
                colors::TEXT
            } else {
                colors::DISABLED_TEXT
            });
        })
    }

    /// Whether the control is enabled.
    pub fn is_enabled(&self) -> bool {
        self.enabled.get()
    }

    /// Enables or disables the control.
    pub fn set_enabled(&self, on: bool) {
        self.enabled.set(on);
        (self.apply)(on);
    }
}
