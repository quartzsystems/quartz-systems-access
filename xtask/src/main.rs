// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! Build tasks. Run with `cargo xtask <task>`.
//!
//! - `icons [--out DIR]`: the application and file-type icons, from the
//!   code-drawn glyph set: `.ico` files (16, 32, 48 and 256 pixels) for
//!   Windows and PNGs (16, 32, 48, 128 and 256) for the Linux hicolor
//!   theme. Written to `target/dist/icons` unless `--out` says otherwise.
//! - `dist [--skip-build]`: the installer for this platform. Builds the
//!   release `qsaccess` and `qsa-install`, packs `qsaccess`, the icons and
//!   the licences into a zstd tar, and appends it to the `qsa-install`
//!   program with a footer (magic, offset, length, SHA-256). Writes
//!   `target/dist/QS400-Access-V1R0M0-<os>-x86_64[.exe]`. `--skip-build`
//!   uses release binaries already built.

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

mod dist {
    use super::*;
    use qsa_install::payload::{self, Entry};

    /// The platform name in the installer's file name.
    pub fn os() -> &'static str {
        if cfg!(windows) { "windows" } else { "linux" }
    }

    /// The installer's file name.
    pub fn file_name() -> String {
        format!(
            "QS400-Access-{}-{}-x86_64{}",
            qsa_install::RELEASE,
            os(),
            std::env::consts::EXE_SUFFIX
        )
    }

    /// What goes into the payload, as (path inside, file, executable), and
    /// generated icon files.
    pub fn entries(root: &Path, release: &Path) -> std::io::Result<Vec<Entry>> {
        let read = |p: &Path| {
            std::fs::read(p)
                .map_err(|e| std::io::Error::new(e.kind(), format!("{}: {e}", p.display())))
        };
        let exe = format!("qsaccess{}", std::env::consts::EXE_SUFFIX);
        let licence = if cfg!(windows) {
            "LICENSE.txt"
        } else {
            "LICENSE"
        };
        let mut out = vec![
            Entry {
                path: exe.clone(),
                data: read(&release.join(&exe))?,
                executable: true,
            },
            Entry {
                path: licence.into(),
                data: read(&root.join("LICENSE"))?,
                executable: false,
            },
            Entry {
                path: "THIRD_PARTY.md".into(),
                data: read(&root.join("THIRD_PARTY.md"))?,
                executable: false,
            },
        ];
        if cfg!(windows) {
            for icon in Icon::ALL {
                out.push(Entry {
                    path: format!("icons/{}.ico", icon.name()),
                    data: qsa_icons::ico(icon),
                    executable: false,
                });
            }
        } else {
            for px in qsa_icons::HICOLOR_SIZES {
                for icon in Icon::ALL {
                    let context = if icon == Icon::App {
                        "apps"
                    } else {
                        "mimetypes"
                    };
                    out.push(Entry {
                        path: format!("icons/hicolor/{px}x{px}/{context}/{}.png", icon.name()),
                        data: qsa_icons::png(icon, px),
                        executable: false,
                    });
                }
            }
        }
        Ok(out)
    }

    pub fn run(skip_build: bool) -> Result<PathBuf, String> {
        let root = root();
        if !skip_build {
            let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
            let status = std::process::Command::new(cargo)
                .current_dir(&root)
                .args(["build", "--release", "-p", "qsaccess", "-p", "qsa-install"])
                .status()
                .map_err(|e| format!("cargo: {e}"))?;
            if !status.success() {
                return Err("the release build failed".into());
            }
        }
        let target = std::env::var_os("CARGO_TARGET_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| root.join("target"));
        let release = target.join("release");
        let entries = entries(&root, &release).map_err(|e| e.to_string())?;
        let payload = payload::pack(&entries).map_err(|e| e.to_string())?;
        let stub_path = release.join(format!("qsa-install{}", std::env::consts::EXE_SUFFIX));
        let stub =
            std::fs::read(&stub_path).map_err(|e| format!("{}: {e}", stub_path.display()))?;
        let out_dir = target.join("dist");
        std::fs::create_dir_all(&out_dir).map_err(|e| e.to_string())?;
        let out = out_dir.join(file_name());
        let bytes = payload::assemble(&stub, &payload);
        payload::write_installer(&out, &bytes).map_err(|e| e.to_string())?;
        payload::verify(&out).map_err(|e| format!("the installer written does not verify: {e}"))?;
        println!(
            "{}: {} bytes (program {}, payload {} compressed from {} files)",
            out.display(),
            bytes.len(),
            stub.len(),
            payload.len(),
            entries.len()
        );
        Ok(out)
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
        Some("dist") => match dist::run(args.iter().any(|a| a == "--skip-build")) {
            Ok(_) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("dist: {e}");
                ExitCode::FAILURE
            }
        },
        _ => {
            eprintln!("usage: cargo xtask icons [--out DIR] | dist [--skip-build]");
            ExitCode::from(2)
        }
    }
}
