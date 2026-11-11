// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! The About box. It carries the single use of the Quartz accent: a thin
//! rule under the product name.

use fltk::app;
use fltk::draw;
use fltk::enums::Align;
use fltk::frame::Frame;
use fltk::prelude::*;
use fltk::widget::Widget;
use fltk::window::Window;
use qsa_ui::controls;
use qsa_ui::glyph::{self, Glyph, Size};
use qsa_ui::propdialog::center_on;
use qsa_ui::theme::{self, colors};

use crate::{PRODUCT, VERSION};

/// Width of the About box.
pub const W: i32 = 420;
/// Height of the About box.
pub const H: i32 = 200;

/// Builds the About box. It is not shown.
pub fn build() -> Window {
    let mut win = Window::new(0, 0, W, H, None);
    crate::help::attach(&mut win, crate::help::topic::GETTING_STARTED);
    win.set_label(&format!("About {PRODUCT}"));
    win.set_color(colors::FACE);
    win.make_modal(true);

    let mut icon = Frame::new(16, 16, 32, 32, None);
    icon.set_image(Some(glyph::image(Glyph::System, Size::Large)));

    let x = 64;
    let mut title = Frame::new(x, 16, W - x - 16, 20, None);
    title.set_label(PRODUCT);
    title.set_label_font(theme::UI_FONT_BOLD);
    title.set_label_size(14);
    title.set_align(Align::Left | Align::Inside);

    let mut rule = Widget::new(x, 40, W - x - 16, 2, None);
    rule.draw(|r| draw::draw_rect_fill(r.x(), r.y(), r.w(), r.h(), colors::QUARTZ_ACCENT));

    let lines = [
        format!("Version {VERSION}"),
        "Quartz Systems".to_owned(),
        "Copyright (C) 2026 Quartz Systems. Some rights reserved.".to_owned(),
    ];
    let mut y = 50;
    for l in lines {
        let mut f = controls::label(x, y, W - x - 16, 18, &l);
        f.set_label_size(theme::UI_SIZE);
        y += 18;
    }

    let mut ok = controls::default_button(
        W - 16 - theme::BUTTON_W,
        H - 12 - theme::BUTTON_H,
        theme::BUTTON_W,
        theme::BUTTON_H,
        "OK",
    );
    win.end();
    let mut w = win.clone();
    ok.set_callback(move |_| w.hide());
    win
}

/// Shows the About box and waits until it is closed.
pub fn show(parent: Option<&dyn WindowExt>) {
    let mut win = build();
    center_on(&mut win, parent);
    win.show();
    while win.shown() {
        app::wait();
    }
}
