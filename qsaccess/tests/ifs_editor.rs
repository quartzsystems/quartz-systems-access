// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! The Integrated File System's editor against QS/400's test file system:
//! text read converted by the system, a binary file and a large one opened
//! read-only, the 200 MiB file refused, a link opened as its target, `InUse`
//! for a file another job holds and its read after the hold ends, and a
//! save that keeps the file's CCSID and changes its last-changed time.

#[path = "../../qap-client/tests/common/mod.rs"]
mod common;

use common::{cfg, start};
use qap_client::proto::ops::ifs::WriteMode;
use qap_client::{Client, Files, Secret, Status};
use qapd::testing::files::{BINARY_FILE, JAPANESE, LINK, LOCKED, MIXED, TEXT, text_file};
use qsa_msg::id;
use qsaccess::ifs::editor::{Loaded, load, save};

fn bob(s: &common::Server) -> Client {
    let mut c = Client::new(s.target());
    c.sign_on("BOB", &Secret::from("SECRET1")).unwrap();
    c
}

fn text(l: Loaded) -> (String, u16, Option<String>) {
    match l {
        Loaded::Text {
            text,
            entry,
            read_only,
        } => (text, entry.ccsid, read_only.map(|m| m.id)),
        Loaded::TooLarge(m) => panic!("{m}"),
    }
}

#[test]
fn files_open_as_text_read_only_or_not_at_all() {
    let s = start(cfg());
    let mut c = bob(&s);
    for ccsid in [37u16, 273, 819, 1208] {
        assert_eq!(
            text(load(&mut c, &text_file(ccsid)).unwrap()),
            (TEXT.to_owned(), ccsid, None)
        );
    }
    assert_eq!(
        text(load(&mut c, &text_file(MIXED)).unwrap()),
        (JAPANESE.to_owned(), MIXED, None)
    );
    // A link opens as its target.
    assert_eq!(text(load(&mut c, LINK).unwrap()).0, TEXT);
    // Binary: read as stored, read-only.
    let (dump, ccsid, ro) = text(load(&mut c, BINARY_FILE).unwrap());
    assert_eq!((ccsid, ro.as_deref()), (65535, Some(id::NOT_TEXT)));
    assert!(dump.starts_with("00000000  00 0E FF 0F 41 80 C1"), "{dump}");
    // Over 8 MiB: read-only.
    let nine: String = std::iter::repeat_n("0123456789abcdef\n", (9 << 20) / 17).collect();
    save(&mut c, "/home/BOB/nine.txt", &nine, WriteMode::Create, 1208).unwrap();
    let (got, _, ro) = text(load(&mut c, "/home/BOB/nine.txt").unwrap());
    assert_eq!((got.len(), ro.as_deref()), (nine.len(), Some(id::NOT_TEXT)));
    // Over 64 MiB: not opened, Download offered.
    let big = s.c.big_file();
    match load(&mut c, big).unwrap() {
        Loaded::TooLarge(m) => assert_eq!(m.id, id::TOO_LARGE_TO_EDIT),
        other => panic!("{other:?}"),
    }
    // Another job holds it: the system's InUse, then the text once it lets go.
    let e = load(&mut c, LOCKED).unwrap_err();
    assert_eq!(e.status(), Some(Status::InUse));
    assert_eq!(e.server_message().unwrap().id, "CPFA09E");
    s.c.release_lock();
    assert_eq!(
        text(load(&mut c, LOCKED).unwrap()).0,
        "held by another job\n"
    );
}

#[test]
fn a_save_keeps_the_ccsid_and_moves_the_change_time() {
    let s = start(cfg());
    let mut c = bob(&s);
    let path = text_file(37);
    let opened = c.get_info(&path).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(20));
    let changed = TEXT.replace("Zweite", "Dritte");
    let after = save(&mut c, &path, &changed, WriteMode::Replace, opened.ccsid).unwrap();
    assert_eq!(after.ccsid, 37);
    assert_ne!(
        after.modified, opened.modified,
        "the editor sees it changed"
    );
    assert_eq!(text(load(&mut c, &path).unwrap()), (changed, 37, None));
    // Save As a new file in another CCSID; Create refuses a name taken.
    let new = "/home/BOB/saved273.txt";
    let e = save(&mut c, new, TEXT, WriteMode::Create, 273).unwrap();
    assert_eq!(e.ccsid, 273);
    assert_eq!(text(load(&mut c, new).unwrap()).0, TEXT);
    let taken = save(&mut c, new, TEXT, WriteMode::Create, 273).unwrap_err();
    assert_eq!(taken.status(), Some(Status::Exists));
    // A long text in pieces makes the file one write would.
    let long: String = std::iter::repeat_n(TEXT, 30_000).collect();
    let e = save(&mut c, "/home/BOB/long37.txt", &long, WriteMode::Create, 37).unwrap();
    assert!(e.size > 2 << 20);
    assert_eq!(text(load(&mut c, "/home/BOB/long37.txt").unwrap()).0, long);
}
