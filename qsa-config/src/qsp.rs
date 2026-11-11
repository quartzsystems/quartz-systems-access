// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! `.qsp` files: a saved 5250 printer session.
//!
//! ```toml
//! [qsaccess]
//! kind = "printer"
//! version = 1
//!
//! [system]
//! name = "qs400.example.net"   # as in .qsd: the System Configuration
//! host = "10.42.70.150"        # fallback, for "Create system from session file"
//! telnet_port = 23
//! tls = false
//!
//! [printer]
//! device_name = "QSAPRT01"     # required
//! message_queue = "QSYSOPR"    # empty: QSYSOPR
//! message_queue_library = "QSYS"   # empty: QSYS
//! title = "Office laser"       # empty: the file name
//! ccsid = 37
//! font = 11
//!
//! [output]
//! target = "pdf"               # "pdf", "printer" or "both"
//! pdf_folder = ""              # empty: Documents/QS400 Access for Desktop/Printer Output/<device>
//! file_name = "{device}-{date}-{time}-{seq}.pdf"
//! open_pdf_when_done = false
//! also_save_text = false
//! system_printer = ""          # empty: the default printer
//! paper = "letter"             # "letter", "a4" or "legal"
//! margins = 0.5                # inches kept clear; absent: a quarter inch
//!
//! [session]
//! auto_connect = true
//! window = { x = 40, y = 40, w = 640, h = 420 }
//! ```

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::doc::{self, Extra, FileError, Header};
use crate::qsd::SystemRef;
use crate::settings::{self, Geometry};
use crate::system::System;

/// File extension, without the dot.
pub const EXTENSION: &str = "qsp";

/// The output file name pattern when none is given.
pub const DEFAULT_FILE_NAME: &str = "{device}-{date}-{time}-{seq}.pdf";

/// The `.qsp` files directly in `dir`, sorted.
pub fn files_in(dir: &Path) -> Vec<PathBuf> {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out: Vec<PathBuf> = rd
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| {
            p.is_file()
                && p.extension()
                    .is_some_and(|e| e.eq_ignore_ascii_case(EXTENSION))
        })
        .collect();
    out.sort();
    out
}

/// Where received output goes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Target {
    /// A PDF in the output folder.
    #[default]
    Pdf,
    /// The system printer.
    Printer,
    /// Both.
    Both,
}

impl Target {
    pub fn pdf(self) -> bool {
        matches!(self, Target::Pdf | Target::Both)
    }

    pub fn printer(self) -> bool {
        matches!(self, Target::Printer | Target::Both)
    }
}

/// Paper, when the data does not say.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Paper {
    #[default]
    Letter,
    A4,
    Legal,
}

/// `[printer]`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Printer {
    /// Required: the printer device on the system.
    pub device_name: String,
    /// Empty: `QSYSOPR`.
    pub message_queue: String,
    /// Empty: `QSYS`.
    pub message_queue_library: String,
    /// Shown in the window title and in Session Manager. Empty: the file
    /// name.
    pub title: String,
    /// The code page the device is created with, and print data is read in.
    pub ccsid: u16,
    /// The font ID the device is created with.
    pub font: u16,
    #[serde(flatten)]
    pub extra: Extra,
}

impl Default for Printer {
    fn default() -> Printer {
        Printer {
            device_name: String::new(),
            message_queue: String::new(),
            message_queue_library: String::new(),
            title: String::new(),
            ccsid: 37,
            font: 11,
            extra: Extra::new(),
        }
    }
}

/// `[output]`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Output {
    pub target: Target,
    /// Empty: [`default_pdf_folder`].
    pub pdf_folder: String,
    /// See [`file_name`]. Empty: [`DEFAULT_FILE_NAME`].
    pub file_name: String,
    pub open_pdf_when_done: bool,
    pub also_save_text: bool,
    /// The operating system's printer name. Empty: the default printer.
    pub system_printer: String,
    pub paper: Paper,
    /// The margins override: inches kept clear on every side. `None`: the
    /// renderer's default.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub margins: Option<f64>,
    #[serde(flatten)]
    pub extra: Extra,
}

impl Default for Output {
    fn default() -> Output {
        Output {
            target: Target::Pdf,
            pdf_folder: String::new(),
            file_name: DEFAULT_FILE_NAME.to_owned(),
            open_pdf_when_done: false,
            also_save_text: false,
            system_printer: String::new(),
            paper: Paper::Letter,
            margins: None,
            extra: Extra::new(),
        }
    }
}

/// `[session]`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Session {
    pub auto_connect: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub window: Option<Geometry>,
    #[serde(flatten)]
    pub extra: Extra,
}

impl Default for Session {
    fn default() -> Session {
        Session {
            auto_connect: true,
            window: None,
            extra: Extra::new(),
        }
    }
}

/// The file as written.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct PrinterDoc {
    qsaccess: Header,
    #[serde(default)]
    system: SystemRef,
    #[serde(default)]
    printer: Printer,
    #[serde(default)]
    output: Output,
    #[serde(default)]
    session: Session,
    #[serde(flatten)]
    extra: Extra,
}

/// A printer session file.
#[derive(Clone, Debug, PartialEq)]
pub struct PrinterFile {
    pub header: Header,
    pub system: SystemRef,
    pub printer: Printer,
    pub output: Output,
    pub session: Session,
    pub extra: Extra,
}

/// The output folder for `device` when none is set:
/// `QS400 Access for Desktop/Printer Output/<device>` in the user's
/// Documents folder ([`crate::documents::folder`]).
pub fn default_pdf_folder(device: &str) -> Option<PathBuf> {
    crate::documents::folder().map(|d| d.join("Printer Output").join(safe_name(device)))
}

/// `s` with every character a file name cannot hold replaced by `_`.
fn safe_name(s: &str) -> String {
    let out: String = s
        .trim()
        .chars()
        .map(|c| {
            if c.is_control() || "\\/:*?\"<>|".contains(c) {
                '_'
            } else {
                c
            }
        })
        .collect();
    if out.is_empty() || out.chars().all(|c| c == '.') {
        "_".to_owned()
    } else {
        out
    }
}

/// A file name from `pattern`: `{device}`, `{date}` (`YYYYMMDD`), `{time}`
/// (`HHMMSS`), `{seq}` (four digits) and `{job}` are filled in, characters a
/// file name cannot hold are replaced, and `.pdf` is added when the pattern
/// has no extension.
pub fn file_name(pattern: &str, device: &str, date: &str, time: &str, seq: u32) -> String {
    let pattern = if pattern.trim().is_empty() {
        DEFAULT_FILE_NAME
    } else {
        pattern.trim()
    };
    let name = pattern
        .replace("{device}", device)
        .replace("{date}", date)
        .replace("{time}", time)
        .replace("{seq}", &format!("{seq:04}"))
        .replace("{job}", &seq.to_string());
    let mut name = safe_name(&name);
    if Path::new(&name).extension().is_none() {
        name.push_str(".pdf");
    }
    name
}

/// Checks an object name (message queue or library): empty, or 1 to 10
/// characters as device names are.
fn check_object(what: &str, name: &str) -> Result<(), FileError> {
    settings::check_device(name)
        .map(|_| ())
        .map_err(|_| FileError::Invalid(format!("{what} {name} is not a valid name")))
}

impl PrinterFile {
    /// A new printer session for `system` and `device`.
    pub fn new(system: &System, device: &str) -> PrinterFile {
        PrinterFile {
            header: Header::new(doc::PRINTER),
            system: SystemRef::from_system(system),
            printer: Printer {
                device_name: device.trim().to_ascii_uppercase(),
                ..Printer::default()
            },
            output: Output::default(),
            session: Session::default(),
            extra: Extra::new(),
        }
    }

    /// The title to show: `[printer] title`, else the file's name, else
    /// the device.
    pub fn display_title(&self, path: Option<&Path>) -> String {
        if !self.printer.title.trim().is_empty() {
            return self.printer.title.trim().to_owned();
        }
        path.and_then(Path::file_stem)
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| self.printer.device_name.clone())
    }

    /// The message queue and library to ask for, with the defaults.
    pub fn message_queue(&self) -> (String, String) {
        let pick = |s: &str, d: &str| {
            let s = s.trim();
            if s.is_empty() { d } else { s }.to_ascii_uppercase()
        };
        (
            pick(&self.printer.message_queue, "QSYSOPR"),
            pick(&self.printer.message_queue_library, "QSYS"),
        )
    }

    /// The folder PDFs go to.
    pub fn pdf_folder(&self) -> Option<PathBuf> {
        let f = self.output.pdf_folder.trim();
        if f.is_empty() {
            default_pdf_folder(&self.printer.device_name)
        } else {
            // A folder set by an earlier release, in the Documents folder
            // it used, is where that folder has been moved to.
            Some(crate::documents::current(Path::new(f)))
        }
    }

    /// Checks every value a printer session needs.
    pub fn validate(&self) -> Result<(), FileError> {
        let bad = |s: String| Err(FileError::Invalid(s));
        if self.system.name.trim().is_empty() {
            return bad("[system] has no name".into());
        }
        if let Err(e) = crate::system::check_name(&self.system.name) {
            return bad(format!("system name {}: {e}", self.system.name));
        }
        if self.system.telnet_port == 0 {
            return bad("Telnet port 0 is not valid".into());
        }
        if self.printer.device_name.trim().is_empty() {
            return bad("[printer] has no device name".into());
        }
        if let Err(e) = settings::check_device(&self.printer.device_name) {
            return bad(e.trim_end_matches('.').to_owned());
        }
        check_object("message queue", &self.printer.message_queue)?;
        check_object("library", &self.printer.message_queue_library)?;
        if !settings::CCSIDS.contains(&self.printer.ccsid) {
            return bad(format!("CCSID {} is not supported", self.printer.ccsid));
        }
        if let Some(m) = self.output.margins
            && !(0.0..=2.0).contains(&m)
        {
            return bad(format!("margins of {m} inches are not between 0 and 2"));
        }
        Ok(())
    }

    /// Parses `.qsp` text.
    pub fn parse(text: &str) -> Result<PrinterFile, FileError> {
        let d: PrinterDoc = doc::parse(text, doc::PRINTER)?;
        let f = PrinterFile {
            header: d.qsaccess,
            system: d.system,
            printer: d.printer,
            output: d.output,
            session: d.session,
            extra: d.extra,
        };
        f.validate()?;
        Ok(f)
    }

    /// The `.qsp` text.
    pub fn to_text(&self) -> String {
        let mut header = self.header.clone();
        header.kind = doc::PRINTER.into();
        header.version = doc::VERSION;
        doc::to_text(&PrinterDoc {
            qsaccess: header,
            system: self.system.clone(),
            printer: self.printer.clone(),
            output: self.output.clone(),
            session: self.session.clone(),
            extra: self.extra.clone(),
        })
    }

    /// Reads a `.qsp` file.
    pub fn load(path: &Path) -> Result<PrinterFile, FileError> {
        PrinterFile::parse(&doc::read(path)?)
    }

    /// Writes a `.qsp` file atomically.
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        doc::write(path, &self.to_text())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::tests::temp_dir;

    fn sample() -> PrinterFile {
        let mut sys = System::new("qs400.example.net");
        sys.fixed_ip = "10.42.70.150".into();
        let mut f = PrinterFile::new(&sys, "qsaprt01");
        f.printer.message_queue = "PRTMSGQ".into();
        f.printer.title = "Office laser".into();
        f.output = Output {
            target: Target::Both,
            pdf_folder: "C:/Spool".into(),
            also_save_text: true,
            system_printer: "Office Laser".into(),
            paper: Paper::A4,
            margins: Some(0.5),
            ..Output::default()
        };
        f.session.window = Some(Geometry {
            x: 1,
            y: 2,
            w: 640,
            h: 420,
        });
        f
    }

    #[test]
    fn round_trip() {
        let f = sample();
        let text = f.to_text();
        assert!(text.contains("kind = \"printer\""));
        assert!(text.contains("target = \"both\""));
        assert!(text.contains("paper = \"a4\""));
        assert_eq!(PrinterFile::parse(&text).unwrap(), f);
        assert_eq!(f.printer.device_name, "QSAPRT01");
    }

    #[test]
    fn defaults_and_unknown_keys() {
        let text = "[qsaccess]\nkind = \"printer\"\nversion = 1\nbuild = \"x\"\n\
                    [system]\nname = \"qs400\"\n\
                    [printer]\ndevice_name = \"PRT01\"\nfuture = 3\n\
                    [output]\nduplex = true\n\
                    [later]\nkey = \"kept\"\n";
        let f = PrinterFile::parse(text).unwrap();
        assert_eq!(f.output.target, Target::Pdf);
        assert_eq!(f.output.file_name, DEFAULT_FILE_NAME);
        assert_eq!(f.output.paper, Paper::Letter);
        assert_eq!(f.output.margins, None);
        assert!(f.session.auto_connect);
        assert!(!f.output.open_pdf_when_done && !f.output.also_save_text);
        assert_eq!(f.message_queue(), ("QSYSOPR".into(), "QSYS".into()));
        let again = f.to_text();
        for kept in ["build = \"x\"", "future = 3", "duplex = true", "[later]"] {
            assert!(again.contains(kept), "{kept} lost:\n{again}");
        }
    }

    #[test]
    fn refused() {
        let base = "[qsaccess]\nkind = \"printer\"\nversion = 1\n[system]\nname = \"qs400\"\n";
        // No device name.
        assert!(matches!(
            PrinterFile::parse(base),
            Err(FileError::Invalid(m)) if m.contains("device name")
        ));
        let bad_device = format!("{base}[printer]\ndevice_name = \"1BAD\"\n");
        assert!(PrinterFile::parse(&bad_device).is_err());
        let bad_queue = format!(
            "{base}[printer]\ndevice_name = \"P\"\nmessage_queue = \"WAY.TOO.LONG.NAME\"\n"
        );
        assert!(PrinterFile::parse(&bad_queue).is_err());
        let bad_target =
            format!("{base}[printer]\ndevice_name = \"P\"\n[output]\ntarget = \"fax\"\n");
        assert!(PrinterFile::parse(&bad_target).is_err());
        let newer = "[qsaccess]\nkind = \"printer\"\nversion = 2\n";
        assert!(matches!(
            PrinterFile::parse(newer),
            Err(FileError::Newer(2))
        ));
        let display = "[qsaccess]\nkind = \"display\"\nversion = 1\n";
        assert!(matches!(
            PrinterFile::parse(display),
            Err(FileError::WrongKind { .. })
        ));
    }

    #[test]
    fn file_names() {
        assert_eq!(
            file_name("", "PRT01", "20261101", "143005", 7),
            "PRT01-20261101-143005-0007.pdf"
        );
        assert_eq!(file_name("{device}/{job}", "P", "d", "t", 12), "P_12.pdf");
        assert_eq!(file_name("out.txt", "P", "d", "t", 1), "out.txt");
        let dir = default_pdf_folder("PRT01").unwrap();
        assert!(dir.ends_with(Path::new("QS400 Access for Desktop/Printer Output/PRT01")));
    }

    #[test]
    fn save_and_load() {
        let dir = temp_dir("qsp");
        let path = dir.join("Office.qsp");
        let f = sample();
        f.save(&path).unwrap();
        assert_eq!(PrinterFile::load(&path).unwrap(), f);
        assert_eq!(f.display_title(Some(&path)), "Office laser");
        assert_eq!(files_in(&dir), vec![path]);
    }
}
