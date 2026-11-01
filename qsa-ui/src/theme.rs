// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! Scheme, colour table, box types and fonts.
//!
//! Everything here is set once, by [`init`], before the first widget is made.
//! Nothing reads the desktop theme: FLTK's system colours are never loaded and
//! every box is drawn by the functions below from the fixed palette.
//!
//! The look is light and flat: a pale grey face, white panels, one-pixel
//! outlines, and push buttons that turn pale blue with a blue edge under the
//! pointer.

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
    /// List, field and panel background.
    pub const WINDOW: Color = Color::from_rgb(0xFF, 0xFF, 0xFF);
    /// Normal text.
    pub const TEXT: Color = Color::from_rgb(0x00, 0x00, 0x00);
    /// Selection background.
    pub const SELECTION: Color = Color::from_rgb(0x2F, 0x6F, 0xD6);
    /// Text on the selection background.
    pub const SELECTION_TEXT: Color = Color::from_rgb(0xFF, 0xFF, 0xFF);
    /// Disabled text.
    pub const DISABLED_TEXT: Color = Color::from_rgb(0x80, 0x80, 0x80);
    /// Status word: active, running.
    pub const STATUS_ACTIVE: Color = Color::from_rgb(0x00, 0x80, 0x00);
    /// Status word: ended, error, not connected.
    pub const STATUS_ERROR: Color = Color::from_rgb(0xC0, 0x00, 0x00);
    /// The Quartz accent. Used once only: the rule in the About box.
    pub const QUARTZ_ACCENT: Color = Color::from_rgb(0x00, 0xD9, 0x92);
    /// Tooltip background.
    pub const TOOLTIP: Color = Color::from_rgb(0xFF, 0xFF, 0xE1);

    /// Push button body.
    pub const BUTTON: Color = Color::from_rgb(0xFD, 0xFD, 0xFD);
    /// Push button body while pressed.
    pub const BUTTON_PRESSED: Color = Color::from_rgb(0xCF, 0xE0, 0xF7);
    /// Push button body under the pointer.
    pub const BUTTON_HOVER: Color = Color::from_rgb(0xE6, 0xEF, 0xFB);
    /// Push button outline under the pointer or pressed.
    pub const BUTTON_HOT_BORDER: Color = Color::from_rgb(0x2F, 0x6F, 0xD6);
    /// Outline of fields, combos and lists.
    pub const FIELD_BORDER: Color = Color::from_rgb(0x7A, 0x7A, 0x7A);
    /// Outline of push buttons and tabs.
    pub const BORDER: Color = Color::from_rgb(0xAD, 0xAD, 0xAD);
    /// Outline of panels, separators and group boxes.
    pub const BORDER_LIGHT: Color = Color::from_rgb(0xD5, 0xD5, 0xD5);
    /// Outline of the default button.
    pub const BORDER_DEFAULT: Color = Color::from_rgb(0x2F, 0x6F, 0xD6);
    /// Light inner edge at the top and left of raised controls.
    pub const HIGHLIGHT: Color = Color::from_rgb(0xFF, 0xFF, 0xFF);
    /// Inactive tab body.
    pub const TAB_INACTIVE: Color = Color::from_rgb(0xE8, 0xE8, 0xE8);
    /// Task pane group header band.
    pub const TASK_HEADER: Color = Color::from_rgb(0xE3, 0xE6, 0xEE);
    /// Rule that divides sections of the task pane.
    pub const TASK_RULE: Color = Color::from_rgb(0x5A, 0x6E, 0x96);
    /// Task links.
    pub const LINK: Color = Color::from_rgb(0x1C, 0x46, 0x96);
    /// Background of a task link under the pointer or focus.
    pub const HOVER: Color = Color::from_rgb(0x2F, 0x6F, 0xD6);
    /// Text of a task link under the pointer or focus.
    pub const HOVER_TEXT: Color = Color::from_rgb(0xFF, 0xFF, 0xFF);
    /// Task link bullet.
    pub const BULLET: Color = Color::from_rgb(0xA4, 0xAB, 0xB5);
}

/// UI text size in pixels.
pub const UI_SIZE: i32 = 11;
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
/// UI sans, bold: group titles.
pub const UI_FONT_BOLD: Font = Font::HelveticaBold;
/// Monospace: anything that shows system output.
pub const MONO_FONT: Font = Font::Courier;
/// Monospace, bold.
pub const MONO_FONT_BOLD: Font = Font::CourierBold;

/// Box type of the default (Enter) button: a blue outline.
pub const DEFAULT_BUTTON_BOX: FrameType = FrameType::PlasticUpBox;
/// Box type of a pressed push button: pale blue body, blue outline.
pub const PRESSED_BUTTON_BOX: FrameType = FrameType::PlasticDownBox;

struct BundledFont {
    file: &'static str,
    data: &'static [u8],
    slots: &'static [Font],
    #[cfg_attr(not(windows), allow(dead_code))]
    gdi_name: &'static str,
    #[cfg_attr(windows, allow(dead_code))]
    pango_name: &'static str,
}

// Placeholders until the Quartz faces exist; see THIRD_PARTY.md.
const FONTS: &[BundledFont] = &[
    BundledFont {
        file: "NotoSans-Regular.ttf",
        data: include_bytes!("../fonts/NotoSans-Regular.ttf"),
        slots: &[Font::Helvetica],
        gdi_name: " Noto Sans",
        pango_name: "Noto Sans",
    },
    BundledFont {
        file: "NotoSans-Bold.ttf",
        data: include_bytes!("../fonts/NotoSans-Bold.ttf"),
        slots: &[Font::HelveticaBold],
        gdi_name: "BNoto Sans",
        pango_name: "Noto Sans Bold",
    },
    BundledFont {
        file: "NotoSansMono-Regular.ttf",
        data: include_bytes!("../fonts/NotoSansMono-Regular.ttf"),
        slots: &[Font::Courier, Font::Screen],
        gdi_name: " Noto Sans Mono",
        pango_name: "Noto Sans Mono",
    },
    BundledFont {
        file: "NotoSansMono-Bold.ttf",
        data: include_bytes!("../fonts/NotoSansMono-Bold.ttf"),
        slots: &[Font::CourierBold, Font::ScreenBold],
        gdi_name: "BNoto Sans Mono",
        pango_name: "Noto Sans Mono Bold",
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
        app::set_scrollbar_size(16);
        app::set_visible_focus(true);
        app::set_menu_linespacing(6);
        Tooltip::set_color(colors::TOOLTIP);
        Tooltip::set_text_color(colors::TEXT);
        Tooltip::set_font(UI_FONT);
        Tooltip::set_font_size(UI_SIZE);
        Tooltip::set_margin_width(4);
        Tooltip::set_margin_height(2);
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
        (Color::Light3, colors::HIGHLIGHT),
    ] {
        let (r, g, b) = colour.to_rgb();
        app::set_color(index, r, g, b);
    }
}

fn set_box_types() {
    use FrameType::*;
    app::set_frame_type_cb(UpBox, up_box, 1, 1, 2, 2);
    app::set_frame_type_cb(UpFrame, up_frame, 1, 1, 2, 2);
    app::set_frame_type_cb(DownBox, down_box, 1, 1, 2, 2);
    app::set_frame_type_cb(DownFrame, down_frame, 1, 1, 2, 2);
    app::set_frame_type_cb(BorderBox, down_box, 1, 1, 2, 2);
    app::set_frame_type_cb(BorderFrame, down_frame, 1, 1, 2, 2);
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
    app::set_frame_type_cb(PRESSED_BUTTON_BOX, pressed_button_box, 1, 1, 2, 2);
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

/// A one-pixel outline.
fn outline(x: i32, y: i32, w: i32, h: i32, c: Color) {
    if w <= 0 || h <= 0 {
        return;
    }
    hline(x, y, w, c);
    hline(x, y + h - 1, w, c);
    vline(x, y + 1, h - 2, c);
    vline(x + w - 1, y + 1, h - 2, c);
}

fn fill(x: i32, y: i32, w: i32, h: i32, c: Color) {
    if w > 0 && h > 0 {
        draw::draw_rect_fill(x, y, w, h, c);
    }
}

/// Raised: a flat push-button outline.
pub fn draw_raised(x: i32, y: i32, w: i32, h: i32) {
    outline(x, y, w, h, colors::BORDER);
}

/// Sunken: fields, combos and lists.
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
    fill(x + 1, y + 1, w - 2, h - 2, c);
    // A button under the pointer is given the hover body; its edge turns blue.
    if c == colors::BUTTON_HOVER {
        outline(x, y, w, h, colors::BUTTON_HOT_BORDER);
    } else {
        draw_raised(x, y, w, h);
    }
}

fn up_frame(x: i32, y: i32, w: i32, h: i32, _c: Color) {
    draw_raised(x, y, w, h);
}

fn down_box(x: i32, y: i32, w: i32, h: i32, c: Color) {
    fill(x + 1, y + 1, w - 2, h - 2, c);
    draw_sunken(x, y, w, h);
}

fn down_frame(x: i32, y: i32, w: i32, h: i32, _c: Color) {
    draw_sunken(x, y, w, h);
}

fn panel_box(x: i32, y: i32, w: i32, h: i32, c: Color) {
    fill(x + 1, y + 1, w - 2, h - 2, c);
    draw_panel(x, y, w, h);
}

fn panel_frame(x: i32, y: i32, w: i32, h: i32, _c: Color) {
    draw_panel(x, y, w, h);
}

fn default_button_box(x: i32, y: i32, w: i32, h: i32, c: Color) {
    fill(x + 1, y + 1, w - 2, h - 2, c);
    outline(x, y, w, h, colors::BORDER_DEFAULT);
}

fn pressed_button_box(x: i32, y: i32, w: i32, h: i32, _c: Color) {
    fill(x + 1, y + 1, w - 2, h - 2, colors::BUTTON_PRESSED);
    outline(x, y, w, h, colors::BUTTON_HOT_BORDER);
}

/// The radio button well: a fixed 12×12 bitmap, so it is pixel-identical on
/// every backend. `o` is the outline, `.` the inside.
const RADIO: [&str; 12] = [
    "    oooo    ",
    "  oo....oo  ",
    " o........o ",
    " o........o ",
    "o..........o",
    "o..........o",
    "o..........o",
    "o..........o",
    " o........o ",
    " o........o ",
    "  oo....oo  ",
    "    oooo    ",
];

fn radio_box(x: i32, y: i32, w: i32, h: i32, c: Color) {
    let ox = x + (w - 12) / 2;
    let oy = y + (h - 12) / 2;
    for (py, row) in RADIO.iter().enumerate() {
        let mut inside: Option<i32> = None;
        for (px, ch) in row.bytes().enumerate() {
            let px = px as i32;
            match ch {
                b'o' => draw::draw_rect_fill(ox + px, oy + py as i32, 1, 1, colors::BORDER),
                b'.' => {
                    inside.get_or_insert(px);
                }
                _ => {}
            }
        }
        if let Some(start) = inside {
            let len = row.bytes().filter(|b| *b == b'.').count() as i32;
            draw::draw_rect_fill(ox + start, oy + py as i32, len, 1, c);
        }
    }
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
