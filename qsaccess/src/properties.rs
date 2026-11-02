// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! The system properties dialog: General, Connection, Ports and Security.

use std::cell::RefCell;
use std::rc::Rc;

use fltk::button::CheckButton;
use fltk::enums::{Align, FrameType};
use fltk::input::{Input, IntInput};
use fltk::menu::Choice;
use fltk::prelude::*;
use fltk::text::{TextBuffer, TextDisplay};
use fltk::window::Window;
use qsa_config::system::{PORT_FIELDS, Ports};
use qsa_config::{AddressLookup, CaCertificate, Invalid, System, Systems};
use qsa_ui::controls::{self, Switch};
use qsa_ui::groupbox::{self, CONTENT_SIDE, CONTENT_TOP};
use qsa_ui::listtable::{Cell, ListTable};
use qsa_ui::propdialog::center_on;
use qsa_ui::theme::{self, colors};
use qsa_ui::{MessageBox, PropertyDialog};

use crate::files;

/// Dialog width.
pub const W: i32 = 470;
/// Dialog height.
pub const H: i32 = 440;

/// Tab indexes.
pub const TAB_GENERAL: usize = 0;
pub const TAB_CONNECTION: usize = 1;
pub const TAB_PORTS: usize = 2;
pub const TAB_SECURITY: usize = 3;

/// Every field of the dialog.
pub struct SystemForm {
    name: Input,
    description: Input,
    use_tls: CheckButton,
    default_user: Input,
    lookup: Choice,
    fixed_ip: Input,
    ports: Vec<(usize, IntInput)>,
    cas: Rc<RefCell<Vec<CaCertificate>>>,
}

/// The Connection tab's note under the default user ID.
pub const SIGN_ON_NOTE: &str = "Passwords are never saved. QS/400 Access signs on to this \
     system once each time it runs, when a task first needs it, and asks again only \
     when that session ends.";

const LOOKUPS: [(AddressLookup, &str); 3] = [
    (AddressLookup::Always, "Always"),
    (AddressLookup::Daily, "Daily"),
    (AddressLookup::Never, "Never"),
];

fn port_field(x: i32, y: i32, value: u16) -> IntInput {
    let mut i = IntInput::new(x, y, 64, theme::FIELD_H, None);
    i.set_frame(FrameType::DownBox);
    i.set_color(colors::WINDOW);
    i.set_text_size(theme::UI_SIZE);
    i.set_selection_color(colors::SELECTION);
    i.set_maximum_size(5);
    i.set_value(&value.to_string());
    i
}

/// Rows of the Ports tab: (service, plain field index, TLS field index).
/// QAP has only a TLS port: it never runs without TLS.
fn port_rows() -> Vec<(&'static str, Option<usize>, Option<usize>)> {
    let mut rows: Vec<(&str, Option<usize>, Option<usize>)> = Vec::new();
    for (i, f) in PORT_FIELDS.iter().enumerate() {
        match f.label.strip_suffix(" (TLS)") {
            Some(service) if rows.last().is_some_and(|r| r.0 == service) => {
                rows.last_mut().expect("checked").2 = Some(i);
            }
            Some(service) => rows.push((service, None, Some(i))),
            None if f.label == "QAP" => rows.push((f.label, None, Some(i))),
            None => rows.push((f.label, Some(i), None)),
        }
    }
    rows
}

/// Builds the dialog for `system`. It is not shown.
pub fn build(title: &str, system: &System) -> (PropertyDialog, SystemForm) {
    let mut d = PropertyDialog::new(title, W, H);
    let fh = theme::FIELD_H;

    // General.
    let mut general = None;
    d.tabs().add_page("General", |r| {
        let lw = 90;
        controls::label(r.x, r.y, lw, fh, "System name:");
        let mut name = controls::field(r.x + lw, r.y, r.w - lw, fh);
        name.set_value(&system.name);
        name.set_tooltip("Host name or IP address of the QS/400 system");
        controls::label(r.x, r.y + 28, lw, fh, "Description:");
        let mut description = controls::field(r.x + lw, r.y + 28, r.w - lw, fh);
        description.set_value(&system.description);
        let use_tls = controls::check(r.x + lw, r.y + 58, 220, fh, "Use TLS for all connections");
        use_tls.set_checked(system.use_tls);
        // The space below is kept for connection verification (a later phase).
        general = Some((name, description, use_tls));
    });
    let (name, description, use_tls) = general.expect("general page built");

    // Connection.
    let mut connection = None;
    d.tabs().add_page("Connection", |r| {
        let gh = CONTENT_TOP + 28 + 3 * 16 + 10;
        let g = groupbox::group_box(r.x, r.y, r.w, gh, "Sign-on");
        let x = r.x + CONTENT_SIDE;
        let y = r.y + CONTENT_TOP;
        controls::label(x, y, 100, fh, "Default user ID:");
        let mut default_user = controls::field(x + 100, y, 150, fh);
        default_user.set_value(&system.default_user);
        default_user.set_maximum_size(128);
        default_user.set_tooltip("The user ID the Sign On dialog starts with");
        let mut note = controls::label(x, y + 28, r.w - 2 * CONTENT_SIDE, 3 * 16, SIGN_ON_NOTE);
        note.set_align(Align::Left | Align::Top | Align::Inside | Align::Wrap);
        g.end();

        let ay = r.y + gh + 10;
        let a = groupbox::group_box(r.x, ay, r.w, CONTENT_TOP + 2 * 28 + 6, "Address");
        let lw = 120;
        controls::label(x, ay + CONTENT_TOP, lw, fh, "Look up address:");
        let mut lookup = controls::combo(x + lw, ay + CONTENT_TOP, 110, fh);
        for (_, l) in LOOKUPS {
            lookup.add_choice(l);
        }
        let li = LOOKUPS
            .iter()
            .position(|(l, _)| *l == system.address_lookup)
            .unwrap_or(0);
        lookup.set_value(li as i32);
        controls::label(x, ay + CONTENT_TOP + 28, lw, fh, "Fixed IP address:");
        let mut fixed_ip = controls::field(x + lw, ay + CONTENT_TOP + 28, 160, fh);
        fixed_ip.set_value(&system.fixed_ip);
        a.end();

        connection = Some((default_user, lookup, fixed_ip));
    });
    let (default_user, mut lookup, mut fixed_ip) = connection.expect("connection page built");
    let ip_switch = Switch::for_input(&mut fixed_ip);
    ip_switch.set_enabled(system.address_lookup == AddressLookup::Never);
    lookup.set_callback(move |c| ip_switch.set_enabled(c.value() == 2));

    // Ports.
    let mut ports = Vec::new();
    d.tabs().add_page("Ports", |r| {
        let (c1, c2, c3) = (r.x, r.x + 150, r.x + 230);
        let head = |x: i32, t: &str| {
            let mut l = controls::label(x, r.y, 80, 18, t);
            l.set_label_font(theme::UI_FONT_BOLD);
        };
        head(c1, "Service");
        head(c2, "Port");
        head(c3, "TLS port");
        let mut y = r.y + 22;
        for (service, plain, tls) in port_rows() {
            controls::label(c1, y, 140, fh, service);
            for (col, idx) in [(c2, plain), (c3, tls)] {
                if let Some(i) = idx {
                    let f = port_field(col, y, (PORT_FIELDS[i].get)(&system.ports));
                    ports.push((i, f));
                }
            }
            y += fh + 3;
        }
        let mut note = controls::label(
            c1,
            y + 6,
            r.w,
            2 * 18,
            "QAP always uses TLS. 5250 sessions use the Telnet TLS port
when Use TLS for all connections is selected.",
        );
        note.set_align(Align::Left | Align::Inside | Align::Top);
        let mut reset = controls::button(
            r.x + r.w - 120,
            r.y + r.h - theme::BUTTON_H,
            120,
            theme::BUTTON_H,
            "Reset to defaults",
        );
        let fields: Vec<(usize, IntInput)> = ports.clone();
        reset.set_callback(move |_| {
            let defaults = Ports::default();
            for (i, f) in &fields {
                let mut f = f.clone();
                f.set_value(&(PORT_FIELDS[*i].get)(&defaults).to_string());
            }
        });
    });

    // Security.
    let cas = Rc::new(RefCell::new(system.ca_certificates.clone()));
    d.tabs().add_page("Security", |r| {
        let mut l = controls::label(r.x, r.y, r.w, 18, "Trusted CA certificates:");
        l.set_align(Align::Left | Align::Inside);
        let bw = theme::BUTTON_W;
        let lw = r.w - bw - 8;
        let mut list = ListTable::new(r.x, r.y + 22, lw, r.h - 22, &[("Certificate", lw - 4)]);
        let bx = r.x + lw + 8;
        let mut add = controls::button(bx, r.y + 22, bw, theme::BUTTON_H, "Add...");
        let mut remove = controls::button(bx, r.y + 22 + 29, bw, theme::BUTTON_H, "Remove");
        let mut view = controls::button(bx, r.y + 22 + 58, bw, theme::BUTTON_H, "View");
        let remove_switch = Switch::for_button(&mut remove);
        let view_switch = Switch::for_button(&mut view);

        let fill = {
            let cas = cas.clone();
            let list = list.clone();
            move || {
                let rows = cas
                    .borrow()
                    .iter()
                    .map(|c| vec![Cell::from(c.summary())])
                    .collect();
                list.clone().set_rows(rows);
            }
        };
        let (rs, vs) = (remove_switch.clone(), view_switch.clone());
        list.on_select(move |sel| {
            rs.set_enabled(sel.is_some());
            vs.set_enabled(sel.is_some());
        });
        fill();
        remove_switch.set_enabled(false);
        view_switch.set_enabled(false);

        {
            let cas = cas.clone();
            let fill = fill.clone();
            add.set_callback(move |b| {
                let parent = b.window();
                if let Some(ca) = add_certificate(parent.as_deref()) {
                    cas.borrow_mut().push(ca);
                    fill();
                }
            });
        }
        {
            let cas = cas.clone();
            let list2 = list.clone();
            let fill = fill.clone();
            remove.set_callback(move |_| {
                if let Some(i) = list2.selected() {
                    cas.borrow_mut().remove(i);
                    fill();
                }
            });
        }
        {
            let cas = cas.clone();
            let list2 = list.clone();
            view.set_callback(move |b| {
                if let Some(ca) = list2.selected().and_then(|i| cas.borrow().get(i).cloned()) {
                    view_certificate(b.window().as_deref(), &ca);
                }
            });
        }
        let cas2 = cas.clone();
        list.on_activate(move |sel| {
            if let Some(ca) = sel.and_then(|i| cas2.borrow().get(i).cloned()) {
                view_certificate(None, &ca);
            }
        });
    });

    let form = SystemForm {
        name,
        description,
        use_tls,
        default_user,
        lookup,
        fixed_ip,
        ports,
        cas,
    };
    (d, form)
}

impl SystemForm {
    /// Reads the fields into a system. On a bad value, gives the tab to show
    /// and the error text.
    pub fn read(&self) -> Result<System, (usize, String)> {
        let mut s = System {
            name: self.name.value().trim().to_owned(),
            description: self.description.value().trim().to_owned(),
            use_tls: self.use_tls.is_checked(),
            default_user: self.default_user.value().trim().to_owned(),
            fixed_ip: self.fixed_ip.value().trim().to_owned(),
            ca_certificates: self.cas.borrow().clone(),
            ..System::default()
        };
        s.address_lookup = LOOKUPS
            .get(self.lookup.value().max(0) as usize)
            .map_or(AddressLookup::Always, |(l, _)| *l);
        for (i, f) in &self.ports {
            let field = &PORT_FIELDS[*i];
            match f.value().trim().parse::<u16>() {
                Ok(v) if v > 0 => (field.set)(&mut s.ports, v),
                _ => return Err((TAB_PORTS, Invalid::PortZero(field.label).to_string())),
            }
        }
        Ok(s)
    }
}

/// The tab that holds the field an error is about.
pub fn tab_for(e: &Invalid) -> usize {
    match e {
        Invalid::NameMissing | Invalid::NameNotHost | Invalid::NameTaken(_) => TAB_GENERAL,
        Invalid::DefaultUserSpaces | Invalid::FixedIpMissing | Invalid::FixedIpBad(_) => {
            TAB_CONNECTION
        }
        Invalid::PortZero(_) | Invalid::PortShared(..) => TAB_PORTS,
        Invalid::CaEmpty => TAB_SECURITY,
    }
}

/// Runs the dialog. `original` is the system being edited, or `None` for a
/// new one. Returns the system to save, or `None` if cancelled.
pub fn edit(
    parent: Option<&dyn WindowExt>,
    systems: &Systems,
    original: Option<&System>,
) -> Option<System> {
    let blank = System::default();
    let title = match original {
        Some(s) => format!("System Properties - {}", s.name),
        None => "New System Configuration".to_owned(),
    };
    run_form(
        parent,
        systems,
        &title,
        original.unwrap_or(&blank),
        original.map(|s| s.name.as_str()),
    )
}

/// Runs the dialog for a new system filled in from `template` (for "Create
/// system from session file"). Returns the system to add, or `None`.
pub fn create(
    parent: Option<&dyn WindowExt>,
    systems: &Systems,
    template: &System,
) -> Option<System> {
    run_form(parent, systems, "New System Configuration", template, None)
}

fn run_form(
    parent: Option<&dyn WindowExt>,
    systems: &Systems,
    title: &str,
    initial: &System,
    old_name: Option<&str>,
) -> Option<System> {
    let (mut d, form) = build(title, initial);
    let mut result = None;
    let mut tabs = d.tabs().clone();
    let ok = d.run(parent, || {
        let s = form.read().map_err(|(tab, e)| {
            tabs.select(tab);
            e
        })?;
        systems.check(old_name, &s).map_err(|e| {
            tabs.select(tab_for(&e));
            e.to_string()
        })?;
        result = Some(s);
        Ok(())
    });
    if ok { result } else { None }
}

/// Asks for a PEM file, then whether to reference or embed it.
fn add_certificate(parent: Option<&dyn WindowExt>) -> Option<CaCertificate> {
    let path = files::choose_open(
        "Add CA Certificate",
        "Certificates (*.{pem,crt,cer})\tAll Files (*)",
    )?;
    let answer = MessageBox::new(
        "Add CA Certificate",
        &format!(
            "Keep a reference to {}, or embed the certificate in the system configuration?\n\n\
             An embedded certificate travels with exported configurations.",
            path.display()
        ),
    )
    .buttons(&["Reference", "Embed", "Cancel"], 0, 2)
    .run(parent);
    match answer {
        0 => Some(CaCertificate::Path(path)),
        1 => match std::fs::read_to_string(&path) {
            Ok(pem) if pem.contains("-----BEGIN CERTIFICATE-----") => Some(CaCertificate::Pem(pem)),
            Ok(_) => {
                MessageBox::new(
                    "Add CA Certificate",
                    &format!("{} does not hold a PEM certificate.", path.display()),
                )
                .run(parent);
                None
            }
            Err(e) => {
                MessageBox::new("Add CA Certificate", &format!("{}: {e}", path.display()))
                    .run(parent);
                None
            }
        },
        _ => None,
    }
}

/// Shows a certificate's PEM text.
fn view_certificate(parent: Option<&dyn WindowExt>, ca: &CaCertificate) {
    let (title, text) = match ca {
        CaCertificate::Path(p) => (
            p.display().to_string(),
            std::fs::read_to_string(p)
                .unwrap_or_else(|e| format!("The file could not be read: {e}")),
        ),
        CaCertificate::Pem(pem) => ("Embedded certificate".to_owned(), pem.clone()),
    };
    let (w, h) = (520, 360);
    let mut win = Window::new(0, 0, w, h, None);
    win.set_label(&format!("View Certificate - {title}"));
    win.set_color(colors::FACE);
    win.make_modal(true);
    let mut buf = TextBuffer::default();
    buf.set_text(&text);
    let mut td = TextDisplay::new(10, 10, w - 20, h - 20 - theme::BUTTON_H - 8, None);
    td.set_buffer(buf);
    td.set_frame(FrameType::DownBox);
    td.set_text_font(theme::MONO_FONT);
    td.set_text_size(theme::MONO_SIZE);
    let mut close = controls::default_button(
        w - 10 - theme::BUTTON_W,
        h - 10 - theme::BUTTON_H,
        theme::BUTTON_W,
        theme::BUTTON_H,
        "Close",
    );
    win.end();
    let mut wc = win.clone();
    close.set_callback(move |_| wc.hide());
    center_on(&mut win, parent);
    win.show();
    while win.shown() {
        fltk::app::wait();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn port_rows_pair_plain_and_tls() {
        let rows = port_rows();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0], ("Telnet", Some(0), Some(1)));
        assert_eq!(rows[1], ("QAP", None, Some(2)));
        let fields: usize = rows
            .iter()
            .map(|r| r.1.is_some() as usize + r.2.is_some() as usize)
            .sum();
        assert_eq!(fields, PORT_FIELDS.len());
    }
}
