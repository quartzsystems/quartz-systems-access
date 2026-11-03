// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! Run Command: CL commands run on a system over its QAP session (QAP
//! version 2's `Command` service).
//!
//! - The command line is in the system-output font, with the last
//!   [`qsa_config::settings::COMMAND_HISTORY_MAX`] commands per system (settings.toml) on Up and
//!   Down. A command that names a password is never kept, and is logged
//!   with that value masked.
//! - Prompt (F4) builds the command from its definition ([`crate::prompt`]);
//!   Check checks it without running it; Run runs it; Cancel ends it while
//!   it runs (QAP `Cancel`); Clear empties the results.
//! - Each run adds to the results: the command, then the job it ran in and
//!   every message it sent, as the system sent them, in the order it sent
//!   them (the list does not sort). Severity shows as the system's two
//!   digits; from 20, an error on the system's scale, the ID is red. A
//!   cancelled command's results are what it sent before it ended, the
//!   cancel's message last. A message's ID opens its second-level text. The status line says how it ended (`QAD5010` to
//!   `QAD5013`); not authorized shows the system's escape message.
//! - Against a system whose QAP version has no `Command` service, the window
//!   opens with everything but Close disabled and `QAD5001` saying why.
//! - From QAP version 4 each window has a connection of its own, attached
//!   to the session ([`qap::attach`]): commands run and are checked in its
//!   own job, so a long command never waits behind, or holds up, another
//!   window. The title and the job line name that job, and the status line
//!   says so (`QAD7001`); a session with its 16 connections refuses another
//!   (`QAD7003`). Prompt reads command definitions on the session's shared
//!   connection. Closing the window closes the connection and never signs
//!   off. A command whose connection is lost is not run again (`QAD7010`);
//!   when the session ends (`SessionEnded`), the window shows the system's
//!   message and offers to sign on again. Before version 4, commands run on
//!   the shared connection, as they did (`QAD7002`).

use fltk::app;
use fltk::enums::{Align, Event, Key};
use fltk::frame::Frame;
use fltk::input::Input;
use fltk::prelude::*;
use fltk::window::Window;
use qap_client::proto::types::JobId;
use qap_client::proto::{Op, ServerMessage};
use qap_client::{Attached, CheckOutcome, CommandOutcome, Error, Pending, Status as QapStatus};
use qsa_config::settings::redact_command;
use qsa_msg::{Message, id};
use qsa_ui::chan::{self, Emit};
use qsa_ui::controls::{self, Switch};
use qsa_ui::listtable::{Cell, ListTable};
use qsa_ui::msgdetails;
use qsa_ui::propdialog::{center_on, show_placed};
use qsa_ui::statusbar::{Status, StatusLine};
use qsa_ui::theme::{self, colors};

use crate::app::Shared;
use crate::qap::{self, Handle, lock};

/// Window width.
pub const W: i32 = 640;
/// Window height.
pub const H: i32 = 460;

/// Columns of the messages: title and width.
pub const COLUMNS: [(&str, i32); 3] = [("ID", 76), ("Severity", 60), ("Text", 480)];

/// What the window's controls ask for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Request {
    Prompt,
    Check,
    Run,
    Cancel,
    Clear,
    Close,
    Details,
    Older,
    Newer,
}

/// One line of the results.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Line {
    /// A command run, which the lines after it belong to.
    Ran(String),
    /// A command checked.
    Checked(String),
    /// A message the system sent, as it sent it.
    Message(ServerMessage),
}

/// The window's widgets.
pub struct RunCommandWindow {
    pub window: Window,
    pub command: Input,
    pub job: Frame,
    pub list: ListTable,
    pub status: StatusLine,
    prompt: Switch,
    check: Switch,
    run: Switch,
    cancel: Switch,
    clear: Switch,
}

impl RunCommandWindow {
    /// Shows that a command runs (only Cancel and Close then), or not.
    fn running(&self, on: bool) {
        for s in [&self.prompt, &self.check, &self.run, &self.clear] {
            s.set_enabled(!on);
        }
        self.cancel.set_enabled(on);
    }

    /// Everything but Close off: the system cannot run commands.
    fn disable(&mut self) {
        for s in [
            &self.prompt,
            &self.check,
            &self.run,
            &self.cancel,
            &self.clear,
        ] {
            s.set_enabled(false);
        }
        self.command.deactivate();
    }
}

/// From this severity a message is an error, and its ID is red.
pub const ERROR_SEVERITY: u8 = 20;

/// The results as table rows.
pub fn rows(lines: &[Line]) -> Vec<Vec<Cell>> {
    lines
        .iter()
        .map(|l| match l {
            Line::Ran(c) => vec![Cell::from(""), Cell::from(""), Cell::from(format!("> {c}"))],
            Line::Checked(c) => vec![
                Cell::from(""),
                Cell::from(""),
                Cell::from(format!("Check: {c}")),
            ],
            Line::Message(m) => {
                let status = if m.severity >= ERROR_SEVERITY {
                    Status::Error
                } else {
                    Status::Neutral
                };
                vec![
                    Cell {
                        text: m.id.clone(),
                        status,
                    },
                    Cell::from(format!("{:02}", m.severity)),
                    Cell::from(m.text.as_str()),
                ]
            }
        })
        .collect()
}

/// The job line above the results.
pub fn job_text(job: Option<&JobId>) -> String {
    match job {
        Some(j) => format!(
            "Job: {}    User: {}    Number: {}",
            j.name, j.user, j.number
        ),
        None => "Job:".to_owned(),
    }
}

/// The command's name: its first word, without a library.
pub fn command_name(command: &str) -> String {
    let first = command.split_whitespace().next().unwrap_or("");
    let first = first.split('(').next().unwrap_or(first);
    first
        .rsplit('/')
        .next()
        .unwrap_or(first)
        .to_ascii_uppercase()
}

/// The status line for how a run ended.
pub fn outcome_status(command: &str, o: &CommandOutcome) -> (String, Status) {
    let name = command_name(command);
    let m = |i: &str| qsa_msg::message(i, &[&name]).to_string();
    match o.status {
        QapStatus::Ok => (m(id::COMMAND_COMPLETED), Status::Neutral),
        QapStatus::Cancelled => (m(id::COMMAND_CANCELLED), Status::Neutral),
        QapStatus::NotFound => (m(id::COMMAND_NOT_FOUND), Status::Error),
        QapStatus::NotAuthorized => match &o.escape {
            Some(e) => (format!("{}: {}", e.id, e.text), Status::Error),
            None => (m(id::COMMAND_FAILED), Status::Error),
        },
        _ => (m(id::COMMAND_FAILED), Status::Error),
    }
}

fn build_with(system: &str) -> (RunCommandWindow, chan::Receiver<Request>) {
    let (tx, rx) = chan::channel::<Request>();
    let m = 10;
    let fh = theme::FIELD_H;
    let (bw, bh) = (theme::BUTTON_W, theme::BUTTON_H);
    let mut window = Window::new(0, 0, W, H, None);
    window.set_label(&format!("Run Command - {system}"));
    window.set_color(colors::FACE);
    window.make_modal(true);

    controls::label(m, m, 70, fh, "Command:");
    let mut command = controls::field(m + 70, m, W - 2 * m - 70, fh);
    command.set_text_font(theme::MONO_FONT);
    command.set_text_size(theme::MONO_SIZE);
    command.set_maximum_size(6000);
    command.set_tooltip("Up and Down recall earlier commands; F4 prompts; Enter runs.");
    command.handle(move |_, ev| {
        if ev != Event::KeyDown {
            return false;
        }
        let r = match app::event_key() {
            Key::Up => Request::Older,
            Key::Down => Request::Newer,
            Key::Enter | Key::KPEnter => Request::Run,
            k if k == Key::F4 => Request::Prompt,
            _ => return false,
        };
        tx.send(r);
        true
    });

    let by = m + fh + 8;
    let names = ["Prompt", "Check", "Run", "Cancel", "Clear"];
    let reqs = [
        Request::Prompt,
        Request::Check,
        Request::Run,
        Request::Cancel,
        Request::Clear,
    ];
    let mut buttons = Vec::new();
    for (i, (name, r)) in names.iter().zip(reqs).enumerate() {
        let x = m + 70 + i as i32 * (bw + 6);
        let mut b = if r == Request::Run {
            controls::default_button(x, by, bw, bh, name)
        } else {
            controls::button(x, by, bw, bh, name)
        };
        if r == Request::Prompt {
            b.set_tooltip("F4");
        }
        b.emit_to(tx, r);
        buttons.push(b);
    }

    let jy = by + bh + 10;
    let mut job = Frame::new(m, jy, W - 2 * m, fh, None);
    job.set_label(&job_text(None));
    job.set_align(Align::Left | Align::Inside);
    job.set_label_size(theme::UI_SIZE);

    let ly = jy + fh + 4;
    let lh = H - ly - 2 * m - bh;
    let mut list = ListTable::new(m, ly, W - 2 * m, lh, &COLUMNS);
    list.on_activate(move |sel| {
        if sel.is_some() {
            tx.send(Request::Details);
        }
    });

    let status = StatusLine::new(m, H - m - bh, W - 3 * m - bw, bh);
    let mut close = controls::button(W - m - bw, H - m - bh, bw, bh, "Close");
    close.emit_to(tx, Request::Close);
    window.end();
    window.set_callback(move |_| tx.send(Request::Close));

    let mut sw: Vec<Switch> = buttons.iter_mut().map(Switch::for_button).collect();
    let clear = sw.pop().expect("five buttons");
    let cancel = sw.pop().expect("five buttons");
    let run = sw.pop().expect("five buttons");
    let check = sw.pop().expect("five buttons");
    let prompt = sw.pop().expect("five buttons");
    let d = RunCommandWindow {
        window,
        command,
        job,
        list,
        status,
        prompt,
        check,
        run,
        cancel,
        clear,
    };
    d.running(false);
    (d, rx)
}

/// Builds the window for `system` showing `command`, `lines` and the
/// status a run left (for screenshots). It is not shown.
pub fn build(
    system: &str,
    command: &str,
    job: Option<&JobId>,
    lines: &[Line],
    status: Option<&Message>,
) -> RunCommandWindow {
    let (mut d, _) = build_with(system);
    d.command.set_value(command);
    d.job.set_label(&job_text(job));
    d.list.set_rows(rows(lines));
    if let Some(m) = status {
        d.status.set(&m.to_string(), shown(m));
    }
    d
}

fn shown(m: &Message) -> Status {
    if m.kind() == qsa_msg::Kind::Error {
        Status::Error
    } else {
        Status::Neutral
    }
}

/// A command running: what was typed, and its reply to come.
struct Running {
    command: String,
    pending: Pending,
}

/// Opens Run Command for `system`, signing on first when this program has
/// no session on it, and runs it until closed.
pub fn open(parent: Option<&dyn WindowExt>, state: &Shared, system: &str) {
    let Some(sys) = state.borrow().systems.get(system).cloned() else {
        return;
    };
    let Some(mut h) = qap::session(parent, state, &sys.name) else {
        return;
    };
    let has = lock(&h).version().unwrap_or(1);
    let can = |h: &Handle, op: Op| lock(h).supports(op).is_ok();
    // From version 4, a connection of the window's own.
    let mut own: Option<Attached> = None;
    if can(&h, Op::RunCommand) && can(&h, Op::Attach) {
        own = Some(match qap::attach(parent, &h, &sys, "Run Command") {
            Some(a) => a,
            None => return,
        });
    }
    let (mut d, rx) = build_with(&sys.name);
    center_on(&mut d.window, parent);
    show_placed(&mut d.window);
    let _ = d.command.take_focus();

    if !can(&h, Op::RunCommand) {
        let m = qap::version_needed("Run Command", &sys.name, Op::RunCommand, has);
        crate::log!("{m}");
        d.status.set(&m.to_string(), Status::Error);
        d.disable();
    } else if let Some(a) = &own {
        show_job(&mut d, &sys.name, a);
    } else {
        let m = qsa_msg::message(id::ATTACH_NEEDS_VERSION_4, &["Run Command"]);
        crate::log!("{m}");
        d.status.set(&m.to_string(), Status::Neutral);
    }
    let mut ended = false;

    let mut lines: Vec<Line> = Vec::new();
    let mut running: Option<Running> = None;
    let mut history: Vec<String> = state.borrow().settings.commands(&sys.name).to_vec();
    let mut pos = history.len();
    while d.window.shown() {
        app::wait();
        if let Some(r) = &running
            && let Some(done) = r.pending.try_wait()
        {
            let r = running.take().expect("checked");
            finish(&mut d, &mut lines, &sys, own.is_some(), &r.command, done);
            d.running(false);
        }
        // The session ended under the window's own connection.
        if !ended && let Some(why) = own.as_mut().and_then(|a| a.poll().cloned()) {
            ended = true;
            if let Some(r) = running.take() {
                finish(
                    &mut d,
                    &mut lines,
                    &sys,
                    true,
                    &r.command,
                    Err(Error::Closed(qap_client::Closed::Quietly)),
                );
            }
            d.running(false);
            if session_ended(&mut d, state, &sys, &why)
                && let Some(nh) = qap::session(Some(&d.window), state, &sys.name)
                && let Some(a) = qap::attach(Some(&d.window), &nh, &sys, "Run Command")
            {
                h = nh;
                show_job(&mut d, &sys.name, &a);
                own = Some(a);
                ended = false;
                d.running(false);
            }
        }
        match rx.recv() {
            Some(Request::Close) => {
                if let Some(r) = running.take() {
                    r.pending.cancel();
                }
                d.window.hide();
            }
            Some(Request::Older) if pos > 0 => {
                pos -= 1;
                d.command.set_value(&history[pos]);
            }
            Some(Request::Newer) if pos < history.len() => {
                pos += 1;
                d.command
                    .set_value(history.get(pos).map_or("", String::as_str));
            }
            Some(Request::Run) if running.is_none() && !ended && can(&h, Op::RunCommand) => {
                let command = d.command.value().trim().to_owned();
                if command.is_empty() {
                    d.status.set("Type a command to run.", Status::Error);
                    continue;
                }
                remember(state, &sys.name, &command);
                history = state.borrow().settings.commands(&sys.name).to_vec();
                pos = history.len();
                lines.push(Line::Ran(command.clone()));
                d.list.set_rows(rows(&lines));
                crate::log!("Run Command on {}: {}", sys.name, redact_command(&command));
                let sent = match own.as_mut() {
                    Some(a) => qap::busy(|| a.run_command(&command)),
                    None => qap::busy(|| lock(&h).run_command(&command)),
                };
                match sent {
                    Ok(pending) => {
                        d.status.set(
                            &format!("Running {}...", command_name(&command)),
                            Status::Neutral,
                        );
                        d.running(true);
                        running = Some(Running { command, pending });
                    }
                    Err(e) => failed(&mut d, &sys, own.is_some(), &e),
                }
            }
            Some(Request::Cancel) => {
                if let Some(r) = &running {
                    r.pending.cancel();
                    d.status.set(
                        &format!("Cancelling {}...", command_name(&r.command)),
                        Status::Neutral,
                    );
                }
            }
            Some(Request::Check) if running.is_none() && !ended => {
                if !can(&h, Op::CheckCommand) {
                    let m = qap::version_needed("Check", &sys.name, Op::CheckCommand, has);
                    d.status.set(&m.to_string(), Status::Error);
                    continue;
                }
                let command = d.command.value().trim().to_owned();
                if command.is_empty() {
                    d.status.set("Type a command to check.", Status::Error);
                    continue;
                }
                d.status.set(
                    &format!("Checking {}...", command_name(&command)),
                    Status::Neutral,
                );
                let r = match own.as_mut() {
                    Some(a) => qap::busy(|| a.check_command(&command)),
                    None => qap::busy(|| lock(&h).check_command(&command)),
                };
                checked(&mut d, &mut lines, &sys, own.is_some(), &command, r);
            }
            Some(Request::Prompt) if running.is_none() => {
                if !can(&h, Op::DescribeCommand) {
                    let m = qap::version_needed("Prompt", &sys.name, Op::DescribeCommand, has);
                    d.status.set(&m.to_string(), Status::Error);
                    continue;
                }
                let typed = d.command.value();
                let parent = Some(&d.window as &dyn WindowExt);
                match crate::prompt::run(parent, state, &sys, &h, &typed) {
                    crate::prompt::Prompted::Built(command) => {
                        d.command.set_value(&command);
                        d.status.set("", Status::Neutral);
                    }
                    crate::prompt::Prompted::Cancelled => {}
                    crate::prompt::Prompted::Failed(text) => d.status.set(&text, Status::Error),
                }
                let _ = d.command.take_focus();
            }
            Some(Request::Clear) if running.is_none() => {
                lines.clear();
                d.list.set_rows(rows(&lines));
                d.job
                    .set_label(&job_text(own.as_ref().map(|a| &a.job().job)));
                d.status.set("", Status::Neutral);
            }
            Some(Request::Details) => details(&d, &lines),
            _ => {}
        }
    }
}

/// Keeps `command` in `system`'s history and saves the settings.
fn remember(state: &Shared, system: &str, command: &str) {
    let mut st = state.borrow_mut();
    if st.settings.add_command(system, command)
        && let Err(e) = st.save_settings()
    {
        crate::log!("settings not saved: {e}");
    }
}

/// The window's own connection, in its title, job line and status line
/// (`QAD7001`).
fn show_job(d: &mut RunCommandWindow, system: &str, a: &Attached) {
    let job = &a.job().job;
    d.window
        .set_label(&qap::window_title("Run Command", system, Some(job)));
    d.job.set_label(&job_text(Some(job)));
    d.job.redraw();
    let m = qap::attached_message(system, "Run Command", job);
    d.status.set(&m.to_string(), Status::Neutral);
}

/// The session ended under the window's own connection: the system's
/// message, Run and Check off, and the offer to sign on again. True when
/// the user chose to.
fn session_ended(
    d: &mut RunCommandWindow,
    state: &Shared,
    sys: &qsa_config::System,
    why: &qap_client::Ended,
) -> bool {
    d.run.set_enabled(false);
    d.check.set_enabled(false);
    let x = qap::ended(sys, why);
    crate::log!("{}", x.line());
    d.status.set(&x.line(), Status::Error);
    // The session is gone for every tool: forget its client.
    state.borrow_mut().qap.forget_ended(&sys.name);
    qap::offer_sign_on(Some(&d.window), &x)
}

/// A run's reply, into the results and the status line. `own`: it ran on
/// the window's own connection.
fn finish(
    d: &mut RunCommandWindow,
    lines: &mut Vec<Line>,
    sys: &qsa_config::System,
    own: bool,
    command: &str,
    done: Result<qap_client::Reply, Error>,
) {
    let o = match done.and_then(CommandOutcome::from_reply) {
        Ok(o) => o,
        Err(e) => return failed(d, sys, own, &e),
    };
    if o.job.is_some() {
        d.job.set_label(&job_text(o.job.as_ref()));
        d.job.redraw();
    }
    lines.extend(o.messages.iter().cloned().map(Line::Message));
    d.list.set_rows(rows(lines));
    let (text, status) = outcome_status(command, &o);
    crate::log!(
        "Run Command on {}: {}: {text}",
        sys.name,
        command_name(command)
    );
    d.status.set(&text, status);
}

/// A request that failed before or instead of an answer. On the window's
/// own connection, a connection lost under it is `QAD7010`: nothing is run
/// again.
fn failed(d: &mut RunCommandWindow, sys: &qsa_config::System, own: bool, e: &Error) {
    let line = match e {
        Error::Closed(c) if own && c.resumable() => {
            qsa_msg::message(id::STATEMENT_LOST, &[&sys.name]).to_string()
        }
        e => qap::explain(sys, e).line(),
    };
    crate::log!("{line}");
    d.status.set(&line, Status::Error);
}

/// Check's answer: the command's syntax, nothing run.
fn checked(
    d: &mut RunCommandWindow,
    lines: &mut Vec<Line>,
    sys: &qsa_config::System,
    own: bool,
    command: &str,
    r: Result<CheckOutcome, Error>,
) {
    let o = match r {
        Ok(o) => o,
        Err(e) => return failed(d, sys, own, &e),
    };
    lines.push(Line::Checked(command.to_owned()));
    lines.extend(o.messages.iter().cloned().map(Line::Message));
    d.list.set_rows(rows(lines));
    let name = command_name(command);
    let (text, status) = match o.status {
        QapStatus::Ok => (format!("Command {name} has no errors."), Status::Neutral),
        QapStatus::NotFound => (
            qsa_msg::message(id::COMMAND_NOT_FOUND, &[&name]).to_string(),
            Status::Error,
        ),
        _ => (
            qsa_msg::message(id::COMMAND_HAS_ERRORS, &[&name]).to_string(),
            Status::Error,
        ),
    };
    d.status.set(&text, status);
}

/// The selected message's second-level text, as the system sent it.
fn details(d: &RunCommandWindow, lines: &[Line]) {
    let Some(Line::Message(m)) = d.list.selected().and_then(|i| lines.get(i)) else {
        return;
    };
    msgdetails::show_system_message(Some(&d.window), None, &qap::shown(m));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn msg(id: &str, sev: u8, text: &str) -> ServerMessage {
        ServerMessage {
            id: id.into(),
            severity: sev,
            data: Vec::new(),
            text: text.into(),
            help: String::new(),
        }
    }

    fn outcome(status: QapStatus, escape: Option<ServerMessage>) -> CommandOutcome {
        CommandOutcome {
            status,
            escape,
            job: None,
            messages: Vec::new(),
        }
    }

    #[test]
    fn names() {
        assert_eq!(command_name("crtlib lib(x)"), "CRTLIB");
        assert_eq!(command_name("QSYS/DSPOBJD OBJ(A)"), "DSPOBJD");
        assert_eq!(command_name("DLYJOB(10)"), "DLYJOB");
        assert_eq!(command_name(""), "");
    }

    #[test]
    fn statuses() {
        let s = |st, esc| outcome_status("CRTLIB LIB(X)", &outcome(st, esc)).0;
        assert_eq!(s(QapStatus::Ok, None), "QAD5010: Command CRTLIB completed.");
        assert_eq!(
            s(
                QapStatus::Error,
                Some(msg("CPF2111", 40, "Library X already exists."))
            ),
            "QAD5011: Command CRTLIB ended with an error."
        );
        assert_eq!(
            s(QapStatus::Cancelled, None),
            "QAD5012: Command CRTLIB was cancelled."
        );
        assert_eq!(
            s(QapStatus::NotFound, None),
            "QAD5013: Command CRTLIB not found."
        );
        assert_eq!(
            s(
                QapStatus::NotAuthorized,
                Some(msg("CPF9802", 40, "Not authorized to object SECRET."))
            ),
            "CPF9802: Not authorized to object SECRET."
        );
    }

    #[test]
    fn rows_show_messages_as_sent() {
        let r = rows(&[
            Line::Ran("CRTLIB LIB(X)".into()),
            Line::Message(msg("CPC2102", 0, "Library X created.")),
            Line::Message(msg("CPF2111", 40, "Library X already exists.")),
            Line::Message(msg("CPDA091", 10, "Value for DTAAUT not valid.")),
            Line::Message(msg("CPFA096", 20, "Data truncated.")),
        ]);
        assert_eq!(r[0][2].text, "> CRTLIB LIB(X)");
        assert_eq!(
            (r[1][0].text.as_str(), r[1][1].text.as_str()),
            ("CPC2102", "00")
        );
        assert_eq!(r[2][0].status, Status::Error);
        assert_eq!(r[2][2].text, "Library X already exists.");
        assert_eq!(
            (r[3][1].text.as_str(), r[3][0].status),
            ("10", Status::Neutral)
        );
        assert_eq!(
            (r[4][1].text.as_str(), r[4][0].status),
            ("20", Status::Error)
        );
        assert_eq!(job_text(None), "Job:");
    }
}
