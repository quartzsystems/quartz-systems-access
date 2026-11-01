// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! The Telnet byte layer and the TN5250E negotiation, fed a byte stream in
//! pieces whose size the first byte sets. Also: escaping then decoding gives
//! the data back, and the other byte parsers never panic.

#![no_main]

use libfuzzer_sys::fuzz_target;
use tn5250::negotiate::{Event, Negotiator, Request, ScreenSize};
use tn5250::telnet::{Telnet, codec};

fuzz_target!(|data: &[u8]| {
    let Some((&step, rest)) = data.split_first() else {
        return;
    };
    let step = step as usize % 61 + 1;
    let mut t = Telnet::new();
    let mut n = Negotiator::new(Request::display(ScreenSize::S27x132, Some("FUZZ01"), 37));
    for piece in rest.chunks(step) {
        let _ = t.receive(piece);
        let _ = t.take_output();
        for ev in n.receive(piece) {
            if let Event::Record(r) = ev {
                let _ = r.encode();
            }
        }
        let _ = n.take_output();
    }
    let mut d = codec::Decoder::new();
    let mut out = Vec::new();
    d.feed(&codec::encode_record(rest), &mut out);
    if rest.len() <= codec::MAX_RECORD {
        assert_eq!(out, vec![codec::Token::Record(rest.to_vec())]);
    }
    let _ = tn5250::negotiate::parse_send(rest);
    let _ = tn5250::negotiate::startup_response(rest, tn5250::codepage::default_page());
    if let Ok(r) = tn5250::record::Record::parse(rest) {
        let mut p = tn5250::printer::Printer::new();
        let now = std::time::Instant::now();
        for _ in 0..2 {
            let _ = p.record(&r, now);
        }
        let _ = p.idle(
            now + std::time::Duration::from_secs(11),
            std::time::Duration::from_secs(10),
        );
    }
    let _ = tn5250::screen::Snapshot::decode(rest);
    let _ = tn5250::replay::Recording::parse(rest);
    let _ = tn5250::telnet::x509::describe(rest);
});
