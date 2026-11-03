// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! Folders walked by this program against QS/400's test file system: the
//! counts a delete confirmation gives, a depth-first delete of `TREE`
//! stopping at `TREE_HELD` (checked out to `ANN`) with the system's
//! message and what came before it deleted, and a folder copied on the
//! system.

#[path = "../../qap-client/tests/common/mod.rs"]
mod common;

use common::{cfg, start};
use qap_client::{Client, Files, Secret, Status};
use qapd::testing::files::{TREE, TREE_DIRS, TREE_FILES, TREE_HELD};
use qsa_transfer::files::{Counts, copy_tree, count, delete_tree};

fn bob(s: &common::Server) -> Client {
    let mut c = Client::new(s.target());
    c.sign_on("BOB", &Secret::from("SECRET1")).unwrap();
    c
}

fn there(c: &mut Client, p: &str) -> bool {
    c.get_info(p).is_ok()
}

#[test]
fn a_recursive_delete_stops_at_a_checked_out_file() {
    let s = start(cfg());
    let mut c = bob(&s);
    let n = count(&mut c, TREE).unwrap();
    assert_eq!(
        (n.files, n.folders),
        (TREE_FILES.len() as u64 + 1, TREE_DIRS.len() as u64)
    );
    let mut deleted = Vec::new();
    let stop = delete_tree(&mut c, TREE, &mut |p| deleted.push(p.to_owned())).unwrap_err();
    // It stops at the file checked out to ANN, with the system's message.
    assert_eq!(stop.at, TREE_HELD);
    assert_eq!(stop.error.status(), Some(Status::InUse));
    assert_eq!(stop.error.server_message().unwrap().id, "CPFA09E");
    // Depth-first in name order: what came before it is gone, the rest is
    // still there.
    assert_eq!(
        deleted,
        [
            "/home/BOB/tree/docs/guide.txt",
            "/home/BOB/tree/docs/old/notes.txt",
            "/home/BOB/tree/docs/old",
            "/home/BOB/tree/docs",
            "/home/BOB/tree/readme.txt",
            "/home/BOB/tree/src/lib/a.txt",
        ]
    );
    for p in &deleted {
        assert!(!there(&mut c, p), "{p}");
    }
    for p in [
        TREE_HELD,
        "/home/BOB/tree/src/lib/z.txt",
        "/home/BOB/tree/src/main.txt",
    ] {
        assert!(there(&mut c, p), "{p}");
    }
    // Its folders still hold it: the system refuses them as not empty.
    let e = c.delete("/home/BOB/tree/src/lib").unwrap_err();
    assert_eq!(
        (e.status(), e.server_message().unwrap().id.as_str()),
        (Some(Status::Error), "CPFA0AC")
    );
    // A count of a folder the profile may not read is the system's refusal.
    let e = count(&mut c, "/home/ANN/private").unwrap_err();
    assert_eq!(e.error.status(), Some(Status::NotAuthorized));
}

#[test]
fn a_folder_is_copied_on_the_system_and_deleted_whole() {
    let s = start(cfg());
    let mut c = bob(&s);
    let mut copied = Vec::new();
    copy_tree(
        &mut c,
        "/home/BOB/text",
        "/home/BOB/copy/text",
        false,
        &mut |p| copied.push(p.to_owned()),
    )
    .unwrap();
    assert_eq!(copied.len(), 6);
    let src = c.get_info("/home/BOB/text/text37.txt").unwrap();
    let dst = c.get_info("/home/BOB/copy/text/text37.txt").unwrap();
    assert_eq!((dst.size, dst.ccsid), (src.size, 37));
    // Again: the files are there, and replace is asked for.
    let e = copy_tree(
        &mut c,
        "/home/BOB/text",
        "/home/BOB/copy/text",
        false,
        &mut |_| {},
    )
    .unwrap_err();
    assert_eq!(e.error.status(), Some(Status::Exists));
    copy_tree(
        &mut c,
        "/home/BOB/text",
        "/home/BOB/copy/text",
        true,
        &mut |_| {},
    )
    .unwrap();
    assert_eq!(
        count(&mut c, "/home/BOB/copy").unwrap(),
        Counts {
            files: 6,
            folders: 1,
            bytes: count(&mut c, "/home/BOB/text").unwrap().bytes,
        }
    );
    let mut n = 0;
    delete_tree(&mut c, "/home/BOB/copy", &mut |_| n += 1).unwrap();
    assert_eq!(n, 8);
    assert_eq!(
        c.get_info("/home/BOB/copy").unwrap_err().status(),
        Some(Status::NotFound)
    );
}
