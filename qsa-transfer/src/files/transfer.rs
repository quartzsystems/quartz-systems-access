// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! One file to or from the system's integrated file system, as the
//! Integrated File System window's transfer queue and `qsaccess run` move
//! it.
//!
//! - Binary (the default) moves the bytes unchanged. Text has the system
//!   convert: a download reads with `as_ccsid` 1208, an upload sends UTF-8
//!   with `data_ccsid` 1208 to a file made with [`Options::ccsid`]. This
//!   side changes only line endings, when asked ([`super::lines`]).
//! - A download is written to `<name>.qsapart` beside the target and
//!   renamed over it only when complete. An upload looks for the file on
//!   the system first (`GetInfo`); one there, or a `Create` that answers
//!   `Exists`, follows the existing-file policy.
//! - What a stopped transfer kept is a [`Resume`]: a binary transfer goes
//!   on from it either way, a text download from the last confirmed
//!   `next_offset` (the partial file kept with it), and a text upload starts
//!   again from the beginning, since converted offsets do not map back to
//!   this computer's bytes.

use std::cell::Cell;
use std::fs::{File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant, UNIX_EPOCH};

use qap_client::ifs::{self, End, Files, ReadOptions, WriteOptions, join};
use qap_client::proto::Op;
use qap_client::proto::ops::ifs::{Entry, EntryKind, WriteMode};
use qap_client::{Error, Status};

use super::lines::{CrlfToLf, LfToCrlf};

/// Which way a file goes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Direction {
    /// From the system to this computer.
    #[default]
    Download,
    /// From this computer to the system.
    Upload,
}

/// What to do when the target already exists.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Policy {
    /// Ask each time (Replace, Skip, Replace all, Skip all, and Keep both
    /// for a download).
    #[default]
    Ask,
    Skip,
    Replace,
    /// Replace when the file being sent was changed later than the target.
    ReplaceIfNewer,
}

/// A batch's transfer options.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Options {
    /// Text: converted by the system, line endings by this side. Otherwise
    /// binary, the bytes unchanged.
    pub text: bool,
    /// The CCSID of a file an upload makes on the system.
    pub ccsid: u16,
    /// Text only: LF on the system, CR LF on this computer.
    pub crlf: bool,
    pub existing: Policy,
}

impl Default for Options {
    fn default() -> Options {
        Options {
            text: false,
            ccsid: ifs::UTF8,
            crlf: cfg!(windows),
            existing: Policy::Ask,
        }
    }
}

/// Where a stopped transfer goes on from: the system's file offset and
/// this computer's file length, both confirmed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Resume {
    pub remote: u64,
    pub local: u64,
}

/// One file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Job {
    pub direction: Direction,
    /// The system's path.
    pub remote: String,
    /// This computer's path.
    pub local: PathBuf,
    /// Its size where it comes from, as known when queued.
    pub size: u64,
    pub options: Options,
    /// Where a retry goes on from.
    pub resume: Option<Resume>,
}

impl Job {
    /// The name the user sees: the file's.
    pub fn name(&self) -> String {
        match self.direction {
            Direction::Download => ifs::file_name(&self.remote).to_owned(),
            Direction::Upload => self.local.file_name().map_or_else(
                || self.local.display().to_string(),
                |n| n.to_string_lossy().into_owned(),
            ),
        }
    }

    /// Whether a retry goes on from where it stopped rather than from the
    /// beginning: a binary upload (from where the system's file ends, asked
    /// for again), and a download, binary or text, that got somewhere.
    pub fn resumes(&self) -> bool {
        match (self.direction, self.resume) {
            (_, None) => false,
            (Direction::Upload, Some(_)) => !self.options.text,
            (Direction::Download, Some(r)) => r.remote > 0,
        }
    }
}

/// An existing target to decide on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Conflict {
    /// The target's name, and the folder it is in.
    pub name: String,
    pub folder: String,
    /// Keep both may be offered (a download).
    pub keep_both: bool,
}

/// The answer for one existing target.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Choice {
    Replace,
    Skip,
    /// A download only: save as `name (2).ext`, the next number free.
    KeepBoth,
}

/// How a transfer ended, when it did not fail.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Every byte moved: how many, and how long it took.
    Done { bytes: u64, elapsed: Duration },
    /// The target was there and kept.
    Skipped,
    /// Cancelled; what a retry goes on from.
    Cancelled(Option<Resume>),
}

/// A transfer that failed, and what a retry goes on from.
#[derive(Debug)]
pub struct Failed {
    pub error: Failure,
    pub resume: Option<Resume>,
}

/// Why a transfer failed.
#[derive(Debug)]
pub enum Failure {
    /// The system refused, or the connection went.
    System(Box<Error>),
    /// This computer's file.
    Local(io::Error),
}

impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Failure::System(e) => match e.server_message() {
                Some(m) => write!(f, "{}: {}", m.id, m.text.trim_end_matches('.')),
                None => write!(f, "{e}"),
            },
            Failure::Local(e) => write!(f, "{e}"),
        }
    }
}

impl From<Error> for Failure {
    fn from(e: Error) -> Failure {
        match e {
            Error::Write(e) | Error::Read(e) => Failure::Local(e),
            e => Failure::System(Box::new(e)),
        }
    }
}

/// The partial file of a download to `target`.
pub fn part_path(target: &Path) -> PathBuf {
    let mut name = target.file_name().unwrap_or_default().to_os_string();
    name.push(".qsapart");
    target.with_file_name(name)
}

/// `path` with ` (2)`, ` (3)`, ... before its extension: the first that
/// `taken` does not say is taken.
pub fn keep_both(path: &Path, taken: impl Fn(&Path) -> bool) -> PathBuf {
    let stem = path
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    let ext = path
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default();
    (2..)
        .map(|n| path.with_file_name(format!("{stem} ({n}){ext}")))
        .find(|p| !taken(p))
        .expect("some number is free")
}

/// A local file's last change, milliseconds since 1970.
fn local_modified(path: &Path) -> Option<i64> {
    let t = std::fs::metadata(path).ok()?.modified().ok()?;
    let d = t.duration_since(UNIX_EPOCH).ok()?;
    i64::try_from(d.as_millis()).ok()
}

/// Whether the one being sent is newer than the target: by their last
/// changes, both known.
fn newer(sent: Option<i64>, target: Option<i64>) -> bool {
    matches!((sent, target), (Some(s), Some(t)) if s > t)
}

/// What to do with an existing target, by `policy` or by asking.
fn decide(
    policy: Policy,
    newer: bool,
    conflict: Conflict,
    ask: &mut dyn FnMut(&Conflict) -> Choice,
) -> Choice {
    match policy {
        Policy::Skip => Choice::Skip,
        Policy::Replace => Choice::Replace,
        Policy::ReplaceIfNewer if newer => Choice::Replace,
        Policy::ReplaceIfNewer => Choice::Skip,
        Policy::Ask => ask(&conflict),
    }
}

/// A writer that counts what goes through it into a cell shared with the
/// progress callback.
struct Counted<W> {
    inner: W,
    n: Rc<Cell<u64>>,
}

impl<W: Write> Write for Counted<W> {
    fn write(&mut self, b: &[u8]) -> io::Result<usize> {
        let n = self.inner.write(b)?;
        self.n.set(self.n.get() + n as u64);
        Ok(n)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

/// Moves `job`'s file. `ask` decides an existing target when the policy is
/// to ask; `progress` is told after each part (its `offset` is the
/// system's side); `cancel` stops it. On a stop, `job.resume` holds what a
/// retry goes on from.
pub fn run<F: Files + ?Sized>(
    f: &mut F,
    job: &mut Job,
    ask: &mut dyn FnMut(&Conflict) -> Choice,
    progress: &mut dyn FnMut(ifs::Progress),
    cancel: &AtomicBool,
) -> Result<Outcome, Failed> {
    match job.direction {
        Direction::Download => download(f, job, ask, progress, cancel),
        Direction::Upload => upload(f, job, ask, progress, cancel),
    }
}

fn download<F: Files + ?Sized>(
    f: &mut F,
    job: &mut Job,
    ask: &mut dyn FnMut(&Conflict) -> Choice,
    progress: &mut dyn FnMut(ifs::Progress),
    cancel: &AtomicBool,
) -> Result<Outcome, Failed> {
    let started = Instant::now();
    let fail = |error: Failure, resume: Option<Resume>| Failed { error, resume };
    let resume = job.resume.filter(|_| job.resumes());
    if resume.is_none() && job.local.exists() {
        let remote = f.get_info(&job.remote).map_err(|e| fail(e.into(), None))?;
        let folder = job
            .local
            .parent()
            .map(|p| p.display().to_string())
            .unwrap_or_default();
        let c = Conflict {
            name: job.name(),
            folder,
            keep_both: true,
        };
        let newer = newer(Some(remote.modified), local_modified(&job.local));
        match decide(job.options.existing, newer, c, ask) {
            Choice::Skip => return Ok(Outcome::Skipped),
            Choice::Replace => {}
            Choice::KeepBoth => job.local = keep_both(&job.local, Path::exists),
        }
    }
    let part = part_path(&job.local);
    let (mut file, after_cr) = match resume {
        Some(r) => {
            let mut file = OpenOptions::new()
                .read(true)
                .write(true)
                .open(&part)
                .map_err(|e| fail(Failure::Local(e), None))?;
            let len = file
                .metadata()
                .map_err(|e| fail(Failure::Local(e), None))?
                .len();
            if len < r.local {
                // The partial file is shorter than was kept: start again.
                job.resume = None;
                return download(f, job, ask, progress, cancel);
            }
            file.set_len(r.local)
                .map_err(|e| fail(Failure::Local(e), None))?;
            let mut last = [0u8];
            let after_cr = r.local > 0
                && file.seek(SeekFrom::Start(r.local - 1)).is_ok()
                && file.read_exact(&mut last).is_ok()
                && last[0] == b'\r';
            file.seek(SeekFrom::End(0))
                .map_err(|e| fail(Failure::Local(e), None))?;
            (file, after_cr)
        }
        None => (
            File::create(&part).map_err(|e| fail(Failure::Local(e), None))?,
            false,
        ),
    };
    let base = resume.unwrap_or_default();
    let written = Rc::new(Cell::new(base.local));
    let counted = Counted {
        inner: &mut file,
        n: Rc::clone(&written),
    };
    let mut sink: Box<dyn Write> = if job.options.text && job.options.crlf {
        Box::new(LfToCrlf::new(counted, after_cr))
    } else {
        Box::new(counted)
    };
    let mut kept = resume;
    let opts = ReadOptions {
        as_ccsid: job.options.text.then_some(ifs::UTF8),
        offset: base.remote,
        ..ReadOptions::default()
    };
    let end = ifs::read(
        |r| f.ifs_request(Op::ReadFile, r),
        &job.remote,
        opts,
        &mut *sink,
        |p| {
            kept = Some(Resume {
                remote: p.offset,
                local: written.get(),
            });
            progress(p);
        },
        cancel,
    );
    drop(sink);
    job.resume = kept;
    match end {
        Ok(End::Done(p)) => {
            file.sync_all().map_err(|e| fail(Failure::Local(e), kept))?;
            drop(file);
            std::fs::rename(&part, &job.local).map_err(|e| fail(Failure::Local(e), kept))?;
            job.resume = None;
            Ok(Outcome::Done {
                bytes: p.bytes,
                elapsed: started.elapsed(),
            })
        }
        Ok(End::Cancelled(_)) => Ok(Outcome::Cancelled(kept)),
        Err(e) => Err(fail(e.into(), kept)),
    }
}

fn upload<F: Files + ?Sized>(
    f: &mut F,
    job: &mut Job,
    ask: &mut dyn FnMut(&Conflict) -> Choice,
    progress: &mut dyn FnMut(ifs::Progress),
    cancel: &AtomicBool,
) -> Result<Outcome, Failed> {
    let started = Instant::now();
    let fail = |error: Failure, resume: Option<Resume>| Failed { error, resume };
    let local_len = std::fs::metadata(&job.local)
        .map_err(|e| fail(Failure::Local(e), None))?
        .len();
    let mut first = WriteMode::Replace;
    // A binary retry goes on where the system's file ends.
    let mut resume_at = None;
    if job.resumes() {
        let at = ifs::resume_offset(f, &job.remote, local_len)
            .map_err(|e| fail(e.into(), job.resume))?;
        if at > 0 {
            resume_at = Some(at);
        }
    } else {
        match f.get_info(&job.remote) {
            Ok(e) => {
                if e.kind == EntryKind::Directory {
                    return Err(fail(
                        Failure::Local(io::Error::other(format!(
                            "{} is a folder on the system",
                            job.remote
                        ))),
                        None,
                    ));
                }
                let c = Conflict {
                    name: ifs::file_name(&job.remote).to_owned(),
                    folder: ifs::parent(&job.remote).to_owned(),
                    keep_both: false,
                };
                let newer = newer(local_modified(&job.local), Some(e.modified));
                match decide(job.options.existing, newer, c, ask) {
                    Choice::Skip => return Ok(Outcome::Skipped),
                    Choice::Replace | Choice::KeepBoth => first = WriteMode::Replace,
                }
            }
            // Not there: made, and refused should it appear meanwhile.
            Err(e) if e.status() == Some(Status::NotFound) => first = WriteMode::Create,
            Err(e) => return Err(fail(e.into(), None)),
        }
    }
    let mut file = File::open(&job.local).map_err(|e| fail(Failure::Local(e), None))?;
    if let Some(at) = resume_at {
        file.seek(SeekFrom::Start(at))
            .map_err(|e| fail(Failure::Local(e), job.resume))?;
    }
    let mut source: Box<dyn Read> = if job.options.text && job.options.crlf {
        Box::new(CrlfToLf::new(file))
    } else {
        Box::new(file)
    };
    let opts = WriteOptions {
        data_ccsid: job.options.text.then_some(ifs::UTF8),
        ccsid: job.options.ccsid,
        first,
        resume_at,
        ..WriteOptions::default()
    };
    let mut kept = None;
    let text = job.options.text;
    let end = ifs::write(
        |w| f.ifs_request(Op::WriteFile, w),
        &job.remote,
        &mut *source,
        opts,
        |p| {
            if !text {
                kept = Some(Resume {
                    remote: p.offset,
                    local: p.offset,
                });
            }
            progress(p);
        },
        cancel,
    );
    match end {
        Ok(End::Done(p)) => {
            job.resume = None;
            Ok(Outcome::Done {
                bytes: p.bytes,
                elapsed: started.elapsed(),
            })
        }
        Ok(End::Cancelled(_)) => {
            job.resume = kept.or(job.resume);
            Ok(Outcome::Cancelled(job.resume))
        }
        // Made meanwhile by someone else: decide as for one already there.
        Err(e) if e.status() == Some(Status::Exists) && first == WriteMode::Create => {
            job.options.existing = match decide(
                job.options.existing,
                false,
                Conflict {
                    name: ifs::file_name(&job.remote).to_owned(),
                    folder: ifs::parent(&job.remote).to_owned(),
                    keep_both: false,
                },
                ask,
            ) {
                Choice::Skip => return Ok(Outcome::Skipped),
                _ => Policy::Replace,
            };
            upload(f, job, ask, progress, cancel)
        }
        Err(e) => {
            // A binary upload goes on with what the system has; the offset
            // is asked for again on the retry.
            job.resume = (!text).then(|| kept.or(job.resume).unwrap_or_default());
            Err(fail(e.into(), job.resume))
        }
    }
}

/// The jobs that download the system's folder `remote` into `local_parent`
/// (as `local_parent/<name>`), its folders made here as they are found.
/// Links are not followed into.
pub fn plan_download<F: Files + ?Sized>(
    f: &mut F,
    remote: &str,
    local_parent: &Path,
    options: Options,
) -> Result<Vec<Job>, super::Stopped> {
    let mut jobs = Vec::new();
    let root = local_parent.join(ifs::file_name(remote));
    let mut todo = vec![(remote.to_owned(), root)];
    while let Some((dir, here)) = todo.pop() {
        std::fs::create_dir_all(&here).map_err(|e| super::Stopped {
            at: here.display().to_string(),
            error: Box::new(Error::Write(e)),
        })?;
        let list = f.all_entries(&dir, "").map_err(|error| super::Stopped {
            at: dir.clone(),
            error: Box::new(error),
        })?;
        for e in list.into_iter().filter(|e| e.name != "." && e.name != "..") {
            let (r, l) = (join(&dir, &e.name), here.join(&e.name));
            match e.kind {
                EntryKind::Directory => todo.push((r, l)),
                EntryKind::Symlink => {}
                _ => jobs.push(Job {
                    direction: Direction::Download,
                    remote: r,
                    local: l,
                    size: e.size,
                    options,
                    resume: None,
                }),
            }
        }
    }
    jobs.sort_by(|a, b| a.remote.cmp(&b.remote));
    Ok(jobs)
}

/// The jobs that upload this computer's folder `local` into the system's
/// folder `remote_parent` (as `remote_parent/<name>`), its folders made on
/// the system as they are found.
pub fn plan_upload<F: Files + ?Sized>(
    f: &mut F,
    local: &Path,
    remote_parent: &str,
    options: Options,
) -> Result<Vec<Job>, super::Stopped> {
    let mut jobs = Vec::new();
    let name = local
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut todo = vec![(local.to_path_buf(), join(remote_parent, &name))];
    while let Some((here, dir)) = todo.pop() {
        f.create_directory(&dir, true)
            .map_err(|error| super::Stopped {
                at: dir.clone(),
                error: Box::new(error),
            })?;
        let list = std::fs::read_dir(&here).map_err(|e| super::Stopped {
            at: here.display().to_string(),
            error: Box::new(Error::Read(e)),
        })?;
        let mut items: Vec<_> = list.filter_map(Result::ok).collect();
        items.sort_by_key(std::fs::DirEntry::file_name);
        for item in items {
            let (l, r) = (item.path(), join(&dir, &item.file_name().to_string_lossy()));
            let Ok(meta) = std::fs::metadata(&l) else {
                continue;
            };
            if meta.is_dir() {
                todo.push((l, r));
            } else {
                jobs.push(Job {
                    direction: Direction::Upload,
                    remote: r,
                    local: l,
                    size: meta.len(),
                    options,
                    resume: None,
                });
            }
        }
    }
    Ok(jobs)
}

/// The job that downloads `entry`, in the system's folder `dir`, into
/// `local_dir`.
pub fn download_job(dir: &str, entry: &Entry, local_dir: &Path, options: Options) -> Job {
    Job {
        direction: Direction::Download,
        remote: join(dir, &entry.name),
        local: local_dir.join(&entry.name),
        size: entry.size,
        options,
        resume: None,
    }
}

/// The job that uploads the file `local` into the system's folder `dir`.
pub fn upload_job(local: &Path, dir: &str, options: Options) -> Job {
    let name = local
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    Job {
        direction: Direction::Upload,
        remote: join(dir, &name),
        local: local.to_path_buf(),
        size: std::fs::metadata(local).map(|m| m.len()).unwrap_or(0),
        options,
        resume: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_for_parts_and_keep_both() {
        assert_eq!(
            part_path(Path::new("/tmp/x/report.txt")),
            Path::new("/tmp/x/report.txt.qsapart")
        );
        let taken = |p: &Path| p.ends_with("report (2).txt");
        assert_eq!(
            keep_both(Path::new("/tmp/report.txt"), taken),
            Path::new("/tmp/report (3).txt")
        );
        assert_eq!(
            keep_both(Path::new("/tmp/README"), |_| false),
            Path::new("/tmp/README (2)")
        );
    }

    #[test]
    fn policies() {
        let c = || Conflict {
            name: "a".into(),
            folder: "/".into(),
            keep_both: false,
        };
        let mut never = |_: &Conflict| -> Choice { panic!("not asked") };
        assert_eq!(decide(Policy::Skip, true, c(), &mut never), Choice::Skip);
        assert_eq!(
            decide(Policy::Replace, false, c(), &mut never),
            Choice::Replace
        );
        assert_eq!(
            decide(Policy::ReplaceIfNewer, true, c(), &mut never),
            Choice::Replace
        );
        assert_eq!(
            decide(Policy::ReplaceIfNewer, false, c(), &mut never),
            Choice::Skip
        );
        assert_eq!(
            decide(Policy::Ask, false, c(), &mut |_| Choice::KeepBoth),
            Choice::KeepBoth
        );
        assert!(newer(Some(2), Some(1)) && !newer(Some(1), Some(1)) && !newer(None, Some(1)));
    }

    #[test]
    fn which_retries_resume() {
        let mut j = Job {
            direction: Direction::Upload,
            remote: "/a".into(),
            local: "a".into(),
            size: 10,
            options: Options::default(),
            resume: Some(Resume {
                remote: 4,
                local: 4,
            }),
        };
        assert!(j.resumes(), "binary up");
        j.resume = Some(Resume::default());
        assert!(j.resumes(), "binary up asks the system where to go on");
        j.options.text = true;
        assert!(!j.resumes(), "text up starts again");
        j.direction = Direction::Download;
        assert!(!j.resumes(), "a download that got nowhere");
        j.resume = Some(Resume {
            remote: 4,
            local: 5,
        });
        assert!(j.resumes(), "text down");
        j.resume = None;
        assert!(!j.resumes());
    }
}
