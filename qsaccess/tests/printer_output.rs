// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! Printer Output against QS/400's test system (qap-client's test server):
//! the filter row's requests and what they list, a read on its own thread
//! written as received to a temporary file, Save as PDF and Save as Text
//! laid out as a printer session lays out the same data, and a read the
//! system refuses shown with the system's message as it sent it.

#[path = "../../qap-client/tests/common/mod.rs"]
mod common;

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use common::{Server, cfg, start};
use qap_client::proto::ops::spool::{ReadFormat, SpoolStatus, SpooledFile};
use qap_client::spool::ReadEnd;
use qap_client::{Client, Secret};
use qsa_config::System;
use qsa_config::system::CaCertificate;
use qsaccess::qap::{Handle, lock};
use qsaccess::splf::model::{self, Filter};
use qsaccess::splf::output::{self, Purpose, ReadEvent, Reading};

fn system(s: &Server) -> System {
    let mut sys = System::new("127.0.0.1");
    sys.ports.qap = s.port;
    sys.ca_certificates = vec![CaCertificate::Path(
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../qsa-net/tests/data/test-ca.pem"
        )
        .into(),
    )];
    sys
}

fn bob(s: &Server) -> Handle {
    let mut c = Client::new(s.target());
    c.sign_on("BOB", &Secret::from("SECRET1")).unwrap();
    Arc::new(Mutex::new(c))
}

fn listed(h: &Handle, f: &Filter) -> (u32, Vec<SpooledFile>) {
    let r = lock(h).list_spooled_files(&f.request(0, 100)).unwrap();
    (r.total, r.files)
}

fn named(h: &Handle, name: &str) -> SpooledFile {
    let all = Filter {
        user: "*ALL".into(),
        ..Filter::default()
    };
    lock(h)
        .all_spooled_files(&all.request(0, 0))
        .unwrap()
        .into_iter()
        .find(|f| f.id.file == name)
        .unwrap()
}

/// Waits for `r` to end; how it ended.
fn finish(r: &mut Reading) -> ReadEnd {
    let end = Instant::now() + Duration::from_secs(10);
    loop {
        for e in r.poll() {
            if let ReadEvent::Done(d) = e {
                return d.unwrap();
            }
        }
        assert!(Instant::now() < end, "the read did not end");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn the_filter_row_lists_what_it_says() {
    let s = start(cfg());
    let h = bob(&s);
    let (total, files) = listed(&h, &Filter::default());
    assert!(files.iter().all(|f| f.user == "BOB"));
    assert_eq!(total as usize, files.len());
    let held = Filter {
        status: Some(SpoolStatus::Held),
        user: "*ALL".into(),
        ..Filter::default()
    };
    assert_eq!(listed(&h, &held).1.len(), 1);
    let on = Filter::on_queue(&model::object_named("QGPL/PRT01").unwrap());
    let (_, prt01) = listed(&h, &on);
    assert!(prt01.iter().all(|f| f.outq.name == "PRT01"));
    assert!(prt01.iter().any(|f| f.status == SpoolStatus::Printing));
    let data = Filter {
        user_data: "MIXED".into(),
        form_type: "*std".into(),
        ..Filter::default()
    };
    assert_eq!(listed(&h, &data).1[0].id.file, "MIXED");
    // The rows the list shows, status words coloured.
    let rows: Vec<_> = prt01
        .iter()
        .map(|f| model::row(f, &model::utc_time))
        .collect();
    let printing = rows.iter().find(|r| r[3].text == "Printing").unwrap();
    assert_eq!(printing[3].status, qsa_ui::Status::Active);
    let message = rows.iter().find(|r| r[3].text == "Message").unwrap();
    assert_eq!(message[3].status, qsa_ui::Status::Error);
}

#[test]
fn save_as_pdf_and_text_as_a_printer_session_lays_them_out() {
    let s = start(cfg());
    let h = bob(&s);
    let f = named(&h, "QSYSPRT");
    let options = model::scs_options(37, qsa_config::qsp::Paper::Letter);
    let mut r = Reading::start(&h, f.clone(), Purpose::Open.format(&f)).unwrap();
    assert_eq!(r.format, ReadFormat::Scs);
    assert_eq!(
        finish(&mut r),
        ReadEnd::Done {
            format: ReadFormat::Scs,
            pages: 30
        }
    );
    assert_eq!(r.progress.pages_read, 30);
    assert!(r.path.starts_with(output::temporary_dir()));

    // What the temporary file holds renders as the whole file does.
    let whole = {
        let req = qap_client::proto::ops::spool::ReadSpooledFile {
            id: f.id.clone(),
            format: ReadFormat::Scs,
            first_page: 1,
            last_page: 0,
        };
        let reply = lock(&h)
            .read_spooled_file(&req)
            .unwrap()
            .wait_done()
            .unwrap();
        reply
            .decode::<qap_client::proto::ops::spool::SpooledData>()
            .unwrap()
            .data
    };
    let doc = output::document(&r.path, &options).unwrap();
    assert!(doc == scs::parse(&whole, &options));

    let dir = std::env::temp_dir().join(format!("qsaccess-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let pdf = dir.join("QSYSPRT.pdf");
    let m = output::finish(&r, &Purpose::SavePdf(pdf.clone()), &options)
        .unwrap()
        .unwrap();
    assert_eq!(m.id, qsa_msg::id::SPOOL_SAVED);
    let bytes = std::fs::read(&pdf).unwrap();
    assert!(bytes.starts_with(b"%PDF"));
    // A printer session renders the same page model the same way; the
    // PDFs differ only in when they were made.
    let session = qsa_print::pdf::render(
        &scs::parse(&whole, &options),
        &qsa_print::pdf::Options {
            title: format!("QSYSPRT {}", model::job_text(&f.id.job)),
            margin: None,
            paper: scs::Paper::Letter,
        },
    )
    .unwrap();
    assert!(bytes.len().abs_diff(session.len()) < 64);

    let txt = dir.join("QSYSPRT.txt");
    output::finish(&r, &Purpose::SaveText(txt.clone()), &options).unwrap();
    let text = std::fs::read_to_string(&txt).unwrap();
    assert_eq!(text, scs::to_text(&scs::parse(&whole, &options)));
    assert_eq!(text.matches('\u{000C}').count(), 29);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_refused_read_shows_the_systems_message_as_sent() {
    let s = start(cfg());
    let sys = system(&s);
    let h = bob(&s);
    for (name, status, id) in [
        ("OPENFILE", qap_client::Status::InUse, "TCP1A57"),
        ("AFPINV", qap_client::Status::Error, "TCP1A56"),
        ("ANNRPT01", qap_client::Status::NotAuthorized, "CPF3303"),
    ] {
        let f = named(&h, name);
        let format = Purpose::Open.format(&f);
        let mut r = Reading::start(&h, f.clone(), format).unwrap();
        let end = Instant::now() + Duration::from_secs(10);
        let e = loop {
            if let Some(ReadEvent::Done(d)) = r.poll().into_iter().last() {
                break d.unwrap_err();
            }
            assert!(Instant::now() < end);
            std::thread::sleep(Duration::from_millis(10));
        };
        assert_eq!(e.status(), Some(status), "{name}");
        let x = output::explain_read(&sys, &f, format, &e);
        let m = x.server.as_ref().expect("the system's own message");
        assert_eq!(m.id, id, "{name}");
        assert!(x.line().starts_with(&format!("{id}: ")), "{}", x.line());
    }
    // Without a message of the system's own, this program says why.
    let f = named(&h, "OPENFILE");
    let bare = qap_client::Error::Status {
        status: qap_client::Status::InUse,
        message: None,
    };
    let x = output::explain_read(&sys, &f, ReadFormat::Scs, &bare);
    assert_eq!(x.message.id, qsa_msg::id::SPOOLED_FILE_OPEN);
    let bare = qap_client::Error::Status {
        status: qap_client::Status::Error,
        message: None,
    };
    let x = output::explain_read(&sys, &named(&h, "AFPINV"), ReadFormat::Pdf, &bare);
    assert_eq!(
        x.message.text,
        "Spooled file AFPINV cannot be shown as PDF."
    );
}

#[test]
fn a_read_cancelled_keeps_nothing_of_its_batch() {
    let s = start(cfg());
    let h = bob(&s);
    let f = named(&h, "QSYSPRT");
    s.c.gate.set(false);
    let mut r = Reading::start(&h, f, ReadFormat::Scs).unwrap();
    std::thread::sleep(Duration::from_millis(150));
    r.cancel();
    std::thread::sleep(Duration::from_millis(100));
    s.c.gate.set(true);
    assert!(matches!(finish(&mut r), ReadEnd::Cancelled(_)));
    assert_eq!(std::fs::metadata(&r.path).unwrap().len(), 0);
}

/// The watch a filter keeps (version 6), and the files it takes from one.
#[test]
fn a_filter_watches_one_queue_or_one_user_or_polls() {
    use qap_client::proto::ops::spool::SpoolTarget;
    use qap_client::proto::types::ObjectName;
    let q = |lib: &str, n: &str| SpoolTarget::Queue {
        outq: ObjectName {
            library: lib.into(),
            name: n.into(),
        },
    };
    let user = |p: &str| SpoolTarget::User { profile: p.into() };
    assert_eq!(Filter::default().target(), Some(user("*CURRENT")));
    let f = |u: &str, o: &str| Filter {
        user: u.into(),
        outq: o.into(),
        ..Filter::default()
    };
    assert_eq!(f("ann", "*ALL").target(), Some(user("ANN")));
    assert_eq!(f("*ALL", "*ALL").target(), None, "every user everywhere");
    assert_eq!(f("*ALL", "QGPL/PRT01").target(), Some(q("QGPL", "PRT01")));
    assert_eq!(
        f("*CURRENT", "QGPL/PRT01").target(),
        Some(q("QGPL", "PRT01"))
    );
}

/// A watch's events keep the page shown as the system lists it, with no
/// timer: in place for a change on the page, the page asked for again when
/// files come, go or move.
#[test]
fn a_watch_keeps_the_page_as_the_system_lists_it() {
    use model::{Effect, apply};
    use qap_client::proto::ops::spool::{
        DeviceType, SpoolChange, SpooledFileChanged, SpooledFileId,
    };
    use qap_client::proto::types::{JobId, ObjectName};
    use qap_client::{Watch, WatchEvent};
    let s = start(qapd::Config {
        watch_poll: Duration::from_millis(20),
        ..cfg()
    });
    let h = bob(&s);
    let mine = Filter::default();
    let prt01 = Filter {
        user: "*ALL".into(),
        outq: "QGPL/PRT01".into(),
        ..Filter::default()
    };
    let qprint = Filter {
        user: "*ALL".into(),
        outq: "QGPL/QPRINT".into(),
        ..Filter::default()
    };
    for f in [&mine, &prt01, &qprint] {
        lock(&h).watch(Watch::Spool(f.target().unwrap())).unwrap();
    }
    lock(&h).watch_events();
    let changes = |n: usize| -> Vec<SpooledFileChanged> {
        let end = Instant::now() + Duration::from_secs(5);
        let mut got = Vec::new();
        while got.len() < n && Instant::now() < end {
            let mut c = lock(&h);
            c.poll();
            got.extend(c.watch_events().into_iter().filter_map(|e| match e {
                WatchEvent::SpoolChanged(c) => Some(c),
                _ => None,
            }));
            drop(c);
            std::thread::sleep(Duration::from_millis(10));
        }
        got
    };
    // Each page as shown, kept by the events alone.
    let mut pages: Vec<(Filter, u32, Vec<SpooledFile>)> = [&mine, &prt01, &qprint]
        .into_iter()
        .map(|f| {
            let (t, files) = listed(&h, f);
            (f.clone(), t, files)
        })
        .collect();
    let take = |pages: &mut Vec<(Filter, u32, Vec<SpooledFile>)>, cs: &[SpooledFileChanged]| {
        let mut relist = vec![false; pages.len()];
        for c in cs {
            for (i, (f, total, files)) in pages.iter_mut().enumerate() {
                if Some(&c.target) == f.target().as_ref()
                    && apply(f, "BOB", files, total, c) == Effect::Relist
                {
                    relist[i] = true;
                }
            }
        }
        // What a window does with Relist: asks for the page once.
        for (i, (f, total, files)) in pages.iter_mut().enumerate() {
            if relist[i] {
                (*total, *files) = listed(&h, f);
            }
        }
        relist
    };
    let id = s.c.spool_add(qapd::testing::spool::File {
        id: SpooledFileId {
            job: JobId {
                name: "QPADEV0001".into(),
                user: "BOB".into(),
                number: "001234".into(),
            },
            file: "WATCHED".into(),
            number: 1,
        },
        outq: ObjectName {
            library: "QGPL".into(),
            name: "QPRINT".into(),
        },
        device: DeviceType::Ascii,
        pages: 1,
        copies: 1,
        data: b"hello\n".to_vec(),
        ..qapd::testing::spool::File::default()
    });
    // Added: BOB's list and QPRINT's ask for their pages; PRT01's does not.
    let relist = take(&mut pages, &changes(2));
    assert_eq!(relist, [true, false, true]);
    // Held: in place, nothing asked for.
    assert!(s.c.spool_change(&id, |f| f.status = SpoolStatus::Held));
    let relist = take(&mut pages, &changes(2));
    assert_eq!(relist, [false, false, false]);
    for (f, total, files) in &pages {
        assert_eq!((*total, files.clone()), listed(&h, f), "{f:?}");
    }
    // Moved from QPRINT to PRT01: Removed there, Added here, Changed in
    // BOB's own list (in place: its queue column).
    assert!(s.c.spool_change(&id, |f| f.outq.name = "PRT01".into()));
    let cs = changes(3);
    let of = |t: &Filter| {
        cs.iter()
            .filter(|c| Some(&c.target) == t.target().as_ref())
            .map(|c| c.change)
            .collect::<Vec<_>>()
    };
    assert_eq!(of(&qprint), [SpoolChange::Removed]);
    assert_eq!(of(&prt01), [SpoolChange::Added]);
    assert_eq!(of(&mine), [SpoolChange::Changed]);
    let relist = take(&mut pages, &cs);
    assert_eq!(relist, [false, true, true]);
    for (f, total, files) in &pages {
        assert_eq!((*total, files.clone()), listed(&h, f), "{f:?}");
    }
    // Deleted: gone from the two lists that had it.
    assert!(s.c.spool_remove(&id));
    let relist = take(&mut pages, &changes(2));
    assert_eq!(relist, [true, true, false]);
    for (f, total, files) in &pages {
        assert_eq!((*total, files.clone()), listed(&h, f), "{f:?}");
        assert!(files.iter().all(|x| x.id != id));
    }
}
