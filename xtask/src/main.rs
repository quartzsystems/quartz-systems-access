// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! Build tasks. Run with `cargo xtask <task>`.
//!
//! - `icons [--out DIR]`: the application and file-type icons, from the
//!   code-drawn glyph set: `.ico` files (16, 32, 48 and 256 pixels) for
//!   Windows and PNGs (16, 32, 48, 128 and 256) for the Linux hicolor
//!   theme. Written to `target/dist/icons` unless `--out` says otherwise.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use qsa_glyph::Icon;

mod icons {
    use super::*;

    /// Writes every icon under `out`:
    /// `windows/NAME.ico` and `hicolor/NxN/{apps,mimetypes}/NAME.png`.
    pub fn write(out: &Path) -> std::io::Result<Vec<PathBuf>> {
        let mut written = Vec::new();
        let win = out.join("windows");
        std::fs::create_dir_all(&win)?;
        for icon in Icon::ALL {
            let p = win.join(format!("{}.ico", icon.name()));
            std::fs::write(&p, qsa_icons::ico(icon))?;
            written.push(p);
        }
        for px in qsa_icons::HICOLOR_SIZES {
            for icon in Icon::ALL {
                let context = if icon == Icon::App {
                    "apps"
                } else {
                    "mimetypes"
                };
                let dir = out.join("hicolor").join(format!("{px}x{px}")).join(context);
                std::fs::create_dir_all(&dir)?;
                let p = dir.join(format!("{}.png", icon.name()));
                std::fs::write(&p, qsa_icons::png(icon, px))?;
                written.push(p);
            }
        }
        Ok(written)
    }
}

/// The workspace root.
fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask is in the workspace")
        .to_owned()
}

fn value(args: &[String], name: &str) -> Option<String> {
    args.windows(2).find(|w| w[0] == name).map(|w| w[1].clone())
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("icons") => {
            let out = value(&args, "--out")
                .map(PathBuf::from)
                .unwrap_or_else(|| root().join("target/dist/icons"));
            match icons::write(&out) {
                Ok(files) => {
                    println!("{} icon files written to {}", files.len(), out.display());
                    ExitCode::SUCCESS
                }
                Err(e) => {
                    eprintln!("icons: {e}");
                    ExitCode::FAILURE
                }
            }
        }
        _ => {
            eprintln!("usage: cargo xtask icons [--out DIR]");
            ExitCode::from(2)
        }
    }
}
