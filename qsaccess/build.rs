// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! Makes the Help topics (`build/help.rs`), and on Windows embeds the
//! application icon, the version information and the manifest in
//! `qsaccess.exe`.

use std::path::PathBuf;

#[path = "build/help.rs"]
mod help;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=build/help.rs");
    let here = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").expect("set by cargo"));
    let out = PathBuf::from(std::env::var_os("OUT_DIR").expect("set by cargo"));
    help::build(&here.join("help"), &out.join("help.rs"));
    qsa_icons::link_windows_res(
        qsa_glyph::Icon::App,
        &qsa_icons::product_version("QS/400 Access for Desktop", "qsaccess.exe"),
    );
}
