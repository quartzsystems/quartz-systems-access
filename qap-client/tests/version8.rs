// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! QAP version 8 (draft) against QS/400's engine and its test system
//! (`qapd::testing::develop` and `jobs`): `Develop`'s library list in the
//! lane, `ReadMember` of a member over 1 MiB joined whole, `WriteMember`
//! of it in pieces and of a member read and written back with its dates
//! kept, the member commands, `Compile`'s diagnostics, `Jobs`, `Sql` 12
//! and 13; and the gate: the desktop app's default still agrees version
//! 6, and there every version 8 method is refused before it is sent.

mod common;

use common::{Server, cfg, start};
use qap_client::develop::{Develop, WriteOptions};
use qap_client::jobs::Jobs;
use qap_client::proto::ops::develop::{ListLibraries, ListMembers, ListObjects, SetLibraryList};
use qap_client::proto::ops::jobs::{JobLog, JobState, ListJobs};
use qap_client::proto::ops::sql::{ListRoutines, RoutineKind};
use qap_client::proto::types::{EndHow, JobId, MemberName, ObjectName, SourceRecord, Where};
use qap_client::proto::{Op, Service, VERSION};
use qap_client::{Attached, Client, CompileOutcome, Error, OFFERED_VERSION, Secret, Status};
use qapd::testing::develop::BIG_RECORDS;

fn client(s: &Server, max_version: u16) -> Client {
    let mut c = Client::new(s.target().with_max_version(max_version));
    c.sign_on("BOB", &Secret::from("SECRET1")).unwrap();
    c
}

fn v8(s: &Server) -> Client {
    client(s, VERSION)
}

fn obj(library: &str, name: &str) -> ObjectName {
    ObjectName {
        library: library.into(),
        name: name.into(),
    }
}

fn mbr(member: &str) -> MemberName {
    MemberName {
        file: obj("HLINV", "QRPGLESRC"),
        member: member.into(),
    }
}

fn message_id(e: &Error) -> Option<&str> {
    e.server_message().map(|m| m.id.as_str())
}

fn frames(c: &Client) -> u64 {
    c.connection().unwrap().frames_received()
}

#[test]
fn the_desktop_app_still_agrees_version_6() {
    let s = start(cfg());
    let mut c = Client::new(s.target());
    c.sign_on("BOB", &Secret::from("SECRET1")).unwrap();
    assert_eq!(OFFERED_VERSION, 6);
    assert_eq!(c.version(), Some(6));
    let h = c.hello().unwrap();
    assert!(!h.services.contains(&Service::Develop) && !h.services.contains(&Service::Jobs));
    // Asked for, the system's newest.
    let c = v8(&s);
    assert_eq!(c.version(), Some(9));
    assert!(c.hello().unwrap().services.contains(&Service::Develop));
}

#[test]
fn below_version_8_each_new_method_is_refused_before_it_is_sent() {
    let s = start(cfg());
    let mut c = Client::new(s.target());
    c.sign_on("BOB", &Secret::from("SECRET1")).unwrap();
    let before = frames(&c);
    let unsupported = |e: Error, op: Op| match e {
        Error::Unsupported { op: o, needs, has } => {
            assert_eq!((o, needs, has), (op, 8, 6));
        }
        other => panic!("{op:?}: {other:?}"),
    };
    unsupported(c.get_library_list().unwrap_err(), Op::GetLibraryList);
    unsupported(
        c.read_member(&mbr("ASSETS"), false).unwrap_err(),
        Op::ReadMember,
    );
    unsupported(c.compile("CRTBNDRPG").unwrap_err(), Op::Compile);
    unsupported(c.all_jobs("*CURRENT", None).unwrap_err(), Op::ListJobs);
    unsupported(
        c.list_indexes("HLINV", "ASSETS").unwrap_err(),
        Op::ListIndexes,
    );
    // Nothing went on the wire, so nothing came back.
    assert_eq!(frames(&c), before);
    // A window's own connection agrees what the shared one did.
    let mut a: Attached = c.attach().unwrap();
    let e = a.list_libraries(&ListLibraries::default()).unwrap_err();
    assert!(
        matches!(
            e,
            Error::Unsupported {
                needs: 8,
                has: 6,
                ..
            }
        ),
        "{e:?}"
    );
    assert!(e.to_string().contains("needs QAP version 8"), "{e}");
}

#[test]
fn the_library_list_is_the_connection_jobs_own() {
    let s = start(cfg());
    let mut c = v8(&s);
    let l = c.get_library_list().unwrap();
    assert_eq!(l.current, None);
    assert_eq!(l.system, ["QSYS", "QSYS2", "QHLPSYS", "QUSRSYS"]);
    assert_eq!(l.user, ["QGPL", "QTEMP"]);
    let set = SetLibraryList {
        current: Some("HLINV".into()),
        user: vec!["HLTEST".into(), "QGPL".into(), "QTEMP".into()],
    };
    let now = c.set_library_list(&set).unwrap();
    assert_eq!(
        (now.current, now.user),
        (set.current.clone(), set.user.clone())
    );
    // A window's connection has a job, and a list, of its own.
    let mut a = c.attach().unwrap();
    assert_eq!(a.get_library_list().unwrap(), l);
    // CHGLIBL's refusals with its messages.
    let e = c
        .set_library_list(&SetLibraryList {
            current: None,
            user: vec!["NOSUCH".into()],
        })
        .unwrap_err();
    assert_eq!(
        (e.status(), message_id(&e)),
        (Some(Status::NotFound), Some("CPF2110"))
    );
    let e = c
        .set_library_list(&SetLibraryList {
            current: None,
            user: vec!["HLPAY".into()],
        })
        .unwrap_err();
    assert_eq!(
        (e.status(), message_id(&e)),
        (Some(Status::NotAuthorized), Some("CPF2182"))
    );
    // As it was set, and the list just read can be set again as it is.
    let again = c.get_library_list().unwrap();
    assert_eq!(again.current.as_deref(), Some("HLINV"));
    let back = c
        .set_library_list(&SetLibraryList {
            current: again.current.clone(),
            user: again.user.clone(),
        })
        .unwrap();
    assert_eq!(back, again);
}

#[test]
fn lists_and_descriptions() {
    let s = start(cfg());
    let mut c = v8(&s);
    let libs = c
        .list_libraries(&ListLibraries {
            pattern: "HL*".into(),
            start: 0,
            max: 0,
        })
        .unwrap();
    let names: Vec<_> = libs.libraries.iter().map(|l| l.name.as_str()).collect();
    // HLPAY is ANN's alone: BOB never sees it.
    assert_eq!(names, ["HLINV", "HLTEST"]);
    let o = c
        .list_objects(&ListObjects {
            library: "HLINV".into(),
            pattern: "ASSETS".into(),
            types: vec!["*PGM".into()],
            attribute: None,
            start: 0,
            max: 0,
        })
        .unwrap();
    assert_eq!(o.objects.len(), 1);
    assert_eq!(
        (
            o.objects[0].r#type.as_str(),
            o.objects[0].attribute.as_str()
        ),
        ("*PGM", "RPGLE")
    );
    let d = c.describe_object(&obj("HLINV", "ASSETS"), "*PGM").unwrap();
    assert!(
        d.iter().any(|a| a.name == "Library" && a.value == "HLINV"),
        "{d:?}"
    );
    let m = c
        .list_members(&ListMembers {
            file: obj("HLINV", "QRPGLESRC"),
            pattern: "B*".into(),
            start: 0,
            max: 0,
        })
        .unwrap();
    let names: Vec<_> = m.members.iter().map(|m| m.name.as_str()).collect();
    assert_eq!(names, ["BAD42", "BIG"]);
    let e = c
        .describe_object(&obj("HLINV", "NOSUCH"), "*PGM")
        .unwrap_err();
    assert_eq!(
        (e.status(), message_id(&e)),
        (Some(Status::NotFound), Some("CPF9801"))
    );
}

#[test]
fn a_member_over_1_mib_is_read_whole_and_written_back_in_pieces() {
    let s = start(cfg());
    let mut c = v8(&s);
    let before = frames(&c);
    let big = c.read_member(&mbr("BIG"), false).unwrap();
    // The reply came in several frames, joined before it was decoded.
    assert!(frames(&c) - before >= 2, "{} frame(s)", frames(&c) - before);
    assert_eq!(big.records.len(), BIG_RECORDS);
    assert_eq!((big.width, big.ccsid), (100, 37));
    assert!(
        big.records
            .iter()
            .zip(1u32..)
            .all(|(r, i)| r.seq == i * 100)
    );
    // The test system's BIG runs to sequence number 12000.00, past the
    // 9999.99 a write takes, and has more records than renumbering takes:
    // its records go back numbered from 0000.01 in steps of 0000.01.
    let records: Vec<_> = big
        .records
        .iter()
        .zip(1u32..)
        .map(|(r, i)| SourceRecord {
            seq: i,
            ..r.clone()
        })
        .collect();
    // The pieces the write takes, each answered before the next.
    let pieces = qap_client::develop::pieces(&mbr("BIG"), &records, &WriteOptions::default())
        .unwrap()
        .len();
    assert!(pieces >= 2, "{pieces}");
    let before = frames(&c);
    let n = c
        .write_member(&mbr("BIG"), &records, &WriteOptions::default())
        .unwrap();
    assert_eq!(n as usize, BIG_RECORDS);
    assert_eq!(frames(&c) - before, pieces as u64);
    let now = c.read_member(&mbr("BIG"), false).unwrap();
    let got: Vec<_> = now.records.iter().map(|r| (r.seq, &r.data)).collect();
    let sent: Vec<_> = records.iter().map(|r| (r.seq, &r.data)).collect();
    assert_eq!(got, sent);
}

#[test]
fn a_member_read_and_written_back_keeps_its_dates() {
    let s = start(cfg());
    let mut c = v8(&s);
    let read = c.read_member(&mbr("ASSETS"), true).unwrap();
    assert_eq!(read.records.len(), 40);
    // Held for this connection: another job's read with a lock is InUse.
    let mut a = c.attach().unwrap();
    let e = a.read_member(&mbr("ASSETS"), true).unwrap_err();
    assert_eq!(
        (e.status(), message_id(&e)),
        (Some(Status::InUse), Some("CPF1002"))
    );
    // Written back as read: nothing changes, the lock ends with it.
    c.write_member(&mbr("ASSETS"), &read.records, &WriteOptions::default())
        .unwrap();
    assert_eq!(c.read_member(&mbr("ASSETS"), false).unwrap(), read);
    assert!(a.read_member(&mbr("ASSETS"), true).is_ok());
    a.unlock_member(&mbr("ASSETS")).unwrap();
    // A changed record is dated today; every other keeps its date.
    let mut changed = read.records.clone();
    changed[3].data = "dcl-s count int(10) inz(1);".into();
    c.write_member(&mbr("ASSETS"), &changed, &WriteOptions::default())
        .unwrap();
    let now = c.read_member(&mbr("ASSETS"), false).unwrap();
    for (i, (was, is)) in read.records.iter().zip(&now.records).enumerate() {
        assert_eq!(was.seq, is.seq, "{i}");
        if i == 3 {
            assert_ne!(is.date, was.date);
        } else {
            assert_eq!(is.date, was.date, "{i}");
        }
    }
    // Another job's lock, and what the server says of a bad write.
    let e = c.read_member(&mbr("LOCKED"), true).unwrap_err();
    assert_eq!(e.status(), Some(Status::InUse));
    let mut long = read.records.clone();
    long[0].data = "x".repeat(101);
    let e = c
        .write_member(&mbr("ASSETS"), &long, &WriteOptions::default())
        .unwrap_err();
    assert_eq!(
        (e.status(), message_id(&e)),
        (Some(Status::InvalidRequest), Some("TCP1A62"))
    );
}

#[test]
fn members_made_copied_renamed_changed_and_deleted() {
    let s = start(cfg());
    let mut c = v8(&s);
    c.create_member(&mbr("NEW1"), "RPGLE", "New").unwrap();
    let e = c.create_member(&mbr("NEW1"), "RPGLE", "New").unwrap_err();
    assert_eq!(e.status(), Some(Status::Exists));
    c.copy_member(&mbr("ASSETS"), &mbr("ASSETSCPY"), false)
        .unwrap();
    let copy = c.read_member(&mbr("ASSETSCPY"), false).unwrap();
    // CPYSRCF keeps sequence numbers and dates.
    assert_eq!(
        copy.records,
        c.read_member(&mbr("ASSETS"), false).unwrap().records
    );
    c.rename_member(&mbr("ASSETSCPY"), "ASSETSOLD").unwrap();
    c.change_member(&mbr("ASSETSOLD"), None, Some("Before"))
        .unwrap();
    assert_eq!(
        c.read_member(&mbr("ASSETSOLD"), false).unwrap().text,
        "Before"
    );
    c.delete_member(&mbr("ASSETSOLD")).unwrap();
    let e = c.read_member(&mbr("ASSETSOLD"), false).unwrap_err();
    assert_eq!(
        (e.status(), message_id(&e)),
        (Some(Status::NotFound), Some("CPF9815"))
    );
    let e = c.unlock_member(&mbr("NEW1")).unwrap_err();
    assert_eq!(
        (e.status(), message_id(&e)),
        (Some(Status::NotFound), Some("TCP1A61"))
    );
}

#[test]
fn a_compile_answers_its_diagnostics() {
    let s = start(cfg());
    let mut c = v8(&s);
    let p = c
        .compile("CRTBNDRPG PGM(QTEMP/BAD42) SRCFILE(HLINV/QRPGLESRC) SRCMBR(BAD42)")
        .unwrap();
    let o = CompileOutcome::from_reply(p.wait_done().unwrap()).unwrap();
    assert_eq!(o.status, Status::Error, "{o:?}");
    assert!(o.job.is_some(), "the body still filled");
    assert!(!o.ok());
    let d: Vec<_> = o.diagnostics.iter().filter(|d| d.id == "RNF7416").collect();
    assert_eq!(d.len(), 1, "{:?}", o.diagnostics);
    assert_eq!(d[0].line, 42);
    assert_eq!(
        d[0].r#where,
        Where::Member {
            member: mbr("BAD42")
        }
    );
    let p = c
        .compile("CRTBNDRPG PGM(QTEMP/ASSETS) SRCFILE(HLINV/QRPGLESRC) SRCMBR(ASSETS)")
        .unwrap();
    let o = CompileOutcome::from_reply(p.wait_done().unwrap()).unwrap();
    assert!(o.ok() && o.diagnostics.is_empty(), "{o:?}");
}

#[test]
fn jobs_listed_logged_held_released_and_ended() {
    let s = start(cfg());
    let mut c = v8(&s);
    let all = c.all_jobs("*CURRENT", None).unwrap();
    let names: Vec<_> = all.iter().map(|j| j.id.name.as_str()).collect();
    for n in ["QPADEV0001", "NIGHTLY", "BUILD", "OLDJOB"] {
        assert!(names.contains(&n), "{names:?}");
    }
    assert!(!names.contains(&"PAYRUN"), "ANN's");
    let queued = c
        .list_jobs(&ListJobs {
            user: "*CURRENT".into(),
            status: Some(JobState::OnJobQueue),
            start: 0,
            max: 0,
        })
        .unwrap();
    assert_eq!(queued.jobs.len(), 1);
    let nightly = queued.jobs[0].id.clone();
    let e = c.all_jobs("ANN", None).unwrap_err();
    assert_eq!(e.status(), Some(Status::NotAuthorized));
    let build = all
        .iter()
        .find(|j| j.id.name == "BUILD")
        .unwrap()
        .id
        .clone();
    let log = c
        .job_log(&JobLog {
            job: build,
            start: 0,
            max: 0,
        })
        .unwrap();
    assert_eq!(
        log.entries.first().map(|e| e.message.id.as_str()),
        Some("CPF1124")
    );
    let old = all
        .iter()
        .find(|j| j.id.name == "OLDJOB")
        .unwrap()
        .id
        .clone();
    let e = c
        .job_log(&JobLog {
            job: old,
            start: 0,
            max: 0,
        })
        .unwrap_err();
    assert_eq!(e.status(), Some(Status::NotFound));
    c.hold_job(&nightly).unwrap();
    c.release_job(&nightly).unwrap();
    c.end_job(&nightly, EndHow::Immediate, None).unwrap();
    let e = c.hold_job(&nightly).unwrap_err();
    assert_eq!(
        (e.status(), message_id(&e)),
        (Some(Status::Error), Some("CPF1362"))
    );
    let e = c
        .hold_job(&JobId {
            name: "NOSUCH".into(),
            user: "BOB".into(),
            number: "999999".into(),
        })
        .unwrap_err();
    assert_eq!(e.status(), Some(Status::NotFound));
}

#[test]
fn routines_and_indexes() {
    let s = start(cfg());
    let mut c = v8(&s);
    let all = c.all_routines("HLINV", "%", &[]).unwrap();
    let names: Vec<_> = all
        .iter()
        .map(|r| (r.name.as_str(), r.specific.as_str()))
        .collect();
    assert_eq!(
        names,
        [
            ("ASSETAGE", "ASSETAGE"),
            ("RESTOCK", "RESTOCK"),
            ("RESTOCK", "RESTOCK2")
        ]
    );
    let page = c
        .list_routines(&ListRoutines {
            schema: "HLINV".into(),
            pattern: "%".into(),
            kinds: vec![RoutineKind::Function],
            start: 0,
            max: 1,
        })
        .unwrap();
    assert_eq!((page.total, page.routines.len()), (1, 1));
    let ix = c.list_indexes("HLINV", "ASSETS").unwrap();
    let got: Vec<_> = ix
        .iter()
        .map(|i| (i.name.as_str(), i.unique, i.columns.clone()))
        .collect();
    assert_eq!(
        got,
        [
            ("ASSETSI1", true, vec!["ASSETID".to_string()]),
            (
                "ASSETSL1",
                false,
                vec!["LOCATION".to_string(), "ASSETID".to_string()]
            )
        ]
    );
    let e = c.list_indexes("HLPAY", "SALARIES").unwrap_err();
    assert_eq!(
        (e.status(), message_id(&e)),
        (Some(Status::NotAuthorized), Some("SQL0551"))
    );
}
