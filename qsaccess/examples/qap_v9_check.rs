// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! Live check of `qap-client`'s QAP versions 8 and 9 (`Develop`, `Jobs`,
//! `Debug`) against a real QS/400, for TESTING.md's "QAP versions 8 and 9
//! (qap-client)". The desktop app never calls these services; Code for
//! QS/400 does. It uses the system as configured in this user's System
//! Configurations (address, port, trust), offering up to version 9:
//!
//! 1. `ReadMember` of `HLINV/QRPGLESRC ASSETS`: every record's sequence
//!    number and date printed as `DSPPFM` shows them, to compare at a 5250
//!    session;
//! 2. `CopyMember` (`CPYSRCF`) of it to `ASSETSCPY`, then `WriteMember` of
//!    the records read back to `ASSETS`, then both members read and
//!    compared record by record (QS/400 ships no `CMPPFM`): identical,
//!    dates unchanged;
//! 3. `tools/conformance/qap/COMPILE42.rpgle` written to member
//!    `COMPILE42` and compiled with `CRTBNDRPG` into `QTEMP`: one
//!    `Diagnostic` at line 42, its ID printed to compare with the listing;
//! 4. `ListJobs *CURRENT` includes the connection's job (`JobInfo`);
//! 5. QS/400's conformance `ASSETS` debug program written to member
//!    `DBGASSETS` and compiled `DBGVIEW(*SOURCE)` into `QTEMP`; `Start Own`,
//!    `AddProgram`, a breakpoint on line 11 (lands on 12), `CALL` through
//!    `RunCommand`: `Stopped` at line 12, `Variables`, `Resume Go`, the
//!    command's reply, and at the sign-off `DebugEnded` (`TCP1A76`).
//!
//! ```text
//! QSA_CHECK_PASSWORD=... cargo run -p qsaccess --example qap_v9_check -- 10.42.70.150 CWELLMAN
//! ```
//!
//! The password is read from `QSA_CHECK_PASSWORD` and never printed. The
//! members it adds (`ASSETSCPY`, `COMPILE42`, `DBGASSETS`) are removed at
//! the end; `ASSETS` is left as it was read. The session is signed off.

use std::process::ExitCode;
use std::time::Duration;

use qap_client::debug::{DebugEvent, Debugger};
use qap_client::develop::{Develop, WriteOptions};
use qap_client::jobs::Jobs;
use qap_client::proto::ops::debug::{DebugTarget, LineBreakpoint, ResumeHow};
use qap_client::proto::types::{MemberName, ObjectName, SourceRecord};
use qap_client::{Client, CommandOutcome, CompileOutcome, Secret, Status};
use qsa_config::ConfigDir;

/// `tools/conformance/qap/COMPILE42.rpgle` at QS/400 7f82a43f.
const COMPILE42: &str = include_str!("../../qap-client/tests/fixtures/COMPILE42.rpgle");
/// `ASSETS` of `tools/conformance/src/qap/debug.rs` at QS/400 7f82a43f.
const DBGASSETS: &str = include_str!("../../qap-client/tests/fixtures/ASSETS.rpgle");

const WAIT: Duration = Duration::from_secs(90);

fn mbr(member: &str) -> MemberName {
    MemberName {
        file: ObjectName {
            library: "HLINV".into(),
            name: "QRPGLESRC".into(),
        },
        member: member.into(),
    }
}

fn records(text: &str) -> Vec<SourceRecord> {
    text.lines()
        .zip(1u32..)
        .map(|(l, i)| SourceRecord {
            seq: i * 100,
            date: 0,
            data: l.trim_end().to_owned(),
        })
        .collect()
}

/// `seq` as `DSPPFM` shows it: `0001.00`.
fn seq(s: u32) -> String {
    format!("{:04}.{:02}", s / 100, s % 100)
}

fn result(name: &str, r: Result<String, String>) -> bool {
    match r {
        Ok(detail) => {
            println!("PASS  {name}: {detail}");
            true
        }
        Err(detail) => {
            println!("FAIL  {name}: {detail}");
            false
        }
    }
}

/// A member made fresh with `text`'s lines.
fn put(c: &mut Client, member: &str, text: &str) -> Result<(), String> {
    let _ = c.delete_member(&mbr(member));
    c.create_member(&mbr(member), "RPGLE", "qap_v9_check")
        .map_err(|e| format!("CreateMember {member}: {e}"))?;
    c.write_member(&mbr(member), &records(text), &WriteOptions::default())
        .map_err(|e| format!("WriteMember {member}: {e}"))?;
    Ok(())
}

fn read_and_write_back(c: &mut Client) -> (bool, bool) {
    let read = match c.read_member(&mbr("ASSETS"), false) {
        Ok(r) => r,
        Err(e) => {
            result("ReadMember", Err(e.to_string()));
            return (false, false);
        }
    };
    println!(
        "      HLINV/QRPGLESRC ASSETS, {} records (SEQNBR DATE):",
        read.records.len()
    );
    for r in &read.records {
        println!("      {}  {:06}  {}", seq(r.seq), r.date, r.data);
    }
    let one = result(
        "ReadMember",
        Ok(format!(
            "{} records; compare SEQNBR and the dates with DSPPFM FILE(HLINV/QRPGLESRC) MBR(ASSETS)",
            read.records.len()
        )),
    );
    let two = (|| {
        c.copy_member(&mbr("ASSETS"), &mbr("ASSETSCPY"), true)
            .map_err(|e| format!("CopyMember: {e}"))?;
        let n = c
            .write_member(&mbr("ASSETS"), &read.records, &WriteOptions::default())
            .map_err(|e| format!("WriteMember: {e}"))?;
        let now = c
            .read_member(&mbr("ASSETS"), false)
            .map_err(|e| e.to_string())?;
        let copy = c
            .read_member(&mbr("ASSETSCPY"), false)
            .map_err(|e| e.to_string())?;
        if now.records != copy.records || now.records != read.records {
            let at = now
                .records
                .iter()
                .zip(&copy.records)
                .position(|(a, b)| a != b);
            return Err(format!("the members differ (first at record {at:?})"));
        }
        Ok(format!(
            "{n} records written back; identical to ASSETSCPY, dates unchanged"
        ))
    })();
    (one, result("WriteMember and the CPYSRCF copy", two))
}

fn compile42(c: &mut Client) -> Result<String, String> {
    put(c, "COMPILE42", COMPILE42)?;
    let p = c
        .compile("CRTBNDRPG PGM(QTEMP/COMPILE42) SRCFILE(HLINV/QRPGLESRC) SRCMBR(COMPILE42)")
        .map_err(|e| e.to_string())?;
    let o = CompileOutcome::from_reply(p.wait(WAIT).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    let at42: Vec<_> = o.diagnostics.iter().filter(|d| d.line == 42).collect();
    let errors = o.diagnostics.iter().filter(|d| d.severity >= 20).count();
    match at42.as_slice() {
        [d] if errors == 1 => Ok(format!(
            "{:?}; one diagnostic at line 42, column {}: {} {} (compare the listing)",
            o.status, d.column, d.id, d.text
        )),
        _ => Err(format!("{:?}: {:?}", o.status, o.diagnostics)),
    }
}

fn own_job(c: &mut Client) -> Result<String, String> {
    let me = c.job_info().map_err(|e| e.to_string())?.job;
    let jobs = c.all_jobs("*CURRENT", None).map_err(|e| e.to_string())?;
    if jobs.iter().any(|j| j.id == me) {
        Ok(format!(
            "{}/{}/{} among {} jobs",
            me.number,
            me.user,
            me.name,
            jobs.len()
        ))
    } else {
        Err(format!("{me:?} not among {} jobs", jobs.len()))
    }
}

fn debug(c: &mut Client) -> Result<String, String> {
    put(c, "DBGASSETS", DBGASSETS)?;
    let p = c
        .run_command(
            "CRTBNDRPG PGM(QTEMP/DBGASSETS) SRCFILE(HLINV/QRPGLESRC) SRCMBR(DBGASSETS) DBGVIEW(*SOURCE)",
        )
        .map_err(|e| e.to_string())?;
    let built = CommandOutcome::from_reply(p.wait(WAIT).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    if built.status != Status::Ok {
        return Err(format!("CRTBNDRPG: {:?} {:?}", built.status, built.escape));
    }
    let pgm = ObjectName {
        library: "QTEMP".into(),
        name: "DBGASSETS".into(),
    };
    let session = c
        .start(DebugTarget::Own)
        .map_err(|e| format!("Start: {e}"))?;
    c.add_program(&pgm, "*PGM")
        .map_err(|e| format!("AddProgram: {e}"))?;
    let set = c
        .set_breakpoints(
            &pgm,
            "",
            &[LineBreakpoint {
                line: 11,
                condition: None,
            }],
        )
        .map_err(|e| format!("SetBreakpoints: {e}"))?;
    let call = c
        .run_command("CALL PGM(QTEMP/DBGASSETS)")
        .map_err(|e| e.to_string())?;
    let line = match session.events.recv_timeout(WAIT) {
        Ok(DebugEvent::Stopped(s)) => s.frame.line,
        other => return Err(format!("expected Stopped, got {other:?}")),
    };
    if line != set[0].line {
        return Err(format!(
            "stopped at {line}, the breakpoint is on {}",
            set[0].line
        ));
    }
    let vars: Vec<String> = c
        .variables(0, None)
        .map_err(|e| format!("Variables: {e}"))?
        .into_iter()
        .map(|v| format!("{}={}", v.name, v.value))
        .collect();
    c.resume(ResumeHow::Go)
        .map_err(|e| format!("Resume: {e}"))?;
    let ran = CommandOutcome::from_reply(call.wait(WAIT).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    if ran.status != Status::Ok {
        return Err(format!("CALL: {:?} {:?}", ran.status, ran.escape));
    }
    cleanup(c);
    c.sign_off().map_err(|e| format!("SignOff: {e}"))?;
    match session.events.recv_timeout(WAIT) {
        Ok(DebugEvent::DebugEnded(d)) => Ok(format!(
            "Stopped at line {line}; variables {}; Resume Go and the CALL's reply; DebugEnded {:?} at the sign-off",
            vars.join(" "),
            d.message.map(|m| m.id)
        )),
        other => Err(format!("expected DebugEnded, got {other:?}")),
    }
}

fn cleanup(c: &mut Client) {
    for m in ["ASSETSCPY", "COMPILE42", "DBGASSETS"] {
        let _ = c.delete_member(&mbr(m));
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [system, user] = args.as_slice() else {
        eprintln!("usage: qap_v9_check <system as configured> <user>");
        return ExitCode::FAILURE;
    };
    let Ok(password) = std::env::var("QSA_CHECK_PASSWORD") else {
        eprintln!("QSA_CHECK_PASSWORD is not set");
        return ExitCode::FAILURE;
    };
    let Some(dir) = ConfigDir::default_location() else {
        eprintln!("no configuration directory");
        return ExitCode::FAILURE;
    };
    let (systems, _) = dir.load_systems();
    let Some(sys) = systems.get(system).cloned() else {
        eprintln!("{system} is not in System Configurations");
        return ExitCode::FAILURE;
    };
    let target = qsaccess::qap::target_for(&sys).with_max_version(qap_client::proto::VERSION);
    let mut c = Client::new(target);
    if let Err(e) = c.sign_on(user, &Secret::from(password.as_str())) {
        eprintln!("sign-on failed: {e}");
        return ExitCode::FAILURE;
    }
    println!("QAP version {} with {system}", c.version().unwrap_or(0));
    if c.version() != Some(9) {
        eprintln!("the system does not speak QAP version 9");
        let _ = c.sign_off();
        return ExitCode::FAILURE;
    }
    let (one, two) = read_and_write_back(&mut c);
    let three = result("Compile COMPILE42", compile42(&mut c));
    let four = result("ListJobs *CURRENT", own_job(&mut c));
    let five = result("Debug Own", debug(&mut c));
    if c.is_signed_on() {
        cleanup(&mut c);
        let _ = c.sign_off();
    }
    if one && two && three && four && five {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
