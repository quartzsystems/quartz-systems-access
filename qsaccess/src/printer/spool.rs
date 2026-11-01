// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! Received print jobs: written to a spool file as they arrive, then
//! rendered to the session's targets.
//!
//! - A job's data goes to `<spool>/<date>-<time>-<seq>.scs` before the
//!   server is told it arrived, so no received job is ever lost. The spool
//!   folder is per device, in the user's local application data.
//! - When the job ends, it is rendered: a PDF in the output folder, a text
//!   file beside it if asked for, and the system printer (GDI on Windows,
//!   CUPS on Linux). Then the spool file is marked done (`.done.scs`); it
//!   stays for Print Again until the session is opened again.
//! - A spool file not marked done (the program stopped mid-job) is rendered
//!   when the session next starts ([`Spooler::recover`]).
//!
//! Without system printing (no CUPS commands, or no printer), a job meant
//! only for the printer is saved as a PDF instead.

use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use qsa_config::qsp::{self, PrinterFile, Target};
use qsa_msg::{Message, id};

/// Where and how output is made: the session file's `[printer]` and
/// `[output]`.
#[derive(Clone, Debug, PartialEq)]
pub struct Output {
    pub device: String,
    pub target: Target,
    /// `None`: no folder could be found (no Documents folder).
    pub pdf_folder: Option<PathBuf>,
    pub file_name: String,
    pub also_save_text: bool,
    /// Empty: the default printer.
    pub system_printer: String,
    pub paper: scs::Paper,
    /// Margin kept clear, 1/1440 inch; `None`: the renderer's default.
    pub margin: Option<u32>,
    pub ccsid: u16,
    pub font: u16,
    /// Where jobs are spooled while they arrive.
    pub spool_dir: PathBuf,
}

impl Output {
    pub fn from_file(f: &PrinterFile) -> Output {
        Output {
            device: f.printer.device_name.trim().to_ascii_uppercase(),
            target: f.output.target,
            pdf_folder: f.pdf_folder(),
            file_name: f.output.file_name.clone(),
            also_save_text: f.output.also_save_text,
            system_printer: f.output.system_printer.trim().to_owned(),
            paper: match f.output.paper {
                qsp::Paper::Letter => scs::Paper::Letter,
                qsp::Paper::A4 => scs::Paper::A4,
                qsp::Paper::Legal => scs::Paper::Legal,
            },
            margin: f.output.margins.map(|m| (m * 1440.0).round() as u32),
            ccsid: f.printer.ccsid,
            font: f.printer.font,
            spool_dir: spool_dir(&f.printer.device_name.trim().to_ascii_uppercase()),
        }
    }
}

/// What went wrong making a job's output.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Problem {
    /// The output folder could not be written (`QAD3014`).
    Folder(PathBuf, String),
    /// The printer failed (`QAD3013`).
    Printer(String, String),
    /// The computer cannot print (`QAD3012`); the PDF was kept instead.
    PrintingUnavailable(String),
}

impl Problem {
    pub fn message(&self) -> Message {
        match self {
            Problem::Folder(dir, _) => qsa_msg::message(
                id::OUTPUT_FOLDER_NOT_WRITABLE,
                &[&dir.display().to_string()],
            ),
            Problem::Printer(p, why) => qsa_msg::message(id::PRINTER_ERROR, &[p, why]),
            Problem::PrintingUnavailable(why) => {
                qsa_msg::message(id::SYSTEM_PRINTING_UNAVAILABLE, &[why])
            }
        }
    }

    /// Whether the output is lost for now: the server should hear that the
    /// printer is not ready.
    pub fn fails_job(&self) -> bool {
        !matches!(self, Problem::PrintingUnavailable(_))
    }
}

/// How a job ended up.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum JobStatus {
    Done,
    Failed,
    /// Transformed data, not printed.
    Discarded,
}

/// One row of Received Output.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JobRecord {
    /// When it ended, seconds since 1970.
    pub time: i64,
    pub title: String,
    pub pages: usize,
    /// Bytes of print data received.
    pub bytes: u64,
    pub target: Target,
    pub pdf: Option<PathBuf>,
    pub text: Option<PathBuf>,
    /// The printer it went to.
    pub printer: Option<String>,
    pub status: JobStatus,
    /// The spool file, while it is kept.
    pub spool: Option<PathBuf>,
    pub problems: Vec<Problem>,
}

impl JobRecord {
    /// The Status column.
    pub fn status_text(&self) -> String {
        match self.status {
            JobStatus::Discarded => "Discarded (transformed)".into(),
            JobStatus::Failed => match self.problems.iter().find(|p| p.fails_job()) {
                Some(Problem::Folder(..)) => "Error: folder".into(),
                Some(Problem::Printer(..)) => "Error: printer".into(),
                _ => "Error".into(),
            },
            JobStatus::Done => match (&self.pdf, &self.printer) {
                (Some(_), Some(_)) => "Saved and printed".into(),
                (Some(_), None) => "Saved".into(),
                (None, Some(_)) => "Printed".into(),
                (None, None) => "Done".into(),
            },
        }
    }

    /// The messages to report for the job: what was made, then problems.
    pub fn messages(&self, device: &str) -> Vec<Message> {
        let pages = self.pages.to_string();
        let mut out = Vec::new();
        if self.status == JobStatus::Discarded {
            out.push(qsa_msg::message(id::TRANSFORMED_DATA, &[]));
            return out;
        }
        if let Some(pdf) = &self.pdf {
            out.push(qsa_msg::message(
                id::OUTPUT_SAVED,
                &[device, &pdf.display().to_string(), &pages],
            ));
        }
        if let Some(p) = &self.printer {
            out.push(qsa_msg::message(id::OUTPUT_PRINTED, &[device, p, &pages]));
        }
        out.extend(self.problems.iter().map(Problem::message));
        out
    }
}

/// How a received job ended.
#[derive(Debug)]
pub enum Ended {
    Rendered(JobRecord),
    /// Host print transform output: discarded.
    Transformed(JobRecord),
}

struct OpenJob {
    path: PathBuf,
    file: File,
    bytes: u64,
    transformed: bool,
}

/// One printer session's spool.
pub struct Spooler {
    out: Output,
    dir: PathBuf,
    seq: u32,
    open: Option<OpenJob>,
}

/// The spool folder for `device`.
pub fn spool_dir(device: &str) -> PathBuf {
    let base = directories::BaseDirs::new()
        .map(|b| b.data_local_dir().to_owned())
        .unwrap_or_else(std::env::temp_dir);
    let safe: String = device
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    base.join("Quartz Systems")
        .join("Access")
        .join("Spool")
        .join(if safe.is_empty() { "_".into() } else { safe })
}

/// `(YYYYMMDD, HHMMSS)` of a time, local.
fn stamp(unix: i64) -> (String, String) {
    match crate::localtime::local(unix) {
        Some((y, mo, d, h, mi)) => (
            format!("{y:04}{mo:02}{d:02}"),
            format!("{h:02}{mi:02}{:02}", unix.rem_euclid(60)),
        ),
        None => ("00000000".into(), "000000".into()),
    }
}

/// Writes `bytes` to `path` through a `.part` file and a rename.
fn write_whole(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let part = path.with_extension(format!(
        "{}.part",
        path.extension().and_then(|e| e.to_str()).unwrap_or("")
    ));
    {
        let mut f = File::create(&part)?;
        f.write_all(bytes)?;
        f.sync_all()?;
    }
    fs::rename(&part, path)
}

impl Spooler {
    pub fn new(out: Output) -> Spooler {
        let dir = out.spool_dir.clone();
        Spooler::in_dir(out, dir)
    }

    /// A spooler using `dir` for its spool files.
    pub fn in_dir(out: Output, dir: PathBuf) -> Spooler {
        Spooler {
            out,
            dir,
            seq: 0,
            open: None,
        }
    }

    pub fn output(&self) -> &Output {
        &self.out
    }

    pub fn set_output(&mut self, out: Output) {
        self.out = out;
    }

    /// Starts a job: opens its spool file.
    pub fn begin(&mut self, transformed: bool) -> io::Result<()> {
        if self.open.is_some() {
            tn5250::debug!(
                "printer: a job began before the last one ended; that one is rendered first"
            );
            let _ = self.end();
        }
        fs::create_dir_all(&self.dir)?;
        let (date, time) = stamp(crate::localtime::now());
        self.seq += 1;
        let mut path = self.dir.join(format!("{date}-{time}-{:04}.scs", self.seq));
        while path.exists() {
            self.seq += 1;
            path = self.dir.join(format!("{date}-{time}-{:04}.scs", self.seq));
        }
        let file = File::create(&path)?;
        self.open = Some(OpenJob {
            path,
            file,
            bytes: 0,
            transformed,
        });
        Ok(())
    }

    /// Adds data to the job, on disk before this returns.
    pub fn data(&mut self, bytes: &[u8]) -> io::Result<()> {
        if self.open.is_none() {
            self.begin(false)?;
        }
        let job = self.open.as_mut().expect("a job is open");
        if job.transformed {
            return Ok(());
        }
        job.file.write_all(bytes)?;
        job.file.sync_data()?;
        job.bytes += bytes.len() as u64;
        Ok(())
    }

    /// Drops the job in progress (the server cleared its buffers).
    pub fn clear(&mut self) {
        if let Some(job) = self.open.take() {
            drop(job.file);
            let _ = fs::remove_file(&job.path);
        }
    }

    /// Ends the job in progress and renders it.
    pub fn end(&mut self) -> Option<Ended> {
        let job = self.open.take()?;
        drop(job.file);
        if job.transformed {
            let _ = fs::remove_file(&job.path);
            return Some(Ended::Transformed(JobRecord {
                time: crate::localtime::now(),
                title: self.out.device.clone(),
                pages: 0,
                bytes: job.bytes,
                target: self.out.target,
                pdf: None,
                text: None,
                printer: None,
                status: JobStatus::Discarded,
                spool: None,
                problems: Vec::new(),
            }));
        }
        let data = fs::read(&job.path).unwrap_or_default();
        let title = format!("{} job {}", self.out.device, self.seq);
        let mut rec = self.render(&data, &title);
        rec.spool = Some(job.path.clone());
        if rec.status == JobStatus::Done {
            let done = job.path.with_extension("done.scs");
            if fs::rename(&job.path, &done).is_ok() {
                rec.spool = Some(done);
            }
        }
        Some(Ended::Rendered(rec))
    }

    /// Renders print data to the session's targets.
    pub fn render(&mut self, data: &[u8], title: &str) -> JobRecord {
        let now = crate::localtime::now();
        let doc = scs::parse(
            data,
            &scs::Options {
                ccsid: self.out.ccsid,
                paper: self.out.paper,
                font: self.out.font,
            },
        );
        let mut rec = JobRecord {
            time: now,
            title: title.to_owned(),
            pages: doc.pages.len(),
            bytes: data.len() as u64,
            target: self.out.target,
            pdf: None,
            text: None,
            printer: None,
            status: JobStatus::Done,
            spool: None,
            problems: Vec::new(),
        };
        let mut want_pdf = self.out.target.pdf();
        if self.out.target.printer()
            && let Err(why) = printing_available()
        {
            rec.problems.push(Problem::PrintingUnavailable(why));
            want_pdf = true;
        }
        if want_pdf {
            match self.write_pdf(&doc, title, now) {
                Ok((pdf, text)) => {
                    rec.pdf = Some(pdf);
                    rec.text = text;
                }
                Err(p) => rec.problems.push(p),
            }
        }
        if self.out.target.printer() && !rec.problems.iter().any(|p| !p.fails_job()) {
            match self.print(&doc, title, rec.pdf.as_deref()) {
                Ok(name) => rec.printer = Some(name),
                Err(p) => rec.problems.push(p),
            }
        }
        if rec.problems.iter().any(Problem::fails_job) {
            rec.status = JobStatus::Failed;
        }
        rec
    }

    fn write_pdf(
        &mut self,
        doc: &scs::Document,
        title: &str,
        now: i64,
    ) -> Result<(PathBuf, Option<PathBuf>), Problem> {
        let Some(dir) = self.out.pdf_folder.clone() else {
            return Err(Problem::Folder(
                PathBuf::from("Documents"),
                "no Documents folder".into(),
            ));
        };
        let fail = |e: io::Error| Problem::Folder(dir.clone(), e.to_string());
        fs::create_dir_all(&dir).map_err(fail)?;
        let (date, time) = stamp(now);
        let mut seq = self.seq.max(1);
        let mut path;
        loop {
            path = dir.join(qsp::file_name(
                &self.out.file_name,
                &self.out.device,
                &date,
                &time,
                seq,
            ));
            if !path.exists() {
                break;
            }
            seq += 1;
        }
        let bytes = qsa_print::pdf::render(
            doc,
            &qsa_print::pdf::Options {
                title: title.to_owned(),
                margin: self.out.margin,
                paper: self.out.paper,
            },
        )
        .map_err(|e| Problem::Folder(dir.clone(), e.to_string()))?;
        write_whole(&path, &bytes).map_err(fail)?;
        let text = if self.out.also_save_text {
            let t = path.with_extension("txt");
            write_whole(&t, scs::to_text(doc).as_bytes()).map_err(fail)?;
            Some(t)
        } else {
            None
        };
        Ok((path, text))
    }

    /// Prints on the system printer; returns its name.
    #[cfg(windows)]
    fn print(
        &self,
        doc: &scs::Document,
        title: &str,
        _pdf: Option<&Path>,
    ) -> Result<String, Problem> {
        use qsa_print::gdi;
        let name = if self.out.system_printer.is_empty() {
            gdi::default_printer().unwrap_or_else(|| "(default)".into())
        } else {
            self.out.system_printer.clone()
        };
        gdi::print(doc, Some(&name), title, self.out.margin, None)
            .map(|()| name.clone())
            .map_err(|e| Problem::Printer(name, e.to_string()))
    }

    /// Prints on the system printer through CUPS; returns its name.
    #[cfg(not(windows))]
    fn print(
        &self,
        doc: &scs::Document,
        title: &str,
        pdf: Option<&Path>,
    ) -> Result<String, Problem> {
        use qsa_print::cups;
        let name = if self.out.system_printer.is_empty() {
            cups::default_printer().unwrap_or_else(|| "(default)".into())
        } else {
            self.out.system_printer.clone()
        };
        let printer =
            (!self.out.system_printer.is_empty()).then_some(self.out.system_printer.as_str());
        let fail = |why: String| Problem::Printer(name.clone(), why);
        match pdf {
            Some(p) => cups::print_pdf(p, printer, title).map_err(|e| fail(e.to_string()))?,
            None => {
                // A PDF for the printer only, removed once lp has it.
                fs::create_dir_all(&self.dir).map_err(|e| fail(e.to_string()))?;
                let tmp = self.dir.join(format!("print-{}.pdf", self.seq));
                let bytes = qsa_print::pdf::render(
                    doc,
                    &qsa_print::pdf::Options {
                        title: title.to_owned(),
                        margin: self.out.margin,
                        paper: self.out.paper,
                    },
                )
                .map_err(|e| fail(e.to_string()))?;
                write_whole(&tmp, &bytes).map_err(|e| fail(e.to_string()))?;
                let r = cups::print_pdf(&tmp, printer, title);
                let _ = fs::remove_file(&tmp);
                r.map_err(|e| fail(e.to_string()))?
            }
        };
        Ok(name)
    }

    /// Prints a received job again on the system printer.
    pub fn print_again(&mut self, rec: &JobRecord) -> Result<String, Problem> {
        printing_available().map_err(Problem::PrintingUnavailable)?;
        let data = rec
            .spool
            .as_ref()
            .and_then(|p| fs::read(p).ok())
            .ok_or_else(|| {
                Problem::Printer(
                    self.out.system_printer.clone(),
                    "the job's data is no longer kept".into(),
                )
            })?;
        let doc = scs::parse(
            &data,
            &scs::Options {
                ccsid: self.out.ccsid,
                paper: self.out.paper,
                font: self.out.font,
            },
        );
        self.print(&doc, &rec.title, rec.pdf.as_deref())
    }

    /// Renders spool files a stopped session left unrendered, and removes
    /// the done ones of earlier sessions.
    pub fn recover(&mut self) -> Vec<JobRecord> {
        let Ok(rd) = fs::read_dir(&self.dir) else {
            return Vec::new();
        };
        let mut left: Vec<PathBuf> = Vec::new();
        for e in rd.flatten() {
            let p = e.path();
            let name = p
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            if name.ends_with(".done.scs") || name.ends_with(".part") {
                let _ = fs::remove_file(&p);
            } else if name.ends_with(".scs") {
                left.push(p);
            }
        }
        left.sort();
        let mut out = Vec::new();
        for p in left {
            let data = fs::read(&p).unwrap_or_default();
            let title = format!("{} recovered job", self.out.device);
            let mut rec = self.render(&data, &title);
            if rec.status == JobStatus::Done {
                let done = p.with_extension("done.scs");
                let _ = fs::rename(&p, &done);
                rec.spool = Some(done);
            } else {
                rec.spool = Some(p);
            }
            out.push(rec);
        }
        out
    }
}

/// Whether this computer can print: `Err` says why not.
pub fn printing_available() -> Result<(), String> {
    #[cfg(windows)]
    {
        if qsa_print::gdi::available() {
            Ok(())
        } else {
            Err("no printer is set up on this computer".into())
        }
    }
    #[cfg(not(windows))]
    {
        qsa_print::cups::available().map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("qsa-spool-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    fn output(dir: &Path) -> Output {
        Output {
            device: "PRT01".into(),
            target: Target::Pdf,
            pdf_folder: Some(dir.join("out")),
            file_name: qsp::DEFAULT_FILE_NAME.into(),
            also_save_text: true,
            system_printer: String::new(),
            paper: scs::Paper::Letter,
            margin: None,
            ccsid: 37,
            font: 11,
            spool_dir: dir.join("spool"),
        }
    }

    #[test]
    fn a_job_is_spooled_then_rendered() {
        let dir = temp("job");
        let mut s = Spooler::in_dir(output(&dir), dir.join("spool"));
        s.begin(false).unwrap();
        let mut b = scs::sample::Builder::new();
        b.text("PAGE ONE").ff().text("PAGE TWO");
        let data = b.bytes();
        s.data(&data[..10]).unwrap();
        // On disk before the job ends.
        let spooled: Vec<_> = fs::read_dir(dir.join("spool")).unwrap().flatten().collect();
        assert_eq!(fs::read(spooled[0].path()).unwrap(), &data[..10]);
        s.data(&data[10..]).unwrap();
        let Some(Ended::Rendered(rec)) = s.end() else {
            panic!()
        };
        assert_eq!(rec.status, JobStatus::Done, "{:?}", rec.problems);
        assert_eq!(rec.pages, 2);
        assert_eq!(rec.bytes, data.len() as u64);
        let pdf = rec.pdf.clone().unwrap();
        assert!(fs::read(&pdf).unwrap().starts_with(b"%PDF-"));
        let name = pdf.file_name().unwrap().to_string_lossy().into_owned();
        assert!(
            name.starts_with("PRT01-") && name.ends_with("-0001.pdf"),
            "{name}"
        );
        assert_eq!(
            fs::read_to_string(rec.text.as_ref().unwrap()).unwrap(),
            "PAGE ONE\n\u{c}PAGE TWO\n"
        );
        assert!(
            rec.spool
                .as_ref()
                .unwrap()
                .to_string_lossy()
                .ends_with(".done.scs")
        );
        assert_eq!(rec.status_text(), "Saved");
        let m = rec.messages("PRT01");
        assert_eq!(m[0].id, id::OUTPUT_SAVED);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn transformed_and_cleared_jobs_make_nothing() {
        let dir = temp("discard");
        let mut s = Spooler::in_dir(output(&dir), dir.join("spool"));
        s.begin(true).unwrap();
        s.data(&[3, 2, 0x1B, 0x45]).unwrap();
        let Some(Ended::Transformed(rec)) = s.end() else {
            panic!()
        };
        assert_eq!(rec.messages("PRT01")[0].id, id::TRANSFORMED_DATA);
        s.begin(false).unwrap();
        s.data(b"\xC1").unwrap();
        s.clear();
        assert!(s.end().is_none());
        assert!(!dir.join("out").exists());
        assert_eq!(fs::read_dir(dir.join("spool")).unwrap().count(), 0);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn an_unwritable_folder_fails_the_job_but_keeps_its_data() {
        let dir = temp("folder");
        let blocker = dir.join("out");
        fs::write(&blocker, "a file where the folder should be").unwrap();
        let mut s = Spooler::in_dir(output(&dir), dir.join("spool"));
        s.begin(false).unwrap();
        s.data(b"\xC1\xC2").unwrap();
        let Some(Ended::Rendered(rec)) = s.end() else {
            panic!()
        };
        assert_eq!(rec.status, JobStatus::Failed);
        assert!(matches!(rec.problems[0], Problem::Folder(..)));
        assert_eq!(rec.problems[0].message().id, id::OUTPUT_FOLDER_NOT_WRITABLE);
        let spool = rec.spool.unwrap();
        assert!(
            spool.to_string_lossy().ends_with(".scs")
                && !spool.to_string_lossy().ends_with(".done.scs")
        );
        assert_eq!(fs::read(&spool).unwrap(), b"\xC1\xC2");
        // Recovered once the folder can be written.
        fs::remove_file(&blocker).unwrap();
        let recovered = s.recover();
        assert_eq!(recovered.len(), 1);
        assert_eq!(recovered[0].status, JobStatus::Done);
        assert!(recovered[0].pdf.as_ref().unwrap().exists());
        // Done spool files go at the next start.
        assert!(s.recover().is_empty());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn output_settings_from_the_session_file() {
        let sys = qsa_config::System::new("qs400");
        let mut f = PrinterFile::new(&sys, "prt01");
        f.output.paper = qsp::Paper::A4;
        f.output.margins = Some(0.5);
        let o = Output::from_file(&f);
        assert_eq!(
            (o.device.as_str(), o.paper, o.margin),
            ("PRT01", scs::Paper::A4, Some(720))
        );
        assert!(o.pdf_folder.unwrap().ends_with("PRT01"));
    }
}
