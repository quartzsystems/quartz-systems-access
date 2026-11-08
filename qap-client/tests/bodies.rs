// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! QAP versions 8 and 9: every new request, reply and event body
//! encoded, decoded, framed in its envelope, and, as a reply, split into
//! frames of at most 1 MiB with `More` and joined again, as the
//! connection joins them; a `ReadMember` reply over 1 MiB split with
//! `More`, and a `WriteMember` of three pieces from `develop::pieces`.
//!
//! Fixtures, from QS/400 at 7f82a43f (the commit the workspace pins):
//!
//! - `tests/fixtures/COMPILE42.rpgle` is `tools/conformance/qap/COMPILE42.rpgle`,
//!   byte for byte: the source the `Compile` exit test compiles (QEMU step
//!   161), one `RNF7416` on line 42.
//! - `tests/fixtures/ASSETS.rpgle` is `ASSETS` in
//!   `tools/conformance/src/qap/debug.rs` (QEMU steps 162 and 163): the
//!   program the `Debug` exit test stops, statements on lines 9, 10, 12, 13
//!   and 14, line 11 a comment.
//! - The date 260105 is `OLD` in `tools/conformance/src/qap/develop.rs`,
//!   the date QEMU step 160 sets `ASSETS`'s records back to.
//!
//! Bodies the conformance tree has no fixture for are written here from
//! the spec's tables (`docs/qap-protocol-spec.md`, versions 8 and 9).

use std::fmt::Debug;

use qap_client::Wire;
use qap_client::develop::{WriteOptions, pieces};
use qap_client::proto::ops::command::CommandText;
use qap_client::proto::ops::debug::{
    AddProgram, BreakpointSet, BreakpointsSet, DebugEnded, DebugModule, DebugTarget, Eval,
    Evaluated, Frame, Frames, LineBreakpoint, ProgramAdded, RemoveServiceEntry, Resume, ResumeHow,
    SetBreakpoints, SetServiceEntry, Start, Started, StopReason, Stopped, Variable, VariableList,
    Variables, View,
};
use qap_client::proto::ops::develop::{
    Attribute, ChangeMember, CompileReply, CopyMember, CreateMember, DescribeObject, Libraries,
    Library, LibraryKind, LibraryList, ListLibraries, ListMembers, ListObjects, MemberData,
    MemberEntry, MemberRef, Members, ObjectDescription, ObjectEntry, Objects, ReadMember,
    RenameMember, SetLibraryList, WriteMember, Written,
};
use qap_client::proto::ops::event;
use qap_client::proto::ops::jobs::{
    EndJob, JobEntry, JobLog, JobLogEntries, JobLogEntry, JobRef, JobState, Jobs, ListJobs,
};
use qap_client::proto::ops::messages::MessageType;
use qap_client::proto::ops::sql::{
    Index, Indexes, ListIndexes, ListRoutines, Routine, RoutineKind, Routines,
};
use qap_client::proto::types::{
    Diagnostic, EndHow, JobId, MemberName, ObjectName, SourceRecord, Where,
};
use qap_client::proto::{Envelope, MAX_FRAME, Op, Reassembly, ServerMessage, Service, Status};

/// `tools/conformance/qap/COMPILE42.rpgle` at 7f82a43f.
const COMPILE42: &str = include_str!("fixtures/COMPILE42.rpgle");
/// `ASSETS` of `tools/conformance/src/qap/debug.rs` at 7f82a43f.
const ASSETS: &str = include_str!("fixtures/ASSETS.rpgle");
/// `OLD` of `tools/conformance/src/qap/develop.rs` at 7f82a43f.
const OLD: u32 = 260_105;

fn decode<T: Wire>(b: &[u8]) -> T {
    qap_client::proto::Envelope {
        body: b.to_vec(),
        ..Envelope::request(1, Op::Ping, &())
    }
    .body_as()
    .unwrap()
}

/// `v` encoded and decoded, and as `op`'s request framed and read back.
fn request<T: Wire + PartialEq + Debug>(op: Op, v: &T) {
    let env = Envelope::request(9, op, v);
    assert_eq!(&decode::<T>(&env.body), v, "{op:?}");
    let back = Envelope::from_frame(&env.to_frame().unwrap()).unwrap();
    assert_eq!((back.service, back.op), op.code(), "{op:?}");
    assert_eq!(&back.body_as::<T>().unwrap(), v, "{op:?}");
}

/// `v` as `op`'s reply: split into frames, each framed and read back, and
/// joined as the connection joins them. How many frames it took.
fn reply<T: Wire + PartialEq + Debug>(op: Op, v: &T) -> usize {
    let req = Envelope::request(9, op, &());
    let body = Envelope::request(9, op, v).body;
    let frames = Envelope::replies(&req, Status::Ok, None, body).unwrap();
    let n = frames.len();
    let mut asm = Reassembly::new();
    let mut whole = None;
    for (i, f) in frames.into_iter().enumerate() {
        let wire = f.to_frame().unwrap();
        assert!(wire.len() <= MAX_FRAME, "{op:?} frame {i}");
        let f = Envelope::from_frame(&wire).unwrap();
        assert_eq!(f.more(), i + 1 < n, "{op:?} frame {i}");
        assert!(whole.is_none(), "{op:?}: joined before its last frame");
        whole = asm.push(f);
    }
    let whole = whole.expect("joined at the last frame");
    assert_eq!(&whole.body_as::<T>().unwrap(), v, "{op:?}");
    n
}

/// `v` as `Debug` event `op`'s body, framed and read back.
fn debug_event<T: Wire + PartialEq + Debug>(op: u16, v: &T) {
    let env = Envelope {
        body: Envelope::request(9, Op::Ping, v).body,
        ..Envelope::event(Service::Debug, op, Status::Ok, None)
    };
    let back = Envelope::from_frame(&env.to_frame().unwrap()).unwrap();
    assert_eq!(
        (back.request_id, back.service, back.op),
        (0, Service::Debug, op)
    );
    assert_eq!(&back.body_as::<T>().unwrap(), v);
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

/// A source file's lines as records `0001.00` upwards, dated `date`.
fn records(text: &str, date: u32) -> Vec<SourceRecord> {
    text.lines()
        .zip(1u32..)
        .map(|(l, i)| SourceRecord {
            seq: i * 100,
            date,
            data: l.trim_end().to_owned(),
        })
        .collect()
}

fn message(id: &str, text: &str) -> ServerMessage {
    ServerMessage {
        id: id.into(),
        severity: 30,
        data: Vec::new(),
        text: text.into(),
        help: format!("Cause . . . . . :   {text}"),
    }
}

fn job() -> JobId {
    JobId {
        name: "QAPC01".into(),
        user: "CWELLMAN".into(),
        number: "000412".into(),
    }
}

#[test]
fn source_records_keep_the_specs_encodings() {
    let r = records(COMPILE42, OLD);
    assert_eq!(r.len(), 43);
    assert_eq!(
        (r[0].seq, r[0].date, r[0].data.as_str()),
        (100, OLD, "**FREE")
    );
    assert_eq!((r[41].seq, r[41].data.as_str()), (4200, "n = 'x';"));
    // seq and date travel as the u32s they are: nothing reformats them.
    let edge = SourceRecord {
        seq: 999_999,
        date: 0,
        data: String::new(),
    };
    let b = Envelope::request(1, Op::Ping, &edge).body;
    assert_eq!(&b[..8], &[0x3f, 0x42, 0x0f, 0, 0, 0, 0, 0]);
    assert_eq!(decode::<SourceRecord>(&b), edge);
}

#[test]
fn develop_bodies() {
    let list = LibraryList {
        current: Some("HLINV".into()),
        system: ["QSYS", "QSYS2", "QHLPSYS", "QUSRSYS"]
            .map(String::from)
            .to_vec(),
        user: ["QGPL", "QTEMP"].map(String::from).to_vec(),
    };
    reply(Op::GetLibraryList, &list);
    reply(
        Op::GetLibraryList,
        &LibraryList {
            current: None,
            ..list.clone()
        },
    );
    request(
        Op::SetLibraryList,
        &SetLibraryList {
            current: None,
            user: Vec::new(),
        },
    );
    request(
        Op::SetLibraryList,
        &SetLibraryList {
            current: list.current.clone(),
            user: list.user.clone(),
        },
    );
    reply(Op::SetLibraryList, &list);
    request(
        Op::ListLibraries,
        &ListLibraries {
            pattern: "HL*".into(),
            start: 0,
            max: 50,
        },
    );
    reply(
        Op::ListLibraries,
        &Libraries {
            total: 2,
            libraries: vec![
                Library {
                    name: "HLINV".into(),
                    kind: LibraryKind::Production,
                    text: "Inventory".into(),
                },
                Library {
                    name: "HLTEST".into(),
                    kind: LibraryKind::Test,
                    text: String::new(),
                },
            ],
        },
    );
    request(
        Op::ListObjects,
        &ListObjects {
            library: "*LIBL".into(),
            pattern: "ASSET*".into(),
            types: vec!["*FILE".into(), "*PGM".into()],
            attribute: Some("RPGLE".into()),
            start: 25,
            max: 25,
        },
    );
    reply(
        Op::ListObjects,
        &Objects {
            total: 1,
            objects: vec![ObjectEntry {
                library: "HLINV".into(),
                name: "ASSETS".into(),
                r#type: "*PGM".into(),
                attribute: "RPGLE".into(),
                text: "Count the assets".into(),
                size: 135_168,
                changed: 1_790_086_400_000,
                owner: "CWELLMAN".into(),
            }],
        },
    );
    request(
        Op::DescribeObject,
        &DescribeObject {
            object: obj("*LIBL", "ASSETS"),
            r#type: "*PGM".into(),
        },
    );
    reply(
        Op::DescribeObject,
        &ObjectDescription {
            attributes: vec![
                Attribute {
                    name: "Library".into(),
                    value: "HLINV".into(),
                },
                Attribute {
                    name: "Owner".into(),
                    value: "CWELLMAN".into(),
                },
            ],
        },
    );
    request(
        Op::ListMembers,
        &ListMembers {
            file: obj("HLINV", "QRPGLESRC"),
            pattern: String::new(),
            start: 0,
            max: 0,
        },
    );
    reply(
        Op::ListMembers,
        &Members {
            total: 1,
            members: vec![MemberEntry {
                name: "COMPILE42".into(),
                srctype: "RPGLE".into(),
                text: "Compile exit test".into(),
                records: 43,
                created: 1_790_000_000_000,
                changed: 1_790_086_400_000,
            }],
        },
    );
    request(
        Op::ReadMember,
        &ReadMember {
            member: mbr("ASSETS"),
            lock: true,
        },
    );
    let data = MemberData {
        srctype: "RPGLE".into(),
        text: "Count the assets".into(),
        width: 100,
        ccsid: 37,
        records: records(ASSETS, OLD),
    };
    assert_eq!(reply(Op::ReadMember, &data), 1);
    request(
        Op::WriteMember,
        &WriteMember {
            member: mbr("ASSETS"),
            records: data.records.clone(),
            renumber: false,
            srctype: None,
            text: Some("Count the assets".into()),
            first: true,
            last: true,
        },
    );
    reply(Op::WriteMember, &Written { records: 14 });
    for op in [Op::UnlockMember, Op::DeleteMember] {
        request(
            op,
            &MemberRef {
                member: mbr("ASSETS"),
            },
        );
        reply(op, &());
    }
    request(
        Op::CreateMember,
        &CreateMember {
            member: mbr("COMPILE42"),
            srctype: "RPGLE".into(),
            text: "Compile exit test".into(),
        },
    );
    request(
        Op::CopyMember,
        &CopyMember {
            from: mbr("ASSETS"),
            to: mbr("ASSETSCPY"),
            replace: true,
        },
    );
    request(
        Op::RenameMember,
        &RenameMember {
            member: mbr("ASSETSCPY"),
            new_name: "ASSETSOLD".into(),
        },
    );
    request(
        Op::ChangeMember,
        &ChangeMember {
            member: mbr("ASSETSOLD"),
            srctype: None,
            text: Some("Before the write".into()),
        },
    );
    for op in [
        Op::CreateMember,
        Op::CopyMember,
        Op::RenameMember,
        Op::ChangeMember,
    ] {
        reply(op, &());
    }
}

#[test]
fn compile_bodies() {
    let command = "CRTBNDRPG PGM(QTEMP/COMPILE42) SRCFILE(HLINV/QRPGLESRC) SRCMBR(COMPILE42)";
    request(
        Op::Compile,
        &CommandText {
            command: command.into(),
        },
    );
    let line42 = COMPILE42.lines().nth(41).unwrap();
    assert_eq!(line42, "n = 'x';");
    reply(
        Op::Compile,
        &CompileReply {
            job: job(),
            messages: vec![message("RNS9310", "Program COMPILE42 not created.")],
            diagnostics: vec![
                Diagnostic {
                    r#where: Where::Member {
                        member: mbr("COMPILE42"),
                    },
                    line: 42,
                    column: 5,
                    end_line: 0,
                    end_column: 7,
                    severity: 30,
                    id: "RNF7416".into(),
                    text: "The types of the right and left hand side do not match.".into(),
                    help: String::new(),
                },
                Diagnostic {
                    r#where: Where::Path {
                        path: "/home/CWELLMAN/inc.rpgleinc".into(),
                    },
                    ..Diagnostic::default()
                },
                Diagnostic {
                    r#where: Where::None,
                    severity: 0,
                    id: "RNF0000".into(),
                    ..Diagnostic::default()
                },
            ],
        },
    );
}

#[test]
fn a_read_member_reply_over_1_mib_is_split_with_more_and_joined_whole() {
    // COMPILE42's lines over and over, numbered 0000.01 upwards, past
    // 1 MiB encoded: the server sends that in several frames.
    let lines: Vec<&str> = COMPILE42.lines().collect();
    let records: Vec<SourceRecord> = (1..=60_000u32)
        .map(|i| SourceRecord {
            seq: i,
            date: OLD,
            data: lines[(i as usize - 1) % lines.len()].trim_end().to_owned(),
        })
        .collect();
    let data = MemberData {
        srctype: "RPGLE".into(),
        text: String::new(),
        width: 100,
        ccsid: 37,
        records,
    };
    let n = reply(Op::ReadMember, &data);
    assert!(n >= 2, "{n} frame(s)");
}

#[test]
fn a_write_member_in_three_pieces() {
    // COMPILE42's lines over and over until the records encode to 2.5 MiB.
    let lines: Vec<&str> = COMPILE42.lines().collect();
    let mut all = Vec::new();
    let mut size = 0;
    while size < MAX_FRAME * 5 / 2 {
        let i = all.len();
        let r = SourceRecord {
            seq: u32::try_from(i + 1).unwrap(),
            date: OLD,
            data: lines[i % lines.len()].trim_end().to_owned(),
        };
        size += Envelope::request(1, Op::Ping, &r).body.len();
        all.push(r);
    }
    let options = WriteOptions {
        renumber: false,
        srctype: Some("RPGLE".into()),
        text: None,
    };
    let p = pieces(&mbr("BIG"), &all, &options).unwrap();
    assert_eq!(p.len(), 3);
    assert_eq!(
        p.iter().map(|p| (p.first, p.last)).collect::<Vec<_>>(),
        [(true, false), (false, false), (false, true)]
    );
    for piece in &p {
        request(Op::WriteMember, piece);
        let frame = Envelope::request(u32::MAX, Op::WriteMember, piece)
            .to_frame()
            .unwrap();
        assert!(frame.len() <= MAX_FRAME);
        assert_eq!(piece.member, mbr("BIG"));
        assert_eq!((&piece.srctype, &piece.text), (&options.srctype, &None));
    }
    let joined: Vec<_> = p.into_iter().flat_map(|p| p.records).collect();
    assert_eq!(joined, all);
}

#[test]
fn jobs_bodies() {
    for status in [None, Some(JobState::Active), Some(JobState::OnOutputQueue)] {
        request(
            Op::ListJobs,
            &ListJobs {
                user: "*CURRENT".into(),
                status,
                start: 0,
                max: 100,
            },
        );
    }
    reply(
        Op::ListJobs,
        &Jobs {
            total: 2,
            jobs: vec![
                JobEntry {
                    id: job(),
                    state: JobState::Active,
                    status: "*ACTIVE".into(),
                    r#type: "BATCH".into(),
                    subsystem: "QUSRWRK".into(),
                    function: "PGM-QAPCNN".into(),
                    entered: 1_790_000_000_000,
                },
                JobEntry {
                    id: JobId {
                        name: "NIGHTLY".into(),
                        user: "CWELLMAN".into(),
                        number: "000202".into(),
                    },
                    state: JobState::OnJobQueue,
                    status: "*JOBQ".into(),
                    r#type: "BATCH".into(),
                    subsystem: String::new(),
                    function: String::new(),
                    entered: 1_790_000_100_000,
                },
            ],
        },
    );
    request(
        Op::JobLog,
        &JobLog {
            job: job(),
            start: 0,
            max: 0,
        },
    );
    reply(
        Op::JobLog,
        &JobLogEntries {
            total: 1,
            entries: vec![JobLogEntry {
                message: message("CPF1124", "Job started."),
                kind: MessageType::Informational,
                sent: 1_790_000_000_000,
                from_program: "QWTPIIPP".into(),
                to_program: "*EXT".into(),
            }],
        },
    );
    for op in [Op::HoldJob, Op::ReleaseJob] {
        request(op, &JobRef { job: job() });
        reply(op, &());
    }
    for (how, delay) in [(EndHow::Controlled, Some(30)), (EndHow::Immediate, None)] {
        request(
            Op::EndJob,
            &EndJob {
                job: job(),
                how,
                delay,
            },
        );
    }
    reply(Op::EndJob, &());
}

#[test]
fn sql_catalog_bodies() {
    request(
        Op::ListRoutines,
        &ListRoutines {
            schema: "HLINV".into(),
            pattern: "RESTOCK%".into(),
            kinds: vec![RoutineKind::Procedure, RoutineKind::Function],
            start: 0,
            max: 100,
        },
    );
    reply(
        Op::ListRoutines,
        &Routines {
            total: 2,
            routines: vec![
                Routine {
                    schema: "HLINV".into(),
                    name: "RESTOCK".into(),
                    specific: "RESTOCK2".into(),
                    kind: RoutineKind::Procedure,
                    text: String::new(),
                },
                Routine {
                    schema: "HLINV".into(),
                    name: "ASSETAGE".into(),
                    specific: "ASSETAGE".into(),
                    kind: RoutineKind::Function,
                    text: "Age in days".into(),
                },
            ],
        },
    );
    request(
        Op::ListIndexes,
        &ListIndexes {
            schema: "HLINV".into(),
            table: "ASSETS".into(),
        },
    );
    reply(
        Op::ListIndexes,
        &Indexes {
            indexes: vec![
                Index {
                    schema: "HLINV".into(),
                    name: "ASSETSI1".into(),
                    unique: true,
                    columns: vec!["ASSETID".into()],
                },
                Index {
                    schema: "HLINV".into(),
                    name: "ASSETSL1".into(),
                    unique: false,
                    columns: vec!["LOCATION".into(), "ASSETID".into()],
                },
            ],
        },
    );
}

fn assets_frame(line: u32) -> Frame {
    Frame {
        program: obj("HLINV", "ASSETS"),
        module: "ASSETS".into(),
        procedure: "ASSETS".into(),
        stmt: line,
        line,
        view: View::Source,
    }
}

#[test]
fn debug_bodies() {
    // The breakpoint on ASSETS's comment line lands on the next statement.
    let lines: Vec<&str> = ASSETS.lines().collect();
    assert_eq!(lines.len(), 14);
    assert!(lines[10].starts_with("//") && lines[11].starts_with("asset.tag"));
    for target in [DebugTarget::Own, DebugTarget::Job { job: job() }] {
        request(Op::DebugStart, &Start { target });
    }
    reply(Op::DebugStart, &Started { session: 1 });
    request(Op::DebugEnd, &());
    reply(Op::DebugEnd, &());
    request(
        Op::AddProgram,
        &AddProgram {
            program: obj("*LIBL", "ASSETS"),
            r#type: "*PGM".into(),
        },
    );
    reply(
        Op::AddProgram,
        &ProgramAdded {
            modules: vec![DebugModule {
                name: "ASSETS".into(),
                views: vec![View::Statement, View::Source],
            }],
        },
    );
    request(
        Op::SetBreakpoints,
        &SetBreakpoints {
            program: obj("HLINV", "ASSETS"),
            module: String::new(),
            lines: vec![
                LineBreakpoint {
                    line: 11,
                    condition: None,
                },
                LineBreakpoint {
                    line: 13,
                    condition: Some("COUNT = 1".into()),
                },
            ],
        },
    );
    reply(
        Op::SetBreakpoints,
        &BreakpointsSet {
            set: vec![
                BreakpointSet { line: 12, stmt: 12 },
                BreakpointSet { line: 13, stmt: 13 },
            ],
        },
    );
    for how in ResumeHow::ALL {
        request(Op::DebugResume, &Resume { how });
    }
    reply(Op::DebugResume, &());
    request(Op::Stack, &());
    reply(
        Op::Stack,
        &Frames {
            frames: vec![assets_frame(12), Frame::default()],
        },
    );
    request(
        Op::Variables,
        &Variables {
            frame: 0,
            parent: Some("ASSET".into()),
        },
    );
    reply(
        Op::Variables,
        &VariableList {
            variables: vec![
                Variable {
                    name: "COUNT".into(),
                    r#type: "INT(10)".into(),
                    value: "1".into(),
                    expandable: false,
                },
                Variable {
                    name: "ASSET".into(),
                    r#type: "DS".into(),
                    value: String::new(),
                    expandable: true,
                },
            ],
        },
    );
    request(
        Op::Eval,
        &Eval {
            frame: 0,
            expression: "NAME:x".into(),
        },
    );
    reply(
        Op::Eval,
        &Evaluated {
            value: "C4C5E2D24040404040 40".into(),
        },
    );
    request(
        Op::SetServiceEntry,
        &SetServiceEntry {
            program: obj("HLINV", "ASSETS"),
            user: "*CURRENT".into(),
        },
    );
    request(
        Op::RemoveServiceEntry,
        &RemoveServiceEntry {
            program: obj("HLINV", "ASSETS"),
        },
    );
    reply(Op::SetServiceEntry, &());
    reply(Op::RemoveServiceEntry, &());

    let reasons = [
        StopReason::Breakpoint,
        StopReason::Step,
        StopReason::ServiceEntry,
        StopReason::Exception {
            message: message("MCH1211", "Attempt made to divide by zero."),
        },
        StopReason::ProgramEnd,
    ];
    for reason in reasons {
        debug_event(
            event::STOPPED,
            &Stopped {
                job: job(),
                reason,
                frame: assets_frame(12),
            },
        );
    }
    debug_event(event::DEBUG_ENDED, &DebugEnded { message: None });
    debug_event(
        event::DEBUG_ENDED,
        &DebugEnded {
            message: Some(message("TCP1A75", "The job being debugged ended.")),
        },
    );
}
