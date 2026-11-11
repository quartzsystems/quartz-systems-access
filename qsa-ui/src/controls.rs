// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! Standard controls with the house look applied, and enable/disable that
//! keeps disabled text at exactly the palette grey.

use std::cell::Cell;
use std::rc::Rc;

use fltk::button::{Button, CheckButton, RadioRoundButton};
use fltk::draw;
use fltk::enums::{Align, Event, FrameType, Key, Shortcut};
use fltk::frame::Frame;
use fltk::input::{Input, SecretInput};
use fltk::menu::Choice;
use fltk::misc::InputChoice;
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
    b.handle(hover);
    b
}

/// Pale blue body under the pointer; the box draws the blue edge.
fn hover<W: WidgetExt>(w: &mut W, ev: Event) -> bool {
    if !theme::hover_effects() {
        return false;
    }
    match ev {
        Event::Enter => {
            w.set_color(colors::BUTTON_HOVER);
            w.redraw();
            true
        }
        Event::Leave => {
            w.set_color(colors::BUTTON);
            w.redraw();
            true
        }
        _ => false,
    }
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

/// An editable combo: a field to type in, as [`field`], with a button at
/// the right that drops down values to pick.
pub fn editable_combo(x: i32, y: i32, w: i32, h: i32) -> InputChoice {
    let mut c = InputChoice::new(x, y, w, h, None);
    c.set_frame(FrameType::DownBox);
    c.set_color(colors::WINDOW);
    c.set_text_size(theme::UI_SIZE);
    c.set_selection_color(colors::SELECTION);
    let mut i = c.input();
    i.set_frame(FrameType::FlatBox);
    i.set_color(colors::WINDOW);
    i.set_selection_color(colors::SELECTION);
    let mut b = c.menu_button();
    b.set_frame(FrameType::FlatBox);
    b.set_color(colors::BUTTON);
    b.set_text_size(theme::UI_SIZE);
    b.set_selection_color(colors::HOVER);
    c
}

/// A password field: like [`field`], showing dots for what is typed.
pub fn secret_field(x: i32, y: i32, w: i32, h: i32) -> SecretInput {
    let mut i = SecretInput::new(x, y, w, h, None);
    i.set_frame(FrameType::DownBox);
    i.set_color(colors::WINDOW);
    i.set_text_size(theme::UI_SIZE);
    i.set_selection_color(colors::SELECTION);
    i
}

/// A drop-down list (combo): a push-button body with a slim chevron at the
/// right. It is drawn here in full; FLTK draws only the open list, which takes
/// its background from the widget colour, so that stays white.
pub fn combo(x: i32, y: i32, w: i32, h: i32) -> Choice {
    let mut c = Choice::new(x, y, w, h, None);
    c.set_frame(FrameType::DownBox);
    c.set_down_frame(FrameType::FlatBox);
    c.set_menu_frame(theme::MENU_BOX);
    c.set_color(colors::WINDOW);
    c.set_text_size(theme::UI_SIZE);
    c.set_selection_color(colors::HOVER);
    c.super_draw(false);
    c.draw(|c| {
        let (x, y, w, h) = (c.x(), c.y(), c.w(), c.h());
        let enabled = c.text_color() != colors::DISABLED_TEXT;
        let body = if enabled {
            colors::BUTTON
        } else {
            colors::BUTTON_DISABLED
        };
        theme::draw_button(x, y, w, h, body);
        let text = c.choice().unwrap_or_default();
        let text_w = w - 4 - 18;
        draw::set_font(theme::UI_FONT, theme::UI_SIZE);
        draw::set_draw_color(c.text_color());
        draw::push_clip(x + 1, y + 1, text_w, h - 2);
        draw::draw_text2(&text, x + 4, y, text_w - 4, h, Align::Left);
        draw::pop_clip();
        if c.has_focus() && fltk::app::visible_focus() {
            draw::set_draw_color(colors::TEXT);
            draw::draw_focus_rect(x + 2, y + 2, text_w, h - 4);
        }
        // Chevron: two one-pixel-thick strokes meeting at the bottom.
        let (cx, cy) = (x + w - 11, y + h / 2 - 2);
        let col = if enabled {
            colors::TEXT
        } else {
            colors::DISABLED_TEXT
        };
        for i in 0..4 {
            draw::draw_rect_fill(cx - 3 + i, cy + i, 2, 1, col);
            draw::draw_rect_fill(cx + 2 - i, cy + i, 2, 1, col);
        }
    });
    c
}

/// The label of a check box or radio. FLTK sets the text only a pixel or two
/// past the box, with no setting for the gap, so a leading space gives the
/// gap Windows leaves.
fn spaced(label: &str) -> String {
    format!(" {label}")
}

/// A link: its text in the link colour, underlined, on the face colour.
/// It is a button, so Tab reaches it and Enter or the space bar selects it.
pub fn link(x: i32, y: i32, w: i32, h: i32, text: &str) -> Button {
    let mut b = Button::new(x, y, w, h, None);
    b.set_label(text);
    b.set_frame(FrameType::FlatBox);
    b.set_color(colors::FACE);
    b.draw(|b| {
        let (x, y, w, h) = (b.x(), b.y(), b.w(), b.h());
        draw::draw_rect_fill(x, y, w, h, b.color());
        draw::set_font(theme::UI_FONT_BOLD, theme::UI_SIZE);
        draw::set_draw_color(colors::LINK);
        let label = b.label();
        draw::draw_text2(&label, x, y, w, h, Align::Left);
        let tw = (draw::width(&label) as i32).min(w);
        let base = y + (h + draw::height()) / 2 - draw::descent();
        draw::draw_rect_fill(x, base + 1, tw, 1, colors::LINK);
        if b.has_focus() {
            draw::set_draw_color(colors::TEXT);
            draw::draw_focus_rect(x - 1, y + 1, tw + 3, h - 2);
        }
    });
    b.handle(|b, ev| {
        let cursor = match ev {
            Event::Enter => fltk::enums::Cursor::Hand,
            Event::Leave => fltk::enums::Cursor::Default,
            _ => return false,
        };
        if let Some(mut win) = b.window() {
            win.set_cursor(cursor);
        }
        true
    });
    b
}

/// A check box.
pub fn check(x: i32, y: i32, w: i32, h: i32, label: &str) -> CheckButton {
    let mut c = CheckButton::new(x, y, w, h, None);
    c.set_label(&spaced(label));
    c.set_label_size(theme::UI_SIZE);
    c.set_selection_color(colors::TEXT);
    c.set_down_frame(theme::CHECK_BOX);
    c.set_color(colors::WINDOW);
    c
}

/// A radio button. Radios in the same group are exclusive.
pub fn radio(x: i32, y: i32, w: i32, h: i32, label: &str) -> RadioRoundButton {
    let mut r = RadioRoundButton::new(x, y, w, h, None);
    r.set_label(&spaced(label));
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
        Self::install_with(w, false, apply)
    }

    fn install_with<W>(w: &mut W, hovers: bool, apply: impl Fn(&mut W, bool) + 'static) -> Switch
    where
        W: WidgetExt + WidgetBase + Clone + 'static,
    {
        let enabled = Rc::new(Cell::new(true));
        let flag = enabled.clone();
        w.super_handle_first(false);
        w.handle(move |w, ev| {
            if flag.get() {
                return hovers && hover(w, ev);
            }
            matches!(
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
        Switch::install_with(b, true, |b, on| {
            // Only push buttons have a body that greys out; check boxes and
            // radios keep their white well.
            if b.frame() == FrameType::UpBox || b.frame() == theme::DEFAULT_BUTTON_BOX {
                b.set_color(if on {
                    colors::BUTTON
                } else {
                    colors::BUTTON_DISABLED
                });
            }
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

    /// Switch for a combo. Its body follows the text colour; see [`combo`].
    pub fn for_choice(c: &mut Choice) -> Switch {
        Switch::install(c, |c, on| {
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
