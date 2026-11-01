// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! The glyph set (drawn in code by [`qsa_glyph`]) as FLTK images.

use fltk::enums::ColorDepth;
use fltk::image::RgbImage;

pub use qsa_glyph::{Glyph, Rgb, Size, palette, rgba};

/// The glyph as an FLTK image, ready for buttons, menus and frames.
pub fn image(glyph: Glyph, size: Size) -> RgbImage {
    let s = size.px();
    RgbImage::new(&rgba(glyph, size), s, s, ColorDepth::Rgba8)
        .expect("glyph buffer is always the right size")
}

/// The application icon, for title bars and the taskbar.
pub fn app_icon() -> RgbImage {
    let s = 32;
    RgbImage::new(
        &qsa_glyph::icon_rgba(qsa_glyph::Icon::App, s as usize),
        s,
        s,
        ColorDepth::Rgba8,
    )
    .expect("icon buffer is always the right size")
}
