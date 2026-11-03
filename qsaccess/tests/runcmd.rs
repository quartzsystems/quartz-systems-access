// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! Run Command's results against QS/400's test system, with the severities
//! it sends: 00 (`CRTLIB`), 10 (`CHGAUT` with a bad `DTAAUT`), 20
//! (`CPYFRMSTMF` of a stream file with `long` in its name), 30 (a required
//! parameter missing) and 40 (`CRTLIB` again). Rows keep the order the
//! system sent; the Severity column is its two digits; from 20 the ID is
//! red. A cancelled command shows what it sent, the cancel's message too.

#[path = "../../qap-client/tests/common/mod.rs"]
mod common;

use std::time::Duration;

use common::{cfg, start};
use qap_client::{Client, CommandOutcome, Secret, Status as QapStatus};
use qsa_ui::statusbar::Status;
use qsaccess::runcmd::{Line, job_text, outcome_status, rows};

fn run(c: &mut Client, command: &str, lines: &mut Vec<Line>) -> CommandOutcome {
    let p = c.run_command(command).unwrap();
    let o = CommandOutcome::from_reply(p.wait(Duration::from_secs(10)).unwrap()).unwrap();
    lines.push(Line::Ran(command.into()));
    lines.extend(o.messages.iter().cloned().map(Line::Message));
    o
}

#[test]
fn the_severity_column_with_the_systems_severities() {
    let s = start(cfg());
    let mut c = Client::new(s.target());
    c.sign_on("BOB", &Secret::from("SECRET1")).unwrap();
    let mut lines = Vec::new();
    for command in [
        "CRTLIB LIB(QATEST)",
        "CHGAUT OBJ('/home/BOB') USER(ANN) DTAAUT(*BAD)",
        "CPYFRMSTMF FROMSTMF('/home/BOB/long.txt') TOMBR('/QSYS.LIB/QGPL.LIB/X.FILE/X.MBR')",
        "CRTLIB",
        "CRTLIB LIB(QATEST)",
    ] {
        run(&mut c, command, &mut lines);
    }
    let r = rows(&lines);
    let shown: Vec<(&str, &str, Status)> = r
        .iter()
        .filter(|row| !row[0].text.is_empty())
        .map(|row| (row[0].text.as_str(), row[1].text.as_str(), row[0].status))
        .collect();
    assert_eq!(
        shown,
        [
            ("CPC2102", "00", Status::Neutral),
            ("CPDA091", "10", Status::Neutral),
            ("CPFA096", "20", Status::Error),
            ("CPD0071", "30", Status::Error),
            ("CPF0006", "30", Status::Error),
            ("CPF2111", "40", Status::Error),
        ]
    );
    // The commands' own rows stay where they ran, between their messages.
    assert_eq!(r[0][2].text, "> CRTLIB LIB(QATEST)");
    assert_eq!(
        r[2][2].text,
        "> CHGAUT OBJ('/home/BOB') USER(ANN) DTAAUT(*BAD)"
    );
}

#[test]
fn a_cancelled_command_shows_what_it_sent_with_the_job() {
    let s = start(cfg());
    let mut c = Client::new(s.target());
    c.sign_on("BOB", &Secret::from("SECRET1")).unwrap();
    let p = c.run_command("DLYJOB DLY(60)").unwrap();
    std::thread::sleep(Duration::from_millis(150));
    p.cancel();
    let o = CommandOutcome::from_reply(p.wait(Duration::from_secs(10)).unwrap()).unwrap();
    assert_eq!(o.status, QapStatus::Cancelled);
    let r = rows(
        &o.messages
            .iter()
            .cloned()
            .map(Line::Message)
            .collect::<Vec<_>>(),
    );
    assert_eq!(r[0][0].text, "CPF1907");
    assert_eq!(r[0][1].text, "30");
    assert!(job_text(o.job.as_ref()).starts_with("Job: QAPCNN    User: QUSER    Number: "));
    assert_eq!(
        outcome_status("DLYJOB DLY(60)", &o).0,
        "QAD5012: Command DLYJOB was cancelled."
    );
}

/// Two Run Command windows on version 4: each has a connection of its own,
/// attached to the one session, so a long command in one does not hold up
/// the other, and each runs in a job of its own.
#[test]
fn each_window_runs_in_its_own_job_beside_a_long_command() {
    let s = start(cfg());
    let mut c = Client::new(s.target());
    c.sign_on("BOB", &Secret::from("SECRET1")).unwrap();
    let mut a = c.attach().unwrap();
    let mut b = c.attach().unwrap();
    assert_ne!(a.job().job.number, b.job().job.number);
    let long = a.run_command("DLYJOB DLY(30)").unwrap();
    let job_a: u32 = a.job().job.number.parse().unwrap();
    assert_eq!(
        s.c.wait_running(job_a, Duration::from_secs(5)).as_deref(),
        Some("DLYJOB")
    );
    let t = std::time::Instant::now();
    let p = b.run_command("CRTLIB LIB(QAOWN)").unwrap();
    let o = CommandOutcome::from_reply(p.wait(Duration::from_secs(10)).unwrap()).unwrap();
    assert_eq!(o.status, QapStatus::Ok);
    assert!(t.elapsed() < Duration::from_secs(5), "{:?}", t.elapsed());
    assert!(b.check_command("CRTLIB LIB(QAX)").unwrap().valid());
    // The shared connection is free too.
    assert_eq!(c.system_info().unwrap().system_name, "QSHQ400");
    long.cancel();
    let o = CommandOutcome::from_reply(long.wait(Duration::from_secs(10)).unwrap()).unwrap();
    assert_eq!(o.status, QapStatus::Cancelled);
    assert_eq!(
        qsaccess::qap::window_title("Run Command", "SYS", Some(&a.job().job)),
        format!(
            "Run Command - SYS ({}/{}/{})",
            a.job().job.number,
            a.job().job.user,
            a.job().job.name
        )
    );
    assert_eq!(
        qsaccess::qap::attached_message("SYS", "Run Command", &b.job().job).id,
        "QAD7001"
    );
}

/// A session has at most 16 connections, the shared one included: the
/// next window's is refused with `TCP1A59`, shown as `QAD7003`.
#[test]
fn the_seventeenth_connection_is_qad7003() {
    let s = start(cfg());
    let mut c = Client::new(s.target());
    c.sign_on("BOB", &Secret::from("SECRET1")).unwrap();
    let held: Vec<_> = (0..15).map(|_| c.attach().unwrap()).collect();
    let e = c.attach().unwrap_err();
    assert_eq!(e.status(), Some(QapStatus::Busy));
    assert_eq!(e.server_message().unwrap().id, "TCP1A59");
    let sys = qsa_config::System::new("SYS");
    let x = qsaccess::qap::explain(&sys, &e);
    assert_eq!(x.message.id, "QAD7003");
    assert_eq!(
        x.message.text,
        "No more connections allowed to SYS. Close a window and try again."
    );
    assert!(x.server.is_none(), "QAD7003 is what is shown");
    // Closing one makes room.
    drop(held);
    common::until("a connection free", || c.attach().is_ok());
}

/// A sign-off on the shared connection ends the session for a window's
/// own connection: `SessionEnded` with `TCP1A58`, shown as sent with
/// `QAD4028`, and nothing attaches to the ended session.
#[test]
fn a_sign_off_elsewhere_ends_the_windows_session() {
    let s = start(cfg());
    let mut c = Client::new(s.target());
    c.sign_on("BOB", &Secret::from("SECRET1")).unwrap();
    let mut a = c.attach().unwrap();
    c.sign_off().unwrap();
    common::until("SessionEnded", || a.poll().is_some());
    let why = a.ended().unwrap().clone();
    assert_eq!(why.message().unwrap().id, "TCP1A58");
    let x = qsaccess::qap::ended(&qsa_config::System::new("SYS"), &why);
    assert_eq!(x.message.id, "QAD4028");
    assert_eq!(x.server.as_ref().unwrap().id, "TCP1A58");
    // Nothing is sent on, or attached to, the ended session.
    let e = a.run_command("CRTLIB LIB(QAEND)").unwrap_err();
    assert_eq!(e.status(), Some(QapStatus::SessionExpired));
    assert_eq!(a.attaches(), 1);
}
