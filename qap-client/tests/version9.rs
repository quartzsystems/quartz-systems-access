// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! QAP version 9 (draft), the `Debug` service, against QS/400's engine
//! and its test debugger (`qapd::testing::debug`: `HLINV/ASSETS`, its
//! statements on lines 9, 10, 12, 13 and 14, display sessions' jobs):
//!
//! - an `Own` session stopping the connection's own `CALL` while its
//!   `RunCommand` waits, the program's fields, `Resume`, and `DebugEnded`
//!   at the sign-off;
//! - `Stopped` and `DebugEnded` arriving on the receiver in order while a
//!   `Stack` request is out and its reply not taken, then the reply
//!   matched to its request. The test system answers `Stack` at once, so
//!   "in flight" here is sent and not yet read: the events reach the
//!   receiver from the connection's thread without the caller looking;
//! - a second `Start` left to the server (`InUse`), and the gate below
//!   version 9.

mod common;

use std::sync::mpsc::RecvTimeoutError;
use std::time::Duration;

use common::{Server, cfg, start_with};
use qap_client::debug::{DebugEvent, DebugSession, Debugger};
use qap_client::proto::Op;
use qap_client::proto::ops::debug::{
    DebugTarget, Frames, LineBreakpoint, ResumeHow, StopReason, Stopped, View,
};
use qap_client::proto::types::ObjectName;
use qap_client::{Client, CommandOutcome, Error, Requests, Secret, Status};
use qapd::Config;
use qapd::testing::System;

const WAIT: Duration = Duration::from_secs(10);

fn server() -> Server {
    // The engine sends a session's events at each look of its watches'
    // thread.
    let cfg = Config {
        watch_poll: Duration::from_millis(20),
        ..cfg()
    };
    start_with(cfg, System::new(), common::TLS13)
}

fn client(s: &Server, max_version: u16) -> Client {
    let mut c = Client::new(s.target().with_max_version(max_version));
    c.sign_on("BOB", &Secret::from("SECRET1")).unwrap();
    c
}

fn assets() -> ObjectName {
    ObjectName {
        library: "HLINV".into(),
        name: "ASSETS".into(),
    }
}

fn at(lines: &[u32]) -> Vec<LineBreakpoint> {
    lines
        .iter()
        .map(|&line| LineBreakpoint {
            line,
            condition: None,
        })
        .collect()
}

fn stopped(s: &DebugSession) -> Stopped {
    match s.events.recv_timeout(WAIT) {
        Ok(DebugEvent::Stopped(st)) => st,
        other => panic!("expected Stopped, got {other:?}"),
    }
}

fn ended_with(s: &DebugSession) -> Option<String> {
    match s.events.recv_timeout(WAIT) {
        Ok(DebugEvent::DebugEnded(d)) => d.message.map(|m| m.id),
        other => panic!("expected DebugEnded, got {other:?}"),
    }
}

fn message_id(e: &Error) -> Option<&str> {
    e.server_message().map(|m| m.id.as_str())
}

#[test]
fn an_own_session_stops_the_connections_call() {
    let s = server();
    let mut c = client(&s, 9);
    let session = c.start(DebugTarget::Own).unwrap();
    let modules = c.add_program(&assets(), "*PGM").unwrap();
    assert_eq!(modules.len(), 1);
    assert!(modules[0].views.contains(&View::Source), "{modules:?}");
    // A comment line's breakpoint lands on the next statement.
    let set = c.set_breakpoints(&assets(), "", &at(&[11])).unwrap();
    assert_eq!((set[0].line, set[0].stmt), (12, 12));

    let cmd = c.run_command("CALL PGM(HLINV/ASSETS)").unwrap();
    let st = stopped(&session);
    assert_eq!(st.reason, StopReason::Breakpoint);
    assert_eq!(
        (st.frame.line, st.frame.program.name.as_str()),
        (12, "ASSETS")
    );
    // The command waits while its program is stopped.
    assert!(cmd.try_wait().is_none());
    let stack = c.stack().unwrap();
    assert_eq!(stack[0].line, 12);
    let names: Vec<_> = c
        .variables(0, None)
        .unwrap()
        .into_iter()
        .map(|v| v.name)
        .collect();
    assert_eq!(names, ["COUNT", "NAME", "ASSET"]);
    let parts: Vec<_> = c
        .variables(0, Some("ASSET"))
        .unwrap()
        .into_iter()
        .map(|v| v.name)
        .collect();
    assert_eq!(parts, ["ASSET.ID", "ASSET.TAG"]);
    assert!(!c.eval(0, "COUNT").unwrap().is_empty());
    let e = c.variables(1, None).unwrap_err();
    assert_eq!(
        (e.status(), message_id(&e)),
        (Some(Status::InvalidRequest), Some("TCP1A77"))
    );

    c.resume(ResumeHow::Go).unwrap();
    let done = CommandOutcome::from_reply(cmd.wait_done().unwrap()).unwrap();
    assert_eq!(done.status, Status::Ok, "{done:?}");
    // Nothing more until the sign-off ends the session.
    assert_eq!(
        session
            .events
            .recv_timeout(Duration::from_millis(200))
            .unwrap_err(),
        RecvTimeoutError::Timeout
    );
    c.sign_off().unwrap();
    assert_eq!(ended_with(&session).as_deref(), Some("TCP1A76"));
}

#[test]
fn stopped_and_debug_ended_arrive_in_order_while_a_stack_request_is_out() {
    let s = server();
    let mut c = client(&s, 9);
    let job = s.c.interactive_job("BOB");
    let session = c.start(DebugTarget::Job { job: job.clone() }).unwrap();
    c.add_program(&assets(), "*PGM").unwrap();
    c.set_breakpoints(&assets(), "ASSETS", &at(&[13])).unwrap();

    // Stack is sent and its reply left where it lands, unread.
    let stack = c.request_here_op(Op::Stack, &()).unwrap();
    let _call = s.c.call_in(&job);
    // Both events come to the receiver without the caller asking for
    // anything: the stop, and the session's end with the job's.
    let st = stopped(&session);
    assert_eq!((st.job.clone(), st.frame.line), (job.clone(), 13));
    s.c.end_job(&job);
    assert_eq!(ended_with(&session).as_deref(), Some("TCP1A75"));

    // The Stack reply was kept for its request all along: the stop's
    // frame, or, when the server took it before the job stopped, CPD9CCC.
    match stack.wait(WAIT).unwrap().decode::<Frames>() {
        Ok(f) => assert_eq!(f.frames[0].line, 13),
        Err(e) => assert_eq!(message_id(&e), Some("CPD9CCC"), "{e:?}"),
    }
}

#[test]
fn a_second_start_is_the_servers_to_answer() {
    let s = server();
    let mut c = client(&s, 9);
    let first = c.start(DebugTarget::Own).unwrap();
    let e = c.start(DebugTarget::Own).unwrap_err();
    assert_eq!(
        (e.status(), message_id(&e)),
        (Some(Status::InUse), Some("TCP1A72"))
    );
    // The session there is goes on.
    c.add_program(&assets(), "*PGM").unwrap();
    c.end().unwrap();
    let e = c.end().unwrap_err();
    assert_eq!(
        (e.status(), message_id(&e)),
        (Some(Status::Error), Some("TCP1A71"))
    );
    // A new session's receiver takes over; the old one's disconnects.
    let second = c.start(DebugTarget::Own).unwrap();
    assert_eq!(
        first.events.recv_timeout(WAIT).unwrap_err(),
        RecvTimeoutError::Disconnected
    );
    assert_eq!(
        second
            .events
            .recv_timeout(Duration::from_millis(100))
            .unwrap_err(),
        RecvTimeoutError::Timeout
    );
    let e = c
        .add_program(
            &ObjectName {
                library: "HLINV".into(),
                name: "NODEBUG".into(),
            },
            "*PGM",
        )
        .unwrap_err();
    assert_eq!(message_id(&e), Some("CPF9C15"));
}

#[test]
fn below_version_9_debug_is_refused_before_it_is_sent() {
    let s = server();
    let mut c = client(&s, 8);
    assert_eq!(c.version(), Some(8));
    let before = c.connection().unwrap().frames_received();
    match c.start(DebugTarget::Own).unwrap_err() {
        Error::Unsupported { op, needs, has } => {
            assert_eq!((op, needs, has), (Op::DebugStart, 9, 8));
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(c.connection().unwrap().frames_received(), before);
}

#[test]
fn the_receiver_disconnects_when_the_connection_closes() {
    let s = server();
    let mut c = client(&s, 9);
    let session = c.start(DebugTarget::Own).unwrap();
    s.c.drop_all();
    assert_eq!(
        session.events.recv_timeout(WAIT).unwrap_err(),
        RecvTimeoutError::Disconnected
    );
    // The session went with its connection: nothing goes on a new one.
    let e = c.stack().unwrap_err();
    assert!(matches!(e, Error::Closed(_)), "{e:?}");
}
