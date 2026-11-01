// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! Writes fuzz corpus seeds from the replay fixtures, one per recording and
//! target, in each target's input format (see `fuzz/fuzz_targets/`).
//!
//! `cargo run -p tn5250 --example fuzz_seeds`

use std::path::Path;

use tn5250::input::{KEYS, Key};
use tn5250::negotiate::{SEND, ScreenSize, USERVAR, VAR};
use tn5250::replay::{Event, Recording};
use tn5250::telnet::{
    BINARY, DO, EOR_OPTION, IAC, NEW_ENVIRON, SB, SE, TERMINAL_TYPE, WILL, codec,
};

fn main() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let fixtures = root.join("fixtures/replay");
    let corpus = root.join("fuzz/corpus");
    let mut names: Vec<_> = std::fs::read_dir(&fixtures)
        .expect("fixtures/replay")
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "qsr"))
        .collect();
    names.sort();
    for path in names {
        let name = path.file_stem().unwrap().to_string_lossy().into_owned();
        let rec = Recording::read(&path).expect("recording");
        if rec.printer.is_some() {
            // Printer sessions seed nothing here: their data is SCS.
            continue;
        }
        let ccsid_index = tn5250::codepage::supported()
            .position(|c| c == rec.ccsid)
            .unwrap_or(0) as u8;
        let flags = u8::from(rec.size == ScreenSize::S27x132) | (ccsid_index << 1);

        let mut stream = vec![flags];
        let mut input = vec![flags];
        let mut wire = vec![7u8];
        wire.extend_from_slice(&[IAC, DO, NEW_ENVIRON, IAC, DO, TERMINAL_TYPE]);
        wire.extend_from_slice(&[IAC, SB, NEW_ENVIRON]);
        wire.extend_from_slice(&[SEND, USERVAR]);
        wire.extend_from_slice(tn5250::wire::RANDOM_SEED.as_bytes());
        wire.extend_from_slice(&[0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, VAR, USERVAR]);
        wire.extend_from_slice(&[IAC, SE, IAC, SB, TERMINAL_TYPE, 1, IAC, SE]);
        wire.extend_from_slice(&[
            IAC, DO, EOR_OPTION, IAC, WILL, EOR_OPTION, IAC, DO, BINARY, IAC, WILL, BINARY,
        ]);
        for (_, ev) in &rec.events {
            match ev {
                Event::In(bytes) => {
                    let r = tn5250::record::Record::parse(bytes).expect("record");
                    let len = (r.data.len() as u16).to_be_bytes();
                    stream.push(r.opcode);
                    stream.extend_from_slice(&len);
                    stream.extend_from_slice(&r.data);
                    input.extend_from_slice(&[0, r.opcode, len[0], len[1]]);
                    input.extend_from_slice(&r.data);
                    wire.extend_from_slice(&codec::encode_record(bytes));
                }
                Event::Key(Key::Char(c)) if (*c as u32) < 256 => {
                    input.extend_from_slice(&[2, *c as u32 as u8]);
                }
                Event::Key(k) => {
                    if let Some(i) = KEYS.iter().position(|x| x == k) {
                        input.extend_from_slice(&[1, i as u8]);
                    }
                }
                Event::Out(_) => {}
            }
        }
        for (target, bytes) in [
            ("stream_parser", &stream),
            ("parser_input", &input),
            ("telnet_decoder", &wire),
        ] {
            let dir = corpus.join(target);
            std::fs::create_dir_all(&dir).unwrap();
            let file = dir.join(format!("seed-{name}"));
            std::fs::write(&file, bytes).unwrap();
            println!("{}: {} bytes", file.display(), bytes.len());
        }
    }
}
