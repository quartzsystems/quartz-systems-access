// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! Screenshot tests against committed goldens, shared by every crate that
//! has windows.
//!
//! Each view is rendered at 100 % scale off the screen and compared with the
//! golden for this platform in `<crate>/tests/goldens/<platform>[-<tag>]/`
//! (`QSA_SCREENSHOT_TAG` names a variant, for example `wayland`, whose text
//! antialiasing differs slightly).
//!
//! - A pixel differs when any channel is off by more than the tolerance
//!   (`QSA_SCREENSHOT_TOLERANCE`, default 40); a view fails when more than
//!   `QSA_SCREENSHOT_MAX_DIFF` per mille of its pixels differ (default 2).
//! - Captures, and a diff image for each failing view, are written to
//!   `target/screenshots/<platform>[-<tag>]/` (or `QSA_SCREENSHOT_OUT`).
//! - `QSA_BLESS=1` writes the captures as the new goldens.
//!
//! FLTK needs one UI thread, so the tests using this run as plain programs
//! (`harness = false`).

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use fltk::{app, prelude::*, window::Window};

use crate::snapshot::{self, Rgb};

/// `windows` or `linux`.
pub fn platform() -> &'static str {
    if cfg!(windows) { "windows" } else { "linux" }
}

/// `windows`, `linux`, or `linux-<tag>` when `QSA_SCREENSHOT_TAG` is set.
pub fn variant() -> String {
    match std::env::var("QSA_SCREENSHOT_TAG") {
        Ok(tag) if !tag.is_empty() => format!("{}-{tag}", platform()),
        _ => platform().to_owned(),
    }
}

/// Lets FLTK draw.
pub fn settle() {
    for _ in 0..15 {
        let _ = app::wait_for(0.02);
    }
}

/// Shows a window, lets it draw, captures it and hides it. The capture draws
/// offscreen, so the window's screen position does not matter; it must be
/// shown all the same, since Windows antialiases text differently for a
/// window that was never mapped.
///
/// The window goes beyond the right edge of every monitor, so it is never
/// seen and the real pointer cannot hover it. On Windows it is shown as a
/// tool window, which FLTK maps without activating it: the user's focus and
/// pointer stay where they were while the tests run.
pub fn render(mut win: Window) -> Rgb {
    let right = (0..app::screen_count())
        .map(|n| {
            let (x, _, w, _) = app::screen_xywh(n);
            x + w
        })
        .max()
        .unwrap_or_else(|| app::screen_size().0 as i32);
    win.set_pos(right + 200, 0);
    if cfg!(windows) {
        win.set_override();
    }
    win.show();
    settle();
    // FLTK may move focus to the first field as the window maps; keep parking
    // it on the window until it stays there.
    for _ in 0..20 {
        app::set_focus(&win);
        win.redraw();
        settle();
        if app::focus().is_some_and(|f| f.as_widget_ptr() == win.as_widget_ptr()) {
            break;
        }
    }
    let img = snapshot::capture(&win);
    win.hide();
    settle();
    img
}

/// Pixels that differ by more than `tol` in any channel, and an image that
/// marks them. `None` if the sizes differ.
pub fn diff(a: &Rgb, b: &Rgb, tol: u8) -> Option<(usize, Rgb)> {
    if (a.w, a.h) != (b.w, b.h) {
        return None;
    }
    let mut count = 0;
    let mut px = Vec::with_capacity(a.px.len());
    for (pa, pb) in a.px.chunks(3).zip(b.px.chunks(3)) {
        let off = pa.iter().zip(pb).any(|(x, y)| x.abs_diff(*y) > tol);
        if off {
            count += 1;
            px.extend_from_slice(&[0xFF, 0x00, 0x00]);
        } else {
            // Faded copy of the capture, so the red marks stand out.
            px.extend(pa.iter().map(|v| 0xC0 + v / 4));
        }
    }
    Some((count, Rgb { w: a.w, h: a.h, px }))
}

fn env_num(name: &str, default: u32) -> u32 {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

/// A screenshot run for one crate.
pub struct Goldens {
    goldens: PathBuf,
    out: PathBuf,
    bless: bool,
    tol: u8,
    max_per_mille: usize,
    failed: Vec<String>,
}

impl Goldens {
    /// Sets up FLTK for screenshots (scale 1, no hover effects, the house
    /// fonts) and a run for the crate whose manifest folder is
    /// `manifest_dir`. `None` (after saying why) if the fonts would not load.
    pub fn start(manifest_dir: &Path) -> Option<(app::App, Goldens)> {
        crate::theme::pin_font_rendering();
        let a = app::App::default();
        app::set_screen_scale(0, 1.0);
        crate::theme::set_hover_effects(false);
        if let Err(e) = crate::theme::init() {
            eprintln!("screenshots: fonts not loaded: {e}");
            return None;
        }
        let out = match std::env::var("QSA_SCREENSHOT_OUT") {
            Ok(d) => PathBuf::from(d),
            Err(_) => manifest_dir
                .join("..")
                .join("target")
                .join("screenshots")
                .join(variant()),
        };
        Some((
            a,
            Goldens {
                goldens: manifest_dir.join("tests").join("goldens").join(variant()),
                out,
                bless: std::env::var("QSA_BLESS").is_ok_and(|v| v == "1"),
                tol: env_num("QSA_SCREENSHOT_TOLERANCE", 40) as u8,
                max_per_mille: env_num("QSA_SCREENSHOT_MAX_DIFF", 2) as usize,
                failed: Vec::new(),
            },
        ))
    }

    /// Whether the test was only asked to list itself (`cargo test --
    /// --list`), so should do nothing.
    pub fn listing(name: &str) -> bool {
        if std::env::args().any(|a| a == "--list") {
            println!("{name}: test");
            return true;
        }
        false
    }

    /// Renders one view and compares it with its golden (or blesses it).
    pub fn check(&mut self, name: &str, win: Window) {
        let img = render(win);
        let file = format!("{name}.png");
        if let Err(e) = snapshot::write_png(&self.out.join(&file), &img) {
            println!("FAILED   {name}: {}: {e}", self.out.join(&file).display());
            self.failed.push(name.into());
            return;
        }
        let golden = self.goldens.join(&file);
        if self.bless {
            match snapshot::write_png(&golden, &img) {
                Ok(()) => println!("blessed  {name}"),
                Err(e) => {
                    println!("FAILED   {name}: {}: {e}", golden.display());
                    self.failed.push(name.into());
                }
            }
            return;
        }
        let Ok(want) = snapshot::read_png(&golden) else {
            println!("MISSING  {name}: no golden at {}", golden.display());
            self.failed.push(name.into());
            return;
        };
        match diff(&img, &want, self.tol) {
            None => {
                println!(
                    "FAILED   {name}: size {}x{}, golden {}x{}",
                    img.w, img.h, want.w, want.h
                );
                self.failed.push(name.into());
            }
            Some((n, d)) => {
                let total = (img.w * img.h) as usize;
                let per_mille = n * 1000 / total.max(1);
                if n * 1000 > self.max_per_mille * total {
                    let _ = snapshot::write_png(&self.out.join(format!("{name}.diff.png")), &d);
                    println!("FAILED   {name}: {n} pixels differ ({per_mille}\u{2030})");
                    self.failed.push(name.into());
                } else {
                    println!("ok       {name}: {n} pixels differ");
                }
            }
        }
    }

    /// Reports the run.
    pub fn finish(self) -> ExitCode {
        println!("screenshots written to {}", self.out.display());
        if self.failed.is_empty() {
            ExitCode::SUCCESS
        } else {
            println!(
                "{} view(s) failed: {}",
                self.failed.len(),
                self.failed.join(", ")
            );
            println!("If the change is intended, run with QSA_BLESS=1 and commit the goldens.");
            ExitCode::FAILURE
        }
    }
}
