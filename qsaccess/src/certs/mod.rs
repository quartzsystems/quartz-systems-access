// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! Certificate Management: every certificate trusted in any system's
//! configuration, one row per certificate, with the systems that trust it.
//!
//! - It edits the same per-system CA lists as the Security tab of each
//!   system's properties. While a System Properties dialog is open, the
//!   list it is editing is the one shown and changed here, and what either
//!   changes shows in the other at once ([`Hub`]).
//! - QAP and Telnet TLS authenticate the system only: no client key or
//!   client certificate is ever used, so there are none to manage.
//! - The operating system's certificate store is trusted for every system
//!   too. It is managed by the operating system; the This computer tab
//!   only says how many certificates it supplies.

pub mod model;
pub mod sample;

use std::cell::{Cell, RefCell};
use std::path::Path;
use std::rc::Rc;
use std::time::{SystemTime, UNIX_EPOCH};

use fltk::app;
use fltk::button::CheckButton;
use fltk::enums::{Align, FrameType};
use fltk::frame::Frame;
use fltk::group::Scroll;
use fltk::prelude::*;
use fltk::text::{TextBuffer, TextDisplay};
use fltk::window::Window;
use qsa_config::system::CaCertificate;
use qsa_msg::{Message, id};
use qsa_ui::chan::{self, Emit};
use qsa_ui::controls::{self, Switch};
use qsa_ui::listtable::{Cell as ListCell, ListTable};
use qsa_ui::propdialog::{center_on, show_placed};
use qsa_ui::statusbar::{Status, StatusLine};
use qsa_ui::theme::{self, colors};
use qsa_ui::{Glyph, MessageBox, Tabs, msgdetails};

use crate::app::Shared;
use crate::files;
use model::{Certificate, Entry, Validity, What};

/// The function's name, in titles, the task pane and the Tools menu.
pub const FUNCTION: &str = "Certificate Management";

/// Window width.
pub const W: i32 = 980;
/// Window height.
pub const H: i32 = 430;

/// Tab indexes.
pub const TAB_CERTIFICATES: usize = 0;
pub const TAB_THIS_COMPUTER: usize = 1;

/// Columns of the list: title and width.
pub const COLUMNS: [(&str, i32); 6] = [
    ("Subject", 196),
    ("Issuer", 170),
    ("Valid from", 76),
    ("Valid to", 150),
    ("SHA-256 fingerprint", 150),
    ("Trusted for", 110),
];
/// The fingerprint's column, drawn in the system-output font.
const FINGERPRINT_COLUMN: usize = 4;

// ---------------------------------------------------------------------
// The CA lists both views edit.

/// A System Properties dialog's Security tab, while the dialog is open: the
/// CA list it is editing, and how to redraw it after a change made here.
#[derive(Clone)]
pub struct Editor {
    /// The system's name as it is saved.
    pub system: String,
    pub list: Rc<RefCell<Vec<CaCertificate>>>,
    pub refill: Rc<dyn Fn()>,
}

/// Something to call when a CA list changes.
type Hook = Rc<dyn Fn()>;

/// Keeps Certificate Management and the Security tabs in step: who is
/// editing which system's CA list, and who wants to hear of a change.
#[derive(Default)]
pub struct Hub {
    editors: RefCell<Vec<Editor>>,
    listeners: RefCell<Vec<(u64, Hook)>>,
    next: Cell<u64>,
}

impl Hub {
    /// Calls `f` after every change to a CA list or to the systems. The
    /// number takes it off again ([`Hub::unlisten`]).
    pub fn listen(&self, f: impl Fn() + 'static) -> u64 {
        let n = self.next.get() + 1;
        self.next.set(n);
        self.listeners.borrow_mut().push((n, Rc::new(f)));
        n
    }

    pub fn unlisten(&self, listener: u64) {
        self.listeners.borrow_mut().retain(|(n, _)| *n != listener);
    }

    /// Tells every listener that a CA list, or the systems, changed.
    pub fn notify(&self) {
        let listeners: Vec<Hook> = self
            .listeners
            .borrow()
            .iter()
            .map(|(_, f)| f.clone())
            .collect();
        for f in listeners {
            f();
        }
    }

    /// A Security tab starts editing a system's CA list.
    pub fn open_editor(&self, editor: Editor) {
        self.close_editor(&editor.system);
        self.editors.borrow_mut().push(editor);
    }

    /// The Security tab editing `system`'s CA list has gone.
    pub fn close_editor(&self, system: &str) {
        self.editors
            .borrow_mut()
            .retain(|e| !e.system.eq_ignore_ascii_case(system));
    }

    /// The Security tab editing `system`'s CA list, if one is open.
    pub fn editor(&self, system: &str) -> Option<Editor> {
        self.editors
            .borrow()
            .iter()
            .find(|e| e.system.eq_ignore_ascii_case(system))
            .cloned()
    }
}

/// Tells Certificate Management that a CA list or the systems changed.
pub fn changed(state: &Shared) {
    let hub = state.borrow().trust.clone();
    hub.notify();
}

/// Each system's CA list as it stands: the one an open System Properties
/// dialog is editing, else the saved one.
pub fn lists(state: &Shared) -> Vec<(String, Vec<CaCertificate>)> {
    let st = state.borrow();
    st.systems
        .iter()
        .map(|s| {
            let list = match st.trust.editor(&s.name) {
                Some(e) => e.list.borrow().clone(),
                None => s.ca_certificates.clone(),
            };
            (s.name.clone(), list)
        })
        .collect()
}

/// Changes the CA lists of `systems` with `change` (true when it changed
/// the list it was given): the list an open System Properties dialog is
/// editing, which that dialog saves or drops, else the saved one, written
/// to the systems file here. Returns the systems whose lists changed.
pub fn apply(
    state: &Shared,
    systems: &[String],
    change: impl Fn(&mut Vec<CaCertificate>) -> bool,
) -> std::io::Result<Vec<String>> {
    let hub = state.borrow().trust.clone();
    let mut done = Vec::new();
    let mut save = false;
    for name in systems {
        if let Some(editor) = hub.editor(name) {
            let did = change(&mut editor.list.borrow_mut());
            if did {
                (editor.refill)();
                done.push(name.clone());
            }
            continue;
        }
        let mut st = state.borrow_mut();
        let Some(mut sys) = st.systems.get(name).cloned() else {
            continue;
        };
        if change(&mut sys.ca_certificates) {
            st.systems.upsert(sys);
            save = true;
            done.push(name.clone());
        }
    }
    let saved = if save {
        state.borrow().save_systems()
    } else {
        Ok(())
    };
    hub.notify();
    saved.map(|()| done)
}

/// Adds a certificate to the CA lists of `systems`. Returns the systems it
/// was added to (the others trusted it already).
pub fn import(
    state: &Shared,
    cert: &Certificate,
    systems: &[String],
) -> std::io::Result<Vec<String>> {
    apply(state, systems, |list| {
        model::add(list, cert, &model::read_file)
    })
}

/// Takes a row out of the CA list of every system that has it. Returns the
/// systems that stopped trusting it.
pub fn remove(state: &Shared, entry: &Entry) -> std::io::Result<Vec<String>> {
    apply(state, &entry.systems, |list| {
        model::remove_entry(list, entry, &model::read_file)
    })
}

/// Makes `wanted` the systems that trust a certificate. Returns the
/// systems it was added to and those it was removed from.
pub fn set_trusted_for(
    state: &Shared,
    entry: &Entry,
    wanted: &[String],
) -> std::io::Result<(Vec<String>, Vec<String>)> {
    let Some(cert) = entry.certificate() else {
        return Ok((Vec::new(), Vec::new()));
    };
    let has = |list: &[String], name: &String| list.iter().any(|s| s.eq_ignore_ascii_case(name));
    let add: Vec<String> = wanted
        .iter()
        .filter(|w| !has(&entry.systems, w))
        .cloned()
        .collect();
    let drop: Vec<String> = entry
        .systems
        .iter()
        .filter(|s| !has(wanted, s))
        .cloned()
        .collect();
    let added = import(state, cert, &add)?;
    let removed = apply(state, &drop, |list| {
        model::remove_entry(list, entry, &model::read_file)
    })?;
    Ok((added, removed))
}

// ---------------------------------------------------------------------
// The window.

thread_local! {
    static WINDOW: RefCell<Option<Rc<Certs>>> = const { RefCell::new(None) };
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Req {
    View,
    Import,
    Export,
    TrustedFor,
    Remove,
    Close,
}

/// Certificate Management's window.
pub struct Certs {
    pub window: Window,
    pub tabs: Tabs,
    pub list: ListTable,
    pub status: StatusLine,
    state: Shared,
    entries: RefCell<Vec<Entry>>,
    /// The time rows are judged at; `None` for the clock (screenshots and
    /// tests give one).
    now: Option<i64>,
    /// Buttons that need a certificate selected.
    for_certificate: Vec<Switch>,
    /// Buttons that need any row selected.
    for_row: Vec<Switch>,
    rx: chan::Receiver<Req>,
    listener: Cell<Option<u64>>,
}

fn clock() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

/// The list's rows for `entries`, judged at `now`.
pub fn rows(entries: &[Entry], now: i64) -> Vec<Vec<ListCell>> {
    entries
        .iter()
        .map(|e| {
            let systems = ListCell::from(e.systems.join(", "));
            match &e.what {
                What::Certificate(c) => {
                    let expired = c.validity(now) == Validity::Expired;
                    vec![
                        ListCell::from(c.info.subject.as_str()),
                        ListCell::from(c.info.issuer.as_str()),
                        ListCell::from(model::date_of(&c.info.not_before)),
                        ListCell {
                            text: c.valid_to(now),
                            status: if expired {
                                Status::Error
                            } else {
                                Status::Neutral
                            },
                        },
                        ListCell::from(c.info.sha256.as_str()),
                        systems,
                    ]
                }
                What::Unreadable { path, why } => vec![
                    ListCell::from(path.display().to_string()),
                    ListCell {
                        text: why.clone(),
                        status: Status::Error,
                    },
                    ListCell::from(""),
                    ListCell::from(""),
                    ListCell::from(""),
                    systems,
                ],
            }
        })
        .collect()
}

/// What the This computer tab says of a store that supplies `count`
/// certificates.
pub fn this_computer_text(count: usize) -> String {
    let supplied = match count {
        0 => "The operating system's certificate store supplies no certificates.".to_owned(),
        1 => "The operating system's certificate store supplies 1 certificate.".to_owned(),
        n => format!("The operating system's certificate store supplies {n} certificates."),
    };
    format!(
        "{supplied}\n\n\
         They are trusted for every system, beside the certificates on the Certificates \
         tab, which are trusted only for the systems named there.\n\n\
         The store is managed by the operating system: add, remove or look at its \
         certificates with the operating system's own tools. Nothing here changes it. \
         {PRODUCT} reads the store when it first makes a TLS connection, so a change \
         to the store takes effect the next time the program starts.",
        PRODUCT = crate::PRODUCT
    )
}

/// Builds the window. It is not shown. `now` fixes the time rows are
/// judged at, and `store` is how many certificates the operating system's
/// store supplies (`None`: ask it).
pub fn build(state: &Shared, now: Option<i64>, store: Option<usize>) -> Rc<Certs> {
    let (tx, rx) = chan::channel::<Req>();
    let m = 10;
    let bw = theme::BUTTON_W + 20;
    let bh = theme::BUTTON_H;
    let mut window = Window::new(0, 0, W, H, None);
    crate::help::attach(&mut window, crate::help::topic::CERTIFICATE_MANAGEMENT);
    window.set_label(FUNCTION);
    window.set_icon(Some(qsa_ui::glyph::app_icon()));
    window.set_color(colors::FACE);

    let mut tabs = Tabs::new(m, m, W - 2 * m, H - 3 * m - bh);
    let mut list = None;
    let mut buttons = Vec::new();
    tabs.add_page("Certificates", |r| {
        let lw = r.w - bw - 8;
        let mut columns = COLUMNS;
        // The last column takes what is left.
        let used: i32 = columns[..5].iter().map(|c| c.1).sum();
        columns[5].1 = (lw - 4 - used).max(columns[5].1);
        let mut l = ListTable::new(r.x, r.y, lw, r.h, &columns);
        l.set_system_output_columns(&[FINGERPRINT_COLUMN]);
        let bx = r.x + lw + 8;
        let mut y = r.y;
        for (label, req, gap) in [
            ("View...", Req::View, 6),
            ("Import...", Req::Import, 6),
            ("Export...", Req::Export, 18),
            ("Trusted for...", Req::TrustedFor, 6),
            ("Remove", Req::Remove, 6),
        ] {
            let mut b = controls::button(bx, y, bw, bh, label);
            b.emit_to(tx, req);
            buttons.push((req, b));
            y += bh + gap;
        }
        list = Some(l);
    });
    let store = store.unwrap_or_else(qsa_net::tls::system_certificate_count);
    tabs.add_page("This computer", |r| {
        let mut text = Frame::new(r.x, r.y, r.w.min(620), r.h, None);
        text.set_label(&this_computer_text(store));
        text.set_label_size(theme::UI_SIZE);
        text.set_align(Align::Left | Align::Top | Align::Inside | Align::Wrap);
    });
    let mut list = list.expect("the Certificates page is built");

    let help_x = W - m - theme::BUTTON_W;
    let close_x = help_x - 6 - theme::BUTTON_W;
    let status = StatusLine::new(m, H - m - bh, close_x - 2 * m, bh);
    let mut close = controls::default_button(close_x, H - m - bh, theme::BUTTON_W, bh, "Close");
    close.emit_to(tx, Req::Close);
    let mut help = controls::button(help_x, H - m - bh, theme::BUTTON_W, bh, "Help");
    help.set_callback(|_| crate::help::show(crate::help::topic::CERTIFICATE_MANAGEMENT));
    window.end();
    window.set_callback(move |_| tx.send(Req::Close));

    let mut for_certificate = Vec::new();
    let mut for_row = Vec::new();
    for (req, b) in &mut buttons {
        match req {
            Req::View | Req::Export | Req::TrustedFor => {
                for_certificate.push(Switch::for_button(b));
            }
            Req::Remove => for_row.push(Switch::for_button(b)),
            Req::Import | Req::Close => {}
        }
    }
    list.on_activate(move |sel| {
        if sel.is_some() {
            tx.send(Req::View);
        }
    });

    let certs = Rc::new(Certs {
        window,
        tabs,
        list,
        status,
        state: state.clone(),
        entries: RefCell::new(Vec::new()),
        now,
        for_certificate,
        for_row,
        rx,
        listener: Cell::new(None),
    });
    {
        let me = Rc::downgrade(&certs);
        certs.list.clone().on_select(move |_| {
            if let Some(c) = me.upgrade() {
                c.update_buttons();
            }
        });
    }
    {
        let me = Rc::downgrade(&certs);
        let hub = state.borrow().trust.clone();
        certs.listener.set(Some(hub.listen(move || {
            if let Some(c) = me.upgrade() {
                c.refresh();
            }
        })));
    }
    certs.refresh();
    certs
}

impl Drop for Certs {
    fn drop(&mut self) {
        if let Some(n) = self.listener.take()
            && let Ok(st) = self.state.try_borrow()
        {
            st.trust.unlisten(n);
        }
    }
}

impl Certs {
    fn now(&self) -> i64 {
        self.now.unwrap_or_else(clock)
    }

    /// The rows as they are listed.
    pub fn entries(&self) -> Vec<Entry> {
        self.entries.borrow().clone()
    }

    /// The selected row.
    pub fn selected(&self) -> Option<Entry> {
        let i = self.list.selected()?;
        self.entries.borrow().get(i).cloned()
    }

    /// Selects the row for the certificate with this fingerprint.
    pub fn select(&self, sha256: &str) {
        let i = self.entries.borrow().iter().position(|e| e.key() == sha256);
        self.list.clone().select(i);
        self.update_buttons();
    }

    fn update_buttons(&self) {
        let sel = self.selected();
        let cert = sel.as_ref().is_some_and(|e| e.certificate().is_some());
        for s in &self.for_certificate {
            s.set_enabled(cert);
        }
        for s in &self.for_row {
            s.set_enabled(sel.is_some());
        }
    }

    /// Reads every system's CA list again and redraws, keeping the
    /// selection (by certificate).
    pub fn refresh(&self) {
        let keep = self.selected().map(|e| e.key());
        let entries = model::entries(&lists(&self.state), &model::read_file);
        let mut list = self.list.clone();
        list.set_rows(rows(&entries, self.now()));
        let index = keep
            .and_then(|k| entries.iter().position(|e| e.key() == k))
            .or(if entries.is_empty() { None } else { Some(0) });
        *self.entries.borrow_mut() = entries;
        list.select(index);
        self.update_buttons();
    }

    /// Shows a message in the status line, and logs it.
    fn report(&self, m: &Message) {
        crate::log!("{m}");
        let status = if m.kind() == qsa_msg::Kind::Error {
            Status::Error
        } else {
            Status::Neutral
        };
        self.status.clone().set(&m.to_string(), status);
    }

    /// Shows an error that has no catalogue message.
    fn error(&self, text: &str) {
        crate::log!("{FUNCTION}: {text}");
        self.status.clone().set(text, Status::Error);
        MessageBox::new(FUNCTION, text)
            .glyph(Glyph::Cross)
            .run(Some(&self.window));
    }

    /// The names of the configured systems.
    fn systems(&self) -> Vec<String> {
        self.state
            .borrow()
            .systems
            .iter()
            .map(|s| s.name.clone())
            .collect()
    }

    fn handle(&self, req: Req) {
        match req {
            Req::Close => self.window.clone().hide(),
            Req::View => {
                if let Some(e) = self.selected()
                    && let Some(c) = e.certificate()
                {
                    let mut d = view_dialog(c, &e.systems, self.now());
                    center_on(&mut d, Some(&self.window));
                    show_placed(&mut d);
                    while d.shown() {
                        app::wait();
                    }
                }
            }
            Req::Import => self.import(),
            Req::Export => self.export(),
            Req::TrustedFor => self.trusted_for(),
            Req::Remove => self.remove(),
        }
    }

    fn import(&self) {
        let systems = self.systems();
        if systems.is_empty() {
            self.error(
                "There is no system configuration to trust a certificate for. \
                 Add one in System Configurations first.",
            );
            return;
        }
        let Some(path) = files::choose_open(
            "Import Certificate",
            "Certificates (*.{pem,crt,cer,der})\tAll Files (*)",
        ) else {
            return;
        };
        let bytes = match std::fs::read(&path) {
            Ok(b) => b,
            Err(e) => {
                self.error(&format!("{} could not be read: {e}", path.display()));
                return;
            }
        };
        let found = model::importable(&bytes);
        if found.is_empty() {
            let m = qsa_msg::message(id::NOT_A_CERTIFICATE, &[&path.display().to_string()]);
            self.report(&m);
            msgdetails::show_message(Some(&self.window), &m);
            return;
        }
        // An expired certificate is imported only when asked for.
        let now = self.now();
        let mut certs = Vec::new();
        for c in found {
            if c.validity(now) == Validity::Expired {
                let m = expired_message(&c);
                self.report(&m);
                if msgdetails::ask_message(Some(&self.window), &m, &["Import", "Skip"]) != 0 {
                    continue;
                }
            }
            certs.push(c);
        }
        if certs.is_empty() {
            return;
        }
        let trusted: Vec<(String, bool)> = systems.into_iter().map(|s| (s, false)).collect();
        let mut d =
            SystemsDialog::build("Import Certificate", &import_intro(&certs, &path), &trusted);
        let Some(chosen) = d.run(Some(&self.window), true) else {
            return;
        };
        let n = chosen.len().to_string();
        let mut last = None;
        for c in &certs {
            match import(&self.state, c, &chosen) {
                Ok(_) => {
                    self.report(&qsa_msg::message(id::CERT_IMPORTED, &[c.name(), &n]));
                    last = Some(c.info.sha256.clone());
                }
                Err(e) => {
                    self.error(&format!(
                        "The system configurations could not be saved: {e}"
                    ));
                    return;
                }
            }
        }
        if let Some(sha256) = last {
            self.select(&sha256);
        }
    }

    fn export(&self) {
        let Some(e) = self.selected() else { return };
        let Some(c) = e.certificate() else { return };
        let Some(path) = files::choose_save(
            "Export Certificate",
            "PEM certificates (*.pem)\tAll Files (*)",
            &files::file_name_for(c.name(), "pem"),
        ) else {
            return;
        };
        let path = files::with_extension(path, "pem");
        if path.exists() && !confirm_replace(&self.window, &path) {
            return;
        }
        match std::fs::write(&path, c.pem()) {
            Ok(()) => self.report(&qsa_msg::message(
                id::CERT_EXPORTED,
                &[c.name(), &path.display().to_string()],
            )),
            Err(e) => self.error(&format!("{} could not be written: {e}", path.display())),
        }
    }

    fn trusted_for(&self) {
        let Some(e) = self.selected() else { return };
        let Some(c) = e.certificate() else { return };
        let trusted: Vec<(String, bool)> = self
            .systems()
            .into_iter()
            .map(|s| {
                let on = e.systems.iter().any(|t| t.eq_ignore_ascii_case(&s));
                (s, on)
            })
            .collect();
        let intro = format!("Certificate {} is trusted for these systems:", c.name());
        let mut d = SystemsDialog::build("Trusted For", &intro, &trusted);
        let Some(chosen) = d.run(Some(&self.window), false) else {
            return;
        };
        match set_trusted_for(&self.state, &e, &chosen) {
            Ok((added, removed)) => {
                if !added.is_empty() {
                    let n = added.len().to_string();
                    self.report(&qsa_msg::message(id::CERT_IMPORTED, &[c.name(), &n]));
                }
                if !removed.is_empty() {
                    let n = removed.len().to_string();
                    self.report(&qsa_msg::message(id::CERT_REMOVED, &[c.name(), &n]));
                }
            }
            Err(err) => self.error(&format!(
                "The system configurations could not be saved: {err}"
            )),
        }
    }

    fn remove(&self) {
        let Some(e) = self.selected() else { return };
        let answer = remove_box(&e).run(Some(&self.window));
        if answer != 0 {
            return;
        }
        match remove(&self.state, &e) {
            Ok(systems) => {
                let n = systems.len().to_string();
                self.report(&qsa_msg::message(id::CERT_REMOVED, &[&e.name(), &n]));
            }
            Err(err) => self.error(&format!(
                "The system configurations could not be saved: {err}"
            )),
        }
    }
}

/// `QAD0044` for a certificate whose Valid to date has passed.
pub fn expired_message(c: &Certificate) -> Message {
    qsa_msg::message(id::CERT_EXPIRED, &[c.name(), &c.info.not_after])
}

/// What Import's choice of systems says it is importing.
pub fn import_intro(certs: &[Certificate], path: &Path) -> String {
    match certs {
        [one] => format!("Trust certificate {} for these systems:", one.name()),
        many => format!(
            "Trust the {} certificates in {} for these systems:",
            many.len(),
            path.file_name().map_or_else(
                || path.display().to_string(),
                |n| n.to_string_lossy().into_owned()
            )
        ),
    }
}

/// What Remove asks: it names the systems that will stop trusting the
/// certificate.
pub fn remove_text(entry: &Entry) -> String {
    let what = match &entry.what {
        What::Certificate(c) => format!("Remove certificate {}?", c.name()),
        What::Unreadable { path, .. } => {
            format!("Remove the reference to {}?", path.display())
        }
    };
    let systems = model::list_of(&entry.systems);
    let effect = match (&entry.what, entry.systems.len()) {
        (What::Certificate(_), 1) => format!(
            "{systems} will stop trusting it. A connection to that system that needs \
             this certificate will then be refused as not trusted."
        ),
        (What::Certificate(_), _) => format!(
            "{systems} will stop trusting it. A connection to one of those systems that \
             needs this certificate will then be refused as not trusted."
        ),
        (What::Unreadable { .. }, _) => {
            format!("It is in the CA list of {systems}. The file itself is not deleted.")
        }
    };
    format!("{what}\n\n{effect}")
}

/// The confirmation Remove asks for.
pub fn remove_box(entry: &Entry) -> MessageBox {
    MessageBox::new("Remove Certificate", &remove_text(entry))
        .glyph(Glyph::Cross)
        .buttons(&["Remove", "Cancel"], 1, 1)
}

fn confirm_replace(parent: &Window, path: &Path) -> bool {
    MessageBox::new(
        "Export Certificate",
        &format!("{} already exists.\n\nReplace it?", path.display()),
    )
    .buttons(&["Replace", "Cancel"], 1, 1)
    .run(Some(parent))
        == 0
}

/// A fingerprint on two lines of sixteen pairs.
fn two_lines(sha256: &str) -> String {
    let pairs: Vec<&str> = sha256.split(':').collect();
    let half = 16.min(pairs.len());
    format!("{}\n{}", pairs[..half].join(":"), pairs[half..].join(":"))
}

/// The Valid to line of View: the time, and where it stands at `now`.
fn valid_to_line(c: &Certificate, now: i64) -> (String, bool) {
    let t = &c.info.not_after;
    match c.validity(now) {
        Validity::Expired => (format!("{t} (expired)"), true),
        Validity::Soon(0) => (format!("{t} (expires today)"), false),
        Validity::Soon(1) => (format!("{t} (1 day left)"), false),
        Validity::Soon(n) => (format!("{t} ({n} days left)"), false),
        Validity::Valid | Validity::NotYet | Validity::Unknown => (t.clone(), false),
    }
}

/// What View shows of a certificate, as the text Copy puts on the
/// clipboard: every field, then the PEM text.
pub fn view_text(c: &Certificate, systems: &[String], now: i64) -> String {
    format!(
        "Subject: {}\nIssuer: {}\nSerial number: {}\nValid from: {}\nValid to: {}\n\
         SHA-256 fingerprint: {}\nTrusted for: {}\n\n{}",
        c.info.subject,
        c.info.issuer,
        c.info.serial,
        c.info.not_before,
        valid_to_line(c, now).0,
        c.info.sha256,
        systems.join(", "),
        c.pem()
    )
}

/// Builds View's dialog for a certificate: every field, the PEM text, Copy
/// and Close. It is not shown.
pub fn view_dialog(c: &Certificate, systems: &[String], now: i64) -> Window {
    let (w, h) = (580, 470);
    let m = 12;
    let mut win = Window::new(0, 0, w, h, None);
    crate::help::attach(&mut win, crate::help::topic::CERTIFICATE_MANAGEMENT);
    win.set_label(&format!("View Certificate - {}", c.name()));
    win.set_color(colors::FACE);
    win.make_modal(true);

    let lw = 130;
    let vw = w - 2 * m - lw;
    let (valid_to, expired) = valid_to_line(c, now);
    let fingerprint = two_lines(&c.info.sha256);
    let trusted = systems.join(", ");
    let mut y = m;
    for (label, value, lines) in [
        ("Subject:", c.info.subject.as_str(), 1),
        ("Issuer:", c.info.issuer.as_str(), 1),
        ("Serial number:", c.info.serial.as_str(), 1),
        ("Valid from:", c.info.not_before.as_str(), 1),
        ("Valid to:", valid_to.as_str(), 1),
        ("SHA-256 fingerprint:", fingerprint.as_str(), 2),
        ("Trusted for:", trusted.as_str(), 1),
    ] {
        let rh = 18 * lines + 2;
        controls::label(m, y, lw, 18, label);
        let mut v = Frame::new(m + lw, y, vw, rh, None);
        v.set_frame(FrameType::FlatBox);
        v.set_color(colors::FACE);
        v.set_label(value);
        v.set_align(Align::Left | Align::Inside | Align::Top | Align::Clip);
        v.set_label_size(theme::UI_SIZE);
        match label {
            "SHA-256 fingerprint:" | "Serial number:" => {
                v.set_label_font(theme::MONO_FONT);
                v.set_label_size(theme::MONO_SIZE);
            }
            "Valid to:" if expired => v.set_label_color(colors::STATUS_ERROR),
            _ => {}
        }
        y += rh + 4;
    }

    let bh = theme::BUTTON_H;
    let by = h - m - bh;
    let mut buf = TextBuffer::default();
    buf.set_text(&c.pem());
    let mut td = TextDisplay::new(m, y + 4, w - 2 * m, by - 8 - (y + 4), None);
    td.set_buffer(buf);
    td.set_frame(FrameType::DownBox);
    td.set_text_font(theme::MONO_FONT);
    td.set_text_size(theme::MONO_SIZE);

    let bw = theme::BUTTON_W;
    let mut copy = controls::button(w - m - 2 * bw - 6, by, bw, bh, "Copy");
    let mut close = controls::default_button(w - m - bw, by, bw, bh, "Close");
    win.end();
    let text = view_text(c, systems, now);
    copy.set_callback(move |_| app::copy(&text));
    let mut wc = win.clone();
    close.set_callback(move |_| wc.hide());
    win
}

/// A choice of systems, each with a tick: Import's, and Trusted for's.
pub struct SystemsDialog {
    pub window: Window,
    checks: Vec<(String, CheckButton)>,
    status: StatusLine,
    answer: Rc<Cell<Option<bool>>>,
}

impl SystemsDialog {
    /// Builds the dialog: `intro`, then a tick for each system (name,
    /// ticked now). It is not shown.
    pub fn build(title: &str, intro: &str, systems: &[(String, bool)]) -> SystemsDialog {
        let m = 12;
        let w = 400;
        let row = theme::FIELD_H + 1;
        let shown = (systems.len() as i32).clamp(4, 10);
        let list_h = shown * row + 6;
        let intro_h = 34;
        let bh = theme::BUTTON_H;
        let bw = theme::BUTTON_W;
        let status_h = 18;
        let h = m + intro_h + 4 + list_h + 4 + status_h + 4 + bh + m;
        let mut window = Window::new(0, 0, w, h, None);
        crate::help::attach(&mut window, crate::help::topic::CERTIFICATE_MANAGEMENT);
        window.set_label(title);
        window.set_color(colors::FACE);
        window.make_modal(true);

        let mut head = Frame::new(m, m, w - 2 * m, intro_h, None);
        head.set_label(intro);
        head.set_label_size(theme::UI_SIZE);
        head.set_align(Align::Left | Align::Top | Align::Inside | Align::Wrap);

        let ly = m + intro_h + 4;
        let mut scroll = Scroll::new(m, ly, w - 2 * m, list_h, None);
        scroll.set_frame(FrameType::BorderBox);
        scroll.set_color(colors::WINDOW);
        scroll.set_type(fltk::group::ScrollType::Vertical);
        theme::style_scrollbar(&mut scroll.scrollbar());
        let mut checks = Vec::new();
        for (i, (name, on)) in systems.iter().enumerate() {
            let mut c = controls::check(
                m + 8,
                ly + 3 + i as i32 * row,
                w - 2 * m - 36,
                theme::FIELD_H,
                name,
            );
            c.set_color(colors::WINDOW);
            c.set_checked(*on);
            checks.push((name.clone(), c));
        }
        scroll.end();

        let status = StatusLine::new(m, ly + list_h + 4, w - 2 * m, status_h);
        let by = h - m - bh;
        let mut all = controls::button(m, by, bw, bh, "Select All");
        let mut none = controls::button(m + bw + 6, by, bw, bh, "Clear All");
        let ok_x = w - m - 2 * bw - 6;
        let mut ok = controls::default_button(ok_x, by, bw, bh, "OK");
        let mut cancel = controls::button(w - m - bw, by, bw, bh, "Cancel");
        window.end();

        for (button, on) in [(&mut all, true), (&mut none, false)] {
            let boxes: Vec<CheckButton> = checks.iter().map(|(_, c)| c.clone()).collect();
            button.set_callback(move |_| {
                for c in &boxes {
                    c.set_checked(on);
                }
            });
        }
        let answer = Rc::new(Cell::new(None));
        let a = answer.clone();
        ok.set_callback(move |_| a.set(Some(true)));
        let a = answer.clone();
        cancel.set_callback(move |_| a.set(Some(false)));
        let a = answer.clone();
        window.set_callback(move |_| a.set(Some(false)));
        SystemsDialog {
            window,
            checks,
            status,
            answer,
        }
    }

    /// The systems ticked.
    pub fn ticked(&self) -> Vec<String> {
        self.checks
            .iter()
            .filter(|(_, c)| c.is_checked())
            .map(|(n, _)| n.clone())
            .collect()
    }

    /// Shows the dialog until OK or Cancel. With `need_one`, OK wants at
    /// least one system ticked. Returns the systems ticked, or `None` if
    /// cancelled.
    pub fn run(&mut self, parent: Option<&dyn WindowExt>, need_one: bool) -> Option<Vec<String>> {
        center_on(&mut self.window, parent);
        show_placed(&mut self.window);
        while self.window.shown() {
            app::wait();
            match self.answer.take() {
                Some(true) => {
                    let ticked = self.ticked();
                    if need_one && ticked.is_empty() {
                        self.status
                            .set("Select at least one system.", Status::Error);
                        continue;
                    }
                    self.window.hide();
                    return Some(ticked);
                }
                Some(false) => {
                    self.window.hide();
                    return None;
                }
                None => {}
            }
        }
        None
    }
}

/// Shows Certificate Management, building it the first time, and raises
/// it.
pub fn open(parent: Option<&dyn WindowExt>, state: &Shared) {
    let existing = WINDOW.with(|w| w.borrow().clone());
    let certs = match existing {
        Some(c) => c,
        None => {
            let c = build(state, None, None);
            center_on(&mut c.window.clone(), parent);
            WINDOW.with(|slot| *slot.borrow_mut() = Some(c.clone()));
            c
        }
    };
    show_placed(&mut certs.window.clone());
}

/// Closes Certificate Management if it is open.
pub fn close() {
    if let Some(c) = WINDOW.with(|w| w.borrow_mut().take()) {
        c.window.clone().hide();
    }
}

/// Whether Certificate Management is open.
pub fn shown() -> bool {
    WINDOW.with(|w| w.borrow().as_ref().is_some_and(|c| c.window.shown()))
}

/// Handles the window's requests. Called by the main loop after each wait.
pub fn pump() {
    let Some(certs) = WINDOW.with(|w| w.borrow().clone()) else {
        return;
    };
    while let Some(r) = certs.rx.recv() {
        certs.handle(r);
    }
    if !certs.window.shown() {
        WINDOW.with(|w| *w.borrow_mut() = None);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use model::tests::cert;
    use qsa_config::{Settings, System, Systems};

    const NOW: i64 = 1_793_534_400; // 2026-11-01 12:00:00 UTC

    fn state(systems: &[(&str, &[&Certificate])]) -> Shared {
        let mut all = Systems::new();
        for (name, certs) in systems {
            all.upsert(System {
                ca_certificates: certs.iter().map(|c| CaCertificate::Pem(c.pem())).collect(),
                ..System::new(name)
            });
        }
        crate::app::AppState::new(None, all, Settings::default()).shared()
    }

    fn trusted_by(state: &Shared, c: &Certificate) -> Vec<String> {
        model::entries(&lists(state), &model::read_file)
            .into_iter()
            .find(|e| e.key() == c.info.sha256)
            .map(|e| e.systems)
            .unwrap_or_default()
    }

    fn names(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn import_adds_to_the_chosen_systems_once() {
        let ca = cert(
            "Example Root",
            "Example Root",
            "250101000000Z",
            "350101000000Z",
        );
        let st = state(&[
            ("a.example.net", &[&ca]),
            ("b.example.net", &[]),
            ("c.example.net", &[]),
        ]);
        let both = names(&["a.example.net", "b.example.net"]);
        // a trusts it already: only b changes.
        assert_eq!(import(&st, &ca, &both).unwrap(), names(&["b.example.net"]));
        assert_eq!(trusted_by(&st, &ca), both);
        assert!(import(&st, &ca, &both).unwrap().is_empty());
        let saved = st.borrow().systems.get("b.example.net").unwrap().clone();
        assert_eq!(saved.ca_certificates, [CaCertificate::Pem(ca.pem())]);
        // A system that is not configured is passed over.
        assert!(
            import(&st, &ca, &names(&["gone.example.net"]))
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn remove_takes_it_from_every_system_and_trusted_for_sets_them() {
        let ca = cert(
            "Example Root",
            "Example Root",
            "250101000000Z",
            "350101000000Z",
        );
        let own = cert(
            "a.example.net",
            "a.example.net",
            "250101000000Z",
            "270101000000Z",
        );
        let st = state(&[
            ("a.example.net", &[&ca, &own]),
            ("b.example.net", &[&ca]),
            ("c.example.net", &[]),
        ]);
        let entry = |c: &Certificate| {
            model::entries(&lists(&st), &model::read_file)
                .into_iter()
                .find(|e| e.key() == c.info.sha256)
                .unwrap()
        };
        let (added, removed) = set_trusted_for(
            &st,
            &entry(&ca),
            &names(&["b.example.net", "c.example.net"]),
        )
        .unwrap();
        assert_eq!(added, names(&["c.example.net"]));
        assert_eq!(removed, names(&["a.example.net"]));
        assert_eq!(
            trusted_by(&st, &ca),
            names(&["b.example.net", "c.example.net"])
        );
        assert_eq!(trusted_by(&st, &own), names(&["a.example.net"]));

        assert_eq!(
            remove(&st, &entry(&ca)).unwrap(),
            names(&["b.example.net", "c.example.net"])
        );
        assert!(trusted_by(&st, &ca).is_empty());
        assert_eq!(model::entries(&lists(&st), &model::read_file).len(), 1);
    }

    #[test]
    fn rows_mark_expired_certificates_and_count_the_days_left() {
        let entries = vec![
            Entry {
                what: What::Certificate(cert("old", "ca", "200101000000Z", "250101000000Z")),
                systems: names(&["a", "b"]),
            },
            Entry {
                what: What::Certificate(cert("soon", "ca", "250101000000Z", "261122120000Z")),
                systems: names(&["a"]),
            },
            Entry {
                what: What::Unreadable {
                    path: "/gone.pem".into(),
                    why: "not found".into(),
                },
                systems: names(&["b"]),
            },
        ];
        let r = rows(&entries, NOW);
        assert_eq!(r[0][0].text, "CN=old, O=Example Corp");
        assert_eq!(r[0][2].text, "2020-01-01");
        assert_eq!(
            (r[0][3].text.as_str(), r[0][3].status),
            ("Expired", Status::Error)
        );
        assert_eq!(r[0][5].text, "a, b");
        assert_eq!(
            (r[1][3].text.as_str(), r[1][3].status),
            ("2026-11-22 (21 days left)", Status::Neutral)
        );
        assert_eq!(r[1][4].text.len(), 95);
        assert_eq!(
            (r[2][0].text.as_str(), r[2][1].status),
            ("/gone.pem", Status::Error)
        );
        assert!(r.iter().all(|row| row.len() == COLUMNS.len()));
    }

    #[test]
    fn the_confirmation_names_the_systems_that_stop_trusting() {
        let c = cert(
            "Example Root",
            "Example Root",
            "250101000000Z",
            "350101000000Z",
        );
        let text = |systems: &[&str]| {
            let e = Entry {
                what: What::Certificate(c.clone()),
                systems: names(systems),
            };
            remove_text(&e)
        };
        assert!(text(&["a.example.net"]).starts_with(
            "Remove certificate Example Root?\n\na.example.net will stop trusting it."
        ));
        assert!(
            text(&["a.example.net", "b.example.net", "c.example.net"])
                .contains("a.example.net, b.example.net and c.example.net will stop trusting it.")
        );
    }

    #[test]
    fn view_gives_every_field_and_the_pem_text() {
        let c = cert("old", "ca", "200101000000Z", "250101000000Z");
        let t = view_text(&c, &names(&["a", "b"]), NOW);
        assert!(t.starts_with(
            "Subject: CN=old, O=Example Corp\nIssuer: CN=ca, O=Example Corp\nSerial number: 01"
        ));
        assert!(t.contains("\nValid to: 2025-01-01 00:00:00 UTC (expired)\n"));
        assert!(t.contains(&format!("\nSHA-256 fingerprint: {}\n", c.info.sha256)));
        assert!(t.contains("\nTrusted for: a, b\n\n-----BEGIN CERTIFICATE-----\n"));
        assert!(t.ends_with("-----END CERTIFICATE-----\n"));
        assert_eq!(
            two_lines(&c.info.sha256)
                .lines()
                .map(str::len)
                .collect::<Vec<_>>(),
            [47, 47]
        );
    }

    #[test]
    fn the_store_is_described_by_its_count() {
        assert!(
            this_computer_text(0)
                .starts_with("The operating system's certificate store supplies no certificates.")
        );
        assert!(this_computer_text(1).contains("supplies 1 certificate."));
        assert!(this_computer_text(142).contains("supplies 142 certificates."));
        assert!(this_computer_text(142).contains("managed by the operating system"));
    }
}
