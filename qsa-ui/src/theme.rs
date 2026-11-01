// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! Scheme, colour table, box types and fonts.
//!
//! Everything here is set once, by [`init`], before the first widget is made.
//! Nothing reads the desktop theme: FLTK's system colours are never loaded and
//! every box is drawn by the functions below from the fixed palette.
//!
//! The look follows the plain Windows desktop controls: a `#F0F0F0` face,
//! white fields, lists and tab pages, light buttons with softly rounded
//! corners that turn blue under the pointer, and `#0078D7` selection. It is
//! the same on every platform.

use std::path::PathBuf;
use std::sync::OnceLock;

use fltk::app;
use fltk::draw;
use fltk::enums::{Color, Font, FrameType};
use fltk::misc::Tooltip;

/// The fixed palette.
pub mod colors {
    use fltk::enums::Color;

    /// Window and dialog face.
    pub const FACE: Color = Color::from_rgb(0xF0, 0xF0, 0xF0);
    /// Field, list, menu bar and tab page background.
    pub const WINDOW: Color = Color::from_rgb(0xFF, 0xFF, 0xFF);
    /// Normal text.
    pub const TEXT: Color = Color::from_rgb(0x00, 0x00, 0x00);
    /// Selection background in lists and task links.
    pub const SELECTION: Color = Color::from_rgb(0x00, 0x78, 0xD7);
    /// Text on the selection background.
    pub const SELECTION_TEXT: Color = Color::from_rgb(0xFF, 0xFF, 0xFF);
    /// Disabled text.
    pub const DISABLED_TEXT: Color = Color::from_rgb(0x8C, 0x8C, 0x8C);
    /// Status word: active, running.
    pub const STATUS_ACTIVE: Color = Color::from_rgb(0x00, 0x80, 0x00);
    /// Status word: ended, error, not connected.
    pub const STATUS_ERROR: Color = Color::from_rgb(0xC0, 0x00, 0x00);
    /// The Quartz accent. Used once only: the rule in the About box.
    pub const QUARTZ_ACCENT: Color = Color::from_rgb(0x00, 0xD9, 0x92);
    /// Tooltip background.
    pub const TOOLTIP: Color = Color::from_rgb(0xFF, 0xFF, 0xFF);

    /// Push button body.
    pub const BUTTON: Color = Color::from_rgb(0xFD, 0xFD, 0xFD);
    /// Push button body under the pointer.
    pub const BUTTON_HOVER: Color = Color::from_rgb(0xE0, 0xEE, 0xF9);
    /// Push button body while pressed.
    pub const BUTTON_PRESSED: Color = Color::from_rgb(0xCC, 0xE4, 0xF7);
    /// Push button body while disabled.
    pub const BUTTON_DISABLED: Color = Color::from_rgb(0xF5, 0xF5, 0xF5);
    /// Push button outline: top and sides.
    pub const BUTTON_BORDER: Color = Color::from_rgb(0xD0, 0xD0, 0xD0);
    /// Push button outline: bottom edge, a shade darker.
    pub const BUTTON_BORDER_BOTTOM: Color = Color::from_rgb(0xB8, 0xB8, 0xB8);
    /// Push button outline while disabled.
    pub const BUTTON_BORDER_DISABLED: Color = Color::from_rgb(0xE3, 0xE3, 0xE3);
    /// Outline of the default button, and of any button under the pointer.
    pub const ACCENT_BORDER: Color = Color::from_rgb(0x00, 0x78, 0xD4);
    /// Outline of a pressed button.
    pub const PRESSED_BORDER: Color = Color::from_rgb(0x00, 0x54, 0x99);
    /// Outline of text fields and combos.
    pub const FIELD_BORDER: Color = Color::from_rgb(0x7A, 0x7A, 0x7A);
    /// Outline of lists.
    pub const LIST_BORDER: Color = Color::from_rgb(0x82, 0x87, 0x90);
    /// Outline of check boxes and radio buttons.
    pub const CHECK_BORDER: Color = Color::from_rgb(0x33, 0x33, 0x33);
    /// Outline of panels, tabs, group boxes and separators.
    pub const BORDER_LIGHT: Color = Color::from_rgb(0xDC, 0xDC, 0xDC);
    /// Column separators in list headers.
    pub const HEADER_RULE: Color = Color::from_rgb(0xE5, 0xE5, 0xE5);
    /// Menu bar and menu item highlight.
    pub const MENU_HIGHLIGHT: Color = Color::from_rgb(0xCC, 0xE8, 0xFF);
    /// Task pane group header band.
    pub const TASK_HEADER: Color = Color::from_rgb(0xE3, 0xE6, 0xF0);
    /// Rule that divides sections of the task pane.
    pub const TASK_RULE: Color = Color::from_rgb(0x2F, 0x5B, 0xC2);
    /// Task links.
    pub const LINK: Color = Color::from_rgb(0x1F, 0x4F, 0xCB);
    /// Background of a task link under the pointer or focus.
    pub const HOVER: Color = SELECTION;
    /// Text of a task link under the pointer or focus.
    pub const HOVER_TEXT: Color = SELECTION_TEXT;
    /// Task link bullet.
    pub const BULLET: Color = Color::from_rgb(0xA8, 0xAD, 0xB8);
    /// Scroll bar track.
    pub const SCROLL_TRACK: Color = Color::from_rgb(0xF0, 0xF0, 0xF0);
    /// Scroll bar thumb.
    pub const SCROLL_THUMB: Color = Color::from_rgb(0xCD, 0xCD, 0xCD);
}

/// UI text size in pixels.
pub const UI_SIZE: i32 = 12;
/// Monospace text size in pixels.
pub const MONO_SIZE: i32 = 12;
/// Standard button size.
pub const BUTTON_W: i32 = 75;
/// Standard button height.
pub const BUTTON_H: i32 = 23;
/// Standard field and combo height.
pub const FIELD_H: i32 = 21;
/// List row height.
pub const ROW_H: i32 = 18;
/// Cell padding.
pub const CELL_PAD: i32 = 4;

/// UI sans, regular.
pub const UI_FONT: Font = Font::Helvetica;
/// UI sans, bold.
pub const UI_FONT_BOLD: Font = Font::HelveticaBold;
/// Monospace: anything that shows system output.
pub const MONO_FONT: Font = Font::Courier;
/// Monospace, bold.
pub const MONO_FONT_BOLD: Font = Font::CourierBold;

/// Box type of the default (Enter) button: a blue outline.
pub const DEFAULT_BUTTON_BOX: FrameType = FrameType::PlasticUpBox;
/// Box type of a pressed push button: pale blue body, dark blue outline.
pub const PRESSED_BUTTON_BOX: FrameType = FrameType::GtkDownBox;
/// Box type of a check box's well. It must be one of the types FLTK draws a
/// check mark in, rather than filling.
pub const CHECK_BOX: FrameType = FrameType::PlasticDownBox;

struct BundledFont {
    file: &'static str,
    data: &'static [u8],
    slots: &'static [Font],
    #[cfg_attr(not(windows), allow(dead_code))]
    gdi_name: &'static str,
    #[cfg_attr(windows, allow(dead_code))]
    pango_name: &'static str,
}

// Open-licence faces until Quartz Systems' own fonts exist; see THIRD_PARTY.md.
const FONTS: &[BundledFont] = &[
    BundledFont {
        file: "Selawik-Regular.ttf",
        data: include_bytes!("../fonts/Selawik-Regular.ttf"),
        slots: &[Font::Helvetica],
        gdi_name: " Selawik",
        pango_name: "Selawik",
    },
    BundledFont {
        file: "Selawik-Bold.ttf",
        data: include_bytes!("../fonts/Selawik-Bold.ttf"),
        slots: &[Font::HelveticaBold],
        gdi_name: "BSelawik",
        pango_name: "Selawik Bold",
    },
    BundledFont {
        file: "CascadiaMono-Regular.ttf",
        data: include_bytes!("../fonts/CascadiaMono-Regular.ttf"),
        slots: &[Font::Courier, Font::Screen],
        gdi_name: " Cascadia Mono",
        pango_name: "Cascadia Mono",
    },
    BundledFont {
        file: "CascadiaMono-Bold.ttf",
        data: include_bytes!("../fonts/CascadiaMono-Bold.ttf"),
        slots: &[Font::CourierBold, Font::ScreenBold],
        gdi_name: "BCascadia Mono",
        pango_name: "Cascadia Mono Bold",
    },
];

static INIT: OnceLock<Result<(), String>> = OnceLock::new();

/// Sets the scheme, colour table, box types and fonts.
///
/// Call once, after `app::App::default()` and before making any widget. Further
/// calls return the first result. Font errors are returned but are not fatal:
/// the colour table and box types are set either way.
pub fn init() -> Result<(), String> {
    INIT.get_or_init(|| {
        app::set_scheme(app::Scheme::Base);
        set_colors();
        set_box_types();
        let fonts = load_fonts();
        app::set_font(UI_FONT);
        app::set_font_size(UI_SIZE);
        app::set_scrollbar_size(17);
        app::set_visible_focus(true);
        app::set_menu_linespacing(8);
        Tooltip::set_color(colors::TOOLTIP);
        Tooltip::set_text_color(colors::TEXT);
        Tooltip::set_font(UI_FONT);
        Tooltip::set_font_size(UI_SIZE);
        Tooltip::set_margin_width(5);
        Tooltip::set_margin_height(3);
        fonts
    })
    .clone()
}

fn set_colors() {
    let (r, g, b) = colors::FACE.to_rgb();
    app::set_background_color(r, g, b);
    let (r, g, b) = colors::WINDOW.to_rgb();
    app::set_background2_color(r, g, b);
    let (r, g, b) = colors::TEXT.to_rgb();
    app::set_foreground_color(r, g, b);
    let (r, g, b) = colors::SELECTION.to_rgb();
    app::set_selection_color(r, g, b);
    let (r, g, b) = colors::DISABLED_TEXT.to_rgb();
    app::set_inactive_color(r, g, b);
    // The indexed colours FLTK draws with by default.
    for (index, colour) in [
        (Color::Background, colors::FACE),
        (Color::Background2, colors::WINDOW),
        (Color::Foreground, colors::TEXT),
        (Color::Selection, colors::SELECTION),
        (Color::Inactive, colors::DISABLED_TEXT),
        (Color::Black, colors::TEXT),
        (Color::White, colors::WINDOW),
        (Color::Dark3, colors::FIELD_BORDER),
        (Color::Dark2, colors::BORDER_LIGHT),
        (Color::Light3, colors::WINDOW),
    ] {
        let (r, g, b) = colour.to_rgb();
        app::set_color(index, r, g, b);
    }
}

fn set_box_types() {
    use FrameType::*;
    app::set_frame_type_cb(UpBox, up_box, 2, 2, 4, 4);
    app::set_frame_type_cb(UpFrame, up_frame, 2, 2, 4, 4);
    app::set_frame_type_cb(DownBox, down_box, 1, 1, 2, 2);
    app::set_frame_type_cb(DownFrame, down_frame, 1, 1, 2, 2);
    app::set_frame_type_cb(BorderBox, list_box, 1, 1, 2, 2);
    app::set_frame_type_cb(BorderFrame, list_frame, 1, 1, 2, 2);
    for (b, f) in [
        (ThinUpBox, ThinUpFrame),
        (ThinDownBox, ThinDownFrame),
        (EngravedBox, EngravedFrame),
        (EmbossedBox, EmbossedFrame),
    ] {
        app::set_frame_type_cb(b, panel_box, 1, 1, 2, 2);
        app::set_frame_type_cb(f, panel_frame, 1, 1, 2, 2);
    }
    app::set_frame_type_cb(DEFAULT_BUTTON_BOX, default_button_box, 2, 2, 4, 4);
    app::set_frame_type_cb(PRESSED_BUTTON_BOX, pressed_button_box, 2, 2, 4, 4);
    app::set_frame_type_cb(CHECK_BOX, check_box, 1, 1, 2, 2);
    app::set_frame_type_cb(RoundDownBox, radio_box, 1, 1, 2, 2);
}

// Every edge is a filled 1-pixel rectangle rather than a line, so that the
// Cairo backends (Linux, X11 and Wayland) cannot antialias it.

fn hline(x: i32, y: i32, w: i32, c: Color) {
    if w > 0 {
        draw::draw_rect_fill(x, y, w, 1, c);
    }
}

fn vline(x: i32, y: i32, h: i32, c: Color) {
    if h > 0 {
        draw::draw_rect_fill(x, y, 1, h, c);
    }
}

fn fill(x: i32, y: i32, w: i32, h: i32, c: Color) {
    if w > 0 && h > 0 {
        draw::draw_rect_fill(x, y, w, h, c);
    }
}

/// A one-pixel square outline.
fn outline(x: i32, y: i32, w: i32, h: i32, c: Color) {
    if w <= 0 || h <= 0 {
        return;
    }
    hline(x, y, w, c);
    hline(x, y + h - 1, w, c);
    vline(x, y + 1, h - 2, c);
    vline(x + w - 1, y + 1, h - 2, c);
}

/// A button shape: body and one-pixel outline with corners rounded by two
/// pixels. The four corner pixels outside the curve are left untouched.
fn rounded(x: i32, y: i32, w: i32, h: i32, body: Color, edge: Color, bottom: Color) {
    if w < 6 || h < 6 {
        fill(x, y, w, h, body);
        outline(x, y, w, h, edge);
        return;
    }
    // Body.
    fill(x + 2, y + 1, w - 4, h - 2, body);
    fill(x + 1, y + 2, w - 2, h - 4, body);
    // Edge.
    hline(x + 2, y, w - 4, edge);
    vline(x, y + 2, h - 4, edge);
    vline(x + w - 1, y + 2, h - 4, edge);
    hline(x + 2, y + h - 1, w - 4, bottom);
    fill(x + 1, y + 1, 1, 1, edge);
    fill(x + w - 2, y + 1, 1, 1, edge);
    fill(x + 1, y + h - 2, 1, 1, bottom);
    fill(x + w - 2, y + h - 2, 1, 1, bottom);
}

/// Raised: a push button in its normal, hover or disabled state, chosen by
/// its body colour.
pub fn draw_button(x: i32, y: i32, w: i32, h: i32, body: Color) {
    use colors::*;
    let (edge, bottom) = if body == BUTTON_HOVER {
        (ACCENT_BORDER, ACCENT_BORDER)
    } else if body == BUTTON_DISABLED {
        (BUTTON_BORDER_DISABLED, BUTTON_BORDER_DISABLED)
    } else {
        (BUTTON_BORDER, BUTTON_BORDER_BOTTOM)
    };
    rounded(x, y, w, h, body, edge, bottom);
}

/// Sunken: fields and combos.
pub fn draw_sunken(x: i32, y: i32, w: i32, h: i32) {
    outline(x, y, w, h, colors::FIELD_BORDER);
}

/// Panel: group boxes, status panels, tab pages.
pub fn draw_panel(x: i32, y: i32, w: i32, h: i32) {
    outline(x, y, w, h, colors::BORDER_LIGHT);
}

/// Horizontal separator, one pixel high.
pub fn draw_separator_h(x: i32, y: i32, w: i32) {
    hline(x, y, w, colors::BORDER_LIGHT);
}

/// Vertical separator, one pixel wide.
pub fn draw_separator_v(x: i32, y: i32, h: i32) {
    vline(x, y, h, colors::BORDER_LIGHT);
}

fn up_box(x: i32, y: i32, w: i32, h: i32, c: Color) {
    draw_button(x, y, w, h, c);
}

fn up_frame(x: i32, y: i32, w: i32, h: i32, _c: Color) {
    outline(x, y, w, h, colors::BUTTON_BORDER);
}

fn down_box(x: i32, y: i32, w: i32, h: i32, c: Color) {
    fill(x + 1, y + 1, w - 2, h - 2, c);
    draw_sunken(x, y, w, h);
}

fn down_frame(x: i32, y: i32, w: i32, h: i32, _c: Color) {
    draw_sunken(x, y, w, h);
}

fn list_box(x: i32, y: i32, w: i32, h: i32, c: Color) {
    fill(x + 1, y + 1, w - 2, h - 2, c);
    outline(x, y, w, h, colors::LIST_BORDER);
}

fn list_frame(x: i32, y: i32, w: i32, h: i32, _c: Color) {
    outline(x, y, w, h, colors::LIST_BORDER);
}

fn panel_box(x: i32, y: i32, w: i32, h: i32, c: Color) {
    fill(x + 1, y + 1, w - 2, h - 2, c);
    draw_panel(x, y, w, h);
}

fn panel_frame(x: i32, y: i32, w: i32, h: i32, _c: Color) {
    draw_panel(x, y, w, h);
}

fn default_button_box(x: i32, y: i32, w: i32, h: i32, c: Color) {
    use colors::*;
    rounded(x, y, w, h, c, ACCENT_BORDER, ACCENT_BORDER);
    // A second, inner rule makes the default button's edge read as heavier.
    if w > 8 && h > 8 && c != BUTTON_DISABLED {
        hline(x + 2, y + 1, w - 4, ACCENT_BORDER);
        hline(x + 2, y + h - 2, w - 4, ACCENT_BORDER);
        vline(x + 1, y + 2, h - 4, ACCENT_BORDER);
        vline(x + w - 2, y + 2, h - 4, ACCENT_BORDER);
    }
}

fn pressed_button_box(x: i32, y: i32, w: i32, h: i32, _c: Color) {
    use colors::*;
    rounded(x, y, w, h, BUTTON_PRESSED, PRESSED_BORDER, PRESSED_BORDER);
}

fn check_box(x: i32, y: i32, w: i32, h: i32, c: Color) {
    fill(x + 1, y + 1, w - 2, h - 2, c);
    outline(x, y, w, h, colors::CHECK_BORDER);
}

/// The radio button well: a fixed 13×13 bitmap, so it is pixel-identical on
/// every backend. `o` is the outline, `.` the inside.
const RADIO: [&str; 13] = [
    "    ooooo    ",
    "  oo.....oo  ",
    " o.........o ",
    " o.........o ",
    "o...........o",
    "o...........o",
    "o...........o",
    "o...........o",
    "o...........o",
    " o.........o ",
    " o.........o ",
    "  oo.....oo  ",
    "    ooooo    ",
];

fn radio_box(x: i32, y: i32, w: i32, h: i32, c: Color) {
    let n = RADIO.len() as i32;
    let ox = x + (w - n) / 2;
    let oy = y + (h - n) / 2;
    for (py, row) in RADIO.iter().enumerate() {
        let py = oy + py as i32;
        let first = row.find('.').map(|i| i as i32);
        let len = row.bytes().filter(|b| *b == b'.').count() as i32;
        if let Some(start) = first {
            draw::draw_rect_fill(ox + start, py, len, 1, c);
        }
        for (px, ch) in row.bytes().enumerate() {
            if ch == b'o' {
                draw::draw_rect_fill(ox + px as i32, py, 1, 1, colors::CHECK_BORDER);
            }
        }
    }
}

/// Styles a scroll bar: a flat track with a plain grey thumb.
pub fn style_scrollbar<S: fltk::prelude::ValuatorExt + fltk::prelude::WidgetExt>(s: &mut S) {
    s.set_frame(FrameType::FlatBox);
    s.set_color(colors::SCROLL_TRACK);
    s.set_selection_color(colors::SCROLL_THUMB);
}

/// Directory the bundled fonts are unpacked to. FLTK can only register a font
/// from a file, so the embedded copies are written out once per build.
fn font_dir() -> PathBuf {
    let stamp: usize = FONTS.iter().map(|f| f.data.len()).sum();
    std::env::temp_dir().join(format!("quartz-access-fonts-{stamp:x}"))
}

fn load_fonts() -> Result<(), String> {
    let dir = font_dir();
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let mut failed = Vec::new();
    for font in FONTS {
        let path = dir.join(font.file);
        let fresh = std::fs::metadata(&path)
            .map(|m| m.len() == font.data.len() as u64)
            .unwrap_or(false);
        if !fresh {
            // Write beside, then rename, so a second process never reads half a file.
            let tmp = path.with_extension(format!("{}.tmp", std::process::id()));
            if let Err(e) =
                std::fs::write(&tmp, font.data).and_then(|()| std::fs::rename(&tmp, &path))
            {
                let _ = std::fs::remove_file(&tmp);
                if !path.exists() {
                    failed.push(format!("{}: {e}", font.file));
                    continue;
                }
            }
        }
        if let Err(e) = Font::load_font(&path) {
            failed.push(format!("{}: {e}", font.file));
            continue;
        }
        #[cfg(windows)]
        let name = font.gdi_name;
        #[cfg(not(windows))]
        let name = font.pango_name;
        for slot in font.slots {
            Font::set_font(*slot, name);
        }
    }
    if failed.is_empty() {
        Ok(())
    } else {
        Err(failed.join("; "))
    }
}
