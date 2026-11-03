// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! Version 5's `Ifs` service: integrated file system files.
//!
//! - [`Files`]: the eight operations, typed, on the shared connection (a
//!   [`Client`]) or a window's own ([`Attached`]). Every `Ifs` operation
//!   runs beside the lane on the server, so none waits for a command or a
//!   statement. Refusals are [`Error::Status`] with the file-system
//!   command's own message as sent: `CPFA0A9` `NotFound`, `CPFA09C`
//!   `NotAuthorized`, `CPFA0A0` `Exists`, `CPFA09E` `InUse`, `CPFA0AC` a
//!   directory not empty (`Error`).
//! - [`Listing`]: a directory's entries a page at a time (500 by default),
//!   for a list that loads the next page as it scrolls.
//! - [`read`]: a file read in requests of [`READ_CHUNK`] file bytes into a
//!   sink, with progress after each and cancel in or between them. With
//!   conversion (`as_ccsid`) a reply holds whole characters only, so the
//!   next request starts at the reply's `next_offset`, never at offset plus
//!   length; at the end a partial character is one substitution and
//!   `next_offset` is the file's size, so the loop always ends. A mixed
//!   file is converted in the shift state it has at the offset, so a read
//!   may start at any confirmed `next_offset`, on any connection: that is
//!   how a text-mode download resumes. A request the connection's close
//!   cut off is read once more on the new connection.
//! - [`write`]: a file written from a reader in requests under 1 MiB, with
//!   progress and cancel. Binary goes as stored: the first request
//!   `Create` or `Replace`, then `Update` at each offset, so a binary
//!   upload resumes with `Update` at the offset the system has (checked
//!   with `GetInfo`, [`resume_offset`]). Text goes with `data_ccsid` (1208
//!   for UTF-8), split only on whole characters, the first request
//!   `Create` or `Replace` and then `Append`: conversion is per request,
//!   and converted offsets do not map back to local bytes, so a text upload
//!   that stops starts again from the beginning.
//!
//! Links: `GetInfo` does not follow a symbolic link, `ReadFile` does. A
//! link opens as its target with `GetInfo` of the `target` path; a broken
//! link's target is `NotFound`.

use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use ipc::Wire;
use qap_proto::Op;
use qap_proto::Status;
use qap_proto::ops::ifs::{
    CreateDirectory, Entries, Entry, EntryKind, FileData, FromTo, ListDirectory, PathRef, ReadFile,
    WriteFile, WriteMode, Written,
};

use crate::conn::lost_and_retryable;
use crate::{Attached, Client, Error, Pending};

/// File bytes asked for by one `ReadFile`.
pub const READ_CHUNK: u64 = 1 << 20;

/// Data bytes in one `WriteFile`, the path aside: the request with its
/// path and envelope stays under the 1 MiB frame.
pub const WRITE_CHUNK: usize = (1 << 20) - 8 * 1024;

/// Entries per `ListDirectory` page.
pub const LIST_PAGE: u32 = 500;

/// The CCSID of UTF-8, as text moves between this program and the system.
pub const UTF8: u16 = 1208;

/// A file whose data has no CCSID: read and written as stored.
pub const BINARY: u16 = 65535;

/// How often a read or write in flight looks for a cancel.
const LOOK: Duration = Duration::from_millis(25);

/// The `Ifs` operations, on whichever connection implements
/// [`Files::ifs_call`] and [`Files::ifs_request`].
pub trait Files {
    /// Sends `op` and waits for its reply, decoded; a read-only operation
    /// the connection's close cut off is sent once more.
    fn ifs_call<B: Wire, R: Wire>(&mut self, op: Op, body: &B) -> Result<R, Error>;

    /// Sends `op` and returns at once.
    fn ifs_request<B: Wire>(&mut self, op: Op, body: &B) -> Result<Pending, Error>;

    /// `ListDirectory`: one page of `path`'s entries matching `pattern`
    /// (`*` and `?`; empty for all), from `start`, at most `max` (0 for no
    /// limit).
    fn list_directory(
        &mut self,
        path: &str,
        pattern: &str,
        start: u32,
        max: u32,
    ) -> Result<Entries, Error> {
        self.ifs_call(
            Op::ListDirectory,
            &ListDirectory {
                path: path.to_owned(),
                pattern: pattern.to_owned(),
                start,
                max,
            },
        )
    }

    /// Every entry of `path` matching `pattern`, page by page.
    fn all_entries(&mut self, path: &str, pattern: &str) -> Result<Vec<Entry>, Error> {
        let mut l = Listing::new(path, pattern);
        let mut out = Vec::new();
        while let Some(page) = l.next_page(self)? {
            out.extend(page);
        }
        Ok(out)
    }

    /// `GetInfo`: one entry, a symbolic link itself and not its target.
    fn get_info(&mut self, path: &str) -> Result<Entry, Error> {
        self.ifs_call(
            Op::GetInfo,
            &PathRef {
                path: path.to_owned(),
            },
        )
    }

    /// `ReadFile` of one range: `length` file bytes from `offset` (0 for to
    /// the end), converted to `as_ccsid` when given.
    fn read_file(
        &mut self,
        path: &str,
        offset: u64,
        length: u64,
        as_ccsid: Option<u16>,
    ) -> Result<FileData, Error> {
        self.ifs_call(
            Op::ReadFile,
            &ReadFile {
                path: path.to_owned(),
                offset,
                length,
                as_ccsid,
            },
        )
    }

    /// `WriteFile` of one request; the file's size in its own CCSID.
    fn write_file(&mut self, req: &WriteFile) -> Result<u64, Error> {
        self.ifs_call::<_, Written>(Op::WriteFile, req)
            .map(|w| w.size)
    }

    /// `CreateDirectory`; with `parents` the missing directories above it
    /// too, and an existing directory is then no error.
    fn create_directory(&mut self, path: &str, parents: bool) -> Result<(), Error> {
        self.ifs_call(
            Op::CreateDirectory,
            &CreateDirectory {
                path: path.to_owned(),
                parents,
            },
        )
    }

    /// `Delete`: a file, a symbolic link (not what it names) or an empty
    /// directory.
    fn delete(&mut self, path: &str) -> Result<(), Error> {
        self.ifs_call(
            Op::Delete,
            &PathRef {
                path: path.to_owned(),
            },
        )
    }

    /// `Rename`: a rename or a move; `replace` replaces `to`.
    fn rename(&mut self, from: &str, to: &str, replace: bool) -> Result<(), Error> {
        self.ifs_call(
            Op::Rename,
            &FromTo {
                from: from.to_owned(),
                to: to.to_owned(),
                replace,
            },
        )
    }

    /// `Copy`: a file copied on the system, its CCSID and data as stored.
    fn copy(&mut self, from: &str, to: &str, replace: bool) -> Result<(), Error> {
        self.ifs_call(
            Op::Copy,
            &FromTo {
                from: from.to_owned(),
                to: to.to_owned(),
                replace,
            },
        )
    }

    /// What `entry` (in directory `dir`) opens as: itself, or for a
    /// symbolic link its target's entry (`GetInfo` of the `target` path,
    /// made absolute against `dir`). A broken link answers `NotFound`.
    fn resolve(&mut self, dir: &str, entry: &Entry) -> Result<(String, Entry), Error> {
        let path = join(dir, &entry.name);
        match (&entry.kind, &entry.target) {
            (EntryKind::Symlink, Some(t)) => {
                let target = if t.starts_with('/') {
                    t.clone()
                } else {
                    join(dir, t)
                };
                let e = self.get_info(&target)?;
                Ok((target, e))
            }
            _ => Ok((path, entry.clone())),
        }
    }
}

impl Files for Client {
    fn ifs_call<B: Wire, R: Wire>(&mut self, op: Op, body: &B) -> Result<R, Error> {
        self.call(op, body)
    }

    fn ifs_request<B: Wire>(&mut self, op: Op, body: &B) -> Result<Pending, Error> {
        self.request(op, body)
    }
}

impl Files for Attached {
    fn ifs_call<B: Wire, R: Wire>(&mut self, op: Op, body: &B) -> Result<R, Error> {
        self.call(op, body)
    }

    fn ifs_request<B: Wire>(&mut self, op: Op, body: &B) -> Result<Pending, Error> {
        self.request(op, body)
    }
}

/// `dir` and `name` as one path.
pub fn join(dir: &str, name: &str) -> String {
    if dir.ends_with('/') {
        format!("{dir}{name}")
    } else {
        format!("{dir}/{name}")
    }
}

/// The directory above `path` (`/` for a path just under it).
pub fn parent(path: &str) -> &str {
    match path.trim_end_matches('/').rfind('/') {
        Some(0) | None => "/",
        Some(i) => &path[..i],
    }
}

/// The last component of `path`.
pub fn file_name(path: &str) -> &str {
    let p = path.trim_end_matches('/');
    p.rsplit('/').next().unwrap_or(p)
}

/// A directory listed a page at a time: where the next page starts and how
/// many entries match, by the last reply.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Listing {
    pub path: String,
    pub pattern: String,
    pub per_page: u32,
    /// The first entry of the next page.
    pub next: u32,
    /// Entries matching, by the last reply; `None` before the first.
    pub total: Option<u32>,
}

impl Listing {
    /// `path`'s entries matching `pattern`, [`LIST_PAGE`] a page.
    pub fn new(path: &str, pattern: &str) -> Listing {
        Listing {
            path: path.to_owned(),
            pattern: pattern.to_owned(),
            per_page: LIST_PAGE,
            next: 0,
            total: None,
        }
    }

    /// Whether every entry has been fetched.
    pub fn done(&self) -> bool {
        self.total.is_some_and(|t| self.next >= t)
    }

    /// The next page, or `None` when every entry has been fetched.
    pub fn next_page<F: Files + ?Sized>(&mut self, f: &mut F) -> Result<Option<Vec<Entry>>, Error> {
        if self.done() {
            return Ok(None);
        }
        let page = f.list_directory(&self.path, &self.pattern, self.next, self.per_page)?;
        self.total = Some(page.total);
        if page.entries.is_empty() {
            // Entries went away since the last page.
            self.next = page.total;
            return Ok(None);
        }
        self.next = self
            .next
            .saturating_add(u32::try_from(page.entries.len()).unwrap_or(u32::MAX));
        Ok(Some(page.entries))
    }
}

/// How a read is made.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReadOptions {
    /// Convert from the file's CCSID to this one; `None` for as stored.
    pub as_ccsid: Option<u16>,
    /// File bytes per request.
    pub chunk: u64,
    /// The file offset to start at: 0, or where an earlier read stopped
    /// (its last confirmed `next_offset`).
    pub offset: u64,
}

impl Default for ReadOptions {
    fn default() -> ReadOptions {
        ReadOptions {
            as_ccsid: None,
            chunk: READ_CHUNK,
            offset: 0,
        }
    }
}

/// How far a read or write has come.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Progress {
    /// The file offset confirmed: for a read, the last reply's
    /// `next_offset`, its data written to the sink; for a write, the source
    /// bytes the system has taken.
    pub offset: u64,
    /// The file's size by the last reply: the system's file for a read, in
    /// its own CCSID for a write.
    pub size: u64,
    /// Bytes this side wrote to the sink (a read) or read from the source
    /// (a write), this run.
    pub bytes: u64,
}

/// How a read or write ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum End {
    /// Done: where it got to.
    Done(Progress),
    /// Cancelled: what was confirmed stays, nothing after it.
    Cancelled(Progress),
}

impl End {
    pub fn progress(&self) -> Progress {
        match self {
            End::Done(p) | End::Cancelled(p) => *p,
        }
    }
}

/// Waits for `pending`, cancelling it when `cancel` is set; the reply, or
/// `None` when it was cancelled. A read-only request (`retry`) the
/// connection's close cut off is sent once more with `again`.
fn wait(
    mut pending: Pending,
    cancel: &AtomicBool,
    retry: bool,
    mut again: impl FnMut() -> Result<Pending, Error>,
) -> Result<Option<crate::Reply>, Error> {
    let mut asked = false;
    let mut sent_again = false;
    loop {
        if !asked && cancel.load(Ordering::SeqCst) {
            pending.cancel();
            asked = true;
        }
        match pending.wait_for(LOOK) {
            None => {}
            Some(Err(e))
                if retry && !asked && !sent_again && lost_and_retryable(&e, pending.op()) =>
            {
                qsa_net::info!(
                    "qap: {:?} lost with the connection ({e}); sending it once more",
                    pending.op()
                );
                sent_again = true;
                pending = again()?;
            }
            Some(r) => {
                let r = r?;
                if asked || r.status == Status::Cancelled {
                    return Ok(None);
                }
                return Ok(Some(r));
            }
        }
    }
}

/// Reads `path` into `sink` in requests of `opts.chunk` file bytes from
/// `opts.offset`, converted to `opts.as_ccsid` when given.
///
/// - `send` starts one `ReadFile`; the application passes one that holds
///   its connection only to send, tests the connection itself.
/// - `progress` is told after each reply's data is in the sink: its
///   `offset` is where a later read resumes.
/// - `cancel` is looked at between requests and while one is in flight.
///
/// A refusal is an [`Error::Status`] with the system's message; a sink
/// that fails is [`Error::Write`].
pub fn read(
    mut send: impl FnMut(&ReadFile) -> Result<Pending, Error>,
    path: &str,
    opts: ReadOptions,
    sink: &mut dyn Write,
    mut progress: impl FnMut(Progress),
    cancel: &AtomicBool,
) -> Result<End, Error> {
    let chunk = opts.chunk.max(1);
    let mut done = Progress {
        offset: opts.offset,
        size: 0,
        bytes: 0,
    };
    loop {
        if cancel.load(Ordering::SeqCst) {
            return Ok(End::Cancelled(done));
        }
        let req = ReadFile {
            path: path.to_owned(),
            offset: done.offset,
            length: chunk,
            as_ccsid: opts.as_ccsid,
        };
        let Some(reply) = wait(send(&req)?, cancel, true, || send(&req))? else {
            return Ok(End::Cancelled(done));
        };
        let mut d: FileData = reply.decode()?;
        let w = sink.write_all(&d.data).map_err(Error::Write);
        let n = d.data.len() as u64;
        d.data.fill(0);
        w?;
        done.size = d.size;
        done.bytes += n;
        if d.next_offset <= done.offset && d.next_offset < d.size {
            return Err(Error::Protocol(format!(
                "ReadFile of {path} made no progress at offset {}",
                done.offset
            )));
        }
        done.offset = d.next_offset;
        progress(done);
        if done.offset >= d.size {
            sink.flush().map_err(Error::Write)?;
            return Ok(End::Done(done));
        }
    }
}

/// How a write is made.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WriteOptions {
    /// Text: the CCSID the source is in (1208 for UTF-8), converted by the
    /// system to the file's; split only on whole characters. `None` for
    /// binary, written as it is.
    pub data_ccsid: Option<u16>,
    /// The CCSID of a file created (0 for 1208).
    pub ccsid: u16,
    /// The first request's mode: `Create` (the file must not exist) or
    /// `Replace`. Ignored when resuming.
    pub first: WriteMode,
    /// Source bytes per request at most ([`WRITE_CHUNK`] less the path).
    pub chunk: usize,
    /// Binary only: resume at this offset with `Update` (the source read on
    /// from there); see [`resume_offset`].
    pub resume_at: Option<u64>,
}

impl Default for WriteOptions {
    fn default() -> WriteOptions {
        WriteOptions {
            data_ccsid: None,
            ccsid: 0,
            first: WriteMode::Replace,
            chunk: WRITE_CHUNK,
            resume_at: None,
        }
    }
}

/// Where a binary upload of `local_len` bytes to `path` resumes: the
/// system's size of the file (`GetInfo`), no further than the local file;
/// 0 when the file is not there.
pub fn resume_offset<F: Files + ?Sized>(
    f: &mut F,
    path: &str,
    local_len: u64,
) -> Result<u64, Error> {
    match f.get_info(path) {
        Ok(e) => Ok(e.size.min(local_len)),
        Err(e) if e.status() == Some(Status::NotFound) => Ok(0),
        Err(e) => Err(e),
    }
}

/// The length of `buf` that ends on a whole UTF-8 character: a partial
/// character at its end is left for the next request. Invalid bytes are
/// not looked into: they are cut as they come.
pub fn whole_utf8(buf: &[u8]) -> usize {
    let n = buf.len();
    // The start of the last character: at most three continuation bytes
    // back.
    let Some(back) = (1..=n.min(4)).find(|&k| buf[n - k] & 0xC0 != 0x80) else {
        return n;
    };
    let lead = buf[n - back];
    let need = match lead {
        b if b < 0x80 => 1,
        b if b & 0xE0 == 0xC0 => 2,
        b if b & 0xF0 == 0xE0 => 3,
        b if b & 0xF8 == 0xF0 => 4,
        _ => 1,
    };
    if back < need { n - back } else { n }
}

/// Reads up to `room` bytes from `source` onto the end of `buf`; how many.
fn fill(source: &mut dyn Read, buf: &mut Vec<u8>, room: usize) -> Result<usize, Error> {
    let start = buf.len();
    buf.resize(start + room, 0);
    let mut got = 0;
    while got < room {
        match source.read(&mut buf[start + got..]) {
            Ok(0) => break,
            Ok(n) => got += n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(e) => {
                buf.truncate(start + got);
                return Err(Error::Read(e));
            }
        }
    }
    buf.truncate(start + got);
    Ok(got)
}

/// Writes `source` to `path` in requests of at most `opts.chunk` bytes,
/// binary or as text (see [`WriteOptions`]).
///
/// - `send` starts one `WriteFile`. A write changes the file, so it is
///   never sent again by itself: a lost connection fails with
///   [`Error::Closed`], and the caller resumes (binary) or starts again
///   (text).
/// - `progress` is told after each request the system has taken.
/// - `cancel` is looked at between requests and while one is in flight.
///
/// A file of no bytes is still written once, so it is made (or emptied).
pub fn write(
    mut send: impl FnMut(&WriteFile) -> Result<Pending, Error>,
    path: &str,
    source: &mut dyn Read,
    opts: WriteOptions,
    mut progress: impl FnMut(Progress),
    cancel: &AtomicBool,
) -> Result<End, Error> {
    let text = opts.data_ccsid.is_some();
    let resume = if text { None } else { opts.resume_at };
    let chunk = opts
        .chunk
        .min(WRITE_CHUNK.saturating_sub(path.len()))
        .max(4);
    let mut done = Progress {
        offset: resume.unwrap_or(0),
        size: 0,
        bytes: 0,
    };
    let mut first = resume.is_none();
    // Bytes read but not sent yet: a partial character held back.
    let mut buf: Vec<u8> = Vec::with_capacity(chunk);
    let mut at_end = false;
    loop {
        if cancel.load(Ordering::SeqCst) {
            return Ok(End::Cancelled(done));
        }
        if !at_end {
            let room = chunk - buf.len();
            at_end = fill(source, &mut buf, room)? < room;
        }
        if buf.is_empty() && !first {
            return Ok(End::Done(done));
        }
        let take = if text && !at_end {
            match whole_utf8(&buf) {
                // Not one whole character in the chunk: send it as it is.
                0 => buf.len(),
                n => n,
            }
        } else {
            buf.len()
        };
        let mode = match (first, text) {
            (true, _) => opts.first,
            (false, true) => WriteMode::Append,
            (false, false) => WriteMode::Update,
        };
        let req = WriteFile {
            path: path.to_owned(),
            mode,
            offset: if mode == WriteMode::Update {
                done.offset
            } else {
                0
            },
            ccsid: opts.ccsid,
            data: buf[..take].to_vec(),
            data_ccsid: opts.data_ccsid,
        };
        let pending = send(&req);
        let mut req = req;
        req.data.fill(0);
        let Some(reply) = wait(pending?, cancel, false, || unreachable!())? else {
            return Ok(End::Cancelled(done));
        };
        let w: Written = reply.decode()?;
        buf.drain(..take);
        first = false;
        done.offset += take as u64;
        done.bytes += take as u64;
        done.size = w.size;
        progress(done);
        if at_end && buf.is_empty() {
            return Ok(End::Done(done));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utf8_is_cut_on_whole_characters() {
        let s = "aü€😀";
        let b = s.as_bytes();
        // Every cut leaves whole characters before it.
        for n in 0..=b.len() {
            let k = whole_utf8(&b[..n]);
            assert!(std::str::from_utf8(&b[..k]).is_ok(), "{n}: {k}");
            assert!(n - k < 4);
        }
        assert_eq!(whole_utf8(b), b.len());
        assert_eq!(whole_utf8(&b[..2]), 1, "ü cut after its first byte");
        assert_eq!(whole_utf8(&b[..b.len() - 1]), b.len() - 4);
        assert_eq!(whole_utf8(b""), 0);
        assert_eq!(whole_utf8(&[0xFF, 0x80]), 2, "invalid bytes go as they are");
    }

    #[test]
    fn paths() {
        assert_eq!(join("/", "home"), "/home");
        assert_eq!(join("/home/BOB", "a.txt"), "/home/BOB/a.txt");
        assert_eq!(parent("/home/BOB/a.txt"), "/home/BOB");
        assert_eq!(parent("/home"), "/");
        assert_eq!(parent("/"), "/");
        assert_eq!(file_name("/home/BOB/a.txt"), "a.txt");
        assert_eq!(file_name("/home/BOB/"), "BOB");
    }
}
