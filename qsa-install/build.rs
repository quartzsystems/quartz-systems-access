// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! On Windows, embeds the icon, the version information and the manifest
//! (run as invoker) in the installer.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    qsa_icons::link_windows_res(
        qsa_glyph::Icon::App,
        &qsa_icons::product_version("QS/400 Access for Desktop Setup", "qsa-install.exe"),
    );
}
