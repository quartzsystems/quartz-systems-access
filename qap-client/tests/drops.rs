// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! Connections that close under their requests, by the test system's
//! forced close (`System::drop_connection` and `drop_all`: the socket
//! closes mid-request with no reply and no event): the session resumes,
//! read-only requests are sent once more, and nothing that changes state
//! is.

mod common;

use std::sync::atomic::AtomicBool;
use std::time::Duration;

use common::{Server, cfg, start, until};
use qap_client::proto::Op;
use qap_client::proto::ops::spool::{ListSpooledFiles, ReadFormat, ReadSpooledFile, SpooledData};
use qap_client::spool::{self, ReadEnd};
use qap_client::{Client, Closed, Error, Secret, retryable};

fn bob(s: &Server) -> Client {
    let mut c = Client::new(s.target());
    c.sign_on("BOB", &Secret::from("SECRET1")).unwrap();
    c
}

#[test]
fn only_read_only_operations_are_retried() {
    for op in [
        Op::ListSpooledFiles,
        Op::ListOutputQueues,
        Op::ListSchemas,
        Op::ListTables,
        Op::ListColumns,
        Op::ReadSpooledFile,
        Op::SystemInfo,
        Op::PasswordStatus,
        Op::DescribeCommand,
        Op::CheckCommand,
        Op::JobInfo,
    ] {
        assert!(retryable(op), "{op:?}");
    }
    for op in [
        Op::SignOn,
        Op::SignOff,
        Op::ChangePassword,
        Op::Resume,
        Op::Attach,
        Op::TelnetTicket,
        Op::RunCommand,
        Op::HoldSpooledFile,
        Op::ReleaseSpooledFile,
        Op::DeleteSpooledFile,
        Op::MoveSpooledFile,
        Op::SetOptions,
        Op::Prepare,
        Op::Execute,
        Op::Fetch,
        Op::CloseCursor,
        Op::Release,
        Op::Commit,
        Op::Rollback,
    ] {
        assert!(!retryable(op), "{op:?}");
    }
}

#[test]
fn a_request_on_a_dropped_connection_resumes_and_goes_once_more() {
    let lines = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    let keep = std::sync::Arc::clone(&lines);
    qsa_net::log::set_sink(qsa_net::log::Level::Info, move |_, l| {
        keep.lock().unwrap().push(l.to_owned());
    });
    let s = start(cfg());
    let mut c = bob(&s);
    assert_eq!(s.c.drop_all(), 1);
    // Sent at once, before the client has seen the close: it is cut off,
    // and goes again on a new connection that resumed the session.
    let info = c.system_info().unwrap();
    assert_eq!(info.system_name, "QSHQ400");
    assert_eq!(c.resumes(), 1);
    assert_eq!(s.jobs().len(), 2);
    // The close is logged at info, with what this side saw (other tests
    // log to the same sink: this server's port tells them apart).
    let mine = format!(
        "qap: 127.0.0.1 port {}: connection closed by the system",
        s.port
    );
    let closed: Vec<String> = lines
        .lock()
        .unwrap()
        .iter()
        .filter(|l| l.starts_with(&mine))
        .cloned()
        .collect();
    assert_eq!(closed.len(), 1, "{closed:?}");
    assert!(
        closed[0].contains("; end of stream") || closed[0].contains("; read failed"),
        "{}",
        closed[0]
    );
    let files = c
        .all_spooled_files(&ListSpooledFiles {
            user: "*CURRENT".into(),
            ..ListSpooledFiles::default()
        })
        .unwrap();
    assert!(!files.is_empty());
}

#[test]
fn a_spooled_file_batch_cut_off_is_read_once_more() {
    let s = start(cfg());
    let mut c = bob(&s);
    let all = c
        .all_spooled_files(&ListSpooledFiles {
            user: "*CURRENT".into(),
            ..ListSpooledFiles::default()
        })
        .unwrap();
    let f = all.iter().find(|f| f.id.file == "QSYSPRT").unwrap().clone();
    let whole = c
        .read_spooled_file(&ReadSpooledFile {
            id: f.id.clone(),
            format: ReadFormat::Scs,
            first_page: 1,
            last_page: 0,
        })
        .unwrap()
        .wait_done()
        .unwrap()
        .decode::<SpooledData>()
        .unwrap()
        .data;

    // The gate holds the first batch once it has started; the connection
    // is dropped under it, and the gate opened again.
    let job = s.last_job();
    s.c.gate.set(false);
    let sys = std::sync::Arc::clone(&s.c);
    let t = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(150));
        assert!(sys.drop_connection(job));
        std::thread::sleep(Duration::from_millis(50));
        sys.gate.set(true);
    });
    let mut sink = Vec::new();
    let end = spool::read(
        |r| c.read_spooled_file(r),
        &f.id,
        ReadFormat::Scs,
        spool::BATCH_PAGES,
        &mut sink,
        |_| {},
        &AtomicBool::new(false),
    )
    .unwrap();
    t.join().unwrap();
    assert!(matches!(end, ReadEnd::Done { pages: 30, .. }), "{end:?}");
    assert_eq!(c.resumes(), 1);
    // Batches joined as the whole file, once each later batch's state is
    // there: same page count, and every page's text.
    let o = scs::Options::default();
    assert_eq!(
        scs::to_text(&scs::parse(&sink, &o)),
        scs::to_text(&scs::parse(&whole, &o))
    );
}

#[test]
fn a_command_cut_off_is_not_run_again() {
    let s = start(cfg());
    let mut c = bob(&s);
    let job = s.last_job();
    let p = c.run_command("DLYJOB DLY(30)").unwrap();
    assert_eq!(
        s.c.wait_running(job, Duration::from_secs(5)).as_deref(),
        Some("DLYJOB")
    );
    assert!(s.c.drop_connection(job));
    match p.wait_done() {
        Err(Error::Closed(Closed::Quietly)) => {}
        other => panic!("{other:?}"),
    }
    // The next request resumes; the command was not sent again.
    assert_eq!(c.system_info().unwrap().system_name, "QSHQ400");
    assert_eq!(c.resumes(), 1);
    let new = s.last_job();
    assert_ne!(new, job);
    assert_eq!(s.c.wait_running(new, Duration::from_millis(300)), None);
}

#[test]
fn an_attached_connection_re_attaches_and_retries_job_info() {
    let s = start(cfg());
    let mut c = bob(&s);
    let mut a = c.attach().unwrap();
    let job = s.last_job();
    let first = a.job().job.number.clone();
    assert!(s.c.drop_connection(job));
    let info = a.job_info().unwrap();
    assert_eq!(a.attaches(), 2, "a new connection, attached");
    assert_ne!(info.job.number, first, "with a new job");
    assert_eq!(info.user, "BOB");
    // The shared connection did not move.
    assert_eq!(c.resumes(), 0);
    until("the drop seen", || s.ended().len() == 1);
}
