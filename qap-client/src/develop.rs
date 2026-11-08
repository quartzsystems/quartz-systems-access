// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! Version 8's `Develop` service (draft): the library list, libraries,
//! objects and source members, on a [`crate::Client`]'s shared connection
//! or a [`crate::Attached`] one, through the [`Develop`] trait.
//!
//! - Lanes: `GetLibraryList`, `SetLibraryList` and `Compile` run in the connection
//!   job's lane with `Command` and `Sql` 1 to 8, since they use the job's
//!   library list; they are sent as `RunCommand` is
//!   ([`Requests::request_op`]) and never sent again after a drop: a new
//!   connection has a new job, with its own list. Every other operation
//!   runs concurrently; the lists, `DescribeObject` and `ReadMember` only
//!   read, so one the connection's close cut off is sent once more
//!   ([`crate::retryable`]).
//! - [`Develop::read_member`]: a reply split with `More` is joined whole
//!   before it is decoded, as every reply is ([`crate::Connection`]).
//!   `SourceRecord`'s `seq` (hundredths: 100 is `0001.00`) and `date`
//!   (`yymmdd`, 0 for none) are the spec's `u32`s, passed on as they come.
//! - [`Develop::write_member`] takes the member's whole record list and
//!   sends it in [`pieces`] of at most one frame each, `first` on the
//!   first and `last` on the last (both on one piece), each after the
//!   reply to the one before, all on the same connection. The caller sees
//!   none of it: the server writes the member when the last arrives, and
//!   a write that stops before it changes nothing.
//!
//! - [`Develop::compile`] runs a command as `RunCommand` does, with no
//!   reply timeout ([`crate::runs_long`]); [`CompileOutcome`] carries its
//!   status, messages and the compiler's diagnostics, `Error` (the compile
//!   failed) with the body still filled.
//!
//! Every refusal is an [`Error::Status`] with the server's message as it
//! sent it: `NotFound` (`CPF9812` a file, `CPF9815` a member, `CPF9801`
//! an object, `CPF2110` a library), `NotAuthorized` (`CPF2182`), `InUse`
//! (`CPF1002`: another job holds the member), `Exists`, `Error` (`TCP1A5A`:
//! data the file's CCSID cannot hold) and `InvalidRequest` (`TCP1A62`: a
//! record longer than the file's data length; sequence numbers out of
//! order).

use qap_proto::ops::command::CommandText;
use qap_proto::ops::develop::{
    Attribute, ChangeMember, CompileReply, CopyMember, CreateMember, DescribeObject, Libraries,
    LibraryList, ListLibraries, ListMembers, ListObjects, MemberData, MemberRef, Members,
    ObjectDescription, Objects, RenameMember, SetLibraryList, WriteMember, Written,
};
use qap_proto::types::{Diagnostic, JobId, MemberName, ObjectName, SourceRecord};
use qap_proto::{Envelope, MAX_FRAME, Op, ServerMessage, Status};

pub use crate::conn::Requests;
use crate::{Error, Pending, Reply};

/// What a [`Develop::write_member`] changes beside the records.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WriteOptions {
    /// Number the records from `0001.00` in steps of `0001.00` (at most
    /// 9999 records); otherwise each `seq` must be ascending, from 1 to
    /// 999999.
    pub renumber: bool,
    /// The new source type; `None` leaves it.
    pub srctype: Option<String>,
    /// The new text; `None` leaves it.
    pub text: Option<String>,
}

/// How a `Compile` ended: `RunCommand`'s outcome and the compiler's
/// diagnostics.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompileOutcome {
    /// `Ok`, `Error` (the compile failed; the escape message, and the body
    /// still filled), `NotFound`, `NotAuthorized` or `Cancelled`.
    pub status: Status,
    /// The escape message that ended it, when one did.
    pub escape: Option<ServerMessage>,
    /// The job it ran in; `None` when the reply had no body (cancelled
    /// while it waited in the lane).
    pub job: Option<JobId>,
    /// Every message the command sent its caller, oldest first.
    pub messages: Vec<ServerMessage>,
    /// What the compiler reported, as its listing shows it; none for a
    /// command that is not a compile.
    pub diagnostics: Vec<Diagnostic>,
}

impl CompileOutcome {
    /// A `Compile` reply as its outcome. Any status a command's outcome
    /// is not carried with is an [`Error::Status`].
    pub fn from_reply(mut reply: Reply) -> Result<CompileOutcome, Error> {
        if !matches!(
            reply.status,
            Status::Ok
                | Status::Error
                | Status::NotFound
                | Status::NotAuthorized
                | Status::Cancelled
        ) {
            reply.body.fill(0);
            return Err(Error::Status {
                status: reply.status,
                message: reply.message.take(),
            });
        }
        let body: Option<CompileReply> = reply.body_as()?;
        let (job, messages, diagnostics) = match body {
            Some(b) => (Some(b.job), b.messages, b.diagnostics),
            None => (None, Vec::new(), Vec::new()),
        };
        Ok(CompileOutcome {
            status: reply.status,
            escape: reply.message.take(),
            job,
            messages,
            diagnostics,
        })
    }

    /// Whether the compile succeeded.
    pub fn ok(&self) -> bool {
        self.status == Status::Ok
    }
}

/// The request ID a piece is measured with: the widest there is.
const MEASURE_ID: u32 = u32::MAX;

/// `records` as the `WriteMember` pieces that carry them, in order, each
/// one frame of at most [`MAX_FRAME`] bytes encoded: `first` on the first,
/// `last` on the last, both on one piece when they fit in one. Every piece
/// carries `options`; the server takes the last's. An empty list is one
/// piece, which empties the member. [`Error::TooLarge`] for a record that
/// cannot fit in a frame by itself.
pub fn pieces(
    member: &MemberName,
    records: &[SourceRecord],
    options: &WriteOptions,
) -> Result<Vec<WriteMember>, Error> {
    let piece = |records: Vec<SourceRecord>| WriteMember {
        member: member.clone(),
        records,
        renumber: options.renumber,
        srctype: options.srctype.clone(),
        text: options.text.clone(),
        first: false,
        last: false,
    };
    // A piece's frame is its envelope with no records plus each record's
    // encoding: Wire's lengths are fixed `u32`s, so the sum is exact.
    let head = ipc::encode(&Envelope::request(
        MEASURE_ID,
        Op::WriteMember,
        &piece(Vec::new()),
    ))
    .len();
    let mut out = Vec::new();
    let mut current = Vec::new();
    let mut size = head;
    for r in records {
        let n = ipc::encode(r).len();
        if head + n > MAX_FRAME {
            return Err(Error::TooLarge(head + n));
        }
        if size + n > MAX_FRAME {
            out.push(piece(std::mem::take(&mut current)));
            size = head;
        }
        current.push(r.clone());
        size += n;
    }
    out.push(piece(current));
    out[0].first = true;
    out.last_mut().expect("one piece at least").last = true;
    Ok(out)
}

/// The `Develop` operations (version 8), on whichever connection
/// implements [`Requests`]. Each is refused here, before anything is sent,
/// on a connection whose version is below 8 ([`Error::Unsupported`]).
pub trait Develop: Requests {
    /// `GetLibraryList`, in the lane: the connection job's library list.
    fn get_library_list(&mut self) -> Result<LibraryList, Error> {
        let timeout = self.reply_timeout();
        self.request_op(Op::GetLibraryList, &())?
            .wait(timeout)?
            .decode()
    }

    /// `SetLibraryList`, in the lane: `CHGLIBL LIBL(user) CURLIB(current)`,
    /// `user` empty for `*NONE` and `current` none for `*CRTDFT`; the list
    /// as it now is. A list `get_library_list` answered can be set again
    /// as it is.
    fn set_library_list(&mut self, req: &SetLibraryList) -> Result<LibraryList, Error> {
        let timeout = self.reply_timeout();
        self.request_op(Op::SetLibraryList, req)?
            .wait(timeout)?
            .decode()
    }

    /// `ListLibraries`: one page of the libraries matching a generic name
    /// (empty for `*ALL`), by name.
    fn list_libraries(&mut self, req: &ListLibraries) -> Result<Libraries, Error> {
        self.call_op(Op::ListLibraries, req)
    }

    /// `ListObjects`: one page of a library's objects (`*LIBL`, `*CURLIB`,
    /// `*USRLIBL`, `*ALL` or a library), by library in search order, then
    /// name, then type.
    fn list_objects(&mut self, req: &ListObjects) -> Result<Objects, Error> {
        self.call_op(Op::ListObjects, req)
    }

    /// `DescribeObject`: the object's `DSPOBJD DETAIL(*FULL)` lines, in
    /// order, under the server's labels.
    fn describe_object(&mut self, object: &ObjectName, ty: &str) -> Result<Vec<Attribute>, Error> {
        let d: ObjectDescription = self.call_op(
            Op::DescribeObject,
            &DescribeObject {
                object: object.clone(),
                r#type: ty.to_owned(),
            },
        )?;
        Ok(d.attributes)
    }

    /// `ListMembers`: one page of a source file's members, by name.
    fn list_members(&mut self, req: &ListMembers) -> Result<Members, Error> {
        self.call_op(Op::ListMembers, req)
    }

    /// `ReadMember`: the member's records whole, every `More` piece of the
    /// reply joined, with its type, text, data length and CCSID. With
    /// `lock` the member is held `*SHRNUP` for this connection until
    /// `unlock_member`, a `write_member` of it here, or the connection's
    /// end.
    fn read_member(&mut self, member: &MemberName, lock: bool) -> Result<MemberData, Error> {
        self.call_op(
            Op::ReadMember,
            &qap_proto::ops::develop::ReadMember {
                member: member.clone(),
                lock,
            },
        )
    }

    /// `WriteMember`: replaces the member's records with `records`, in
    /// [`pieces`] under one frame each, the first on a live connection and
    /// each after it on the same one once the one before is answered. The
    /// member's records as written. A record whose data is unchanged keeps
    /// the `date` sent; the server dates the others today.
    fn write_member(
        &mut self,
        member: &MemberName,
        records: &[SourceRecord],
        options: &WriteOptions,
    ) -> Result<u32, Error> {
        let timeout = self.reply_timeout();
        let mut written = 0;
        for (i, p) in pieces(member, records, options)?.iter().enumerate() {
            let w: Written = if i == 0 {
                self.call_op(Op::WriteMember, p)?
            } else {
                self.request_here_op(Op::WriteMember, p)?
                    .wait(timeout)?
                    .decode()?
            };
            written = w.records;
        }
        Ok(written)
    }

    /// `UnlockMember`: ends this connection's lock; `NotFound` (`TCP1A61`)
    /// when it holds none.
    fn unlock_member(&mut self, member: &MemberName) -> Result<(), Error> {
        self.call_op(
            Op::UnlockMember,
            &MemberRef {
                member: member.clone(),
            },
        )
    }

    /// `CreateMember`: `ADDPFM`.
    fn create_member(
        &mut self,
        member: &MemberName,
        srctype: &str,
        text: &str,
    ) -> Result<(), Error> {
        self.call_op(
            Op::CreateMember,
            &CreateMember {
                member: member.clone(),
                srctype: srctype.to_owned(),
                text: text.to_owned(),
            },
        )
    }

    /// `CopyMember`: `CPYSRCF` of one member, sequence numbers and dates
    /// kept; `replace` for `MBROPT(*REPLACE)`.
    fn copy_member(
        &mut self,
        from: &MemberName,
        to: &MemberName,
        replace: bool,
    ) -> Result<(), Error> {
        self.call_op(
            Op::CopyMember,
            &CopyMember {
                from: from.clone(),
                to: to.clone(),
                replace,
            },
        )
    }

    /// `RenameMember`: `RNMM`.
    fn rename_member(&mut self, member: &MemberName, new_name: &str) -> Result<(), Error> {
        self.call_op(
            Op::RenameMember,
            &RenameMember {
                member: member.clone(),
                new_name: new_name.to_owned(),
            },
        )
    }

    /// `DeleteMember`: `RMVM`.
    fn delete_member(&mut self, member: &MemberName) -> Result<(), Error> {
        self.call_op(
            Op::DeleteMember,
            &MemberRef {
                member: member.clone(),
            },
        )
    }

    /// `ChangeMember`: `CHGPFM`, `srctype` and `text` none leaving each as
    /// it is.
    fn change_member(
        &mut self,
        member: &MemberName,
        srctype: Option<&str>,
        text: Option<&str>,
    ) -> Result<(), Error> {
        self.call_op(
            Op::ChangeMember,
            &ChangeMember {
                member: member.clone(),
                srctype: srctype.map(str::to_owned),
                text: text.map(str::to_owned),
            },
        )
    }

    /// Starts `Compile`, in the lane: `command` run as `RunCommand` runs
    /// it, with no reply timeout, answered with the diagnostics the
    /// compiler left on the job. Wait on the [`Pending`] (or cancel it)
    /// and read it with [`CompileOutcome::from_reply`]; never sent again
    /// after a drop.
    fn compile(&mut self, command: &str) -> Result<Pending, Error> {
        self.request_op(
            Op::Compile,
            &CommandText {
                command: command.to_owned(),
            },
        )
    }
}

impl<T: Requests + ?Sized> Develop for T {}

#[cfg(test)]
mod tests {
    use super::*;

    fn member() -> MemberName {
        MemberName {
            file: ObjectName {
                library: "HLINV".into(),
                name: "QRPGLESRC".into(),
            },
            member: "BIG".into(),
        }
    }

    fn records(n: u32, width: usize) -> Vec<SourceRecord> {
        (1..=n)
            .map(|i| SourceRecord {
                seq: i * 100,
                date: 260_105,
                data: "x".repeat(width),
            })
            .collect()
    }

    fn framed(p: &WriteMember) -> usize {
        ipc::encode(&Envelope::request(MEASURE_ID, Op::WriteMember, p)).len()
    }

    #[test]
    fn one_piece_is_first_and_last() {
        let p = pieces(&member(), &records(3, 10), &WriteOptions::default()).unwrap();
        assert_eq!(p.len(), 1);
        assert!(p[0].first && p[0].last);
        let p = pieces(&member(), &[], &WriteOptions::default()).unwrap();
        assert_eq!((p.len(), p[0].records.len()), (1, 0));
        assert!(p[0].first && p[0].last);
    }

    #[test]
    fn pieces_fill_frames_and_keep_the_order() {
        let all = records(12_000, 100);
        let o = WriteOptions {
            renumber: false,
            srctype: Some("RPGLE".into()),
            text: Some("Big".into()),
        };
        let p = pieces(&member(), &all, &o).unwrap();
        assert!(p.len() > 1);
        for (i, piece) in p.iter().enumerate() {
            assert!(framed(piece) <= MAX_FRAME, "piece {i}: {}", framed(piece));
            assert_eq!(piece.first, i == 0);
            assert_eq!(piece.last, i + 1 == p.len());
            assert_eq!((&piece.srctype, &piece.text), (&o.srctype, &o.text));
        }
        // Each piece but the last is full: one more record would not fit.
        let one = ipc::encode(&all[0]).len();
        for piece in &p[..p.len() - 1] {
            assert!(framed(piece) + one > MAX_FRAME);
        }
        let joined: Vec<_> = p.into_iter().flat_map(|p| p.records).collect();
        assert_eq!(joined, all);
    }

    #[test]
    fn a_record_too_large_for_a_frame() {
        let e = pieces(&member(), &records(1, MAX_FRAME), &WriteOptions::default()).unwrap_err();
        assert!(matches!(e, Error::TooLarge(_)), "{e:?}");
    }
}
