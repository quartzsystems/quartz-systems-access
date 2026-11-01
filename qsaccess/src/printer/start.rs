// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! The Start 5250 Printer dialog: system, printer device (required),
//! message queue and library, CCSID, where output goes, and TLS.

use fltk::app;
use fltk::button::CheckButton;
use fltk::input::Input;
use fltk::menu::Choice;
use fltk::prelude::*;
use fltk::window::Window;
use qsa_config::qsp::{PrinterFile, Target};
use qsa_config::settings::check_device;
use qsa_ui::chan::{self, Emit};
use qsa_ui::controls;
use qsa_ui::propdialog::center_on;
use qsa_ui::statusbar::{Status, StatusLine};
use qsa_ui::theme::{self, colors};

use crate::app::Shared;
use crate::session::start::CCSIDS;

pub const W: i32 = 420;
pub const H: i32 = 318;

/// The output choices, in the order shown.
pub const TARGETS: &[(Target, &str)] = &[
    (Target::Pdf, "PDF files"),
    (Target::Printer, "System printer"),
    (Target::Both, "PDF files and system printer"),
];

/// The dialog's widgets.
pub struct StartDialog {
    pub window: Window,
    system: Choice,
    device: Input,
    msgq: Input,
    msgq_lib: Input,
    ccsid: Choice,
    target: Choice,
    tls: CheckButton,
    status: StatusLine,
}

impl StartDialog {
    /// The session the fields describe, or what is wrong with them.
    fn read(&self, state: &Shared) -> Result<PrinterFile, String> {
        let name = self.system.choice().ok_or("Select a system.")?;
        let device = self.device.value();
        if device.trim().is_empty() {
            return Err(qsa_msg::message(qsa_msg::id::PRINTER_DEVICE_REQUIRED, &[]).text);
        }
        let device = check_device(&device)?;
        let msgq = check_device(&self.msgq.value()).map_err(|_| "Message queue must be 1 to 10 letters, digits, #, $, @ or _, starting with a letter, #, $ or @.".to_owned())?;
        let lib = check_device(&self.msgq_lib.value()).map_err(|_| "Library must be 1 to 10 letters, digits, #, $, @ or _, starting with a letter, #, $ or @.".to_owned())?;
        let st = state.borrow();
        let system = st.systems.get(&name).ok_or("Select a system.")?;
        let mut f = PrinterFile::new(system, &device);
        f.system.tls = self.tls.is_checked();
        if f.system.tls != system.use_tls {
            f.system.telnet_port = if f.system.tls {
                system.ports.telnet_tls
            } else {
                system.ports.telnet
            };
        }
        f.printer.message_queue = msgq;
        f.printer.message_queue_library = lib;
        f.printer.ccsid = CCSIDS
            .get(self.ccsid.value().max(0) as usize)
            .map_or(37, |(c, _)| *c);
        f.output.target = TARGETS
            .get(self.target.value().max(0) as usize)
            .map_or(Target::Pdf, |(t, _)| *t);
        Ok(f)
    }
}

fn build_with(state: &Shared, system: Option<&str>) -> (StartDialog, chan::Receiver<bool>) {
    let (tx, rx) = chan::channel::<bool>();
    let m = 12;
    let (lw, fh) = (124, theme::FIELD_H);
    let fw = W - 2 * m - lw;
    let mut window = Window::new(0, 0, W, H, None);
    window.set_label("Start 5250 Printer");
    window.set_color(colors::FACE);
    window.make_modal(true);
    let mut y = m;
    let row = |y: &mut i32, label: &str| {
        controls::label(m, *y, lw, fh, label);
        let at = *y;
        *y += fh + 10;
        at
    };
    let at = row(&mut y, "System:");
    let mut sys = controls::combo(m + lw, at, fw, fh);
    let at = row(&mut y, "Device name:");
    let device = controls::field(m + lw, at, 120, fh);
    controls::label(m + lw + 128, at, fw - 128, fh, "(required)");
    let at = row(&mut y, "Message queue:");
    let mut msgq = controls::field(m + lw, at, 120, fh);
    msgq.set_value("QSYSOPR");
    let at = row(&mut y, "Library:");
    let mut msgq_lib = controls::field(m + lw, at, 120, fh);
    msgq_lib.set_value("QSYS");
    let at = row(&mut y, "CCSID:");
    let mut ccsid = controls::combo(m + lw, at, fw, fh);
    for (_, name) in CCSIDS {
        ccsid.add_choice(name);
    }
    ccsid.set_value(0);
    let at = row(&mut y, "Output:");
    let mut target = controls::combo(m + lw, at, fw, fh);
    for (_, name) in TARGETS {
        target.add_choice(name);
    }
    target.set_value(0);
    let at = row(&mut y, "");
    let tls = controls::check(m + lw, at, fw, fh, "Use TLS");

    let bh = theme::BUTTON_H;
    let bw = theme::BUTTON_W;
    let status = StatusLine::new(m, H - m - bh - bh - 10, W - 2 * m, bh);
    let mut ok = controls::default_button(W - 2 * m - 2 * bw + m / 2, H - m - bh, bw, bh, "Start");
    ok.emit_to(tx, true);
    let mut cancel = controls::button(W - m - bw, H - m - bh, bw, bh, "Cancel");
    cancel.emit_to(tx, false);
    window.end();
    window.set_callback(move |_| tx.send(false));

    {
        let st = state.borrow();
        for s in st.systems.iter() {
            sys.add_choice(&s.name);
        }
        let want = system
            .map(str::to_owned)
            .unwrap_or_else(|| st.settings.last_system.clone());
        let i = st
            .systems
            .iter()
            .position(|s| s.name.eq_ignore_ascii_case(&want))
            .or(if st.systems.is_empty() { None } else { Some(0) });
        sys.set_value(i.map_or(-1, |i| i as i32));
        if let Some(s) = sys.choice().and_then(|n| st.systems.get(&n)) {
            tls.set_checked(s.use_tls);
        }
    }
    {
        let state = state.clone();
        let tls = tls.clone();
        sys.set_callback(move |c| {
            if let Some(s) = c
                .choice()
                .and_then(|n| state.borrow().systems.get(&n).cloned())
            {
                tls.set_checked(s.use_tls);
            }
        });
    }
    (
        StartDialog {
            window,
            system: sys,
            device,
            msgq,
            msgq_lib,
            ccsid,
            target,
            tls,
            status,
        },
        rx,
    )
}

/// Builds the dialog without showing it (for screenshots).
pub fn build(state: &Shared, system: Option<&str>) -> StartDialog {
    build_with(state, system).0
}

/// Runs the dialog over `parent`; on Start, the new printer session.
pub fn run(
    parent: Option<&dyn WindowExt>,
    state: &Shared,
    system: Option<&str>,
) -> Option<PrinterFile> {
    let (mut d, rx) = build_with(state, system);
    center_on(&mut d.window, parent);
    d.window.show();
    while d.window.shown() {
        app::wait();
        match rx.recv() {
            Some(true) => match d.read(state) {
                Ok(f) => {
                    d.window.hide();
                    return Some(f);
                }
                Err(e) => d.status.set(&e, Status::Error),
            },
            Some(false) => d.window.hide(),
            None => {}
        }
    }
    None
}
