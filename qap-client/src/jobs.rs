// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! Version 8's `Jobs` service (draft): a user's jobs and job logs, hold,
//! release and end, through the [`Jobs`] trait on a [`crate::Client`] or a
//! [`crate::Attached`]. Every operation runs concurrently as the signed-on
//! profile, with its command's checks: the profile's own jobs need
//! nothing, another user's (and `*ALL`) need `*JOBCTL`.
//!
//! `ListJobs` and `JobLog` only read, so one the connection's close cut
//! off is sent once more ([`crate::retryable`]); `HoldJob`, `ReleaseJob`
//! and `EndJob` never are.
//!
//! Every refusal is an [`Error::Status`] with the server's message as it
//! sent it: `NotFound` (`CPF2204` an unknown profile, `CPF1321` no such
//! job, or an ended one without a spooled job log), `NotAuthorized`
//! (`CPF1343`), `Error` (the command's escape message, `CPF1362` for a job
//! that has ended), `InvalidRequest` (`EndJob` of the connection's own
//! job: sign off instead).
//!
//! `EndHow` is the common type `qap_proto::types::EndHow`, version 7's
//! values; this crate had no copy of its own to move.

use qap_proto::Op;
use qap_proto::ops::jobs::{
    EndJob, JobEntry, JobLog, JobLogEntries, JobRef, JobState, Jobs as JobList, ListJobs,
};
use qap_proto::types::{EndHow, JobId};

use crate::Error;
pub use crate::conn::Requests;
use crate::spool::ALL_PAGE;

/// The `Jobs` operations (version 8), on whichever connection implements
/// [`Requests`]. Each is refused here, before anything is sent, on a
/// connection whose version is below 8 ([`Error::Unsupported`]).
pub trait Jobs: Requests {
    /// `ListJobs`: one page of `user`'s jobs (`*CURRENT`, `*ALL` or a
    /// profile) in `status` (none: all), active first, then on job queues,
    /// then on output queues, each by when it entered.
    fn list_jobs(&mut self, req: &ListJobs) -> Result<JobList, Error> {
        self.call_op(Op::ListJobs, req)
    }

    /// Every job `user` has in `status` (none: all), page by page.
    fn all_jobs(&mut self, user: &str, status: Option<JobState>) -> Result<Vec<JobEntry>, Error> {
        let mut req = ListJobs {
            user: user.to_owned(),
            status,
            start: 0,
            max: ALL_PAGE,
        };
        let mut out = Vec::new();
        loop {
            let page = self.list_jobs(&req)?;
            let n = page.jobs.len();
            out.extend(page.jobs);
            if n == 0 || out.len() >= page.total as usize {
                return Ok(out);
            }
            req.start = u32::try_from(out.len()).unwrap_or(u32::MAX);
        }
    }

    /// `JobLog`: one page of a job's log, oldest first: of an active job,
    /// its log now; of an ended one, its spooled `QPJOBLOG`.
    fn job_log(&mut self, req: &JobLog) -> Result<JobLogEntries, Error> {
        self.call_op(Op::JobLog, req)
    }

    /// `HoldJob`: `HLDJOB`.
    fn hold_job(&mut self, job: &JobId) -> Result<(), Error> {
        self.call_op(Op::HoldJob, &JobRef { job: job.clone() })
    }

    /// `ReleaseJob`: `RLSJOB`.
    fn release_job(&mut self, job: &JobId) -> Result<(), Error> {
        self.call_op(Op::ReleaseJob, &JobRef { job: job.clone() })
    }

    /// `EndJob`: `ENDJOB OPTION(how) DELAY(delay)`, `delay` none for the
    /// job's default.
    fn end_job(&mut self, job: &JobId, how: EndHow, delay: Option<u32>) -> Result<(), Error> {
        self.call_op(
            Op::EndJob,
            &EndJob {
                job: job.clone(),
                how,
                delay,
            },
        )
    }
}

impl<T: Requests + ?Sized> Jobs for T {}
