// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! The printer session's Settings dialog: the device and message queue,
//! and where and how received output goes.

use std::path::PathBuf;

use fltk::app;
use fltk::button::{Button, CheckButton};
use fltk::input::Input;
use fltk::menu::Choice;
use fltk::prelude::*;
use fltk::window::Window;
use qsa_config::qsp::{Paper, PrinterFile, Target};
use qsa_config::settings::check_device;
use qsa_ui::chan::{self, Emit};
use qsa_ui::controls;
use qsa_ui::propdialog::center_on;
use qsa_ui::statusbar::{Status, StatusLine};
use qsa_ui::theme::{self, colors};

use super::start::TARGETS;

pub const W: i32 = 480;
pub const H: i32 = 470;

const PAPERS: &[(Paper, &str)] = &[
    (Paper::Letter, "Letter (8.5 x 11 in)"),
    (Paper::A4, "A4 (210 x 297 mm)"),
    (Paper::Legal, "Legal (8.5 x 14 in)"),
];

/// What the dialog's buttons ask for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Act {
    Ok,
    Cancel,
    Browse,
    PrintSetup,
}

/// The printers to offer, or why there are none.
fn system_printers() -> Result<Vec<String>, String> {
    #[cfg(windows)]
    {
        Ok(qsa_print::gdi::printers())
    }
    #[cfg(not(windows))]
    {
        qsa_print::cups::printers().map_err(|e| e.to_string())
    }
}

/// The dialog's widgets.
pub struct SettingsDialog {
    pub window: Window,
    device: Input,
    msgq: Input,
    msgq_lib: Input,
    title: Input,
    target: Choice,
    folder: Input,
    file_name: Input,
    paper: Choice,
    margins: Input,
    printer: Choice,
    printers: Vec<String>,
    open_when_done: CheckButton,
    also_text: CheckButton,
    auto_connect: CheckButton,
    status: StatusLine,
}

impl SettingsDialog {
    fn read(&self, base: &PrinterFile) -> Result<PrinterFile, String> {
        let mut f = base.clone();
        let device = self.device.value();
        if device.trim().is_empty() {
            return Err(qsa_msg::message(qsa_msg::id::PRINTER_DEVICE_REQUIRED, &[]).text);
        }
        f.printer.device_name = check_device(&device)?;
        let name_err = |what: &str| {
            format!(
                "{what} must be 1 to 10 letters, digits, #, $, @ or _, starting with a letter, #, $ or @."
            )
        };
        f.printer.message_queue =
            check_device(&self.msgq.value()).map_err(|_| name_err("Message queue"))?;
        f.printer.message_queue_library =
            check_device(&self.msgq_lib.value()).map_err(|_| name_err("Library"))?;
        f.printer.title = self.title.value().trim().to_owned();
        f.output.target = TARGETS
            .get(self.target.value().max(0) as usize)
            .map_or(Target::Pdf, |(t, _)| *t);
        f.output.pdf_folder = self.folder.value().trim().to_owned();
        f.output.file_name = self.file_name.value().trim().to_owned();
        f.output.paper = PAPERS
            .get(self.paper.value().max(0) as usize)
            .map_or(Paper::Letter, |(p, _)| *p);
        let m = self.margins.value();
        f.output.margins =
            if m.trim().is_empty() {
                None
            } else {
                match m.trim().trim_end_matches("in").trim().parse::<f64>() {
                    Ok(v) if (0.0..=2.0).contains(&v) => Some(v),
                    _ => return Err(
                        "Margins must be a number of inches from 0 to 2, or empty for the default."
                            .into(),
                    ),
                }
            };
        f.output.system_printer = match self.printer.value() {
            i if i >= 1 => self
                .printers
                .get(i as usize - 1)
                .cloned()
                .unwrap_or_default(),
            _ => String::new(),
        };
        f.output.open_pdf_when_done = self.open_when_done.is_checked();
        f.output.also_save_text = self.also_text.is_checked();
        f.session.auto_connect = self.auto_connect.is_checked();
        f.validate().map_err(|e| e.to_string())?;
        Ok(f)
    }

    fn select_printer(&mut self, name: &str) {
        if !self.printers.iter().any(|p| p == name) {
            self.printers.push(name.to_owned());
            self.printer.add_choice(&name.replace('/', "\\/"));
        }
        let i = self.printers.iter().position(|p| p == name).unwrap_or(0);
        self.printer.set_value(i as i32 + 1);
    }
}

fn build_with(f: &PrinterFile) -> (SettingsDialog, chan::Receiver<Act>) {
    let (tx, rx) = chan::channel::<Act>();
    let m = 12;
    let (lw, fh) = (128, theme::FIELD_H);
    let fw = W - 2 * m - lw;
    let mut window = Window::new(0, 0, W, H, None);
    window.set_label("Printer Session Settings");
    window.set_color(colors::FACE);
    window.make_modal(true);
    let mut y = m;
    let row = |y: &mut i32, label: &str| {
        controls::label(m, *y, lw, fh, label);
        let at = *y;
        *y += fh + 8;
        at
    };
    let at = row(&mut y, "Device name:");
    let mut device = controls::field(m + lw, at, 120, fh);
    device.set_value(&f.printer.device_name);
    let at = row(&mut y, "Message queue:");
    let mut msgq = controls::field(m + lw, at, 120, fh);
    msgq.set_value(&f.printer.message_queue);
    controls::label(m + lw + 128, at, fw - 128, fh, "(empty: QSYSOPR)");
    let at = row(&mut y, "Library:");
    let mut msgq_lib = controls::field(m + lw, at, 120, fh);
    msgq_lib.set_value(&f.printer.message_queue_library);
    controls::label(m + lw + 128, at, fw - 128, fh, "(empty: QSYS)");
    let at = row(&mut y, "Title:");
    let mut title = controls::field(m + lw, at, fw, fh);
    title.set_value(&f.printer.title);

    let at = row(&mut y, "Output:");
    let mut target = controls::combo(m + lw, at, fw, fh);
    for (_, name) in TARGETS {
        target.add_choice(name);
    }
    target.set_value(
        TARGETS
            .iter()
            .position(|(t, _)| *t == f.output.target)
            .unwrap_or(0) as i32,
    );
    let at = row(&mut y, "PDF folder:");
    let bw = theme::BUTTON_W;
    let mut folder = controls::field(m + lw, at, fw - bw - 6, fh);
    folder.set_value(&f.output.pdf_folder);
    if f.output.pdf_folder.trim().is_empty()
        && let Some(d) = f.pdf_folder()
    {
        folder.set_tooltip(&format!("Empty: {}", d.display()));
    }
    let mut browse = controls::button(W - m - bw, at, bw, fh, "Browse...");
    browse.emit_to(tx, Act::Browse);
    let at = row(&mut y, "File name:");
    let mut file_name = controls::field(m + lw, at, fw, fh);
    file_name.set_value(&f.output.file_name);
    file_name.set_tooltip("{device}, {date}, {time} and {seq} are filled in");
    let at = row(&mut y, "Paper:");
    let mut paper = controls::combo(m + lw, at, fw, fh);
    for (_, name) in PAPERS {
        paper.add_choice(name);
    }
    paper.set_value(
        PAPERS
            .iter()
            .position(|(p, _)| *p == f.output.paper)
            .unwrap_or(0) as i32,
    );
    let at = row(&mut y, "Margins (inches):");
    let mut margins = controls::field(m + lw, at, 60, fh);
    margins.set_value(&f.output.margins.map(|v| v.to_string()).unwrap_or_default());
    controls::label(m + lw + 68, at, fw - 68, fh, "(empty: a quarter inch)");

    let at = row(&mut y, "System printer:");
    let setup_w = if cfg!(windows) { bw + 6 } else { 0 };
    let mut printer = controls::combo(m + lw, at, fw - setup_w, fh);
    printer.add_choice("Default printer");
    let printers = match system_printers() {
        Ok(list) => {
            for p in &list {
                printer.add_choice(&p.replace('/', "\\/"));
            }
            list
        }
        Err(why) => {
            printer.deactivate();
            printer.set_tooltip(
                &qsa_msg::message(qsa_msg::id::SYSTEM_PRINTING_UNAVAILABLE, &[&why]).text,
            );
            Vec::new()
        }
    };
    printer.set_value(0);
    if cfg!(windows) {
        let mut setup: Button = controls::button(W - m - bw, at, bw, fh, "Print Setup...");
        setup.emit_to(tx, Act::PrintSetup);
    }
    let at = row(&mut y, "");
    let open_when_done = controls::check(m + lw, at, fw, fh, "Open each PDF when it is saved");
    open_when_done.set_checked(f.output.open_pdf_when_done);
    let at = row(&mut y, "");
    let also_text = controls::check(m + lw, at, fw, fh, "Also save a text file");
    also_text.set_checked(f.output.also_save_text);
    let at = row(&mut y, "");
    let auto_connect = controls::check(m + lw, at, fw, fh, "Connect when the session is opened");
    auto_connect.set_checked(f.session.auto_connect);

    let bh = theme::BUTTON_H;
    let status = StatusLine::new(m, H - m - bh - bh - 8, W - 2 * m, bh);
    let mut ok = controls::default_button(W - 2 * m - 2 * bw + m / 2, H - m - bh, bw, bh, "OK");
    ok.emit_to(tx, Act::Ok);
    let mut cancel = controls::button(W - m - bw, H - m - bh, bw, bh, "Cancel");
    cancel.emit_to(tx, Act::Cancel);
    window.end();
    window.set_callback(move |_| tx.send(Act::Cancel));
    let mut d = SettingsDialog {
        window,
        device,
        msgq,
        msgq_lib,
        title,
        target,
        folder,
        file_name,
        paper,
        margins,
        printer,
        printers,
        open_when_done,
        also_text,
        auto_connect,
        status,
    };
    if !f.output.system_printer.is_empty() {
        d.select_printer(&f.output.system_printer);
    }
    (d, rx)
}

/// Builds the dialog without showing it (for screenshots).
pub fn build(f: &PrinterFile) -> SettingsDialog {
    build_with(f).0
}

/// Runs the dialog over `parent`; on OK, the changed session.
pub fn run(parent: Option<&dyn WindowExt>, f: &PrinterFile) -> Option<PrinterFile> {
    let (mut d, rx) = build_with(f);
    center_on(&mut d.window, parent);
    d.window.show();
    while d.window.shown() {
        app::wait();
        match rx.recv() {
            Some(Act::Ok) => match d.read(f) {
                Ok(f) => {
                    d.window.hide();
                    return Some(f);
                }
                Err(e) => d.status.set(&e, Status::Error),
            },
            Some(Act::Cancel) => d.window.hide(),
            Some(Act::Browse) => {
                let start = PathBuf::from(d.folder.value());
                let start = if start.is_dir() {
                    start
                } else {
                    f.pdf_folder().unwrap_or_else(crate::files::sessions_dir)
                };
                if let Some(dir) = crate::files::choose_folder(&start, "PDF Folder") {
                    d.folder.set_value(&dir.display().to_string());
                }
            }
            Some(Act::PrintSetup) => {
                #[cfg(windows)]
                {
                    let hwnd = d.window.raw_handle() as isize;
                    if let Some(name) = qsa_print::gdi::print_setup(hwnd) {
                        d.select_printer(&name);
                    }
                }
            }
            None => {}
        }
    }
    None
}
