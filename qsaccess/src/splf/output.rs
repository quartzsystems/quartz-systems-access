// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! A spooled file brought to this computer, and what is made of it.
//!
//! - [`Reading`]: a read on its own thread, written to a temporary file as
//!   each batch arrives ([`qap_client::spool::read`]), with progress and
//!   cancel. The client is locked only to send each batch.
//! - An SCS file becomes the page model ([`document`]) and from it a PDF,
//!   text or a printed copy, laid out as a 5250 printer session lays out
//!   the same data ([`super::model::scs_options`], `qsa-print`).
//! - Another device type's file is what the server made of it: its PDF or
//!   text, as sent.
//! - Temporary files live in one folder per process, removed when the
//!   program ends ([`remove_temporary`]).

use std::fs::{self, File};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, mpsc};

use qap_client::Error;
use qap_client::proto::Status;
use qap_client::proto::ops::spool::{ReadFormat, SpooledFile};
use qap_client::spool::{self, Progress, ReadEnd};
use qsa_config::System;

use crate::qap::{self, Explained, Handle, lock};

use super::model;

/// This process's folder for temporary files.
pub fn temporary_dir() -> PathBuf {
    std::env::temp_dir().join(format!("qsaccess-{}", std::process::id()))
}

/// A new temporary file for `f`, ending in `ext`.
pub fn temporary_file(f: &SpooledFile, ext: &str) -> io::Result<PathBuf> {
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let dir = temporary_dir();
    fs::create_dir_all(&dir)?;
    let n = SEQ.fetch_add(1, Ordering::SeqCst);
    Ok(dir.join(format!(
        "{}-{}-{n}.{ext}",
        f.id.file.replace(|c: char| !c.is_ascii_alphanumeric(), "_"),
        f.id.number
    )))
}

/// Removes this process's temporary files; when the program ends. Read-only
/// copies (the Integrated File System's Open) are made writable first, since
/// Windows deletes no read-only file.
pub fn remove_temporary() {
    let dir = temporary_dir();
    if !dir.exists() {
        return;
    }
    let mut todo = vec![dir.clone()];
    while let Some(d) = todo.pop() {
        for e in fs::read_dir(&d).into_iter().flatten().flatten() {
            let p = e.path();
            if p.is_dir() {
                todo.push(p);
            } else if let Ok(m) = fs::metadata(&p) {
                let mut perm = m.permissions();
                if perm.readonly() {
                    #[allow(clippy::permissions_set_readonly_false)]
                    perm.set_readonly(false);
                    let _ = fs::set_permissions(&p, perm);
                }
            }
        }
    }
    if let Err(e) = fs::remove_dir_all(&dir) {
        crate::log!("{}: {e}", dir.display());
    }
}

/// What a read reports.
#[derive(Debug)]
pub enum ReadEvent {
    Progress(Progress),
    Done(Result<ReadEnd, Error>),
}

/// A read on its own thread, into [`Reading::path`].
pub struct Reading {
    pub file: SpooledFile,
    pub format: ReadFormat,
    /// The data as received, each batch's leading state included.
    pub path: PathBuf,
    cancel: Arc<AtomicBool>,
    rx: mpsc::Receiver<ReadEvent>,
    /// The last progress reported.
    pub progress: Progress,
}

impl Reading {
    /// Starts reading `file` as `format` with `h`.
    pub fn start(h: &Handle, file: SpooledFile, format: ReadFormat) -> io::Result<Reading> {
        let ext = match format {
            ReadFormat::Scs => "scs",
            ReadFormat::Text => "txt",
            ReadFormat::Pdf => "pdf",
        };
        let path = temporary_file(&file, ext)?;
        let mut sink = File::create(&path)?;
        let cancel = Arc::new(AtomicBool::new(false));
        let (tx, rx) = mpsc::channel();
        let (h, id, stop) = (Arc::clone(h), file.id.clone(), Arc::clone(&cancel));
        std::thread::Builder::new()
            .name(format!("read {}", file.id.file))
            .spawn(move || {
                let progress = |p: Progress| {
                    let _ = tx.send(ReadEvent::Progress(p));
                    fltk::app::awake();
                };
                let r = spool::read(
                    |req| lock(&h).read_spooled_file(req),
                    &id,
                    format,
                    spool::BATCH_PAGES,
                    &mut sink,
                    progress,
                    &stop,
                );
                let _ = tx.send(ReadEvent::Done(r));
                fltk::app::awake();
            })?;
        Ok(Reading {
            file,
            format,
            path,
            cancel,
            rx,
            progress: Progress {
                pages_read: 0,
                total: 0,
            },
        })
    }

    /// Asks the read to stop: the batch in flight is cancelled and what it
    /// brings back discarded.
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::SeqCst);
    }

    pub fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::SeqCst)
    }

    /// What happened since the last look.
    pub fn poll(&mut self) -> Vec<ReadEvent> {
        let events: Vec<ReadEvent> = self.rx.try_iter().collect();
        for e in &events {
            if let ReadEvent::Progress(p) = e {
                self.progress = *p;
            }
        }
        events
    }

    /// The status bar's line while it reads: `QAD6010`.
    pub fn line(&self) -> String {
        qsa_msg::message(
            qsa_msg::id::SPOOL_READING,
            &[
                &self.file.id.file,
                &self.progress.pages_read.to_string(),
                &self.progress.total.max(self.file.pages).to_string(),
            ],
        )
        .to_string()
    }
}

/// A format's name in messages.
pub fn format_word(f: ReadFormat) -> &'static str {
    match f {
        ReadFormat::Scs => "SCS",
        ReadFormat::Text => "text",
        ReadFormat::Pdf => "PDF",
    }
}

/// A read's failure as messages: the system's own when it sent one, else
/// `QAD6012` (still being written) and `QAD6013` (the format cannot be
/// made), else [`qap::explain`]'s.
pub fn explain_read(sys: &System, f: &SpooledFile, format: ReadFormat, e: &Error) -> Explained {
    let mut x = qap::explain(sys, e);
    if x.server.is_none() {
        match e.status() {
            Some(Status::InUse) => {
                x.message = qsa_msg::message(qsa_msg::id::SPOOLED_FILE_OPEN, &[&f.id.file]);
            }
            Some(Status::Error) => {
                x.message = qsa_msg::message(
                    qsa_msg::id::SPOOL_FORMAT_NOT_POSSIBLE,
                    &[&f.id.file, format_word(format)],
                );
            }
            _ => {}
        }
    }
    x
}

/// The page model of SCS read to `path`, laid out with `options`.
pub fn document(path: &Path, options: &scs::Options) -> io::Result<scs::Document> {
    let data = fs::read(path)?;
    Ok(scs::parse(&data, options))
}

/// `doc` as a PDF, as a 5250 printer session writes one.
pub fn pdf(doc: &scs::Document, title: &str, paper: scs::Paper) -> Result<Vec<u8>, String> {
    qsa_print::pdf::render(
        doc,
        &qsa_print::pdf::Options {
            title: title.to_owned(),
            margin: None,
            paper,
        },
    )
    .map_err(|e| e.to_string())
}

/// Writes `bytes` to `path` whole: to a file beside it, then renamed.
pub fn write_whole(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let tmp = path.with_extension("part");
    fs::write(&tmp, bytes)?;
    fs::rename(&tmp, path).inspect_err(|_| {
        let _ = fs::remove_file(&tmp);
    })
}

/// The printers of this computer and its default one.
pub fn printers() -> Result<(Vec<String>, Option<String>), String> {
    #[cfg(windows)]
    {
        Ok((
            qsa_print::gdi::printers(),
            qsa_print::gdi::default_printer(),
        ))
    }
    #[cfg(not(windows))]
    {
        let all = qsa_print::cups::printers().map_err(|e| e.to_string())?;
        Ok((all, qsa_print::cups::default_printer()))
    }
}

/// Prints `doc` on `printer` (the default printer when empty): GDI on
/// Windows, a PDF to CUPS elsewhere. The printer's name.
pub fn print(
    doc: &scs::Document,
    printer: &str,
    title: &str,
    paper: scs::Paper,
) -> Result<String, String> {
    crate::printer::spool::printing_available()?;
    let chosen = (!printer.trim().is_empty()).then_some(printer.trim());
    #[cfg(windows)]
    {
        let _ = paper;
        let name = chosen
            .map(str::to_owned)
            .or_else(qsa_print::gdi::default_printer)
            .unwrap_or_else(|| "(default)".into());
        qsa_print::gdi::print(doc, Some(&name), title, None, None)
            .map(|()| name)
            .map_err(|e| e.to_string())
    }
    #[cfg(not(windows))]
    {
        let name = chosen
            .map(str::to_owned)
            .or_else(qsa_print::cups::default_printer)
            .unwrap_or_else(|| "(default)".into());
        let bytes = pdf(doc, title, paper)?;
        let dir = temporary_dir();
        fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let tmp = dir.join(format!("print-{}.pdf", std::process::id()));
        fs::write(&tmp, &bytes).map_err(|e| e.to_string())?;
        let r = qsa_print::cups::print_pdf(&tmp, chosen, title);
        let _ = fs::remove_file(&tmp);
        r.map(|_| name).map_err(|e| e.to_string())
    }
}

/// What a finished read is for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Purpose {
    /// Look at it: SCS in the viewer, another type in the system's viewer.
    Open,
    /// Save it as PDF to this path.
    SavePdf(PathBuf),
    /// Save it as text to this path.
    SaveText(PathBuf),
    /// Print it on this printer (empty: the default).
    Print(String),
}

impl Purpose {
    /// The format to read `f` in for this purpose: an SCS file always as
    /// SCS, which is rendered here; another as what the server may make.
    pub fn format(&self, f: &SpooledFile) -> ReadFormat {
        use qap_client::proto::ops::spool::DeviceType;
        match (f.device_type, self) {
            (DeviceType::Scs, _) => ReadFormat::Scs,
            (_, Purpose::SaveText(_)) => ReadFormat::Text,
            (_, Purpose::SavePdf(_) | Purpose::Print(_)) => ReadFormat::Pdf,
            (d, _) => spool::format_for(d),
        }
    }
}

/// Makes what `purpose` asks of a finished read: the message to report,
/// if any.
pub fn finish(
    r: &Reading,
    purpose: &Purpose,
    options: &scs::Options,
) -> Result<Option<qsa_msg::Message>, String> {
    let name = r.file.id.file.clone();
    let title = format!("{} {}", name, model::job_text(&r.file.id.job));
    let doc = || document(&r.path, options).map_err(|e| e.to_string());
    let saved = |path: &Path| {
        Some(qsa_msg::message(
            qsa_msg::id::SPOOL_SAVED,
            &[&name, &path.display().to_string()],
        ))
    };
    let printed = |printer: &str| {
        Some(qsa_msg::message(
            qsa_msg::id::SPOOL_PRINTED,
            &[&name, printer],
        ))
    };
    match (purpose, r.format) {
        (Purpose::Open, ReadFormat::Scs) => {
            let path = r.path.with_extension("pdf");
            let bytes = pdf(&doc()?, &title, options.paper)?;
            write_whole(&path, &bytes).map_err(|e| e.to_string())?;
            crate::shell::open(&path);
            Ok(None)
        }
        (Purpose::Open, _) => {
            crate::shell::open(&r.path);
            Ok(None)
        }
        (Purpose::SavePdf(path), ReadFormat::Scs) => {
            let bytes = pdf(&doc()?, &title, options.paper)?;
            write_whole(path, &bytes).map_err(|e| e.to_string())?;
            Ok(saved(path))
        }
        (Purpose::SaveText(path), ReadFormat::Scs) => {
            write_whole(path, scs::to_text(&doc()?).as_bytes()).map_err(|e| e.to_string())?;
            Ok(saved(path))
        }
        (Purpose::SavePdf(path) | Purpose::SaveText(path), _) => {
            fs::copy(&r.path, path).map_err(|e| e.to_string())?;
            Ok(saved(path))
        }
        (Purpose::Print(printer), ReadFormat::Scs) => {
            let used = print(&doc()?, printer, &title, options.paper)?;
            Ok(printed(&used))
        }
        (Purpose::Print(printer), ReadFormat::Pdf) => {
            print_pdf(&r.path, printer, &title).map(|p| printed(&p))
        }
        (Purpose::Print(_), f) => Err(format!(
            "the system sent {}, which is not printed here",
            format_word(f)
        )),
    }
}

/// Prints a PDF the system made: through CUPS; Windows prints only what
/// this program lays out.
fn print_pdf(path: &Path, printer: &str, title: &str) -> Result<String, String> {
    #[cfg(windows)]
    {
        let _ = (path, printer, title);
        Err("a PDF made by the system is not printed here on Windows; save it and print it from a PDF viewer".into())
    }
    #[cfg(not(windows))]
    {
        crate::printer::spool::printing_available()?;
        let chosen = (!printer.trim().is_empty()).then_some(printer.trim());
        let name = chosen
            .map(str::to_owned)
            .or_else(qsa_print::cups::default_printer)
            .unwrap_or_else(|| "(default)".into());
        qsa_print::cups::print_pdf(path, chosen, title)
            .map(|_| name)
            .map_err(|e| e.to_string())
    }
}
