// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! Icon files and Windows resources, made at build time from the glyphs in
//! [`qsa_glyph`]:
//!
//! - [`png`]: one PNG;
//! - [`ico`]: a Windows `.ico` with several sizes;
//! - [`windows_res`]: a compiled resource file (`.res`) holding the icon,
//!   the version information and the application manifest. The MSVC linker
//!   takes it as an input, so no resource compiler is needed.

use qsa_glyph::{Icon, icon_rgba};

/// Sizes in a Windows `.ico`.
pub const ICO_SIZES: [u32; 4] = [16, 32, 48, 256];
/// Sizes written for the Linux hicolor icon theme.
pub const HICOLOR_SIZES: [u32; 5] = [16, 32, 48, 128, 256];

/// An icon as PNG, `px` square.
pub fn png(icon: Icon, px: u32) -> Vec<u8> {
    encode_png(&icon_rgba(icon, px as usize), px)
}

fn encode_png(rgba: &[u8], px: u32) -> Vec<u8> {
    let mut out = Vec::new();
    {
        let mut e = png::Encoder::new(&mut out, px, px);
        e.set_color(png::ColorType::Rgba);
        e.set_depth(png::BitDepth::Eight);
        let mut w = e.write_header().expect("PNG header to a Vec");
        w.write_image_data(rgba).expect("PNG data to a Vec");
    }
    out
}

/// An icon as a Windows `.ico` at [`ICO_SIZES`].
pub fn ico(icon: Icon) -> Vec<u8> {
    let mut dir = ::ico::IconDir::new(::ico::ResourceType::Icon);
    for px in ICO_SIZES {
        let img = ::ico::IconImage::from_rgba_data(px, px, icon_rgba(icon, px as usize));
        dir.add_entry(::ico::IconDirEntry::encode(&img).expect("icon entry"));
    }
    let mut out = Vec::new();
    dir.write(&mut out).expect("icon to a Vec");
    out
}

/// What the version resource says.
#[derive(Clone, Debug)]
pub struct VersionInfo {
    pub product: String,
    pub company: String,
    pub description: String,
    /// Major, minor, patch, build.
    pub version: [u16; 4],
    /// The product version as people read it, for example
    /// `1.1.1 (V1R1M1)`.
    pub version_text: String,
    pub internal_name: String,
    pub original_filename: String,
    pub copyright: String,
}

/// The manifest every binary carries: run with the rights of whoever
/// starts it. Without one, Windows guesses from a name such as
/// `uninstall.exe` that the program needs administrator rights.
pub const MANIFEST: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <trustInfo xmlns="urn:schemas-microsoft-com:asm.v3">
    <security>
      <requestedPrivileges>
        <requestedExecutionLevel level="asInvoker" uiAccess="false"/>
      </requestedPrivileges>
    </security>
  </trustInfo>
  <compatibility xmlns="urn:schemas-microsoft-com:compatibility.v1">
    <application>
      <supportedOS Id="{8e0f7a12-bfb3-4fe8-b9a5-48fd50a15a9a}"/>
    </application>
  </compatibility>
</assembly>
"#;

const RT_ICON: u16 = 3;
const RT_GROUP_ICON: u16 = 14;
const RT_VERSION: u16 = 16;
const RT_MANIFEST: u16 = 24;
const LANG_EN_US: u16 = 0x0409;

fn pad4(v: &mut Vec<u8>) {
    while !v.len().is_multiple_of(4) {
        v.push(0);
    }
}

fn put16(v: &mut Vec<u8>, x: u16) {
    v.extend_from_slice(&x.to_le_bytes());
}

fn put32(v: &mut Vec<u8>, x: u32) {
    v.extend_from_slice(&x.to_le_bytes());
}

fn wide(v: &mut Vec<u8>, s: &str) {
    for u in s.encode_utf16() {
        put16(v, u);
    }
    put16(v, 0);
}

/// One resource: a 32-byte header with ordinal type and name, then the
/// data, padded to four bytes.
fn resource(out: &mut Vec<u8>, kind: u16, id: u16, flags: u16, lang: u16, data: &[u8]) {
    put32(out, data.len() as u32);
    put32(out, 32);
    put16(out, 0xFFFF);
    put16(out, kind);
    put16(out, 0xFFFF);
    put16(out, id);
    put32(out, 0); // data version
    put16(out, flags);
    put16(out, lang);
    put32(out, 0); // version
    put32(out, 0); // characteristics
    out.extend_from_slice(data);
    pad4(out);
}

/// One node of a version resource: length, value length, type, key, then
/// the value and the children, each starting on a four-byte boundary.
fn node(key: &str, value: &[u8], value_len: u16, text: bool, children: &[Vec<u8>]) -> Vec<u8> {
    let mut v = Vec::new();
    put16(&mut v, 0);
    put16(&mut v, value_len);
    put16(&mut v, u16::from(text));
    wide(&mut v, key);
    pad4(&mut v);
    v.extend_from_slice(value);
    for c in children {
        pad4(&mut v);
        v.extend_from_slice(c);
    }
    let len = v.len() as u16;
    v[0..2].copy_from_slice(&len.to_le_bytes());
    v
}

fn string(key: &str, value: &str) -> Vec<u8> {
    let mut val = Vec::new();
    wide(&mut val, value);
    node(key, &val, (val.len() / 2) as u16, true, &[])
}

/// The `VS_VERSIONINFO` structure.
pub fn version_resource(v: &VersionInfo) -> Vec<u8> {
    let [a, b, c, d] = v.version.map(u32::from);
    let mut fixed = Vec::new();
    put32(&mut fixed, 0xFEEF_04BD);
    put32(&mut fixed, 0x0001_0000);
    put32(&mut fixed, (a << 16) | b);
    put32(&mut fixed, (c << 16) | d);
    put32(&mut fixed, (a << 16) | b);
    put32(&mut fixed, (c << 16) | d);
    put32(&mut fixed, 0x3F); // file flags mask
    put32(&mut fixed, 0); // file flags
    put32(&mut fixed, 0x0004_0004); // VOS_NT_WINDOWS32
    put32(&mut fixed, 1); // VFT_APP
    put32(&mut fixed, 0);
    put32(&mut fixed, 0);
    put32(&mut fixed, 0);
    let dotted = format!("{a}.{b}.{c}.{d}");
    let strings: Vec<Vec<u8>> = [
        ("CompanyName", v.company.as_str()),
        ("FileDescription", v.description.as_str()),
        ("FileVersion", dotted.as_str()),
        ("InternalName", v.internal_name.as_str()),
        ("LegalCopyright", v.copyright.as_str()),
        ("OriginalFilename", v.original_filename.as_str()),
        ("ProductName", v.product.as_str()),
        ("ProductVersion", v.version_text.as_str()),
    ]
    .iter()
    .map(|(k, s)| string(k, s))
    .collect();
    // US English, Unicode.
    let table = node("040904b0", &[], 0, true, &strings);
    let sfi = node("StringFileInfo", &[], 0, true, &[table]);
    let mut translation = Vec::new();
    put16(&mut translation, LANG_EN_US);
    put16(&mut translation, 0x04B0);
    let var = node("Translation", &translation, 4, false, &[]);
    let vfi = node("VarFileInfo", &[], 0, true, &[var]);
    node(
        "VS_VERSION_INFO",
        &fixed,
        fixed.len() as u16,
        false,
        &[sfi, vfi],
    )
}

/// A `.res` file with `icon` (as icon group 1, so Explorer shows it for the
/// file), the version information and [`MANIFEST`].
pub fn windows_res(icon: Icon, version: &VersionInfo) -> Vec<u8> {
    let mut out = Vec::new();
    // The empty resource every .res file starts with.
    resource(&mut out, 0, 0, 0, 0, &[]);
    let mut group = Vec::new();
    put16(&mut group, 0);
    put16(&mut group, 1);
    put16(&mut group, ICO_SIZES.len() as u16);
    for (i, px) in ICO_SIZES.iter().enumerate() {
        let data = png(icon, *px);
        let id = i as u16 + 1;
        resource(&mut out, RT_ICON, id, 0x1010, LANG_EN_US, &data);
        let side = if *px >= 256 { 0 } else { *px as u8 };
        group.push(side);
        group.push(side);
        group.push(0); // colours
        group.push(0);
        put16(&mut group, 1); // planes
        put16(&mut group, 32); // bits per pixel
        put32(&mut group, data.len() as u32);
        put16(&mut group, id);
    }
    resource(&mut out, RT_GROUP_ICON, 1, 0x1030, LANG_EN_US, &group);
    resource(
        &mut out,
        RT_VERSION,
        1,
        0x0030,
        LANG_EN_US,
        &version_resource(version),
    );
    resource(
        &mut out,
        RT_MANIFEST,
        1,
        0x0030,
        LANG_EN_US,
        MANIFEST.as_bytes(),
    );
    out
}

/// The version information of QS/400 Access for Desktop's binaries: the
/// version in VERSION, through the workspace version, and its release.
pub fn product_version(description: &str, file: &str) -> VersionInfo {
    let part = |s: &str| s.parse::<u16>().expect("version parts fit in 16 bits");
    let (major, minor, patch) = (
        env!("CARGO_PKG_VERSION_MAJOR"),
        env!("CARGO_PKG_VERSION_MINOR"),
        env!("CARGO_PKG_VERSION_PATCH"),
    );
    VersionInfo {
        product: "QS/400 Access for Desktop".into(),
        company: "Quartz Systems".into(),
        description: description.into(),
        version: [part(major), part(minor), part(patch), 0],
        version_text: format!("{major}.{minor}.{patch} (V{major}R{minor}M{patch})"),
        internal_name: file.trim_end_matches(".exe").into(),
        original_filename: file.into(),
        copyright: "(C) 2026 Quartz Systems".into(),
    }
}

/// Writes a `.res` for a binary into `OUT_DIR` and tells Cargo to link it
/// into the crate's binaries, when building for Windows with MSVC. For
/// build scripts.
pub fn link_windows_res(icon: Icon, version: &VersionInfo) {
    let os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let env = std::env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
    if os != "windows" || env != "msvc" {
        return;
    }
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR"));
    let path = out.join("resources.res");
    std::fs::write(&path, windows_res(icon, version)).expect("write resources.res");
    println!("cargo:rustc-link-arg-bins={}", path.display());
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Splits a `.res` into (type, name, data).
    fn resources(res: &[u8]) -> Vec<(u16, u16, Vec<u8>)> {
        let mut out = Vec::new();
        let mut i = 0;
        let u16_at = |i: usize| u16::from_le_bytes([res[i], res[i + 1]]);
        let u32_at = |i: usize| u32::from_le_bytes([res[i], res[i + 1], res[i + 2], res[i + 3]]);
        while i < res.len() {
            let size = u32_at(i) as usize;
            let header = u32_at(i + 4) as usize;
            assert_eq!(header, 32);
            out.push((
                u16_at(i + 10),
                u16_at(i + 14),
                res[i + 32..i + 32 + size].to_vec(),
            ));
            i += 32 + size;
            i = i.div_ceil(4) * 4;
        }
        out
    }

    #[test]
    fn resource_file_holds_icons_version_and_manifest() {
        let v = product_version("QS/400 Access for Desktop", "qsaccess.exe");
        let res = windows_res(Icon::App, &v);
        let r = resources(&res);
        let kinds: Vec<u16> = r.iter().map(|x| x.0).collect();
        assert_eq!(kinds, [0, 3, 3, 3, 3, 14, 16, 24]);
        for (_, _, data) in r.iter().filter(|x| x.0 == RT_ICON) {
            assert_eq!(&data[..8], b"\x89PNG\r\n\x1a\n");
        }
        let group = &r[5].2;
        assert_eq!(&group[..6], &[0, 0, 1, 0, 4, 0]);
        assert_eq!(group[6 + 3 * 14], 0, "256 is written as 0");
        let version = &r[6].2;
        assert_eq!(
            u16::from_le_bytes([version[0], version[1]]) as usize,
            version.len()
        );
        let text: String = version
            .chunks(2)
            .map(|c| char::from_u32(u16::from_le_bytes([c[0], c[1]]).into()).unwrap_or('?'))
            .collect();
        let file_version = format!("{}.0", env!("CARGO_PKG_VERSION"));
        for want in [
            "VS_VERSION_INFO",
            "QS/400 Access for Desktop",
            "Quartz Systems",
            v.version_text.as_str(),
            file_version.as_str(),
            "Translation",
        ] {
            assert!(text.contains(want), "{want} missing");
        }
        assert!(String::from_utf8_lossy(&r[7].2).contains("asInvoker"));
    }

    #[test]
    fn ico_and_png_files() {
        let i = ico(Icon::Session);
        assert_eq!(&i[..6], &[0, 0, 1, 0, 4, 0]);
        let p = png(Icon::Macro, 48);
        assert_eq!(&p[..8], b"\x89PNG\r\n\x1a\n");
    }
}
