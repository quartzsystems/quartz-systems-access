// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! Replays a recording through the QS/400 console's terminal and renders the
//! final screen with the console's scaler at scale 1 (720×384 for 24×80),
//! as an RGB PNG.
//!
//! `cargo run --release -- RECORDING.qsr OUT.png`
//!
//! `cargo run --release -- crop SCREENDUMP.ppm X0 Y0 OUT.png` cuts the 24×80 grid
//! (720×384 at scale 1) out of a QEMU screendump, at the origin the console
//! logged (`80x24 grid at scale 1 from (X0, Y0)`).

use console::Key;
use console::scale::{Canvas, Mode, PixelFormat, Scale};
use console::terminal::ConsoleTerminal;
use tn5250_qs400::record::Record;

fn field<'a>(line: &'a str, name: &str) -> Option<&'a str> {
    let key = format!("\"{name}\":\"");
    let rest = &line[line.find(&key)? + key.len()..];
    rest.find('"').map(|e| &rest[..e])
}

fn unhex(s: &str) -> Vec<u8> {
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).expect("hex"))
        .collect()
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("crop") {
        crop(&args[1..]);
        return;
    }
    let [input, output] = args.as_slice() else {
        eprintln!("usage: console-reference RECORDING.qsr OUT.png");
        std::process::exit(2);
    };
    let text = std::fs::read_to_string(input).expect("recording");
    let mut lines = text.lines();
    let head = lines.next().expect("header");
    assert!(head.contains("\"size\":\"24x80\""), "the console is 24x80 only");
    let mut term = ConsoleTerminal::new();
    for line in lines {
        match field(line, "d") {
            Some("in") => {
                let bytes = unhex(field(line, "x").expect("x"));
                let rec = Record::decode(&bytes).expect("record");
                term.apply(&rec);
            }
            Some("key") => match field(line, "k") {
                Some("Enter") => {
                    term.key(Key::Enter);
                }
                Some(k) => panic!("key {k} is not mapped here"),
                None => panic!("key without a name"),
            },
            _ => {}
        }
    }
    let screen = term.screen();
    let (w, h) = (80 * 9, 24 * 16);
    let mut canvas = Canvas::new(Mode::packed(w, h, PixelFormat::XRGB8888), 24, 80, Scale::Auto);
    canvas.draw(&screen);
    let mut px = Vec::with_capacity(w * h * 3);
    for y in 0..h {
        for x in 0..w {
            let p = canvas.pixel(x, y);
            px.extend_from_slice(&[(p >> 16) as u8, (p >> 8) as u8, p as u8]);
        }
    }
    std::fs::write(output, png(w as u32, h as u32, &px)).expect("write PNG");
    eprintln!("{output}: {w}x{h}");
}

/// Cuts the grid out of a binary PPM (P6, 8-bit).
fn crop(args: &[String]) {
    let [input, x0, y0, output] = args else {
        eprintln!("usage: console-reference crop SCREENDUMP.ppm X0 Y0 OUT.png");
        std::process::exit(2);
    };
    let (x0, y0): (usize, usize) = (x0.parse().expect("X0"), y0.parse().expect("Y0"));
    let data = std::fs::read(input).expect("screendump");
    let mut fields = Vec::new();
    let mut i = 0;
    while fields.len() < 4 {
        while data[i].is_ascii_whitespace() {
            i += 1;
        }
        if data[i] == b'#' {
            while data[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        let s = i;
        while !data[i].is_ascii_whitespace() {
            i += 1;
        }
        fields.push(String::from_utf8_lossy(&data[s..i]).into_owned());
    }
    i += 1;
    assert_eq!(fields[0], "P6", "binary PPM only");
    let (w, _h): (usize, usize) = (fields[1].parse().unwrap(), fields[2].parse().unwrap());
    assert_eq!(fields[3], "255", "8-bit PPM only");
    let (gw, gh) = (720, 384);
    let mut px = Vec::with_capacity(gw * gh * 3);
    for y in y0..y0 + gh {
        let row = i + (y * w + x0) * 3;
        px.extend_from_slice(&data[row..row + gw * 3]);
    }
    std::fs::write(output, png(gw as u32, gh as u32, &px)).expect("write PNG");
    eprintln!("{output}: {gw}x{gh} from ({x0}, {y0})");
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &b in data {
        crc ^= u32::from(b);
        for _ in 0..8 {
            crc = if crc & 1 != 0 { (crc >> 1) ^ 0xEDB8_8320 } else { crc >> 1 };
        }
    }
    !crc
}

/// An uncompressed (stored deflate) RGB PNG.
fn png(w: u32, h: u32, px: &[u8]) -> Vec<u8> {
    let stride = w as usize * 3;
    let mut raw = Vec::new();
    for row in px.chunks(stride) {
        raw.push(0);
        raw.extend_from_slice(row);
    }
    let (mut a, mut b) = (1u32, 0u32);
    for &x in &raw {
        a = (a + u32::from(x)) % 65521;
        b = (b + a) % 65521;
    }
    let mut z = vec![0x78, 0x01];
    let blocks: Vec<&[u8]> = raw.chunks(65535).collect();
    for (i, blk) in blocks.iter().enumerate() {
        z.push(u8::from(i + 1 == blocks.len()));
        let n = blk.len() as u16;
        z.extend_from_slice(&n.to_le_bytes());
        z.extend_from_slice(&(!n).to_le_bytes());
        z.extend_from_slice(blk);
    }
    z.extend_from_slice(&((b << 16) | a).to_be_bytes());
    let mut out = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    let mut chunk = |kind: &[u8; 4], data: &[u8]| {
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        let mut c = kind.to_vec();
        c.extend_from_slice(data);
        out.extend_from_slice(&c);
        out.extend_from_slice(&crc32(&c).to_be_bytes());
    };
    let mut ihdr = w.to_be_bytes().to_vec();
    ihdr.extend_from_slice(&h.to_be_bytes());
    ihdr.extend_from_slice(&[8, 2, 0, 0, 0]);
    chunk(b"IHDR", &ihdr);
    chunk(b"IDAT", &z);
    chunk(b"IEND", &[]);
    out
}
