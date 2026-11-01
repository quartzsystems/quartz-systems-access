// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! Offscreen rendering of windows to RGB pixels, and a minimal PNG writer
//! and reader, for screenshots and screenshot tests.
//!
//! Windows are drawn into an image surface rather than read back from the
//! screen, so other windows, the desktop and the compositor never appear in
//! a capture.

use std::io;
use std::path::Path;

use fltk::prelude::*;
use fltk::surface::ImageSurface;

/// An RGB image, 8 bits per channel, row-major.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rgb {
    pub w: u32,
    pub h: u32,
    pub px: Vec<u8>,
}

impl Rgb {
    /// The pixel at (x, y).
    pub fn at(&self, x: u32, y: u32) -> [u8; 3] {
        let i = ((y * self.w + x) * 3) as usize;
        [self.px[i], self.px[i + 1], self.px[i + 2]]
    }
}

/// Renders a shown window, without its title bar, at 1:1 scale.
pub fn capture<W: WindowExt>(win: &W) -> Rgb {
    let (w, h) = (win.w(), win.h());
    let surf = ImageSurface::new(w, h, false);
    ImageSurface::push_current(&surf);
    fltk::draw::draw_rect_fill(0, 0, w, h, crate::theme::colors::FACE);
    surf.draw(win, 0, 0);
    let img = surf.image();
    ImageSurface::pop_current();
    let img = img.expect("image surface yields an image");
    let (iw, ih) = (img.data_w() as u32, img.data_h() as u32);
    let data = img.to_rgb_data();
    let depth = data.len() / (iw * ih) as usize;
    let px = if depth == 3 {
        data
    } else {
        data.chunks(depth)
            .flat_map(|p| [p[0], p[1], p[2]])
            .collect()
    };
    Rgb { w: iw, h: ih, px }
}

fn crc32(data: &[u8], mut crc: u32) -> u32 {
    crc = !crc;
    for &b in data {
        crc ^= b as u32;
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

fn adler32(data: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for chunk in data.chunks(5552) {
        for &x in chunk {
            a += x as u32;
            b += a;
        }
        a %= 65521;
        b %= 65521;
    }
    (b << 16) | a
}

fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    let start = out.len();
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let crc = crc32(&out[start..], 0);
    out.extend_from_slice(&crc.to_be_bytes());
}

/// Encodes an RGB image as PNG, uncompressed (stored deflate blocks).
pub fn encode_png(img: &Rgb) -> Vec<u8> {
    let stride = img.w as usize * 3;
    let mut raw = Vec::with_capacity((stride + 1) * img.h as usize);
    for row in img.px.chunks(stride) {
        raw.push(0);
        raw.extend_from_slice(row);
    }
    let mut z = vec![0x78, 0x01];
    let blocks: Vec<&[u8]> = raw.chunks(65535).collect();
    for (i, block) in blocks.iter().enumerate() {
        z.push(u8::from(i + 1 == blocks.len()));
        let n = block.len() as u16;
        z.extend_from_slice(&n.to_le_bytes());
        z.extend_from_slice(&(!n).to_le_bytes());
        z.extend_from_slice(block);
    }
    z.extend_from_slice(&adler32(&raw).to_be_bytes());

    let mut out = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&img.w.to_be_bytes());
    ihdr.extend_from_slice(&img.h.to_be_bytes());
    ihdr.extend_from_slice(&[8, 2, 0, 0, 0]);
    chunk(&mut out, b"IHDR", &ihdr);
    chunk(&mut out, b"IDAT", &z);
    chunk(&mut out, b"IEND", &[]);
    out
}

/// Writes an RGB image as PNG.
pub fn write_png(path: &Path, img: &Rgb) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(path, encode_png(img))
}

/// Reads a PNG written by [`encode_png`]. Any PNG FLTK can load is accepted.
pub fn read_png(path: &Path) -> io::Result<Rgb> {
    let img = fltk::image::PngImage::load(path)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, format!("{e}")))?;
    let (w, h) = (img.data_w() as u32, img.data_h() as u32);
    let data = img.to_rgb_data();
    let depth = data.len() / (w * h) as usize;
    let px = match depth {
        3 => data,
        4 => data.chunks(4).flat_map(|p| [p[0], p[1], p[2]]).collect(),
        1 => data.iter().flat_map(|&g| [g, g, g]).collect(),
        2 => data.chunks(2).flat_map(|p| [p[0], p[0], p[0]]).collect(),
        _ => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "unexpected pixel depth",
            ));
        }
    };
    Ok(Rgb { w, h, px })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc_and_adler_known_values() {
        assert_eq!(crc32(b"123456789", 0), 0xCBF4_3926);
        assert_eq!(adler32(b"Wikipedia"), 0x11E6_0398);
    }

    #[test]
    fn png_round_trip() {
        let img = Rgb {
            w: 300,
            h: 260,
            px: (0..300 * 260 * 3).map(|i| (i * 7 % 251) as u8).collect(),
        };
        let dir = std::env::temp_dir().join(format!("qsa-png-{}", std::process::id()));
        let path = dir.join("t.png");
        write_png(&path, &img).unwrap();
        let back = read_png(&path).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(back, img);
    }
}
