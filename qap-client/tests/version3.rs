// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! QAP version 3's `Spool` service against QS/400's engine and test system
//! (`qapd::testing::fixtures`): every operation and every status it
//! answers; the batched reader, whose batches join byte for byte into the
//! whole file once each later batch's leading state is taken off, and
//! which, written as received, renders the pages the whole file does;
//! cancelling a read in a batch and between batches; a read beside a long
//! command; and the 64 MiB limit on a joined reply.

mod common;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use common::{Server, cfg, start};
use qap_client::proto::ops::spool::{
    DeviceType, ListOutputQueues, ListSpooledFiles, ReadFormat, ReadSpooledFile, SpoolStatus,
    SpooledData, SpooledFile, SpooledFileId,
};
use qap_client::proto::types::ObjectName;
use qap_client::proto::{Op, Service};
use qap_client::spool::{self, Action, Paging, Progress, ReadEnd};
use qap_client::{Client, Closed, CommandOutcome, Error, Secret, Status};

fn bob(s: &Server) -> Client {
    let mut c = Client::new(s.target());
    c.sign_on("BOB", &Secret::from("SECRET1")).unwrap();
    c
}

fn q(library: &str, name: &str) -> ObjectName {
    ObjectName {
        library: library.into(),
        name: name.into(),
    }
}

fn list(user: &str) -> ListSpooledFiles {
    ListSpooledFiles {
        user: user.into(),
        ..ListSpooledFiles::default()
    }
}

/// The test system's file `name`, as `*ALL` lists it.
fn file(c: &mut Client, name: &str) -> SpooledFile {
    c.all_spooled_files(&list("*ALL"))
        .unwrap()
        .into_iter()
        .find(|f| f.id.file == name)
        .unwrap_or_else(|| panic!("no {name}"))
}

/// The refusal's status and message ID.
fn refused(e: Error) -> (Status, String) {
    let id = e.server_message().map(|m| m.id.clone()).unwrap_or_default();
    (e.status().unwrap_or_else(|| panic!("{e:?}")), id)
}

/// One range, as one request.
fn range(c: &mut Client, id: &SpooledFileId, first: u32, last: u32) -> Result<SpooledData, Error> {
    let req = ReadSpooledFile {
        id: id.clone(),
        format: ReadFormat::Scs,
        first_page: first,
        last_page: last,
    };
    c.read_spooled_file(&req)?
        .wait_done()?
        .decode::<SpooledData>()
}

/// A later batch's leading state (format commands, then the absolute
/// vertical move, AVPP `X'34C4'`, and, unless at column 1, the horizontal
/// one, AHPP `X'34C0'`) and the
/// pages after it: the split for which the pages go on where `whole` is at.
fn strip_state<'a>(b: &'a [u8], whole_rest: &[u8]) -> (&'a [u8], &'a [u8]) {
    let mut i = 0;
    while b.get(i) == Some(&0x2B) {
        i += 2 + usize::from(b[i + 2].max(1));
    }
    assert_eq!(
        b.get(i..i + 2),
        Some(&[0x34, 0xC4][..]),
        "the line comes next"
    );
    i += 3;
    // The column, unless the page itself starts with one.
    if b.get(i..i + 2) == Some(&[0x34, 0xC0][..]) && !whole_rest.starts_with(&b[i..]) {
        i += 3;
    }
    (&b[..i], &b[i..])
}

const SCS_FILES: [&str; 5] = ["QPJOBLOG", "QSYSPRT", "LANDSCAPE", "MIXED", "OVERFLOW"];

#[test]
fn spool_is_there_from_version_3() {
    let s = start(cfg());
    let mut c = Client::new(s.target());
    c.connect().unwrap();
    assert_eq!(c.version(), Some(4));
    assert!(c.hello().unwrap().services.contains(&Service::Spool));
    assert_eq!(Op::ListSpooledFiles.since(), 3);
    let e = c.list_spooled_files(&list("*CURRENT")).unwrap_err();
    assert_eq!(e.status(), Some(Status::NotSignedOn));
}

#[test]
fn listing_filters_and_pages() {
    let s = start(cfg());
    let mut c = bob(&s);
    // *CURRENT: BOB's own, every one of them.
    let mine = c.all_spooled_files(&list("*CURRENT")).unwrap();
    assert!(mine.iter().all(|f| f.user == "BOB"));
    assert!(mine.iter().any(|f| f.id.file == "QSYSPRT"));
    // *ALL and a profile.
    let all = c.all_spooled_files(&list("*ALL")).unwrap();
    let ann = c.all_spooled_files(&list("ANN")).unwrap();
    assert_eq!(ann.len(), 11, "ANNRPT01 to 10 and ANNPRIV");
    assert_eq!(all.len(), mine.len() + ann.len());
    // A queue: QPRINT's 60, in queue order.
    let qprint = ListSpooledFiles {
        outq: Some(q("QGPL", "QPRINT")),
        ..list("*ALL")
    };
    let first = c
        .list_spooled_files(&ListSpooledFiles {
            max: 25,
            ..qprint.clone()
        })
        .unwrap();
    assert_eq!((first.total, first.files.len()), (60, 25));
    assert_eq!(first.files[0].id.file, "QPJOBLOG");
    let last = c
        .list_spooled_files(&ListSpooledFiles {
            start: 50,
            max: 25,
            ..qprint.clone()
        })
        .unwrap();
    assert_eq!((last.total, last.files.len()), (60, 10));
    // Paging walks the same pages.
    let mut p = Paging::new(25);
    p.update(first.total);
    assert!(p.next_page() && p.next_page());
    assert_eq!((p.start, p.page()), (50, (3, 3)));
    assert_eq!(p.shown(last.files.len()), Some((51, 60)));
    // Status, form type and user data.
    let held = c
        .all_spooled_files(&ListSpooledFiles {
            status: Some(SpoolStatus::Held),
            ..list("*ALL")
        })
        .unwrap();
    assert_eq!(
        held.iter().map(|f| f.id.file.as_str()).collect::<Vec<_>>(),
        ["STS1"]
    );
    let std = c
        .all_spooled_files(&ListSpooledFiles {
            form_type: Some("*STD".into()),
            ..list("*ALL")
        })
        .unwrap();
    assert_eq!(std.len(), all.len());
    let mixed = c
        .all_spooled_files(&ListSpooledFiles {
            user_data: Some("MIXED".into()),
            ..list("*CURRENT")
        })
        .unwrap();
    assert_eq!(mixed.len(), 1);
    // Every field of one file.
    let f = &mixed[0];
    assert_eq!(f.id.job.name, "QPADEV0001");
    assert_eq!(f.id.job.user, "BOB");
    assert_eq!(f.id.job.number, "001234");
    assert_eq!(
        (f.outq.library.as_str(), f.outq.name.as_str()),
        ("QGPL", "QPRINT")
    );
    assert_eq!(
        (f.status, f.device_type),
        (SpoolStatus::Ready, DeviceType::Scs)
    );
    assert_eq!((f.pages, f.copies, f.form_type.as_str()), (4, 1, "*STD"));
    assert!(f.size > 0 && f.created > 0);
}

#[test]
fn listing_refusals() {
    let s = start(cfg());
    let mut c = bob(&s);
    let nosuch = ListSpooledFiles {
        outq: Some(q("QGPL", "NOSUCH")),
        ..list("*ALL")
    };
    assert_eq!(
        refused(c.list_spooled_files(&nosuch).unwrap_err()),
        (Status::NotFound, "CPF3357".into())
    );
    assert_eq!(
        refused(c.list_spooled_files(&list("NOBODY")).unwrap_err()),
        (Status::NotFound, "CPF2204".into())
    );
    let private = ListSpooledFiles {
        outq: Some(q("QUSRSYS", "PRIVATE")),
        ..list("*ALL")
    };
    assert_eq!(
        refused(c.list_spooled_files(&private).unwrap_err()),
        (Status::NotAuthorized, "CPF3303".into())
    );
}

#[test]
fn output_queues() {
    let s = start(cfg());
    let mut c = bob(&s);
    let all = c.all_output_queues("*ALL").unwrap();
    let names: Vec<String> = all
        .iter()
        .map(|o| format!("{}/{}", o.outq.library, o.outq.name))
        .collect();
    assert_eq!(names, ["QGPL/QPRINT", "QGPL/PRT01", "QUSRSYS/PRIVATE"]);
    assert_eq!((all[0].files, all[0].writer.as_str()), (60, ""));
    assert_eq!(all[1].writer, "PRT01");
    assert_eq!(all[0].text, "Default printer output");
    assert!(!all[0].held);
    assert_eq!(c.all_output_queues("QGPL").unwrap().len(), 2);
    assert_eq!(c.all_output_queues("*LIBL").unwrap().len(), 3);
    let one = c
        .list_output_queues(&ListOutputQueues {
            library: "*ALL".into(),
            start: 1,
            max: 1,
        })
        .unwrap();
    assert_eq!((one.total, one.queues[0].outq.name.as_str()), (3, "PRT01"));
    assert_eq!(
        refused(c.all_output_queues("NOLIB").unwrap_err()),
        (Status::NotFound, "CPF2110".into())
    );
    assert_eq!(
        refused(c.all_output_queues("SECLIB").unwrap_err()),
        (Status::NotAuthorized, "CPF9802".into())
    );
}

#[test]
fn reading_every_status() {
    let s = start(cfg());
    let mut c = bob(&s);
    let listing = file(&mut c, "QSYSPRT");
    let whole = range(&mut c, &listing.id, 1, 0).unwrap();
    assert_eq!((whole.format, whole.pages), (ReadFormat::Scs, 30));
    // pages is the whole file's, for any range; past the end reads to it.
    assert_eq!(range(&mut c, &listing.id, 3, 4).unwrap().pages, 30);
    assert_eq!(
        range(&mut c, &listing.id, 26, 99).unwrap().data,
        range(&mut c, &listing.id, 26, 0).unwrap().data
    );
    // The pages asked for are not the file's.
    for (first, last) in [(0, 0), (31, 0), (5, 4)] {
        assert_eq!(
            refused(range(&mut c, &listing.id, first, last).unwrap_err()),
            (Status::InvalidRequest, "CPD0084".into()),
            "{first}-{last}"
        );
    }
    // No such file.
    let gone = SpooledFileId {
        number: 999,
        ..listing.id.clone()
    };
    assert_eq!(
        refused(range(&mut c, &gone, 1, 0).unwrap_err()),
        (Status::NotFound, "CPF3309".into())
    );
    // ANN's, on a queue that does not show its files to anyone.
    let anns = file(&mut c, "ANNRPT01");
    assert_eq!(
        refused(range(&mut c, &anns.id, 1, 0).unwrap_err()),
        (Status::NotAuthorized, "CPF3303".into())
    );
    // Still open, and being written: InUse with TCP1A57.
    for name in ["OPENFILE", "STS2", "STS4"] {
        let f = file(&mut c, name);
        let e = range(&mut c, &f.id, 1, 0).unwrap_err();
        assert_eq!(refused(e), (Status::InUse, "TCP1A57".into()), "{name}");
    }
    // Every other status reads.
    for (n, st) in SpoolStatus::ALL.into_iter().enumerate() {
        let f = file(&mut c, &format!("STS{n}"));
        assert_eq!(f.status, st);
        if !matches!(st, SpoolStatus::Open | SpoolStatus::Writing) {
            assert!(range(&mut c, &f.id, 1, 0).is_ok(), "{st:?}");
        }
    }
    // AFP: no format can be made of it, Error with TCP1A56.
    let afp = file(&mut c, "AFPINV");
    assert_eq!(afp.device_type, DeviceType::Afp);
    for format in [ReadFormat::Scs, ReadFormat::Pdf, ReadFormat::Text] {
        let req = ReadSpooledFile {
            id: afp.id.clone(),
            format,
            first_page: 1,
            last_page: 0,
        };
        let r = c.read_spooled_file(&req).unwrap().wait_done().unwrap();
        assert_eq!(r.status, Status::Error, "{format:?}");
        let m = r.message.as_ref().unwrap();
        assert_eq!(m.id, "TCP1A56");
        assert!(!m.text.is_empty() && !m.help.is_empty());
    }
    let mut sink = Vec::new();
    let e = spool::read(
        |r| c.read_spooled_file(r),
        &afp.id,
        spool::format_for(afp.device_type),
        spool::BATCH_PAGES,
        &mut sink,
        |_| {},
        &AtomicBool::new(false),
    )
    .unwrap_err();
    assert_eq!(refused(e).1, "TCP1A56");
    assert!(sink.is_empty());
}

#[test]
fn hold_release_delete_and_move() {
    let s = start(cfg());
    let mut c = bob(&s);
    let status = |c: &mut Client, name: &str| file(c, name).status;
    let rpt = file(&mut c, "RPT001");
    for immediately in [false, true] {
        c.act(&Action::Hold {
            id: rpt.id.clone(),
            immediately,
        })
        .unwrap();
        assert_eq!(status(&mut c, "RPT001"), SpoolStatus::Held);
        c.act(&Action::Release(rpt.id.clone())).unwrap();
        assert_eq!(status(&mut c, "RPT001"), SpoolStatus::Ready);
    }
    // The test system refuses a release of a file not held.
    let again = c.act(&Action::Release(rpt.id.clone())).unwrap_err();
    assert_eq!(refused(again), (Status::Error, "CPF3337".into()));
    // ANN's: BOB may not touch them.
    let anns = file(&mut c, "ANNRPT02");
    for a in [
        Action::Hold {
            id: anns.id.clone(),
            immediately: false,
        },
        Action::Release(anns.id.clone()),
        Action::Delete(anns.id.clone()),
        Action::Move {
            id: anns.id.clone(),
            outq: q("QGPL", "PRT01"),
            first: false,
        },
    ] {
        assert_eq!(
            refused(c.act(&a).unwrap_err()),
            (Status::NotAuthorized, "CPF3303".into()),
            "{a:?}"
        );
    }
    // Printing: not deleted, not moved.
    let printing = file(&mut c, "STS5");
    assert_eq!(printing.status, SpoolStatus::Printing);
    for a in [
        Action::Delete(printing.id.clone()),
        Action::Move {
            id: printing.id.clone(),
            outq: q("QGPL", "QPRINT"),
            first: false,
        },
    ] {
        assert_eq!(
            refused(c.act(&a).unwrap_err()),
            (Status::InUse, "CPF3330".into())
        );
    }
    // Moves: to the end of a queue, and to its top.
    let prt01 = ListSpooledFiles {
        outq: Some(q("QGPL", "PRT01")),
        ..list("*ALL")
    };
    for (name, first) in [("RPT002", false), ("RPT003", true)] {
        let f = file(&mut c, name);
        c.act(&Action::Move {
            id: f.id.clone(),
            outq: q("QGPL", "PRT01"),
            first,
        })
        .unwrap();
        assert_eq!(file(&mut c, name).outq.name, "PRT01");
    }
    let on: Vec<String> = c
        .all_spooled_files(&prt01)
        .unwrap()
        .into_iter()
        .map(|f| f.id.file)
        .collect();
    assert_eq!(on.first().map(String::as_str), Some("RPT003"));
    assert_eq!(on.last().map(String::as_str), Some("RPT002"));
    let to_nowhere = Action::Move {
        id: file(&mut c, "RPT002").id,
        outq: q("QGPL", "NOSUCH"),
        first: false,
    };
    assert_eq!(
        refused(c.act(&to_nowhere).unwrap_err()),
        (Status::NotFound, "CPF3357".into())
    );
    // Delete, then it is not there.
    let doomed = file(&mut c, "RPT004");
    c.act(&Action::Delete(doomed.id.clone())).unwrap();
    assert!(
        c.all_spooled_files(&list("*ALL"))
            .unwrap()
            .iter()
            .all(|f| f.id != doomed.id)
    );
    assert_eq!(
        refused(c.act(&Action::Delete(doomed.id)).unwrap_err()),
        (Status::NotFound, "CPF3309".into())
    );
}

#[test]
fn actions_on_several_files_run_side_by_side() {
    let s = start(cfg());
    let mut c = bob(&s);
    let mut ids: Vec<SpooledFileId> = (5..=9)
        .map(|i| file(&mut c, &format!("RPT{i:03}")).id)
        .collect();
    ids.push(file(&mut c, "ANNRPT03").id);
    // Every request out before any reply is waited for.
    let pending: Vec<_> = ids
        .iter()
        .map(|id| {
            c.spool_action(&Action::Hold {
                id: id.clone(),
                immediately: true,
            })
            .unwrap()
        })
        .collect();
    let results: Vec<Result<(), Error>> = pending
        .into_iter()
        .map(|p| Action::outcome(p.wait(Duration::from_secs(5)).unwrap()))
        .collect();
    assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 5);
    let e = results.into_iter().find_map(Result::err).unwrap();
    assert_eq!(refused(e), (Status::NotAuthorized, "CPF3303".into()));
}

#[test]
fn batches_join_byte_for_byte_into_the_whole_file() {
    let s = start(cfg());
    let mut c = bob(&s);
    for name in SCS_FILES {
        let f = file(&mut c, name);
        let whole = range(&mut c, &f.id, 1, 0).unwrap();
        assert_eq!(whole.pages, f.pages, "{name}: the listing's pages");
        for batch in [25, 7, 2, 1] {
            let mut joined = Vec::new();
            let mut first = 1;
            while first <= whole.pages {
                let r = range(&mut c, &f.id, first, first + batch - 1).unwrap();
                assert_eq!(r.pages, whole.pages);
                if first == 1 {
                    joined.extend_from_slice(&r.data);
                } else {
                    let (state, pages) = strip_state(&r.data, &whole.data[joined.len()..]);
                    assert!(!state.is_empty());
                    joined.extend_from_slice(pages);
                }
                first += batch;
            }
            assert!(joined == whole.data, "{name} in batches of {batch}");
        }
    }
}

/// `name` read by the batched reader in batches of `batch`, as received,
/// and the progress it reported.
fn read_as_received(c: &mut Client, name: &str, batch: u32) -> (Vec<u8>, Vec<Progress>, ReadEnd) {
    let f = file(c, name);
    let mut sink = Vec::new();
    let mut seen = Vec::new();
    let end = spool::read(
        |r| c.read_spooled_file(r),
        &f.id,
        ReadFormat::Scs,
        batch,
        &mut sink,
        |p| seen.push(p),
        &AtomicBool::new(false),
    )
    .unwrap();
    (sink, seen, end)
}

#[test]
fn the_reader_writes_batches_as_received_and_reports_progress() {
    let s = start(cfg());
    let mut c = bob(&s);
    let (data, seen, end) = read_as_received(&mut c, "QSYSPRT", spool::BATCH_PAGES);
    assert_eq!(
        end,
        ReadEnd::Done {
            format: ReadFormat::Scs,
            pages: 30
        }
    );
    let p = |pages_read| Progress {
        pages_read,
        total: 30,
    };
    assert_eq!(seen, [p(25), p(30)]);
    // As received: the second batch's leading state included.
    let id = file(&mut c, "QSYSPRT").id;
    let mut expect = range(&mut c, &id, 1, 25).unwrap().data;
    expect.extend(range(&mut c, &id, 26, 50).unwrap().data);
    assert!(data == expect);
    assert!(data.len() > range(&mut c, &id, 1, 0).unwrap().data.len());
}

#[test]
fn batches_as_received_render_the_whole_files_pages() {
    let s = start(cfg());
    let mut c = bob(&s);
    let o = scs::Options::default();
    for name in SCS_FILES {
        let id = file(&mut c, name).id;
        let whole = scs::parse(&range(&mut c, &id, 1, 0).unwrap().data, &o);
        for batch in [spool::BATCH_PAGES, 2] {
            let (data, _, _) = read_as_received(&mut c, name, batch);
            let batched = scs::parse(&data, &o);
            assert_eq!(batched.pages.len(), whole.pages.len(), "{name} by {batch}");
            assert!(batched == whole, "{name} in batches of {batch}");
            assert_eq!(scs::to_text(&batched), scs::to_text(&whole));
        }
    }
}

/// Every page a batch of its own: each batch after the first starts with
/// QS/400's leading state, whose move to the line is AVPP (`X'34C4'`), and
/// the pages lay out as the file read whole, a page begun by overflow
/// (`OVERFLOW`'s page 26) included.
#[test]
fn one_page_batches_render_the_whole_files_pages() {
    let s = start(cfg());
    let mut c = bob(&s);
    let o = scs::Options::default();
    for name in SCS_FILES {
        let id = file(&mut c, name).id;
        let whole = scs::parse(&range(&mut c, &id, 1, 0).unwrap().data, &o);
        let (data, _, _) = read_as_received(&mut c, name, 1);
        assert!(scs::parse(&data, &o) == whole, "{name} a page at a time");
    }
}

#[test]
fn cancel_in_a_batch_and_between_batches() {
    let s = start(cfg());
    let mut c = bob(&s);
    let id = file(&mut c, "QSYSPRT").id;

    // In a batch: the gate holds the read once it has started; the cancel
    // goes out, and the batch that comes back is discarded.
    s.c.gate.set(false);
    let cancel = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&cancel);
    let gate = Arc::clone(&s.c);
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(150));
        flag.store(true, Ordering::SeqCst);
        std::thread::sleep(Duration::from_millis(150));
        gate.gate.set(true);
    });
    let mut sink = Vec::new();
    let end = spool::read(
        |r| c.read_spooled_file(r),
        &id,
        ReadFormat::Scs,
        spool::BATCH_PAGES,
        &mut sink,
        |_| {},
        &cancel,
    )
    .unwrap();
    assert_eq!(
        end,
        ReadEnd::Cancelled(Progress {
            pages_read: 0,
            total: 0
        })
    );
    assert!(sink.is_empty(), "nothing of the cancelled batch written");

    // Between batches: what was read stays; nothing after it.
    let cancel = AtomicBool::new(false);
    let mut sink = Vec::new();
    let end = spool::read(
        |r| c.read_spooled_file(r),
        &id,
        ReadFormat::Scs,
        10,
        &mut sink,
        |p| {
            if p.pages_read == 10 {
                cancel.store(true, Ordering::SeqCst);
            }
        },
        &cancel,
    )
    .unwrap();
    assert_eq!(
        end,
        ReadEnd::Cancelled(Progress {
            pages_read: 10,
            total: 30
        })
    );
    assert!(sink == range(&mut c, &id, 1, 10).unwrap().data);
    // The session goes on.
    assert_eq!(file(&mut c, "QSYSPRT").pages, 30);
}

#[test]
fn a_read_runs_beside_a_long_command() {
    let s = start(cfg());
    let c = Arc::new(Mutex::new(bob(&s)));
    let at = Instant::now();
    let command = c.lock().unwrap().run_command("DLYJOB DLY(2)").unwrap();
    let id = file(&mut c.lock().unwrap(), "QSYSPRT").id;
    let mut sink = Vec::new();
    // The application's way: the client locked only to send each batch.
    let end = spool::read(
        |r| c.lock().unwrap().read_spooled_file(r),
        &id,
        ReadFormat::Scs,
        5,
        &mut sink,
        |_| {},
        &AtomicBool::new(false),
    )
    .unwrap();
    assert!(matches!(end, ReadEnd::Done { pages: 30, .. }));
    assert!(
        at.elapsed() < Duration::from_secs(2),
        "the read did not wait"
    );
    assert!(command.try_wait().is_none(), "the command still runs");
    let o = CommandOutcome::from_reply(command.wait_done().unwrap()).unwrap();
    assert_eq!(o.status, Status::Ok);
    assert!(at.elapsed() >= Duration::from_secs(2));
}

#[test]
fn a_reply_past_the_limit_is_refused_and_the_session_resumes() {
    let s = start(cfg());
    let mut t = s.target();
    t.options.max_reply = 2 << 20;
    let mut c = Client::new(t);
    c.sign_on("BOB", &Secret::from("SECRET1")).unwrap();
    *s.c.products.lock().unwrap() = 150_000;
    assert!(matches!(c.system_info(), Err(Error::ReplyTooLarge)));
    let closed = c.connection().and_then(|n| n.closed());
    assert!(
        matches!(closed, Some(Closed::ReplyTooLarge(_))),
        "{closed:?}"
    );
    assert!(c.is_signed_on());
    // The next request resumes the session on a new connection.
    *s.c.products.lock().unwrap() = 3;
    assert_eq!(c.system_info().unwrap().products.len(), 3);
    assert_eq!(c.resumes(), 1);
    // A reply under the limit, split, is joined as before.
    *s.c.products.lock().unwrap() = 50_000;
    assert_eq!(c.system_info().unwrap().products.len(), 50_000);
}
