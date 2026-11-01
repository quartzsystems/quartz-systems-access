// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! Compiles `codepages/cpNNN.txt` into byte-to-char and char-to-byte tables.

use std::env;
use std::fmt::Write as _;
use std::fs;
use std::path::Path;

/// CCSIDs with a data file, in the order they are listed.
const CCSIDS: &[u16] = &[37, 273, 285, 297, 500, 1140];

fn main() {
    let mut out = String::new();
    for &ccsid in CCSIDS {
        let path = format!("codepages/cp{ccsid}.txt");
        println!("cargo:rerun-if-changed={path}");
        let text = fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
        let table = parse(&path, &text);
        let mut from: Vec<(u32, u8)> = (0..=255u8).map(|b| (table[b as usize], b)).collect();
        from.sort_unstable();
        for w in from.windows(2) {
            assert!(
                w[0].0 != w[1].0,
                "{path}: U+{:04X} is mapped from both {:02X} and {:02X}",
                w[0].0,
                w[0].1,
                w[1].1
            );
        }
        write!(out, "static TO_{ccsid}: [char; 256] = [").unwrap();
        for cp in table {
            write!(out, "'\\u{{{cp:x}}}',").unwrap();
        }
        writeln!(out, "];").unwrap();
        write!(out, "static FROM_{ccsid}: [(char, u8); 256] = [").unwrap();
        for (cp, b) in from {
            write!(out, "('\\u{{{cp:x}}}', {b}),").unwrap();
        }
        writeln!(out, "];").unwrap();
    }
    writeln!(
        out,
        "type Entry = (u16, &'static [char; 256], &'static [(char, u8); 256]);"
    )
    .unwrap();
    write!(out, "static TABLES: &[Entry] = &[").unwrap();
    for &ccsid in CCSIDS {
        write!(out, "({ccsid}, &TO_{ccsid}, &FROM_{ccsid}),").unwrap();
    }
    writeln!(out, "];").unwrap();
    let dest = Path::new(&env::var("OUT_DIR").expect("OUT_DIR")).join("codepages.rs");
    fs::write(dest, out).expect("write codepages.rs");
}

/// 256 lines `BB UUUU`, in byte order; `#` lines are comments.
fn parse(path: &str, text: &str) -> [u32; 256] {
    let mut table = [u32::MAX; 256];
    let mut next = 0usize;
    for (n, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut parts = line.split_whitespace();
        let (Some(b), Some(u), None) = (parts.next(), parts.next(), parts.next()) else {
            panic!("{path}:{}: expected 'BB UUUU'", n + 1);
        };
        let b = usize::from_str_radix(b, 16).unwrap_or_else(|_| panic!("{path}:{}: byte", n + 1));
        let u =
            u32::from_str_radix(u, 16).unwrap_or_else(|_| panic!("{path}:{}: code point", n + 1));
        assert!(b == next, "{path}:{}: byte {b:02X} out of order", n + 1);
        assert!(char::from_u32(u).is_some(), "{path}:{}: U+{u:04X}", n + 1);
        table[b] = u;
        next += 1;
    }
    assert!(next == 256, "{path}: {next} bytes, want 256");
    table
}
