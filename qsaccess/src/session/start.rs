// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! The Start 5250 Display dialog: system, screen size, CCSID, device name,
//! TLS and colour map. It opens with the selected system and the values last
//! used for it, and remembers them per system in `settings.toml`.

use std::cell::RefCell;
use std::rc::Rc;

use fltk::app;
use fltk::button::CheckButton;
use fltk::input::Input;
use fltk::menu::Choice;
use fltk::prelude::*;
use fltk::window::Window;
use qsa_config::settings::{ColorMap, Display, ScreenSize};
use qsa_ui::controls;
use qsa_ui::propdialog::center_on;
use qsa_ui::statusbar::{Status, StatusLine};
use qsa_ui::theme::{self, colors};

use crate::app::Shared;

pub const W: i32 = 400;
pub const H: i32 = 284;

/// The CCSIDs offered, with their names.
pub const CCSIDS: &[(u16, &str)] = &[
    (37, "37 - USA, Canada"),
    (273, "273 - Austria, Germany"),
    (285, "285 - United Kingdom"),
    (297, "297 - France"),
    (500, "500 - International"),
    (1140, "1140 - USA, Canada, euro"),
];

/// What the dialog asks for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StartOptions {
    pub system: String,
    pub size: ScreenSize,
    pub ccsid: u16,
    /// Empty: the system chooses.
    pub device: String,
    pub tls: bool,
    pub color: ColorMap,
}

pub use qsa_config::settings::check_device;

#[derive(Clone)]
struct Form {
    system: Choice,
    size: Choice,
    ccsid: Choice,
    device: Input,
    tls: CheckButton,
    color: Choice,
}

/// The dialog's widgets.
pub struct StartDialog {
    pub window: Window,
    form: Form,
    status: StatusLine,
}

/// The values to show for a system: its last ones, TLS from its
/// configuration.
fn options_for(state: &Shared, system: &str) -> StartOptions {
    let st = state.borrow();
    let d = st.settings.display_for(system);
    let tls = st.systems.get(system).is_some_and(|s| s.use_tls);
    StartOptions {
        system: system.to_owned(),
        size: d.screen_size,
        ccsid: d.ccsid,
        device: d.device_name,
        tls,
        color: d.color_map,
    }
}

impl Form {
    fn show(&mut self, o: &StartOptions) {
        self.size.set_value(match o.size {
            ScreenSize::S24x80 => 0,
            ScreenSize::S27x132 => 1,
        });
        let ci = CCSIDS.iter().position(|(c, _)| *c == o.ccsid).unwrap_or(0);
        self.ccsid.set_value(ci as i32);
        self.device.set_value(&o.device);
        self.tls.set_checked(o.tls);
        self.color.set_value(match o.color {
            ColorMap::Console => 0,
            ColorMap::BlackOnWhite => 1,
        });
    }

    fn read(&self) -> Result<StartOptions, String> {
        let system = self.system.choice().ok_or("Select a system.")?;
        Ok(StartOptions {
            system,
            size: if self.size.value() == 1 {
                ScreenSize::S27x132
            } else {
                ScreenSize::S24x80
            },
            ccsid: CCSIDS
                .get(self.ccsid.value().max(0) as usize)
                .map_or(37, |(c, _)| *c),
            device: check_device(&self.device.value())?,
            tls: self.tls.is_checked(),
            color: if self.color.value() == 1 {
                ColorMap::BlackOnWhite
            } else {
                ColorMap::Console
            },
        })
    }
}

fn build_with(state: &Shared, system: Option<&str>) -> (StartDialog, app::Receiver<bool>) {
    let (tx, rx) = app::channel::<bool>();
    let m = 12;
    let (lw, fh) = (112, theme::FIELD_H);
    let fw = W - 2 * m - lw;
    let mut window = Window::new(0, 0, W, H, None);
    window.set_label("Start 5250 Display");
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
    let at = row(&mut y, "Screen size:");
    let mut size = controls::combo(m + lw, at, 140, fh);
    size.add_choice("24 x 80|27 x 132");
    let at = row(&mut y, "CCSID:");
    let mut ccsid = controls::combo(m + lw, at, fw, fh);
    for (_, name) in CCSIDS {
        ccsid.add_choice(name);
    }
    let at = row(&mut y, "Device name:");
    let device = controls::field(m + lw, at, 120, fh);
    controls::label(m + lw + 128, at, fw - 128, fh, "(optional)");
    let at = row(&mut y, "");
    let tls = controls::check(m + lw, at, fw, fh, "Use TLS");
    let at = row(&mut y, "Colour map:");
    let mut color = controls::combo(m + lw, at, fw, fh);
    color.add_choice("QS\\/400 console (green on black)|Black on white");

    let bh = theme::BUTTON_H;
    let bw = theme::BUTTON_W;
    let status = StatusLine::new(m, H - m - bh - bh - 10, W - 2 * m, bh);
    let mut ok = controls::default_button(W - 2 * m - 2 * bw + m / 2, H - m - bh, bw, bh, "Start");
    ok.emit(tx, true);
    let mut cancel = controls::button(W - m - bw, H - m - bh, bw, bh, "Cancel");
    cancel.emit(tx, false);
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
    }
    let mut form = Form {
        system: sys.clone(),
        size,
        ccsid,
        device,
        tls,
        color,
    };
    if let Some(name) = sys.choice() {
        form.show(&options_for(state, &name));
    }
    {
        let state = state.clone();
        let f = Rc::new(RefCell::new(form.clone()));
        sys.set_callback(move |c| {
            if let Some(name) = c.choice() {
                f.borrow_mut().show(&options_for(&state, &name));
            }
        });
    }
    (
        StartDialog {
            window,
            form,
            status,
        },
        rx,
    )
}

/// Builds the dialog without showing it (for screenshots).
pub fn build(state: &Shared, system: Option<&str>) -> StartDialog {
    build_with(state, system).0
}

/// Runs the dialog over `parent`. On Start, remembers the values for the
/// system and returns them.
pub fn run(
    parent: Option<&dyn WindowExt>,
    state: &Shared,
    system: Option<&str>,
) -> Option<StartOptions> {
    let (mut d, rx) = build_with(state, system);
    center_on(&mut d.window, parent);
    d.window.show();
    while d.window.shown() {
        app::wait();
        match rx.recv() {
            Some(true) => match d.form.read() {
                Ok(o) => {
                    remember(state, &o);
                    d.window.hide();
                    return Some(o);
                }
                Err(e) => d.status.set(&e, Status::Error),
            },
            Some(false) => d.window.hide(),
            None => {}
        }
    }
    None
}

/// Keeps the options as the system's last values.
fn remember(state: &Shared, o: &StartOptions) {
    let mut st = state.borrow_mut();
    st.settings.set_display(
        &o.system,
        Display {
            screen_size: o.size,
            ccsid: o.ccsid,
            device_name: o.device.clone(),
            color_map: o.color,
            legacy_keymap: None,
        },
    );
    if let Err(e) = st.save_settings() {
        crate::log!("settings not saved: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_ccsid_offered_is_built_in() {
        for (c, _) in CCSIDS {
            assert!(tn5250::codepage::by_ccsid(*c).is_some(), "{c}");
        }
        let offered: Vec<u16> = CCSIDS.iter().map(|(c, _)| *c).collect();
        assert_eq!(offered, qsa_config::settings::CCSIDS);
    }
}
