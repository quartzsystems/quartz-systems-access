// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! Version 3's `Spool` service: spooled files and output queues.
//!
//! - Lists: [`Client::list_spooled_files`] and
//!   [`Client::list_output_queues`] fetch one page (`start`, `max`);
//!   [`Paging`] keeps where a list is and says what Previous and Next
//!   fetch. [`Client::all_spooled_files`] and [`Client::all_output_queues`]
//!   page through everything, for pickers and tests.
//! - Actions on one file ([`Action`]): hold, release, delete and move.
//!   [`Client::spool_action`] sends one and returns at once, so actions on
//!   several files run side by side; [`Action::outcome`] reads its reply.
//!   [`Client::act`] does both.
//! - Reading ([`read`]): an SCS file is read in batches of
//!   [`BATCH_PAGES`] pages, each written to the sink as it arrives, its
//!   leading state included, so the sink holds what renders as the whole
//!   file does. Progress after every batch; a cancel between batches, or
//!   in one, sends QAP `Cancel` for the batch in flight and discards what
//!   comes back. A text or PDF read is one request: those cannot be joined
//!   from ranges. A batch whose connection closes quietly or drops before
//!   it comes is read once more on the new connection.
//!
//! Statuses: lists answer `Ok`, `NotFound` (the queue, library or
//! profile) or `NotAuthorized`; a read also `InUse` (still open or being
//! written, `TCP1A57` on QS/400) and `Error` (the format cannot be made of
//! the file's device type, `TCP1A56`); hold, release, delete and move also
//! `InUse` (printing, for delete and move). Every refusal is an
//! [`Error::Status`] with the server's message as it sent it.

use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use qap_proto::Op;
use qap_proto::Status;
use qap_proto::ops::spool::{
    HoldSpooledFile, ListOutputQueues, ListSpooledFiles, MoveSpooledFile, OutputQueue,
    OutputQueues, ReadFormat, ReadSpooledFile, SpooledData, SpooledFile, SpooledFileId,
    SpooledFileRef, SpooledFiles,
};
use qap_proto::types::ObjectName;

use crate::conn::lost_and_retryable;
use crate::{Client, Error, Pending, Reply};

/// Pages per read of an SCS file.
pub const BATCH_PAGES: u32 = 25;

/// How often a read in flight looks for a cancel.
const LOOK: Duration = Duration::from_millis(25);

/// Entries per request when paging through a whole list.
pub(crate) const ALL_PAGE: u32 = 500;

/// Where a paged list is: the first entry shown (0-based), how many a page
/// shows, and how many there are by the last reply.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Paging {
    pub start: u32,
    pub per_page: u32,
    pub total: u32,
}

impl Paging {
    /// The first page of `per_page` (at least 1).
    pub fn new(per_page: u32) -> Paging {
        Paging {
            start: 0,
            per_page: per_page.max(1),
            total: 0,
        }
    }

    /// Takes in a reply's total. When entries went away and the page shown
    /// is past the end, it moves back to the last page; true then, and the
    /// page is to be fetched again.
    pub fn update(&mut self, total: u32) -> bool {
        self.total = total;
        if self.start > 0 && self.start >= total {
            self.start = self.last_start();
            return true;
        }
        false
    }

    /// The start of the last page.
    fn last_start(&self) -> u32 {
        self.total.saturating_sub(1) / self.per_page * self.per_page
    }

    /// The 1-based page shown, and how many pages there are (at least 1).
    pub fn page(&self) -> (u32, u32) {
        (
            self.start / self.per_page + 1,
            self.total.div_ceil(self.per_page).max(1),
        )
    }

    pub fn has_previous(&self) -> bool {
        self.start > 0
    }

    pub fn has_next(&self) -> bool {
        self.start.saturating_add(self.per_page) < self.total
    }

    /// To the previous page; false when there is none.
    pub fn previous_page(&mut self) -> bool {
        if !self.has_previous() {
            return false;
        }
        self.start = self.start.saturating_sub(self.per_page);
        true
    }

    /// To the next page; false when there is none.
    pub fn next_page(&mut self) -> bool {
        if !self.has_next() {
            return false;
        }
        self.start += self.per_page;
        true
    }

    /// Another page size, keeping the first entry shown on the page.
    pub fn set_per_page(&mut self, per_page: u32) {
        self.per_page = per_page.max(1);
        self.start = self.start / self.per_page * self.per_page;
    }

    /// The entries shown, 1-based and inclusive (`None` for an empty
    /// list), for "1-25 of 60".
    pub fn shown(&self, on_page: usize) -> Option<(u32, u32)> {
        let n = u32::try_from(on_page).unwrap_or(u32::MAX);
        (n > 0).then(|| (self.start + 1, self.start + n))
    }
}

/// What one request does to one spooled file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    /// Now, or (false) at the end of the page.
    Hold {
        id: SpooledFileId,
        immediately: bool,
    },
    Release(SpooledFileId),
    Delete(SpooledFileId),
    /// To `outq`; `first` to the top of it.
    Move {
        id: SpooledFileId,
        outq: ObjectName,
        first: bool,
    },
}

impl Action {
    /// The file it acts on.
    pub fn id(&self) -> &SpooledFileId {
        match self {
            Action::Hold { id, .. } | Action::Move { id, .. } => id,
            Action::Release(id) | Action::Delete(id) => id,
        }
    }

    /// Its operation.
    pub fn op(&self) -> Op {
        match self {
            Action::Hold { .. } => Op::HoldSpooledFile,
            Action::Release(_) => Op::ReleaseSpooledFile,
            Action::Delete(_) => Op::DeleteSpooledFile,
            Action::Move { .. } => Op::MoveSpooledFile,
        }
    }

    /// Its reply: `Ok(())`, or the refusal with the server's message.
    pub fn outcome(reply: Reply) -> Result<(), Error> {
        reply.decode::<()>()
    }
}

/// How far a read has come: pages written of the whole file's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Progress {
    pub pages_read: u32,
    pub total: u32,
}

/// How a read ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReadEnd {
    /// Every page written: the format the server made and the file's
    /// pages.
    Done { format: ReadFormat, pages: u32 },
    /// Cancelled; what was written before stays, nothing after it.
    Cancelled(Progress),
}

impl Client {
    /// `ListSpooledFiles` (version 3): one page of the files matching.
    pub fn list_spooled_files(&mut self, req: &ListSpooledFiles) -> Result<SpooledFiles, Error> {
        self.call(Op::ListSpooledFiles, req)
    }

    /// `ListOutputQueues` (version 3): one page of the queues in `library`
    /// (`*ALL`, `*LIBL` or a name).
    pub fn list_output_queues(&mut self, req: &ListOutputQueues) -> Result<OutputQueues, Error> {
        self.call(Op::ListOutputQueues, req)
    }

    /// Every file matching `req`, its `start` and `max` aside.
    pub fn all_spooled_files(&mut self, req: &ListSpooledFiles) -> Result<Vec<SpooledFile>, Error> {
        let mut req = req.clone();
        req.start = 0;
        req.max = ALL_PAGE;
        let mut out = Vec::new();
        loop {
            let page = self.list_spooled_files(&req)?;
            let n = page.files.len();
            out.extend(page.files);
            if n == 0 || out.len() >= page.total as usize {
                return Ok(out);
            }
            req.start = u32::try_from(out.len()).unwrap_or(u32::MAX);
        }
    }

    /// Every output queue in `library`.
    pub fn all_output_queues(&mut self, library: &str) -> Result<Vec<OutputQueue>, Error> {
        let mut req = ListOutputQueues {
            library: library.to_owned(),
            start: 0,
            max: ALL_PAGE,
        };
        let mut out = Vec::new();
        loop {
            let page = self.list_output_queues(&req)?;
            let n = page.queues.len();
            out.extend(page.queues);
            if n == 0 || out.len() >= page.total as usize {
                return Ok(out);
            }
            req.start = u32::try_from(out.len()).unwrap_or(u32::MAX);
        }
    }

    /// Sends `action` and returns at once; read the reply with
    /// [`Action::outcome`].
    pub fn spool_action(&mut self, action: &Action) -> Result<Pending, Error> {
        match action {
            Action::Hold { id, immediately } => self.request(
                Op::HoldSpooledFile,
                &HoldSpooledFile {
                    id: id.clone(),
                    immediately: *immediately,
                },
            ),
            Action::Release(id) => {
                self.request(Op::ReleaseSpooledFile, &SpooledFileRef { id: id.clone() })
            }
            Action::Delete(id) => {
                self.request(Op::DeleteSpooledFile, &SpooledFileRef { id: id.clone() })
            }
            Action::Move { id, outq, first } => self.request(
                Op::MoveSpooledFile,
                &MoveSpooledFile {
                    id: id.clone(),
                    outq: outq.clone(),
                    first: *first,
                },
            ),
        }
    }

    /// Does `action` and waits for it.
    pub fn act(&mut self, action: &Action) -> Result<(), Error> {
        let timeout = self.target().options.reply_timeout;
        let reply = self.spool_action(action)?.wait(timeout);
        Action::outcome(reply?)
    }

    /// Starts `ReadSpooledFile` (version 3) for one range. It has no reply
    /// timeout; [`read`] batches a whole file over it.
    pub fn read_spooled_file(&mut self, req: &ReadSpooledFile) -> Result<Pending, Error> {
        self.request(Op::ReadSpooledFile, req)
    }
}

/// The format to read a file of `device` in: its SCS when it has one, else
/// what the server may make of it (`Text` for ASCII and line data, `Pdf`
/// for the rest).
pub fn format_for(device: qap_proto::ops::spool::DeviceType) -> ReadFormat {
    use qap_proto::ops::spool::DeviceType;
    match device {
        DeviceType::Scs => ReadFormat::Scs,
        DeviceType::Ascii | DeviceType::Line => ReadFormat::Text,
        DeviceType::Afp => ReadFormat::Pdf,
    }
}

/// Reads spooled file `id` as `format` into `sink`.
///
/// - `send` starts one range; the application passes one that locks its
///   shared client only to send, tests the client itself.
/// - `Scs` goes in batches of `batch` pages, each written as it arrives;
///   `progress` is told after each, the total from the first reply.
///   `Text` and `Pdf` are one request for every page.
/// - `cancel` is looked at between batches and while one is in flight:
///   then the batch is cancelled and whatever it brings back discarded.
///
/// A refusal (`NotFound`, `NotAuthorized`, `InUse`, `Error`, ...) is an
/// [`Error::Status`] with the server's message; a write that fails is
/// [`Error::Write`].
pub fn read(
    mut send: impl FnMut(&ReadSpooledFile) -> Result<Pending, Error>,
    id: &SpooledFileId,
    format: ReadFormat,
    batch: u32,
    sink: &mut dyn Write,
    mut progress: impl FnMut(Progress),
    cancel: &AtomicBool,
) -> Result<ReadEnd, Error> {
    let batch = if format == ReadFormat::Scs {
        batch.max(1)
    } else {
        0
    };
    let mut done = Progress {
        pages_read: 0,
        total: 0,
    };
    let mut first = 1u32;
    loop {
        if cancel.load(Ordering::SeqCst) {
            return Ok(ReadEnd::Cancelled(done));
        }
        let req = ReadSpooledFile {
            id: id.clone(),
            format,
            first_page: first,
            last_page: if batch == 0 {
                0
            } else {
                first.saturating_add(batch - 1)
            },
        };
        let mut pending = send(&req)?;
        let mut asked = false;
        let mut sent_again = false;
        let reply = loop {
            if !asked && cancel.load(Ordering::SeqCst) {
                pending.cancel();
                asked = true;
            }
            match pending.wait_for(LOOK) {
                None => {}
                // The connection went under the batch: it only reads, so it
                // goes once more on the new connection `send` opens.
                Some(Err(e))
                    if !asked && !sent_again && lost_and_retryable(&e, Op::ReadSpooledFile) =>
                {
                    qsa_net::info!(
                        "qap: pages {}-{} lost with the connection ({e}); reading them once more",
                        req.first_page,
                        req.last_page
                    );
                    sent_again = true;
                    pending = send(&req)?;
                }
                Some(r) => break r?,
            }
        };
        if reply.status == Status::Cancelled || asked {
            // Its body, if any, is discarded unread.
            drop(reply);
            return Ok(ReadEnd::Cancelled(done));
        }
        let mut data: SpooledData = reply.decode()?;
        let written = sink.write_all(&data.data).map_err(Error::Write);
        data.data.fill(0);
        written?;
        done.total = data.pages;
        done.pages_read = if batch == 0 {
            data.pages
        } else {
            req.last_page.min(data.pages)
        };
        progress(done);
        if batch == 0 || done.pages_read >= done.total {
            sink.flush().map_err(Error::Write)?;
            return Ok(ReadEnd::Done {
                format: data.format,
                pages: data.pages,
            });
        }
        first = done.pages_read + 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paging() {
        let mut p = Paging::new(25);
        assert!(!p.update(60));
        assert_eq!(p.page(), (1, 3));
        assert!(!p.has_previous() && p.has_next());
        assert_eq!(p.shown(25), Some((1, 25)));
        assert!(p.next_page() && p.next_page());
        assert_eq!((p.start, p.page()), (50, (3, 3)));
        assert_eq!(p.shown(10), Some((51, 60)));
        assert!(!p.next_page(), "no page after the last");
        // Files went away: back to the last page there is.
        assert!(p.update(30));
        assert_eq!((p.start, p.page()), (25, (2, 2)));
        assert!(p.previous_page());
        assert!(!p.previous_page());
        // An empty list is one empty page.
        assert!(!p.update(0));
        assert_eq!((p.page(), p.shown(0)), ((1, 1), None));
        // A new page size keeps the first entry on the page shown.
        let mut p = Paging::new(25);
        p.update(300);
        p.start = 75;
        p.set_per_page(50);
        assert_eq!((p.start, p.page()), (50, (2, 6)));
        p.set_per_page(100);
        assert_eq!(p.start, 0);
        assert_eq!(Paging::new(0).per_page, 1);
    }

    #[test]
    fn formats_by_device_type() {
        use qap_proto::ops::spool::DeviceType;
        assert_eq!(format_for(DeviceType::Scs), ReadFormat::Scs);
        assert_eq!(format_for(DeviceType::Afp), ReadFormat::Pdf);
        assert_eq!(format_for(DeviceType::Ascii), ReadFormat::Text);
        assert_eq!(format_for(DeviceType::Line), ReadFormat::Text);
    }
}
