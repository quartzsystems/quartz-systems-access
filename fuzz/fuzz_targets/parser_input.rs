// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! The parser and the keyboard together: records from the host with key
//! events between them, through a `Terminal`.
//!
//! Input: a flags byte (`common::setup`), then steps:
//!
//! - `00`: a record (opcode, 16-bit length, data);
//! - `01`: a key, the next byte indexing `tn5250::input::KEYS`;
//! - `02`: a character, the next byte as Latin-1;
//! - `03`: a paste, a length byte and that many Latin-1 bytes;
//! - anything else: the key `KEYS[tag]`.

#![no_main]

#[path = "common.rs"]
mod common;

use libfuzzer_sys::fuzz_target;
use tn5250::input::{KEYS, Key};
use tn5250::terminal::Terminal;

fn latin1(b: &[u8]) -> String {
    b.iter().map(|&c| char::from(c)).collect()
}

fuzz_target!(|data: &[u8]| {
    let mut inp = common::Input::new(data);
    let (size, ccsid) = common::setup(inp.u8());
    let mut t = Terminal::new(size, ccsid);
    while !inp.done() {
        let tag = inp.u8();
        match tag {
            0 => {
                let rec = inp.record();
                for r in t.receive(&rec).replies {
                    let _ = r.encode();
                }
            }
            1 => {
                let k = KEYS[inp.u8() as usize % KEYS.len()];
                let _ = t.key(k);
            }
            2 => {
                let _ = t.key(Key::Char(char::from(inp.u8())));
            }
            3 => {
                let n = inp.u8() as usize;
                let text = latin1(inp.take(n));
                let _ = t.paste(&text);
            }
            other => {
                let _ = t.key(KEYS[other as usize % KEYS.len()]);
            }
        }
        common::check(&t.screen);
    }
});
