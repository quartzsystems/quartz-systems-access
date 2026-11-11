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
use qsa_config::system::{PORT_FIELDS, Ports};
use qsa_config::{AddressLookup, CaCertificate, Invalid, System, Systems};
use qsa_ui::controls::{self, Switch};
use qsa_ui::groupbox::{self, CONTENT_SIDE, CONTENT_TOP};
use qsa_ui::listtable::{Cell, ListTable};
use qsa_ui::statusbar::Status;
use qsa_ui::theme::{self, colors};
use qsa_ui::{MessageBox, PropertyDialog};

use crate::certs::model::{self, Entry, What};
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
    /// The certificates in it, row for row as the Security tab lists them.
    ca_entries: Rc<RefCell<Vec<Entry>>>,
    /// The Security tab's list of them.
    ca_table: ListTable,
    /// Redraws the list from `cas`.
    refill: Rc<dyn Fn()>,
    /// Called after the tab's Add or Remove changed `cas`.
    ca_changed: Changed,
}

/// Shows the certificate of a row of the Security tab over a window.
type ShowRow = dyn Fn(Option<usize>, Option<&dyn WindowExt>);

/// What to call after the Security tab changed its CA list, once set.
type Changed = Rc<RefCell<Option<Rc<dyn Fn()>>>>;

/// Runs what [`SystemForm::on_ca_change`] set, without holding it borrowed.
fn tell(changed: &Changed) {
    let f = changed.borrow().clone();
    if let Some(f) = f {
        f();
    }
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
    crate::help::for_dialog(
        &mut d,
        crate::help::topic::GETTING_STARTED,
        Some(crate::help::SYSTEM_CONFIGURATIONS),
    );
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
    let ca_changed: Changed = Rc::default();
    let mut security = None;
    let ca_entries: Rc<RefCell<Vec<Entry>>> = Rc::default();
    let trusted_for: Vec<String> = Some(system.name.trim().to_owned())
        .filter(|n| !n.is_empty())
        .into_iter()
        .collect();
    d.tabs().add_page("Security", |r| {
        let mut l = controls::label(r.x, r.y, r.w, 18, "Trusted CA certificates:");
        l.set_align(Align::Left | Align::Inside);
        let bw = theme::BUTTON_W;
        let lw = r.w - bw - 8;
        let date_w = 150;
        let mut list = ListTable::new(
            r.x,
            r.y + 22,
            lw,
            r.h - 22,
            &[("Certificate", lw - 4 - date_w), ("Valid to", date_w)],
        );
        let bx = r.x + lw + 8;
        let mut add = controls::button(bx, r.y + 22, bw, theme::BUTTON_H, "Add...");
        let mut remove = controls::button(bx, r.y + 22 + 29, bw, theme::BUTTON_H, "Remove");
        let mut view = controls::button(bx, r.y + 22 + 58, bw, theme::BUTTON_H, "View...");
        let remove_switch = Switch::for_button(&mut remove);
        let view_switch = Switch::for_button(&mut view);

        // One row for each certificate the list holds, as Certificate
        // Management shows them, and one for a file that gives none.
        let fill: Rc<dyn Fn()> = Rc::new({
            let cas = cas.clone();
            let entries = ca_entries.clone();
            let list = list.clone();
            move || {
                let now = crate::certs::clock();
                let found =
                    model::entries(&[(String::new(), cas.borrow().clone())], &model::read_file);
                let rows = found
                    .iter()
                    .map(|e| match &e.what {
                        What::Certificate(c) => vec![
                            Cell::from(c.info.subject.as_str()),
                            Cell {
                                text: c.valid_to(now),
                                status: if c.validity(now) == model::Validity::Expired {
                                    Status::Error
                                } else {
                                    Status::Neutral
                                },
                            },
                        ],
                        // The reason is in Certificate Management, which has
                        // the room for it.
                        What::Unreadable { path, .. } => vec![
                            Cell::from(path.display().to_string()),
                            Cell {
                                text: "File not readable".to_owned(),
                                status: Status::Error,
                            },
                        ],
                    })
                    .collect();
                *entries.borrow_mut() = found;
                list.clone().set_rows(rows);
            }
        });
        {
            let entries = ca_entries.clone();
            let (rs, vs) = (remove_switch.clone(), view_switch.clone());
            list.on_select(move |sel| {
                let entry = sel.and_then(|i| entries.borrow().get(i).cloned());
                rs.set_enabled(entry.is_some());
                vs.set_enabled(entry.is_some_and(|e| e.certificate().is_some()));
            });
        }
        fill();
        remove_switch.set_enabled(false);
        view_switch.set_enabled(false);

        {
            let cas = cas.clone();
            let fill = fill.clone();
            let changed = ca_changed.clone();
            add.set_callback(move |b| {
                let parent = b.window();
                let added = add_certificate(parent.as_deref(), &mut cas.borrow_mut());
                if added {
                    fill();
                    tell(&changed);
                }
            });
        }
        {
            let cas = cas.clone();
            let entries = ca_entries.clone();
            let list2 = list.clone();
            let fill = fill.clone();
            let changed = ca_changed.clone();
            remove.set_callback(move |_| {
                let entry = list2
                    .selected()
                    .and_then(|i| entries.borrow().get(i).cloned());
                if let Some(e) = entry {
                    model::remove_entry(&mut cas.borrow_mut(), &e, &model::read_file);
                    fill();
                    tell(&changed);
                }
            });
        }
        let show: Rc<ShowRow> = Rc::new({
            let entries = ca_entries.clone();
            let trusted_for = trusted_for.clone();
            move |row, parent| {
                let entry = row.and_then(|i| entries.borrow().get(i).cloned());
                if let Some(c) = entry.as_ref().and_then(Entry::certificate) {
                    crate::certs::show_view(parent, c, &trusted_for);
                }
            }
        });
        {
            let list2 = list.clone();
            let show = show.clone();
            view.set_callback(move |b| {
                let parent = b.window();
                show(list2.selected(), parent.as_deref());
            });
        }
        list.on_activate(move |sel| show(sel, None));
        security = Some((list, fill));
    });
    let (ca_table, refill) = security.expect("security page built");

    let form = SystemForm {
        name,
        description,
        use_tls,
        default_user,
        lookup,
        fixed_ip,
        ports,
        cas,
        ca_entries,
        ca_table,
        refill,
        ca_changed,
    };
    (d, form)
}

impl SystemForm {
    /// The CA list the Security tab is editing.
    pub fn ca_list(&self) -> Rc<RefCell<Vec<CaCertificate>>> {
        self.cas.clone()
    }

    /// Redraws the Security tab's list after the CA list was changed from
    /// outside the dialog.
    pub fn refill_cas(&self) -> Rc<dyn Fn()> {
        self.refill.clone()
    }

    /// Calls `f` after the Security tab's Add or Remove changed the CA
    /// list.
    pub fn on_ca_change(&self, f: impl Fn() + 'static) {
        *self.ca_changed.borrow_mut() = Some(Rc::new(f));
    }

    /// Adds an entry to the CA list, as Add does once a file is chosen.
    pub fn add_ca(&self, ca: CaCertificate) {
        self.cas.borrow_mut().push(ca);
        (self.refill)();
        tell(&self.ca_changed);
    }

    /// Takes the certificate of row `row` out of the CA list, as Remove
    /// does for the selected row.
    pub fn remove_ca(&self, row: usize) {
        let entry = self.ca_entries.borrow().get(row).cloned();
        if let Some(e) = entry {
            model::remove_entry(&mut self.cas.borrow_mut(), &e, &model::read_file);
            (self.refill)();
            tell(&self.ca_changed);
        }
    }

    /// What the Security tab's list shows, row by row: each certificate's
    /// subject, or the path of a file that gives none.
    pub fn ca_rows(&self) -> Vec<String> {
        (0..self.ca_table.len())
            .filter_map(|i| self.ca_table.row(i))
            .map(|cells| cells.first().map(|c| c.text.clone()).unwrap_or_default())
            .collect()
    }

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

/// A Security tab sharing the CA list it is editing with Certificate
/// Management, until this is dropped.
pub struct Editing {
    hub: Rc<crate::certs::Hub>,
    system: String,
}

impl Drop for Editing {
    fn drop(&mut self) {
        self.hub.close_editor(&self.system);
        // Certificate Management goes back to the saved list.
        self.hub.notify();
    }
}

/// Shares the CA list `form` is editing for the system saved as `system`
/// with Certificate Management: while the dialog is open that list is the
/// one both show and change, and a change in either shows in the other at
/// once. The dialog's OK saves the list and Cancel drops it, as before.
pub fn share(state: &crate::app::Shared, system: &str, form: &SystemForm) -> Editing {
    let hub = state.borrow().trust.clone();
    hub.open_editor(crate::certs::Editor {
        system: system.to_owned(),
        list: form.ca_list(),
        refill: form.refill_cas(),
    });
    let h = hub.clone();
    form.on_ca_change(move || h.notify());
    hub.notify();
    Editing {
        hub,
        system: system.to_owned(),
    }
}

/// Runs the dialog for `original`, a configured system, with its Security
/// tab in step with Certificate Management ([`share`]). Returns the system
/// to save, or `None` if cancelled.
pub fn edit_shared(
    parent: Option<&dyn WindowExt>,
    state: &crate::app::Shared,
    original: &System,
) -> Option<System> {
    // A copy: the state is not held borrowed while the dialog runs.
    let systems = state.borrow().systems.clone();
    let title = format!("System Properties - {}", original.name);
    let (d, form) = build(&title, original);
    let _editing = share(state, &original.name, &form);
    run_built(parent, &systems, d, form, Some(&original.name))
}

fn run_form(
    parent: Option<&dyn WindowExt>,
    systems: &Systems,
    title: &str,
    initial: &System,
    old_name: Option<&str>,
) -> Option<System> {
    let (d, form) = build(title, initial);
    run_built(parent, systems, d, form, old_name)
}

fn run_built(
    parent: Option<&dyn WindowExt>,
    systems: &Systems,
    mut d: PropertyDialog,
    form: SystemForm,
    old_name: Option<&str>,
) -> Option<System> {
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

/// Add: asks for a certificate file (PEM or DER), then whether to keep a
/// reference to it or its certificates themselves, and adds that to
/// `list`. True when the list changed. A file with no certificate in it is
/// refused with `QAD0043`.
fn add_certificate(parent: Option<&dyn WindowExt>, list: &mut Vec<CaCertificate>) -> bool {
    let Some(path) = files::choose_open(
        "Add CA Certificate",
        "Certificates (*.{pem,crt,cer,der})\tAll Files (*)",
    ) else {
        return false;
    };
    let certs = match std::fs::read(&path) {
        Ok(bytes) => model::importable(&bytes),
        Err(e) => {
            MessageBox::new(
                "Add CA Certificate",
                &format!("{} could not be read: {e}", path.display()),
            )
            .glyph(qsa_ui::Glyph::Cross)
            .run(parent);
            return false;
        }
    };
    if certs.is_empty() {
        let m = qsa_msg::message(
            qsa_msg::id::NOT_A_CERTIFICATE,
            &[&path.display().to_string()],
        );
        crate::log!("{m}");
        qsa_ui::msgdetails::show_message(parent, &m);
        return false;
    }
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
        0 => {
            list.push(CaCertificate::Path(path));
            true
        }
        1 => {
            // Each certificate once: one the list has already is left.
            let mut added = false;
            for c in &certs {
                added |= model::add(list, c, &model::read_file);
            }
            added
        }
        _ => false,
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
