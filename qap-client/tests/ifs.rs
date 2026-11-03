// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! QAP version 5's `Ifs` service against QS/400's engine and its test file
//! system (`qapd::testing::files`): every operation and status with the
//! file-system commands' own messages, paging 1,200 entries, links, the 200
//! MiB file both ways byte for byte, the converted files read in chunks
//! that cut inside characters joining into the system's whole conversion,
//! text written in pieces making the same file as one write, and a read
//! and a write resumed after the connection is dropped under them; and
//! version 6's `Stat`, its change counter and the stamps that fall back to
//! `GetInfo` before it.

mod common;

use std::io::{Read, Write};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use common::{Server, cfg, start};
use qap_client::ifs::{self, End, Listing, ReadOptions, WriteOptions};
use qap_client::proto::Op;
use qap_client::proto::ops::ifs::{EntryKind, WriteFile, WriteMode};
use qap_client::{Client, Error, Files, Secret, Status};
use qapd::testing::files::{
    BIG_SIZE, BINARY, BINARY_FILE, BROKEN, BROKEN_TARGET, CHECKED_OUT, JAPANESE, LINK, LOCKED,
    MANY, MANY_COUNT, MIXED, PRIVATE, PRIVATE_FILE, TEXT, TEXT_CCSIDS, TEXT_DIR, TREE, big_byte,
    text_file,
};

fn bob(s: &Server) -> Client {
    let mut c = Client::new(s.target());
    c.sign_on("BOB", &Secret::from("SECRET1")).unwrap();
    c
}

/// The status and message ID a refusal carried.
fn refused(e: Error) -> (Status, String) {
    let id = e.server_message().map(|m| m.id.clone()).unwrap_or_default();
    (e.status().unwrap_or(Status::Ok), id)
}

fn never() -> AtomicBool {
    AtomicBool::new(false)
}

/// The whole of `path`, read in `chunk`-byte requests.
fn read_all(c: &mut Client, path: &str, chunk: u64, as_ccsid: Option<u16>) -> Vec<u8> {
    let mut out = Vec::new();
    let end = ifs::read(
        |r| c.request(Op::ReadFile, r),
        path,
        ReadOptions {
            as_ccsid,
            chunk,
            offset: 0,
        },
        &mut out,
        |_| {},
        &never(),
    )
    .unwrap();
    assert!(matches!(end, End::Done(_)), "{end:?}");
    out
}

/// `data` written to `path` in `chunk`-byte requests.
fn write_all(c: &mut Client, path: &str, data: &[u8], chunk: usize, opts: WriteOptions) -> End {
    ifs::write(
        |w| c.request(Op::WriteFile, w),
        path,
        &mut &data[..],
        WriteOptions { chunk, ..opts },
        |_| {},
        &never(),
    )
    .unwrap()
}

#[test]
fn listing_pages_patterns_and_kinds() {
    let s = start(cfg());
    let mut c = bob(&s);
    let mut l = Listing::new(MANY, "*");
    let mut pages = Vec::new();
    while let Some(p) = l.next_page(&mut c).unwrap() {
        pages.push(p.len());
    }
    assert_eq!(pages, [500, 500, 200]);
    assert_eq!(l.total, Some(u32::try_from(MANY_COUNT).unwrap()));
    assert!(l.done());
    let all = c.all_entries(MANY, "").unwrap();
    assert_eq!(all.len(), MANY_COUNT);
    assert_eq!(
        (all[0].name.as_str(), all[1199].name.as_str()),
        ("f0000.txt", "f1199.txt")
    );
    let some = c.list_directory(MANY, "f11??.TXT", 90, 0).unwrap();
    assert_eq!((some.total, some.entries.len()), (100, 10));
    let f = &some.entries[0];
    assert_eq!(
        (f.name.as_str(), f.kind, f.size, f.owner.as_str(), f.ccsid),
        ("f1190.txt", EntryKind::File, 10, "BOB", 1208)
    );
    // Links: GetInfo does not follow one; resolving opens its target, and
    // a broken one's target is not found.
    let home = c.all_entries("/home/BOB", "*").unwrap();
    let by = |n: &str| home.iter().find(|e| e.name == n).unwrap().clone();
    let link = by("link1208");
    assert_eq!(link.kind, EntryKind::Symlink);
    assert_eq!(c.get_info(LINK).unwrap().kind, EntryKind::Symlink);
    let (to, target) = c.resolve("/home/BOB", &link).unwrap();
    assert_eq!(
        (to.as_str(), target.kind),
        (text_file(1208).as_str(), EntryKind::File)
    );
    let broken = by("broken");
    assert_eq!(broken.target.as_deref(), Some(BROKEN_TARGET));
    assert_eq!(
        refused(c.resolve("/home/BOB", &broken).unwrap_err()),
        (Status::NotFound, "CPFA0A9".into())
    );
    assert_eq!(c.get_info(BROKEN).unwrap().kind, EntryKind::Symlink);
    assert_eq!(c.get_info(TEXT_DIR).unwrap().kind, EntryKind::Directory);
    assert_eq!(c.get_info(BINARY_FILE).unwrap().ccsid, 65535);
    // /QSYS.LIB lists as the commands list it.
    let q = c.all_entries("/QSYS.LIB", "*.LIB").unwrap();
    assert!(
        q.iter()
            .any(|e| e.name == "QGPL.LIB" && e.kind == EntryKind::Directory)
    );
    // Refusals with the file-system commands' messages.
    assert_eq!(
        refused(c.list_directory("/home/BOB/nope", "*", 0, 0).unwrap_err()),
        (Status::NotFound, "CPFA0A9".into())
    );
    assert_eq!(
        refused(c.list_directory(PRIVATE, "*", 0, 0).unwrap_err()),
        (Status::NotAuthorized, "CPFA09C".into())
    );
    assert_eq!(
        refused(c.get_info(PRIVATE_FILE).unwrap_err()).0,
        Status::NotAuthorized
    );
}

#[test]
fn reads_as_stored_and_converted() {
    let s = start(cfg());
    let mut c = bob(&s);
    let d = c.read_file(LINK, 0, 0, None).unwrap();
    assert_eq!(d.data, TEXT.as_bytes(), "ReadFile follows a link");
    let d = c.read_file(&text_file(37), 1000, 5, None).unwrap();
    assert_eq!((d.data.len(), d.next_offset), (0, 1000));
    assert_eq!(
        refused(c.read_file(BROKEN, 0, 0, None).unwrap_err()).0,
        Status::NotFound
    );
    assert_eq!(
        refused(c.read_file(PRIVATE_FILE, 0, 0, None).unwrap_err()),
        (Status::NotAuthorized, "CPFA09C".into())
    );
    assert_eq!(
        refused(c.read_file(TEXT_DIR, 0, 0, None).unwrap_err()).0,
        Status::Error
    );
    assert_eq!(
        refused(c.read_file(&text_file(37), 0, 0, Some(0)).unwrap_err()).0,
        Status::InvalidRequest
    );
    // A binary file never converts: read it without as_ccsid.
    assert_eq!(
        refused(c.read_file(BINARY_FILE, 0, 0, Some(1208)).unwrap_err()),
        (Status::Error, "TCP1A5A".into())
    );
    assert_eq!(read_all(&mut c, BINARY_FILE, 3, None), BINARY);
    // UTF-8 cut inside a character: the reply stops before it.
    let at = TEXT.find('ü').unwrap() as u64;
    let d = c.read_file(&text_file(1208), 0, at + 1, Some(37)).unwrap();
    assert_eq!(d.next_offset, at);
}

#[test]
fn converted_files_join_exactly_whatever_the_chunk() {
    let s = start(cfg());
    let mut c = bob(&s);
    for from in TEXT_CCSIDS {
        // A chunk shorter than a character of the file could not move on.
        let chunks: &[u64] = if from == 1208 {
            &[4, 5, 6, 7, 64]
        } else {
            &[1, 2, 3, 5, 7, 64]
        };
        for &chunk in chunks {
            let got = read_all(&mut c, &text_file(from), chunk, Some(1208));
            assert_eq!(String::from_utf8(got).unwrap(), TEXT, "{from} by {chunk}");
        }
        // As the system converts the whole file in one request.
        let whole = c
            .read_file(&text_file(from), 0, 0, Some(1208))
            .unwrap()
            .data;
        assert_eq!(
            read_all(
                &mut c,
                &text_file(from),
                if from == 1208 { 4 } else { 3 },
                Some(1208)
            ),
            whole
        );
    }
    // Mixed single- and double-byte: boundaries inside pairs and at shifts.
    let path = text_file(MIXED);
    let whole = c.read_file(&path, 0, 0, Some(1208)).unwrap().data;
    assert_eq!(String::from_utf8(whole.clone()).unwrap(), JAPANESE);
    for chunk in [2, 3, 4, 5, 6, 7, 11, 64] {
        assert_eq!(
            read_all(&mut c, &path, chunk, Some(1208)),
            whole,
            "by {chunk}"
        );
    }
}

#[test]
fn a_text_download_resumes_from_its_last_next_offset_on_another_connection() {
    let s = start(cfg());
    let mut c = bob(&s);
    let path = text_file(MIXED);
    let whole = c.read_file(&path, 0, 0, Some(1208)).unwrap().data;
    // Cancelled after three replies.
    let cancel = AtomicBool::new(false);
    let mut first = Vec::new();
    let mut replies = 0;
    let end = ifs::read(
        |r| c.request(Op::ReadFile, r),
        &path,
        ReadOptions {
            as_ccsid: Some(1208),
            chunk: 5,
            offset: 0,
        },
        &mut first,
        |_| {
            replies += 1;
            if replies == 3 {
                cancel.store(true, Ordering::SeqCst);
            }
        },
        &cancel,
    )
    .unwrap();
    let End::Cancelled(at) = end else {
        panic!("{end:?}")
    };
    assert!(at.offset > 0 && at.offset < at.size);
    assert_eq!(at.bytes as usize, first.len());
    // The rest from that offset, on a window's own connection.
    let mut a = c.attach().unwrap();
    let mut rest = Vec::new();
    let end = ifs::read(
        |r| a.request(Op::ReadFile, r),
        &path,
        ReadOptions {
            as_ccsid: Some(1208),
            chunk: 7,
            offset: at.offset,
        },
        &mut rest,
        |_| {},
        &never(),
    )
    .unwrap();
    assert!(matches!(end, End::Done(_)));
    assert_eq!([first, rest].concat(), whole);
}

#[test]
fn write_modes_and_their_statuses() {
    let s = start(cfg());
    let mut c = bob(&s);
    let p = "/home/BOB/new.txt";
    let w = |mode, offset, data: &[u8]| WriteFile {
        path: p.into(),
        mode,
        offset,
        data: data.to_vec(),
        ..WriteFile::default()
    };
    assert_eq!(c.write_file(&w(WriteMode::Create, 0, b"hello")).unwrap(), 5);
    let e = c.get_info(p).unwrap();
    assert_eq!((e.ccsid, e.owner.as_str()), (1208, "BOB"));
    assert_eq!(
        refused(c.write_file(&w(WriteMode::Create, 0, b"x")).unwrap_err()),
        (Status::Exists, "CPFA0A0".into())
    );
    assert_eq!(c.write_file(&w(WriteMode::Update, 1, b"EL")).unwrap(), 5);
    assert_eq!(
        c.write_file(&w(WriteMode::Append, 0, b" world")).unwrap(),
        11
    );
    assert_eq!(read_all(&mut c, p, 64, None), b"hELlo world");
    // Replace and Append make a missing file; Update does not.
    for (mode, path) in [
        (WriteMode::Replace, "/home/BOB/r.txt"),
        (WriteMode::Append, "/home/BOB/a.txt"),
    ] {
        let mut req = w(mode, 0, b"ab");
        req.path = path.into();
        assert_eq!(c.write_file(&req).unwrap(), 2, "{mode:?}");
    }
    let mut req = w(WriteMode::Update, 0, b"x");
    req.path = "/home/BOB/none.txt".into();
    assert_eq!(refused(c.write_file(&req).unwrap_err()).0, Status::NotFound);
    let mut req = w(WriteMode::Create, 0, b"x");
    req.path = "/home/ANN/private/x.txt".into();
    assert_eq!(
        refused(c.write_file(&req).unwrap_err()).0,
        Status::NotAuthorized
    );
    // data_ccsid 0 is not a CCSID; 65535 converts to nothing.
    let mut req = w(WriteMode::Append, 0, b"x");
    req.data_ccsid = Some(0);
    assert_eq!(
        refused(c.write_file(&req).unwrap_err()).0,
        Status::InvalidRequest
    );
    req.data_ccsid = Some(65535);
    assert_eq!(
        refused(c.write_file(&req).unwrap_err()),
        (Status::Error, "TCP1A5A".into())
    );
}

#[test]
fn directories_delete_rename_and_copy() {
    let s = start(cfg());
    let mut c = bob(&s);
    c.create_directory("/home/BOB/d1", false).unwrap();
    assert_eq!(
        refused(c.create_directory("/home/BOB/d1", false).unwrap_err()),
        (Status::Exists, "CPFA0A0".into())
    );
    // With parents, a directory already there is no error.
    c.create_directory("/home/BOB/d1", true).unwrap();
    c.create_directory("/home/BOB/d2/x/y", true).unwrap();
    assert_eq!(
        c.get_info("/home/BOB/d2/x/y").unwrap().kind,
        EntryKind::Directory
    );
    assert_eq!(
        refused(c.create_directory("/home/BOB/nope/z", false).unwrap_err()).0,
        Status::NotFound
    );
    // A directory with something in it is not deleted.
    assert_eq!(
        refused(c.delete("/home/BOB/d2/x").unwrap_err()),
        (Status::Error, "CPFA0AC".into())
    );
    c.delete("/home/BOB/d2/x/y").unwrap();
    c.delete("/home/BOB/d2/x").unwrap();
    // A link is deleted, not what it names.
    c.delete(LINK).unwrap();
    assert!(c.get_info(&text_file(1208)).is_ok());
    // Copy keeps the CCSID and the data as stored; a directory is refused.
    let to = "/home/BOB/copy37.txt";
    c.copy(&text_file(37), to, false).unwrap();
    assert_eq!(c.get_info(to).unwrap().ccsid, 37);
    assert_eq!(
        read_all(&mut c, to, 64, None),
        read_all(&mut c, &text_file(37), 64, None)
    );
    assert_eq!(
        refused(c.copy(&text_file(37), to, false).unwrap_err()).0,
        Status::Exists
    );
    c.copy(&text_file(273), to, true).unwrap();
    assert_eq!(c.get_info(to).unwrap().ccsid, 273);
    assert_eq!(
        refused(c.copy(TREE, "/home/BOB/treecopy", false).unwrap_err()),
        (Status::Error, "CPFA0DA".into())
    );
    // Rename: a move, refused onto a name taken unless asked to replace.
    c.rename(to, "/home/BOB/d1/moved.txt", false).unwrap();
    assert_eq!(refused(c.get_info(to).unwrap_err()).0, Status::NotFound);
    c.copy(&text_file(37), to, false).unwrap();
    assert_eq!(
        refused(c.rename(to, "/home/BOB/d1/moved.txt", false).unwrap_err()).0,
        Status::Exists
    );
    c.rename(to, "/home/BOB/d1/moved.txt", true).unwrap();
    assert_eq!(
        refused(c.delete("/home/BOB/gone.txt").unwrap_err()).0,
        Status::NotFound
    );
}

#[test]
fn files_held_by_other_jobs_and_users() {
    let s = start(cfg());
    let mut c = bob(&s);
    // Another job's open, sharing nothing: reads and writes are InUse.
    assert_eq!(
        refused(c.read_file(LOCKED, 0, 0, None).unwrap_err()),
        (Status::InUse, "CPFA09E".into())
    );
    let append = WriteFile {
        path: LOCKED.into(),
        mode: WriteMode::Append,
        data: b"x".to_vec(),
        ..WriteFile::default()
    };
    assert_eq!(refused(c.write_file(&append).unwrap_err()).0, Status::InUse);
    s.c.release_lock();
    assert_eq!(
        c.read_file(LOCKED, 0, 0, None).unwrap().data,
        b"held by another job\n"
    );
    // Checked out to ANN: Delete, Rename and WriteFile are InUse, reads
    // are not refused.
    assert_eq!(
        refused(c.delete(CHECKED_OUT).unwrap_err()),
        (Status::InUse, "CPFA09E".into())
    );
    assert_eq!(
        refused(c.rename(CHECKED_OUT, "/home/BOB/x.txt", false).unwrap_err()).0,
        Status::InUse
    );
    let mut w = append.clone();
    w.path = CHECKED_OUT.into();
    assert_eq!(refused(c.write_file(&w).unwrap_err()).0, Status::InUse);
    assert!(c.read_file(CHECKED_OUT, 0, 0, None).is_ok());
    // A file another job holds open still deletes, as RMVLNK does.
    s.c.files.hold_lock();
    c.delete(LOCKED).unwrap();
}

#[test]
fn text_written_in_pieces_makes_the_same_file_as_one_write() {
    let s = start(cfg());
    let mut c = bob(&s);
    for (ccsid, text) in [(37u16, TEXT), (273, TEXT), (MIXED, JAPANESE)] {
        let text_opts = WriteOptions {
            data_ccsid: Some(ifs::UTF8),
            ccsid,
            first: WriteMode::Create,
            ..WriteOptions::default()
        };
        let one = format!("/home/BOB/one{ccsid}.txt");
        write_all(&mut c, &one, text.as_bytes(), ifs::WRITE_CHUNK, text_opts);
        let whole = read_all(&mut c, &one, 1 << 20, None);
        // Chunks of a few bytes cut inside characters; they are sent whole.
        for chunk in [4, 5, 7, 11] {
            let path = format!("/home/BOB/p{ccsid}-{chunk}.txt");
            let end = write_all(&mut c, &path, text.as_bytes(), chunk, text_opts);
            assert_eq!(end.progress().bytes, text.len() as u64);
            assert_eq!(
                read_all(&mut c, &path, 1 << 20, None),
                whole,
                "{ccsid} by {chunk}"
            );
            let back = read_all(&mut c, &path, 64, Some(ifs::UTF8));
            assert_eq!(String::from_utf8(back).unwrap(), text, "{ccsid} by {chunk}");
        }
        assert_eq!(c.get_info(&one).unwrap().ccsid, ccsid);
    }
    // Replace empties an existing file first; an empty source still makes
    // the file.
    let p = "/home/BOB/one37.txt";
    let end = write_all(&mut c, p, b"", 64, WriteOptions::default());
    assert_eq!(end.progress().size, 0);
    assert_eq!(c.get_info(p).unwrap().size, 0);
}

/// `n` bytes of the big file's pattern from `at`, as a reader.
struct Pattern {
    at: u64,
    end: u64,
}

impl Read for Pattern {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = usize::try_from((self.end - self.at).min(buf.len() as u64)).unwrap();
        for (i, b) in buf[..n].iter_mut().enumerate() {
            *b = big_byte(self.at + i as u64);
        }
        self.at += n as u64;
        Ok(n)
    }
}

/// A sink that checks every byte against the big file's pattern.
struct Check {
    at: u64,
}

impl Write for Check {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        for (i, b) in buf.iter().enumerate() {
            assert_eq!(
                *b,
                big_byte(self.at + i as u64),
                "byte {}",
                self.at + i as u64
            );
        }
        self.at += buf.len() as u64;
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[test]
fn the_200_mib_file_both_ways_byte_for_byte() {
    let s = start(cfg());
    let big = s.c.big_file();
    let mut c = bob(&s);
    let mut check = Check { at: 0 };
    let mut last = ifs::Progress::default();
    let end = ifs::read(
        |r| c.request(Op::ReadFile, r),
        big,
        ReadOptions::default(),
        &mut check,
        |p| last = p,
        &never(),
    )
    .unwrap();
    assert_eq!(end, End::Done(last));
    assert_eq!(
        (check.at, last.offset, last.size),
        (BIG_SIZE, BIG_SIZE, BIG_SIZE)
    );
    // And back up, then read again.
    let up = "/home/BOB/up.bin";
    let end = ifs::write(
        |w| c.request(Op::WriteFile, w),
        up,
        &mut Pattern {
            at: 0,
            end: BIG_SIZE,
        },
        WriteOptions {
            ccsid: ifs::BINARY,
            ..WriteOptions::default()
        },
        |_| {},
        &never(),
    )
    .unwrap();
    assert_eq!(end.progress().size, BIG_SIZE);
    let e = c.get_info(up).unwrap();
    assert_eq!((e.size, e.ccsid), (BIG_SIZE, 65535));
    let mut check = Check { at: 0 };
    let end = ifs::read(
        |r| c.request(Op::ReadFile, r),
        up,
        ReadOptions::default(),
        &mut check,
        |_| {},
        &never(),
    )
    .unwrap();
    assert!(matches!(end, End::Done(_)));
    assert_eq!(check.at, BIG_SIZE);
}

#[test]
fn a_read_cut_off_by_a_drop_is_read_again_on_the_new_connection() {
    let s = start(cfg());
    let big = s.c.big_file();
    let mut c = bob(&s);
    let sys = Arc::clone(&s.c);
    let jobs = Arc::clone(&s.jobs);
    let sent = AtomicUsize::new(0);
    let mut check = Check { at: 0 };
    let size = 8 << 20;
    let end = ifs::read(
        |r| {
            let p = c.request(Op::ReadFile, r);
            // The third request's connection drops under it.
            if sent.fetch_add(1, Ordering::SeqCst) == 2 {
                let job = *jobs.lock().unwrap().last().unwrap();
                assert!(sys.drop_connection(job));
            }
            p
        },
        big,
        ReadOptions::default(),
        &mut LimitedCheck {
            inner: &mut check,
            limit: size,
        },
        |_| {},
        &never(),
    );
    // The sink stops the read once it has the first 8 MiB.
    assert!(matches!(end, Err(Error::Write(_))), "{end:?}");
    assert!(check.at >= size);
    assert!(
        sent.load(Ordering::SeqCst) >= 9,
        "the lost request went again"
    );
    assert!(c.resumes() >= 1, "on a new connection");
}

/// A [`Check`] that fails once it has `limit` bytes.
struct LimitedCheck<'a> {
    inner: &'a mut Check,
    limit: u64,
}

impl Write for LimitedCheck<'_> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.inner.write_all(buf)?;
        if self.inner.at >= self.limit {
            return Err(std::io::Error::other("enough"));
        }
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[test]
fn a_binary_upload_resumes_with_update_after_a_drop() {
    let s = start(cfg());
    let mut c = bob(&s);
    let sys = Arc::clone(&s.c);
    let jobs = Arc::clone(&s.jobs);
    let size: u64 = 6 << 20;
    let path = "/home/BOB/resume.bin";
    let sent = AtomicUsize::new(0);
    let opts = WriteOptions {
        ccsid: ifs::BINARY,
        ..WriteOptions::default()
    };
    let e = ifs::write(
        |w| {
            let p = c.request(Op::WriteFile, w);
            if sent.fetch_add(1, Ordering::SeqCst) == 2 {
                let job = *jobs.lock().unwrap().last().unwrap();
                assert!(sys.drop_connection(job));
            }
            p
        },
        path,
        &mut Pattern { at: 0, end: size },
        opts,
        |_| {},
        &never(),
    )
    .unwrap_err();
    // A write is never sent again by itself.
    assert!(matches!(e, Error::Closed(_)), "{e:?}");
    assert_eq!(sent.load(Ordering::SeqCst), 3);
    // Resume where the system's file ends, with Update from there.
    let at = ifs::resume_offset(&mut c, path, size).unwrap();
    assert!(at > 0 && at < size, "{at}");
    let end = ifs::write(
        |w| {
            assert_eq!(w.mode, WriteMode::Update);
            c.request(Op::WriteFile, w)
        },
        path,
        &mut Pattern { at, end: size },
        WriteOptions {
            resume_at: Some(at),
            ..opts
        },
        |_| {},
        &never(),
    )
    .unwrap();
    assert_eq!(end.progress().size, size);
    let mut check = Check { at: 0 };
    ifs::read(
        |r| c.request(Op::ReadFile, r),
        path,
        ReadOptions::default(),
        &mut check,
        |_| {},
        &never(),
    )
    .unwrap();
    assert_eq!(check.at, size);
    assert_eq!(
        ifs::resume_offset(&mut c, "/home/BOB/none.bin", 10).unwrap(),
        0
    );
}

#[test]
fn a_cancel_in_a_write_stops_it() {
    let s = start(cfg());
    let mut c = bob(&s);
    let cancel = AtomicBool::new(false);
    let mut n = 0;
    let end = ifs::write(
        |w| c.request(Op::WriteFile, w),
        "/home/BOB/c.bin",
        &mut Pattern {
            at: 0,
            end: 4 << 20,
        },
        WriteOptions {
            chunk: 1 << 20,
            ..WriteOptions::default()
        },
        |_| {
            n += 1;
            if n == 2 {
                cancel.store(true, Ordering::SeqCst);
            }
        },
        &cancel,
    )
    .unwrap();
    // Two requests, each under the frame: what the system has is what
    // was confirmed, and nothing more was sent.
    assert_eq!(end, End::Cancelled(end.progress()));
    let at = end.progress().offset;
    assert_eq!(at, 2 * (ifs::WRITE_CHUNK - "/home/BOB/c.bin".len()) as u64);
    assert_eq!(c.get_info("/home/BOB/c.bin").unwrap().size, at);
}

#[test]
fn stat_is_get_info_and_a_change_counter() {
    let s = start(cfg());
    let mut c = bob(&s);
    let path = text_file(37);
    let st = c.stat(&path).unwrap();
    assert_eq!(st.entry, c.get_info(&path).unwrap());
    // Up on every change, two in one millisecond included; never down.
    s.c.files.change_twice(&path);
    let after = c.stat(&path).unwrap();
    assert_eq!(after.entry.modified, st.entry.modified);
    assert!(
        after.change >= st.change + 2,
        "{} then {}",
        st.change,
        after.change
    );
    c.write_file(&WriteFile {
        path: path.clone(),
        mode: WriteMode::Append,
        data: b"x".to_vec(),
        ..WriteFile::default()
    })
    .unwrap();
    assert!(c.stat(&path).unwrap().change > after.change);
    // A directory changes with its entries.
    let dir = c.stat(TEXT_DIR).unwrap().change;
    c.copy(&path, &format!("{TEXT_DIR}/copied.txt"), false)
        .unwrap();
    assert!(c.stat(TEXT_DIR).unwrap().change > dir);
    // A link is itself; its target is stamped through `checked`.
    let link = c.stat(LINK).unwrap();
    assert_eq!(link.entry.kind, EntryKind::Symlink);
    let (target, entry, stamp) = c.checked(LINK).unwrap();
    assert_eq!(target, text_file(1208));
    assert_eq!(entry.kind, EntryKind::File);
    assert_eq!(stamp, ifs::Stamp::Change(c.stat(&target).unwrap().change));
    // As GetInfo refuses.
    assert_eq!(
        refused(c.stat(BROKEN_TARGET).unwrap_err()).0,
        Status::NotFound
    );
    assert_eq!(
        refused(c.stat(PRIVATE_FILE).unwrap_err()).0,
        Status::NotAuthorized
    );
    assert_eq!(refused(c.checked(BROKEN).unwrap_err()).0, Status::NotFound);
}

#[test]
fn stat_is_version_6_and_stamps_fall_back_to_get_info() {
    let s = start(cfg());
    let mut t = s.target();
    t.options.max_version = 5;
    let mut c = Client::new(t);
    c.sign_on("BOB", &Secret::from("SECRET1")).unwrap();
    match c.stat("/home/BOB").unwrap_err() {
        Error::Unsupported { op, needs, has } => assert_eq!((op, needs, has), (Op::Stat, 6, 5)),
        e => panic!("{e:?}"),
    }
    let (e, stamp) = c.stamp(&text_file(37)).unwrap();
    assert_eq!(stamp, ifs::Stamp::Modified(e.modified));
}

#[test]
fn not_before_version_5() {
    let (port, _, _) = common::v2::start_v2();
    let mut c = Client::new(common::target(port));
    c.sign_on("BOB", &Secret::from("SECRET1")).unwrap();
    match c.get_info("/home/BOB").unwrap_err() {
        Error::Unsupported { op, needs, has } => {
            assert_eq!((op, needs, has), (Op::GetInfo, 5, 2));
        }
        e => panic!("{e:?}"),
    }
}

#[test]
fn a_session_at_version_4_sends_nothing_of_ifs_and_sql_still_works() {
    let s = start(cfg());
    let mut t = s.target();
    t.options.max_version = 4;
    let mut c = Client::new(t);
    c.sign_on("BOB", &Secret::from("SECRET1")).unwrap();
    assert_eq!(c.version(), Some(4));
    let frames = c.connection().unwrap().frames_received();
    for op in [
        Op::ListDirectory,
        Op::GetInfo,
        Op::ReadFile,
        Op::WriteFile,
        Op::CreateDirectory,
        Op::Delete,
        Op::Rename,
        Op::Copy,
    ] {
        match c.supports(op).unwrap_err() {
            Error::Unsupported { needs, has, .. } => assert_eq!((needs, has), (5, 4)),
            e => panic!("{e:?}"),
        }
    }
    assert!(matches!(
        c.list_directory("/home/BOB", "*", 0, 0),
        Err(Error::Unsupported { .. })
    ));
    assert_eq!(
        c.connection().unwrap().frames_received(),
        frames,
        "nothing was sent"
    );
    let mut q = c.sql_connection().unwrap();
    let p = q.prepare("SELECT * FROM HLINV.ASSETS").unwrap();
    assert!(!p.columns.is_empty());
}
