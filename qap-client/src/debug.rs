// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! Version 9's `Debug` service (draft): the system's source debugger, its
//! stops reported over QAP, through the [`Debugger`] trait on a
//! [`crate::Client`] or a [`crate::Attached`].
//!
//! - A connection has one debug session at a time, of its own job
//!   (`Own`: the programs `RunCommand` calls on that connection) or of
//!   another (`Job`). [`Debugger::start`] goes on a live connection; every
//!   other operation only on the connection the session started on, never
//!   a new one, so after a drop each fails with [`Error::Closed`]: the
//!   connection's close ended the session.
//! - A `Start` while a session is active is sent all the same; the server
//!   answers `InUse` (`TCP1A72`), and the session there is goes on.
//! - The events `Stopped` (100) and `DebugEnded` (101) come in on the
//!   connection's thread, as `MessageArrived` and the watches' do, and go
//!   straight to the [`DebugSession`]'s receiver, oldest first, whatever
//!   the caller is waiting on: a `RunCommand` whose program stopped gets
//!   no reply until it is resumed, and `Stack`, `Variables`, `Eval` and
//!   `Resume` answer meanwhile. The receiver disconnects when the
//!   connection closes or a later session starts on it; `End` sends no
//!   event, a sign-off sends `DebugEnded` (`TCP1A76`).
//! - Every operation runs concurrently, never in the lane.
//!
//! Every refusal is an [`Error::Status`] with the server's message as it
//! sent it; every operation but `Start` answers `Error` with `TCP1A71`
//! when no session is active on the connection.

use std::sync::mpsc::Receiver;

use ipc::Wire;
use qap_proto::Op;
use qap_proto::ops::debug::{
    AddProgram, BreakpointSet, BreakpointsSet, DebugEnded, DebugModule, DebugTarget, Eval,
    Evaluated, Frame, Frames, LineBreakpoint, ProgramAdded, RemoveServiceEntry, Resume, ResumeHow,
    SetBreakpoints, SetServiceEntry, Start, Started, Stopped, Variable, VariableList, Variables,
};
use qap_proto::types::ObjectName;

use crate::Error;
pub use crate::conn::Requests;

/// What a debug session reported, as the spec's events carry it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DebugEvent {
    /// `Debug` 100: a debugged job stopped, and waits for `Resume`.
    Stopped(Stopped),
    /// `Debug` 101: the session ended other than by `End`: the debugged
    /// job ended (`TCP1A75`) or the connection signed off (`TCP1A76`).
    DebugEnded(DebugEnded),
}

/// A debug session started.
#[derive(Debug)]
pub struct DebugSession {
    /// The session's number on its connection, as `Start` answered it.
    pub session: u32,
    /// Its events, oldest first; drained by the caller.
    pub events: Receiver<DebugEvent>,
}

/// Sends `op` on the connection open now and waits for its reply.
fn here<T: Requests + ?Sized, B: Wire, R: Wire>(c: &mut T, op: Op, body: &B) -> Result<R, Error> {
    let timeout = c.reply_timeout();
    c.request_here_op(op, body)?.wait(timeout)?.decode()
}

/// The `Debug` operations (version 9), on whichever connection implements
/// [`Requests`]. Each is refused here, before anything is sent, on a
/// connection whose version is below 9 ([`Error::Unsupported`]).
pub trait Debugger: Requests {
    /// `Start`: a debug session of `target` on a live connection, and the
    /// receiver its events arrive on from now.
    fn start(&mut self, target: DebugTarget) -> Result<DebugSession, Error> {
        let timeout = self.reply_timeout();
        let p = self.request_op(Op::DebugStart, &Start { target })?;
        let reply = match p.wait_for(timeout) {
            Some(r) => r?,
            None => {
                p.cancel();
                return Err(Error::TimedOut);
            }
        };
        let started: Started = reply.decode()?;
        Ok(DebugSession {
            session: started.session,
            events: p.debug_events(),
        })
    }

    /// `End`: a stopped job goes on, an attached job is let go, the
    /// service entry points are removed. No event follows.
    fn end(&mut self) -> Result<(), Error> {
        here(self, Op::DebugEnd, &())
    }

    /// `AddProgram`: puts `program` (`*PGM` or `*SRVPGM`, found as
    /// `STRDBG` finds it) under debug; its modules and their views.
    fn add_program(&mut self, program: &ObjectName, ty: &str) -> Result<Vec<DebugModule>, Error> {
        let a: ProgramAdded = here(
            self,
            Op::AddProgram,
            &AddProgram {
                program: program.clone(),
                r#type: ty.to_owned(),
            },
        )?;
        Ok(a.modules)
    }

    /// `SetBreakpoints`: `module`'s breakpoints (empty: the first module)
    /// from now; `lines` empty removes them. Where each landed, in order.
    fn set_breakpoints(
        &mut self,
        program: &ObjectName,
        module: &str,
        lines: &[LineBreakpoint],
    ) -> Result<Vec<BreakpointSet>, Error> {
        let s: BreakpointsSet = here(
            self,
            Op::SetBreakpoints,
            &SetBreakpoints {
                program: program.clone(),
                module: module.to_owned(),
                lines: lines.to_vec(),
            },
        )?;
        Ok(s.set)
    }

    /// `Resume`: the stopped job goes on as `how` says.
    fn resume(&mut self, how: ResumeHow) -> Result<(), Error> {
        here(self, Op::DebugResume, &Resume { how })
    }

    /// `Stack`: the stopped job's frames, the newest first.
    fn stack(&mut self) -> Result<Vec<Frame>, Error> {
        let f: Frames = here(self, Op::Stack, &())?;
        Ok(f.frames)
    }

    /// `Variables`: frame `frame`'s variables (0 the newest), or the parts
    /// of `parent`, a structure or array whose `expandable` was set.
    fn variables(&mut self, frame: u32, parent: Option<&str>) -> Result<Vec<Variable>, Error> {
        let v: VariableList = here(
            self,
            Op::Variables,
            &Variables {
                frame,
                parent: parent.map(str::to_owned),
            },
        )?;
        Ok(v.variables)
    }

    /// `Eval`: `expression` (`NAME`, `NAME:x`, `NAME:c`) in frame `frame`,
    /// as `EVAL` shows it.
    fn eval(&mut self, frame: u32, expression: &str) -> Result<String, Error> {
        let e: Evaluated = here(
            self,
            Op::Eval,
            &Eval {
                frame,
                expression: expression.to_owned(),
            },
        )?;
        Ok(e.value)
    }

    /// `SetServiceEntry`: the next call of `program` by a job running as
    /// `user` (`*CURRENT` or a profile) attaches that job to this session
    /// and stops it at the program's first statement.
    fn set_service_entry(&mut self, program: &ObjectName, user: &str) -> Result<(), Error> {
        here(
            self,
            Op::SetServiceEntry,
            &SetServiceEntry {
                program: program.clone(),
                user: user.to_owned(),
            },
        )
    }

    /// `RemoveServiceEntry`: `NotFound` (`TCP1A73`) when this session set
    /// none for `program`.
    fn remove_service_entry(&mut self, program: &ObjectName) -> Result<(), Error> {
        here(
            self,
            Op::RemoveServiceEntry,
            &RemoveServiceEntry {
                program: program.clone(),
            },
        )
    }
}

impl<T: Requests + ?Sized> Debugger for T {}
