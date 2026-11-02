// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! Session Properties: the settings a session file keeps. Changes to the
//! system, screen size, CCSID, device name and TLS take effect at the next
//! connection; the rest at once.

use fltk::button::CheckButton;
use fltk::enums::Align;
use fltk::input::Input;
use fltk::menu::Choice;
use fltk::prelude::*;
use qsa_config::Systems;
use qsa_config::qsd::{HOTSPOT_FUNCTION_KEYS, HOTSPOT_PAGING, Hotspots, SessionFile};
use qsa_config::settings::{ColorMap, ScreenSize, check_device};
use qsa_ui::PropertyDialog;
use qsa_ui::controls::{self, Switch};
use qsa_ui::groupbox::{self, CONTENT_SIDE, CONTENT_TOP};
use qsa_ui::theme;

use super::start::CCSIDS;

/// Dialog width.
pub const W: i32 = 440;
/// Dialog height.
pub const H: i32 = 456;

/// What the dialog edits.
#[derive(Clone, Debug, PartialEq)]
pub struct Props {
    pub title: String,
    pub system: String,
    pub size: ScreenSize,
    pub ccsid: u16,
    pub device: String,
    pub tls: bool,
    pub color: ColorMap,
    pub auto_connect: bool,
    pub auto_sign_on: bool,
    pub hotspots: Hotspots,
}

impl Props {
    /// The values a session file holds.
    pub fn from_file(f: &SessionFile) -> Props {
        Props {
            title: f.session.title.clone(),
            system: f.system.name.clone(),
            size: f.session.screen_size,
            ccsid: f.session.ccsid,
            device: f.session.device_name.clone(),
            tls: f.system.tls,
            color: f.session.colour_map,
            auto_connect: f.session.auto_connect,
            auto_sign_on: f.session.auto_sign_on,
            hotspots: f.hotspots.clone(),
        }
    }

    /// Writes the values into a session file. The fallback host and port
    /// follow the system when it is configured.
    pub fn apply(&self, f: &mut SessionFile, systems: &Systems) {
        f.session.title = self.title.trim().to_owned();
        if !f.system.name.eq_ignore_ascii_case(&self.system) {
            f.system.name = self.system.clone();
        }
        if let Some(sys) = systems.get(&self.system) {
            let r = qsa_config::qsd::SystemRef::from_system(sys);
            f.system.name = r.name;
            f.system.host = r.host;
            f.system.telnet_port = if self.tls {
                sys.ports.telnet_tls
            } else {
                sys.ports.telnet
            };
        }
        f.system.tls = self.tls;
        f.session.screen_size = self.size;
        f.session.ccsid = self.ccsid;
        f.session.device_name = self.device.clone();
        f.session.colour_map = self.color;
        f.session.auto_connect = self.auto_connect;
        f.session.auto_sign_on = self.auto_sign_on;
        f.hotspots = self.hotspots.clone();
    }
}

/// Every field of the dialog.
pub struct PropsForm {
    title: Input,
    system: Choice,
    size: Choice,
    ccsid: Choice,
    device: Input,
    tls: CheckButton,
    color: Choice,
    auto_connect: CheckButton,
    auto_sign_on: CheckButton,
    hot_on: CheckButton,
    hot_fkeys: CheckButton,
    hot_paging: CheckButton,
    hotspots: Hotspots,
}

impl PropsForm {
    /// The values entered, or what is wrong with them.
    pub fn read(&self) -> Result<Props, String> {
        let system = self.system.choice().ok_or("Select a system.")?;
        let mut hotspots = self.hotspots.clone();
        hotspots.enabled = self.hot_on.is_checked();
        hotspots.set(HOTSPOT_FUNCTION_KEYS, self.hot_fkeys.is_checked());
        hotspots.set(HOTSPOT_PAGING, self.hot_paging.is_checked());
        Ok(Props {
            title: self.title.value().trim().to_owned(),
            system: system.replace("\\/", "/"),
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
            auto_connect: self.auto_connect.is_checked(),
            // Only when it can be used.
            auto_sign_on: self.auto_sign_on.is_checked() && self.auto_sign_on.active(),
            hotspots,
        })
    }
}

/// The label of the automatic sign-on check box.
pub const AUTO_SIGN_ON: &str = "Sign on automatically (QAP ticket, no password)";

/// Builds the dialog for `p`. `systems` fills the System list; a system
/// not configured is listed too, so the session's own value can stay.
/// `qap_version` is the QAP version the session's system speaks, when
/// known: automatic sign-on needs Telnet over TLS and version 2, and is
/// disabled with the reason (`QAD5030`, `QAD5001`) otherwise.
pub fn build(
    title: &str,
    p: &Props,
    systems: &Systems,
    qap_version: Option<u16>,
) -> (PropertyDialog, PropsForm) {
    let mut d = PropertyDialog::new(title, W, H);
    let fh = theme::FIELD_H;
    let mut general = None;
    d.tabs().add_page("General", |r| {
        let lw = 104;
        let fw = r.w - lw;
        let mut y = r.y;
        let mut row = |label: &str| {
            controls::label(r.x, y, lw, fh, label);
            let at = y;
            y += fh + 8;
            at
        };
        let at = row("Title:");
        let mut title = controls::field(r.x + lw, at, fw, fh);
        title.set_value(&p.title);
        title.set_tooltip("Shown in the window title and in Session Manager");
        let at = row("System:");
        let mut system = controls::combo(r.x + lw, at, fw, fh);
        let mut names: Vec<String> = systems.iter().map(|s| s.name.clone()).collect();
        if !p.system.is_empty() && !systems.contains(&p.system) {
            names.insert(0, p.system.clone());
        }
        for n in &names {
            system.add_choice(&n.replace('/', "\\/"));
        }
        let i = names
            .iter()
            .position(|n| n.eq_ignore_ascii_case(&p.system))
            .map_or(-1, |i| i as i32);
        system.set_value(i);
        let at = row("Screen size:");
        let mut size = controls::combo(r.x + lw, at, 120, fh);
        size.add_choice("24 x 80|27 x 132");
        size.set_value(if p.size == ScreenSize::S27x132 { 1 } else { 0 });
        let at = row("CCSID:");
        let mut ccsid = controls::combo(r.x + lw, at, fw, fh);
        for (_, name) in CCSIDS {
            ccsid.add_choice(name);
        }
        let ci = CCSIDS.iter().position(|(c, _)| *c == p.ccsid).unwrap_or(0);
        ccsid.set_value(ci as i32);
        let at = row("Device name:");
        let mut device = controls::field(r.x + lw, at, 120, fh);
        device.set_value(&p.device);
        device.set_maximum_size(10);
        controls::label(r.x + lw + 128, at, fw - 128, fh, "(optional)");
        let at = row("");
        let mut tls = controls::check(r.x + lw, at, fw, fh, "Use TLS");
        tls.set_checked(p.tls);
        let at = row("");
        let mut auto_sign_on = controls::check(r.x + lw, at, fw, fh, AUTO_SIGN_ON);
        auto_sign_on.set_checked(p.auto_sign_on);
        let at = row("");
        let mut why = controls::label(r.x + lw + 20, at - 4, fw - 20, 30, "");
        why.set_align(Align::Left | Align::Top | Align::Inside | Align::Wrap);
        why.set_label_color(theme::colors::DISABLED_TEXT);
        let switch = Switch::for_button(&mut auto_sign_on);
        let system_name = p.system.clone();
        let show = move |tls: bool, why: &mut fltk::frame::Frame| {
            let blocked = crate::qap::auto_sign_on_blocked(&system_name, tls, qap_version);
            switch.set_enabled(blocked.is_none());
            why.set_label(&blocked.map(|m| m.to_string()).unwrap_or_default());
            why.redraw();
        };
        show(p.tls, &mut why);
        tls.set_callback(move |b| show(b.is_checked(), &mut why));
        let at = row("Colour map:");
        let mut color = controls::combo(r.x + lw, at, fw, fh);
        color.add_choice("QS\\/400 console (green on black)|Black on white");
        color.set_value(if p.color == ColorMap::BlackOnWhite { 1 } else { 0 });
        let at = row("");
        let auto_connect = controls::check(r.x + lw, at, fw, fh, "Connect when the session opens");
        auto_connect.set_checked(p.auto_connect);
        let mut note = controls::label(
            r.x,
            y + 6,
            r.w,
            34,
            "Changes to the system, screen size, CCSID, device name and TLS take effect at the next connection.",
        );
        note.set_align(Align::Left | Align::Top | Align::Inside | Align::Wrap);
        general = Some((
            title,
            system,
            size,
            ccsid,
            device,
            tls,
            auto_sign_on,
            color,
            auto_connect,
        ));
    });
    let (title, system, size, ccsid, device, tls, auto_sign_on, color, auto_connect) =
        general.expect("general page built");

    let mut hot = None;
    d.tabs().add_page("Hotspots", |r| {
        let mut hot_on = controls::check(r.x, r.y, r.w, fh, "Act on text clicked on the screen");
        hot_on.set_checked(p.hotspots.enabled);
        let gy = r.y + fh + 10;
        let g = groupbox::group_box(r.x, gy, r.w, CONTENT_TOP + 2 * 24 + 6, "Kinds of text");
        let x = r.x + CONTENT_SIDE;
        let w = r.w - 2 * CONTENT_SIDE;
        let mut fkeys = controls::check(
            x,
            gy + CONTENT_TOP,
            w,
            fh,
            "Function keys, such as F3=Exit: send the key",
        );
        fkeys.set_checked(p.hotspots.kinds.iter().any(|k| k == HOTSPOT_FUNCTION_KEYS));
        let mut paging = controls::check(
            x,
            gy + CONTENT_TOP + 24,
            w,
            fh,
            "More... and Bottom: page down or up",
        );
        paging.set_checked(p.hotspots.kinds.iter().any(|k| k == HOTSPOT_PAGING));
        g.end();
        let mut note = controls::label(
            r.x,
            gy + CONTENT_TOP + 2 * 24 + 16,
            r.w,
            50,
            "Only protected text on the screen is looked at, never what is in an input field. The pointer becomes a hand over text that can be clicked.",
        );
        note.set_align(Align::Left | Align::Top | Align::Inside | Align::Wrap);
        let s1 = Switch::for_button(&mut fkeys);
        let s2 = Switch::for_button(&mut paging);
        let on = p.hotspots.enabled;
        s1.set_enabled(on);
        s2.set_enabled(on);
        hot_on.set_callback(move |b| {
            s1.set_enabled(b.is_checked());
            s2.set_enabled(b.is_checked());
        });
        hot = Some((hot_on, fkeys, paging));
    });
    let (hot_on, hot_fkeys, hot_paging) = hot.expect("hotspots page built");
    (
        d,
        PropsForm {
            title,
            system,
            size,
            ccsid,
            device,
            tls,
            color,
            auto_connect,
            auto_sign_on,
            hot_on,
            hot_fkeys,
            hot_paging,
            hotspots: p.hotspots.clone(),
        },
    )
}

/// Runs the dialog over `parent`. `Some` with the new values on OK.
pub fn edit(
    parent: Option<&dyn WindowExt>,
    title: &str,
    p: &Props,
    systems: &Systems,
    qap_version: Option<u16>,
) -> Option<Props> {
    let (mut d, form) = build(title, p, systems, qap_version);
    let mut result = None;
    let ok = d.run(parent, || {
        result = Some(form.read()?);
        Ok(())
    });
    if ok { result } else { None }
}

#[cfg(test)]
mod tests {
    use super::*;
    use qsa_config::System;

    #[test]
    fn apply_follows_the_configured_system() {
        let mut systems = Systems::new();
        let mut sys = System::new("qs400.example.net");
        sys.fixed_ip = "10.42.70.150".into();
        sys.ports.telnet_tls = 9992;
        systems.upsert(sys);
        let mut f = SessionFile::new(&System::new("old"));
        let mut p = Props::from_file(&f);
        p.system = "QS400.EXAMPLE.NET".into();
        p.title = " Payroll ".into();
        p.tls = true;
        p.hotspots.enabled = false;
        p.apply(&mut f, &systems);
        assert_eq!(f.system.name, "qs400.example.net");
        assert_eq!(f.system.host, "10.42.70.150");
        assert_eq!(f.system.telnet_port, 9992);
        assert!(f.system.tls);
        assert_eq!(f.session.title, "Payroll");
        assert!(!f.hotspots.enabled);
        assert_eq!(Props::from_file(&f).title, "Payroll");
    }
}
