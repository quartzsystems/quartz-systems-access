// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! The UI sampler: one window showing every widget once, for screenshots and
//! for checking the look on each platform.

use fltk::draw;
use fltk::enums::{Align, FrameType, Shortcut};
use fltk::frame::Frame;
use fltk::menu::{MenuBar, MenuFlag};
use fltk::prelude::*;
use fltk::widget::Widget;
use fltk::window::Window;

use crate::controls::{self, Switch};
use crate::glyph::{self, Glyph, Size};
use crate::groupbox::{self, CONTENT_SIDE, CONTENT_TOP};
use crate::listtable::{Cell, ListTable};
use crate::msgbox::MessageBox;
use crate::propdialog::PropertyDialog;
use crate::statusbar::{Status, StatusBar};
use crate::tabs::Tabs;
use crate::taskpane::TaskPane;
use crate::theme::{self, colors};
use crate::toolbar::Toolbar;

/// Width of the sampler window.
pub const W: i32 = 900;
/// Height of the sampler window.
pub const H: i32 = 600;

/// Height of a menu bar.
pub const MENU_H: i32 = 22;

/// A menu bar with the house look.
pub fn menu_bar(x: i32, y: i32, w: i32) -> MenuBar {
    let mut m = MenuBar::new(x, y, w, MENU_H, None);
    m.set_frame(FrameType::FlatBox);
    m.set_down_frame(FrameType::FlatBox);
    m.set_color(colors::FACE);
    m.set_selection_color(colors::SELECTION);
    m.set_text_size(theme::UI_SIZE);
    m
}

/// Builds the sampler window. It is not shown.
pub fn build() -> Window {
    let mut win = Window::new(0, 0, W, H, None);
    win.set_label("QS/400 Access for Desktop - UI Sampler");
    win.set_color(colors::FACE);

    let mut menu = menu_bar(0, 0, W);
    for path in [
        "&File/&New...",
        "&File/&Open...",
        "&File/E&xit",
        "&Edit/&Copy",
        "&View/&Refresh",
        "&Help/&About...",
    ] {
        menu.add(path, Shortcut::None, MenuFlag::Normal, |_| {});
    }

    let mut status = StatusBar::new(0, H - StatusBar::HEIGHT, W);
    status.set_message("Ready");
    status.set_status("Not connected", Status::Error);

    let mut tb = Toolbar::new(0, MENU_H, W);
    for g in Glyph::ALL {
        let mut s = status.clone();
        tb.add(g, g.name(), move || s.set_message(g.name()));
        if g == Glyph::System {
            tb.separator();
        }
    }
    tb.end();

    let top = MENU_H + Toolbar::HEIGHT + 6;
    let bottom = H - StatusBar::HEIGHT - 6;

    // Left: the task pane.
    let mut pane = TaskPane::new(6, top, 190, bottom - top);
    pane.add_group("General");
    for (g, t) in [
        (Glyph::Terminal, "5250 Display"),
        (Glyph::Disk, "Data Transfer"),
        (Glyph::Folder, "Integrated File System"),
    ] {
        let mut s = status.clone();
        pane.add_link(Some(g), t, move || s.set_message(t));
    }
    pane.add_group("Database");
    for (g, t) in [(Glyph::Table, "Schemas"), (Glyph::Table, "Run SQL")] {
        let mut s = status.clone();
        pane.add_link(Some(g), t, move || s.set_message(t));
    }
    pane.end();

    // Right, top row: buttons and fields.
    let rx = 204;
    let rw = W - rx - 6;
    let controls_box = groupbox::group_box(rx, top, rw, 132, "Controls");
    {
        let x = rx + CONTENT_SIDE;
        let mut y = top + CONTENT_TOP;
        let bw = theme::BUTTON_W;
        let bh = theme::BUTTON_H;
        let mut bx = x;
        let verbs = [
            "OK",
            "Cancel",
            "Apply",
            "Close",
            "Help",
            "Browse...",
            "Add",
            "Remove",
        ];
        for (i, v) in verbs.iter().enumerate() {
            if i == 0 {
                controls::default_button(bx, y, bw, bh, v);
            } else {
                controls::button(bx, y, bw, bh, v);
            }
            bx += bw + 6;
        }
        y += bh + 6;
        bx = x;
        for v in ["New...", "Edit...", "Delete", "Import...", "Export..."] {
            controls::button(bx, y, bw, bh, v);
            bx += bw + 6;
        }
        let mut disabled = controls::button(bx, y, bw, bh, "Disabled");
        Switch::for_button(&mut disabled).set_enabled(false);
        y += bh + 8;
        controls::label(x, y, 60, theme::FIELD_H, "Field:");
        let mut f = controls::field(x + 60, y, 150, theme::FIELD_H);
        f.set_value("sys1.example.net");
        let mut off = controls::field(x + 216, y, 120, theme::FIELD_H);
        off.set_value("192.0.2.10");
        Switch::for_input(&mut off).set_enabled(false);
        let mut c = controls::combo(x + 342, y, 130, theme::FIELD_H);
        c.add_choice("Always|Daily|Never");
        c.set_value(1);
        let ck = controls::check(x + 480, y, 90, theme::FIELD_H, "Use TLS");
        ck.set_checked(true);
    }
    controls_box.end();

    // Right, second row: radio buttons, status words, glyphs.
    let y2 = top + 132 + 6;
    let radios = groupbox::group_box(rx, y2, 180, 96, "Credentials");
    {
        let x = rx + CONTENT_SIDE;
        let mut y = y2 + CONTENT_TOP;
        let mut first = true;
        for t in ["Use default user", "Prompt once", "Prompt every time"] {
            let mut r = controls::radio(x, y, 160, 20, t);
            if first {
                r.set_value(true);
                first = false;
            }
            y += 22;
        }
    }
    radios.end();

    let words = groupbox::group_box(rx + 186, y2, 150, 96, "Status words");
    {
        let x = rx + 186 + CONTENT_SIDE;
        let mut y = y2 + CONTENT_TOP;
        for (t, s) in [
            ("Active", Status::Active),
            ("Ended", Status::Error),
            ("Not connected", Status::Error),
            ("Disabled text", Status::Neutral),
        ] {
            let mut l = controls::label(x, y, 130, 18, t);
            l.set_label_color(if t == "Disabled text" {
                colors::DISABLED_TEXT
            } else {
                s.color()
            });
            y += 18;
        }
    }
    words.end();

    let gx = rx + 342;
    let glyphs = groupbox::group_box(gx, y2, rw - 342, 96, "Glyphs");
    {
        let mut x = gx + CONTENT_SIDE;
        for g in Glyph::ALL {
            let mut small = Frame::new(x + 8, y2 + CONTENT_TOP, 16, 16, None);
            small.set_image(Some(glyph::image(g, Size::Small)));
            let mut large = Frame::new(x, y2 + CONTENT_TOP + 22, 32, 32, None);
            large.set_image(Some(glyph::image(g, Size::Large)));
            x += 36;
        }
    }
    glyphs.end();

    // Right, third row: list and tabs with monospace output.
    let y3 = y2 + 96 + 6;
    let h3 = bottom - y3;
    let half = (rw - 6) / 2;
    let mut list = ListTable::new(
        rx,
        y3,
        half,
        h3,
        &[("Name", 110), ("Description", 120), ("TLS", 40)],
    );
    let mut rows: Vec<Vec<Cell>> = vec![
        vec!["SYS1".into(), "Production".into(), "Yes".into()],
        vec!["SYS2".into(), "Test".into(), "No".into()],
        vec!["DEV".into(), "Development".into(), "Yes".into()],
    ];
    rows.push(vec![
        "OLD".into(),
        Cell {
            text: "Ended".into(),
            status: Status::Error,
        },
        "No".into(),
    ]);
    list.set_rows(rows);
    list.select(Some(0));

    let mut tabs = Tabs::new(rx + half + 6, y3, rw - half - 6, h3);
    tabs.add_page("General", |r| {
        controls::label(r.x, r.y, 80, theme::FIELD_H, "System:");
        let mut f = controls::field(r.x + 80, r.y, r.w - 80, theme::FIELD_H);
        f.set_value("SYS1");
        let mut b = controls::button(r.x, r.y + 30, 140, theme::BUTTON_H, "Property dialog...");
        b.set_callback(|b| {
            let mut d = PropertyDialog::new("Sample Properties", 420, 300);
            d.tabs().add_page("General", |r| {
                controls::label(r.x, r.y, 80, theme::FIELD_H, "Name:");
                controls::field(r.x + 80, r.y, 200, theme::FIELD_H);
            });
            d.tabs().add_page("Ports", |_| {});
            d.on_help(|| {
                MessageBox::new("Help", "Help is not available in the sampler.").run(None);
            });
            let parent = b.window();
            d.run(parent.as_deref(), || Err("Name is required.".to_owned()));
        });
    });
    tabs.add_page("Output", |r| {
        let mut out = Widget::new(r.x, r.y, r.w, r.h, None);
        out.draw(|w| {
            draw::draw_box(
                FrameType::DownBox,
                w.x(),
                w.y(),
                w.w(),
                w.h(),
                colors::WINDOW,
            );
            draw::set_font(theme::MONO_FONT, theme::MONO_SIZE);
            draw::set_draw_color(colors::TEXT);
            let lines = [
                "Job started at 09:14:03.",
                "  42 records copied.",
                "Job ended normally.",
            ];
            let lh = draw::height();
            for (i, l) in lines.iter().enumerate() {
                draw::draw_text2(
                    l,
                    w.x() + 4,
                    w.y() + 3 + i as i32 * lh,
                    w.w() - 8,
                    lh,
                    Align::Left,
                );
            }
        });
    });
    tabs.add_page("Security", |_| {});

    win.end();
    win
}
