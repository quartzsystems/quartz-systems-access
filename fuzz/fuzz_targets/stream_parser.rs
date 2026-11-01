// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! The 5250 data stream parser fed a sequence of records.
//!
//! Input: a flags byte (`common::setup`), then records, each an opcode, a
//! 16-bit length and the data.

#![no_main]

#[path = "common.rs"]
mod common;

use libfuzzer_sys::fuzz_target;
use tn5250::screen::Screen;

fuzz_target!(|data: &[u8]| {
    let mut inp = common::Input::new(data);
    let (size, ccsid) = common::setup(inp.u8());
    let page = tn5250::codepage::by_ccsid(ccsid).unwrap();
    let mut s = Screen::new(size, page);
    while !inp.done() {
        let rec = inp.record();
        let a = tn5250::stream::apply(&mut s, &rec);
        for r in &a.replies {
            let _ = r.encode();
        }
        common::check(&s);
    }
    let _ = tn5250::replay::Golden::of(&s).to_text();
});
