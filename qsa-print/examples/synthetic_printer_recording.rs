// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! Writes a synthetic printer session recording: the built-in test page as
//! a server would send it (print records of at most 1000 bytes, the first
//! and last of the chain marked, then the null print record), each answered
//! with print complete. It stands in for a recording from a real system in
//! `fixtures/replay/`.
//!
//! `cargo run -p qsa-print --example synthetic_printer_recording -- FILE`

use tn5250::printer::{Response, flag, flow, op};
use tn5250::replay::Recorder;
use tn5250::session::Direction;

fn print_record(flags: u16, data: &[u8]) -> Vec<u8> {
    let mut r = vec![0, 0, 0x12, 0xA0];
    r.extend_from_slice(&flow::FROM_SERVER.to_be_bytes());
    r.push(10);
    r.extend_from_slice(&flags.to_be_bytes());
    r.push(op::PRINT);
    r.extend_from_slice(&[0; 6]);
    r.extend_from_slice(data);
    let len = r.len() as u16;
    r[..2].copy_from_slice(&len.to_be_bytes());
    r
}

fn main() {
    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "fixtures/replay/synthetic-printer-test-page.qsr".into());
    let mut rec = Recorder::create_printer(std::path::Path::new(&path), "QSAPRT01", 37)
        .expect("recording file");
    let job = scs::sample::test_page();
    let chunks: Vec<&[u8]> = job.chunks(1000).collect();
    let complete = Response::Complete.encode(op::PRINT);
    for (i, c) in chunks.iter().enumerate() {
        let mut flags = 0;
        if i == 0 {
            flags |= flag::FIRST_OF_CHAIN;
        }
        if i == chunks.len() - 1 {
            flags |= flag::LAST_OF_CHAIN;
        }
        rec.record(Direction::In, &print_record(flags, c));
        rec.record(Direction::Out, &complete);
    }
    rec.record(Direction::In, &print_record(flag::LAST_OF_CHAIN, &[0]));
    rec.record(Direction::Out, &complete);
    println!("{path}: {} print records", chunks.len() + 1);
}
