// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! Run SQL's connection worker against QS/400's test system: statements
//! run in order with their messages as sent, results in blocks with Fetch
//! More and Fetch All, a failure that stops Run All, cancel keeping the
//! rows so far (marked incomplete), the commit level and `InUse`, a drop
//! mid-fetch (`QAD7010`, nothing run again), and `SessionEnded` after a
//! sign-off on the shared connection.

#[path = "../../qap-client/tests/common/mod.rs"]
mod common;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use common::{Server, cfg, start};
use qap_client::proto::ops::sql::{CommitLevel, SetOptions};
use qap_client::{Client, Secret};
use qsaccess::runsql::script;
use qsaccess::runsql::worker::{Cmd, Ev, Line, Standing, Worker};

struct W {
    tx: mpsc::Sender<Cmd>,
    rx: mpsc::Receiver<Ev>,
    cancel: Arc<AtomicBool>,
    c: Client,
}

fn window(s: &Server) -> W {
    let mut c = Client::new(s.target());
    c.sign_on("BOB", &Secret::from("SECRET1")).unwrap();
    let q = c.sql_connection().unwrap();
    let (tx, crx) = mpsc::channel();
    let (etx, rx) = mpsc::channel();
    let cancel = Arc::new(AtomicBool::new(false));
    let w = Worker::new(
        q,
        qsa_config::System::new("SYS"),
        crx,
        etx,
        Arc::clone(&cancel),
        Box::new(|| {}),
    );
    std::thread::spawn(move || w.run());
    W { tx, rx, cancel, c }
}

/// Events until `done` says so, at most ten seconds.
fn until(w: &W, mut done: impl FnMut(&Ev) -> bool) -> Vec<Ev> {
    let end = Instant::now() + Duration::from_secs(10);
    let mut out = Vec::new();
    while Instant::now() < end {
        if let Ok(e) = w.rx.recv_timeout(Duration::from_millis(50)) {
            let stop = done(&e);
            out.push(e);
            if stop {
                return out;
            }
        }
    }
    panic!("timed out; events so far: {out:?}");
}

fn run(w: &W, sql: &str, stop: bool) -> Vec<Ev> {
    w.tx.send(Cmd::Run {
        statements: script::split(sql),
        stop_on_error: stop,
        first_rows: 500,
    })
    .unwrap();
    until(w, |e| matches!(e, Ev::Done { .. }))
}

fn lines(evs: &[Ev]) -> Vec<&Line> {
    evs.iter()
        .filter_map(|e| match e {
            Ev::Message(l) => Some(l),
            _ => None,
        })
        .collect()
}

fn rows(evs: &[Ev], result: usize) -> usize {
    evs.iter()
        .map(|e| match e {
            Ev::Rows { result: r, rows } if *r == result => rows.len(),
            _ => 0,
        })
        .sum()
}

fn standing(evs: &[Ev], result: usize) -> Option<Standing> {
    evs.iter().rev().find_map(|e| match e {
        Ev::Standing {
            result: r,
            standing,
        } if *r == result => Some(standing.clone()),
        _ => None,
    })
}

#[test]
fn a_script_runs_with_its_messages_and_results() {
    let s = start(cfg());
    let w = window(&s);
    let evs = run(
        &w,
        "SELECT * FROM HLINV.ASSETS;\n\
         INSERT INTO HLTEST.JRN VALUES(1, 'one');\n\
         SELEKT nonsense;\n\
         SELECT * FROM HLTEST.JRN",
        false,
    );
    assert_eq!(rows(&evs, 0), 12);
    assert_eq!(standing(&evs, 0), Some(Standing::Complete));
    let l = lines(&evs);
    assert_eq!(l.len(), 4);
    assert_eq!(
        (l[0].statement, l[0].rows, l[0].failed),
        (1, Some(12), false)
    );
    assert_eq!(
        (l[1].id.as_str(), l[1].text.as_str(), l[1].rows),
        ("QAD7020", "1 rows changed.", Some(1))
    );
    // The failure as the system sent it, with SQLSTATE and SQLCODE, and
    // its line in the script.
    assert!(l[2].failed);
    assert_eq!(
        (l[2].id.as_str(), l[2].sqlstate.as_str(), l[2].sqlcode),
        ("SQL0104", "42601", Some(-104))
    );
    assert_eq!(l[2].script_line, 3);
    // Without Stop at First Error, the run went on.
    assert_eq!(rows(&evs, 1), 1);
    assert!(matches!(evs.last(), Some(Ev::Done { cancelled: false })));

    // With it, the run ends at the failure.
    let evs = run(&w, "SELEKT 1; SELECT * FROM HLINV.ASSETS", true);
    assert_eq!(lines(&evs).len(), 1);
    assert!(!evs.iter().any(|e| matches!(e, Ev::Result { .. })));
}

#[test]
fn fetch_more_and_fetch_all() {
    let s = start(cfg());
    let w = window(&s);
    let evs = run(&w, "SELECT * FROM HLTEST.BIG", false);
    assert_eq!(rows(&evs, 0), 500);
    assert_eq!(standing(&evs, 0), Some(Standing::More));
    w.tx.send(Cmd::Fetch {
        result: 0,
        all: false,
    })
    .unwrap();
    let evs = until(&w, |e| matches!(e, Ev::Done { .. }));
    assert_eq!(rows(&evs, 0), 500);
    w.tx.send(Cmd::Fetch {
        result: 0,
        all: true,
    })
    .unwrap();
    let evs = until(&w, |e| matches!(e, Ev::Done { .. }));
    assert_eq!(rows(&evs, 0), 2000);
    assert_eq!(standing(&evs, 0), Some(Standing::Complete));
}

#[test]
fn a_cancelled_query_keeps_its_rows_marked_incomplete() {
    let s = start(cfg());
    let w = window(&s);
    w.tx.send(Cmd::Run {
        statements: script::split("SELECT * FROM HLTEST.SLOW"),
        stop_on_error: false,
        first_rows: 0,
    })
    .unwrap();
    // Fetch everything (no limit) so the fetch runs long, then cancel.
    let _ = until(&w, |e| matches!(e, Ev::Result { .. }));
    std::thread::sleep(Duration::from_millis(300));
    w.cancel.store(true, Ordering::SeqCst);
    let evs = until(&w, |e| matches!(e, Ev::Done { .. }));
    assert_eq!(
        standing(&evs, 0),
        Some(Standing::Incomplete("cancelled".into()))
    );
    let got = rows(&evs, 0);
    assert!(got > 0 && got < 300, "{got}");
    assert!(lines(&evs).iter().any(|l| l.id == "SQL0952"));
    assert!(matches!(evs.last(), Some(Ev::Done { cancelled: true })));
}

#[test]
fn the_commit_level_and_in_use() {
    let s = start(cfg());
    let w = window(&s);
    w.tx.send(Cmd::SetOptions(SetOptions {
        commit: CommitLevel::ReadCommitted,
        ..SetOptions::default()
    }))
    .unwrap();
    let evs = until(&w, |e| matches!(e, Ev::Options(_)));
    assert!(
        matches!(evs.last(), Some(Ev::Options(Ok(o))) if o.commit == CommitLevel::ReadCommitted)
    );
    run(&w, "INSERT INTO HLTEST.JRN VALUES(7, 'x')", false);
    let evs = until(&w, |e| matches!(e, Ev::Uncommitted(true)));
    assert!(!evs.is_empty());
    w.tx.send(Cmd::SetOptions(SetOptions::default())).unwrap();
    let evs = until(&w, |e| matches!(e, Ev::Options(_)));
    match evs.last() {
        Some(Ev::Options(Err(l))) => assert_eq!(l.id, "SQL0752"),
        other => panic!("{other:?}"),
    }
    w.tx.send(Cmd::Rollback).unwrap();
    let evs = until(&w, |e| matches!(e, Ev::Message(_)));
    assert_eq!(lines(&evs)[0].text, "Changes rolled back.");
    let _ = until(&w, |e| matches!(e, Ev::Uncommitted(false)));
    let evs = run(&w, "SELECT * FROM HLTEST.JRN", false);
    assert_eq!(rows(&evs, 0), 0);
}

#[test]
fn a_drop_mid_fetch_is_qad7010_and_nothing_runs_again() {
    let s = start(cfg());
    let w = window(&s);
    // A first block of 100 rows, then Fetch All, dropped under it.
    w.tx.send(Cmd::Run {
        statements: script::split("SELECT * FROM HLTEST.SLOW"),
        stop_on_error: false,
        first_rows: 100,
    })
    .unwrap();
    let evs = until(&w, |e| matches!(e, Ev::Done { .. }));
    assert_eq!(rows(&evs, 0), 100);
    w.tx.send(Cmd::Fetch {
        result: 0,
        all: true,
    })
    .unwrap();
    let job = *s.jobs().last().unwrap();
    std::thread::sleep(Duration::from_millis(200));
    assert!(s.c.drop_connection(job));
    let evs = until(&w, |e| matches!(e, Ev::Lost));
    assert_eq!(
        standing(&evs, 0),
        Some(Standing::Incomplete("the connection was lost".into()))
    );
    // The window keeps the 100 it has: no rows came after the drop.
    assert_eq!(rows(&evs, 0), 0);
    // Nothing more comes by itself; the next run attaches anew.
    std::thread::sleep(Duration::from_millis(400));
    assert!(
        w.rx.try_iter()
            .all(|e| !matches!(e, Ev::Result { .. } | Ev::Rows { .. }))
    );
    let jobs = s.jobs().len();
    let evs = run(&w, "SELECT * FROM HLINV.ASSETS", false);
    assert_eq!(rows(&evs, 1), 12);
    assert_eq!(s.jobs().len(), jobs + 1, "a new connection was attached");
}

#[test]
fn a_sign_off_on_the_shared_connection_ends_the_windows_session() {
    let s = start(cfg());
    let mut w = window(&s);
    let evs = run(&w, "SELECT * FROM HLINV.ASSETS", false);
    assert_eq!(rows(&evs, 0), 12);
    w.c.sign_off().unwrap();
    let evs = until(&w, |e| matches!(e, Ev::Ended(_)));
    match evs.last() {
        Some(Ev::Ended(why)) => assert_eq!(why.message().unwrap().id, "TCP1A58"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn closing_never_signs_off_and_rolls_back() {
    let s = start(cfg());
    let mut w = window(&s);
    w.tx.send(Cmd::SetOptions(SetOptions {
        commit: CommitLevel::ReadCommitted,
        ..SetOptions::default()
    }))
    .unwrap();
    let _ = until(&w, |e| matches!(e, Ev::Options(_)));
    run(&w, "INSERT INTO HLTEST.JRN VALUES(9, 'kept?')", false);
    w.tx.send(Cmd::Close(Some(false))).unwrap();
    let _ = until(&w, |e| matches!(e, Ev::Closed));
    // The session goes on, and the change was rolled back.
    let w2 = window_on(&s, &mut w.c);
    let evs = run(&w2, "SELECT * FROM HLTEST.JRN", false);
    assert_eq!(rows(&evs, 0), 0);
    assert_eq!(w.c.system_info().unwrap().system_name, "QSHQ400");
}

/// Another window on `c`'s session.
fn window_on(_s: &Server, c: &mut Client) -> W {
    let q = c.sql_connection().unwrap();
    let (tx, crx) = mpsc::channel();
    let (etx, rx) = mpsc::channel();
    let cancel = Arc::new(AtomicBool::new(false));
    let w = Worker::new(
        q,
        qsa_config::System::new("SYS"),
        crx,
        etx,
        Arc::clone(&cancel),
        Box::new(|| {}),
    );
    std::thread::spawn(move || w.run());
    W {
        tx,
        rx,
        cancel,
        c: Client::new(_s.target()),
    }
}
