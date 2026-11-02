// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! Prompt: a command built from its definition, as F4 does on a 5250
//! display. The definition comes from QAP `DescribeCommand` (with
//! `CHGCMDDFT` defaults applied), kept per system for the session.
//!
//! - One row per parameter shown: its prompt text, its keyword and a value
//!   field as wide as its length. Special and listed values are in an
//!   editable combo; a required parameter's prompt is bold; defaults are
//!   filled in. Qualifiers are indented rows under their parameter, and
//!   elements an indented group of rows. A list parameter shows one entry
//!   and a "+" button that adds another, up to its `max_values`.
//! - `Additional` parameters wait behind Additional Parameters (F10);
//!   constants (`Never`) are never shown.
//! - F1 or "?" on a row shows that node's help; Help shows the command's.
//!   QS/400 ships no command help text yet, so these show the prompt text,
//!   the values allowed, and a note that no help is available.
//! - Show Command String writes the string the values make ([`form`]).
//!   OK checks it with `CheckCommand` first: errors are listed in the
//!   dialog with `QAD5020`, and it stays open; a valid command goes to the
//!   command line. Cancel keeps nothing.
//! - The parameters scroll inside the dialog, which never grows past the
//!   screen. Adding an entry or showing the additional parameters lays the
//!   dialog out again where it is.

pub mod form;

use fltk::app;
use fltk::button::Button;
use fltk::enums::{Align, Event, FrameType, Key};
use fltk::group::{Scroll, ScrollType};
use fltk::input::Input;
use fltk::misc::InputChoice;
use fltk::output::Output;
use fltk::prelude::*;
use fltk::window::Window;
use qap_client::proto::ops::command::{CommandDefinition, ParmKind, ParmNode, Show};
use qap_client::proto::types::ObjectName;
use qap_client::{CheckOutcome, ServerMessage, Status as QapStatus};
use qsa_config::System;
use qsa_msg::id;
use qsa_ui::MessageBox;
use qsa_ui::chan::{self, Emit};
use qsa_ui::controls;
use qsa_ui::listtable::ListTable;
use qsa_ui::msgdetails;
use qsa_ui::propdialog::{center_on, show_placed, work_area};
use qsa_ui::statusbar::{Status, StatusLine};
use qsa_ui::theme::{self, colors};

use crate::app::Shared;
use crate::qap::{self, Handle, lock};
use crate::runcmd::{self, Line, command_name};
use form::Form;

/// Dialog width.
pub const W: i32 = 640;
/// Height of a row of the parameters.
const ROW: i32 = 26;
/// Where keywords and fields start, from the rows' left.
const KEY_X: i32 = 250;
const FIELD_X: i32 = 322;
/// Indent of a qualifier or element row.
const INDENT: i32 = 18;
/// Margin.
const M: i32 = 10;
/// Height of the check messages.
const MESSAGES_H: i32 = 22 + 3 * 18;
/// Columns of the check messages.
const COLUMNS: [(&str, i32); 3] = [("ID", 76), ("Severity", 60), ("Text", 476)];

/// What the note says when the system has no help text.
pub const NO_HELP: &str = "No help is available: QS/400 does not ship help text for its \
    commands yet.";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Request {
    Ok,
    Cancel,
    Additional,
    ShowString,
    Help,
    NodeHelp(u16),
    Add(u16),
    Details,
}

/// A value's field.
enum Widget {
    Field(Input),
    Combo(InputChoice),
}

impl Widget {
    fn value(&self) -> String {
        match self {
            Widget::Field(i) => i.value(),
            Widget::Combo(c) => c.value().unwrap_or_default(),
        }
    }
}

/// A field and the node and entry it gives a value for.
struct Field {
    node: u16,
    entry: usize,
    widget: Widget,
}

/// The dialog's widgets.
pub struct PromptDialog {
    pub window: Window,
    pub string: Output,
    pub messages: ListTable,
    pub status: StatusLine,
    fields: Vec<Field>,
}

/// The dialog's title: the command's text and name.
pub fn title(def: &CommandDefinition) -> String {
    if def.text.trim().is_empty() {
        def.name.name.clone()
    } else {
        format!("{} ({})", def.text.trim(), def.name.name)
    }
}

/// Whether the definition has parameters under Additional Parameters.
fn has_additional(form: &Form) -> bool {
    form.parameters().any(|n| n.show == Show::Additional)
}

/// Whether a parameter is shown.
fn shown(p: &ParmNode, additional: bool) -> bool {
    match p.show {
        Show::Always => true,
        Show::Additional => additional,
        Show::Never => false,
    }
}

/// The width of a field for its node's length.
fn field_w(n: &ParmNode, combo: bool) -> i32 {
    let chars = i32::try_from(n.length.clamp(4, 40)).unwrap_or(40);
    let w = 12 + chars * 7 + if combo { 20 } else { 0 };
    w.min(W - 2 * M - FIELD_X - 70)
}

/// The values a node offers in its combo.
fn choices(n: &ParmNode) -> Vec<String> {
    let mut v = n.special_values.clone();
    for x in &n.values {
        if !v.iter().any(|s| s.eq_ignore_ascii_case(x)) {
            v.push(x.clone());
        }
    }
    v
}

/// A node's help: its own text, or its prompt and what it takes, with a
/// note that the system has no help for it.
pub fn node_help(form: &Form, i: u16) -> String {
    let Some(n) = form.node(i) else {
        return String::new();
    };
    let kwd = if n.keyword.is_empty() {
        form.node(form.top(i))
            .map(|p| p.keyword.clone())
            .unwrap_or_default()
    } else {
        n.keyword.clone()
    };
    let mut out = format!("{} ({kwd})\n\n", n.prompt);
    if n.help.trim().is_empty() {
        out.push_str(NO_HELP);
    } else {
        out.push_str(n.help.trim());
    }
    out.push_str("\n\n");
    let kind = match n.kind {
        ParmKind::Name => "a name",
        ParmKind::Character => "characters",
        ParmKind::Decimal => "a decimal number",
        ParmKind::Integer => "an integer",
        ParmKind::Logical => "a logical value",
        ParmKind::Date => "a date",
        ParmKind::Time => "a time",
        ParmKind::Generic => "a generic name",
        ParmKind::Path => "a path name",
        ParmKind::Command => "a command",
    };
    if n.length > 0 {
        out.push_str(&format!("Value: {kind}, up to {} long.\n", n.length));
    }
    if n.required {
        out.push_str("A value is required.\n");
    }
    if let Some(d) = &n.default {
        out.push_str(&format!("Default: {d}\n"));
    }
    if !n.special_values.is_empty() {
        out.push_str(&format!(
            "Special values: {}\n",
            n.special_values.join(", ")
        ));
    }
    if !n.values.is_empty() {
        out.push_str(&format!("Values allowed: {}\n", n.values.join(", ")));
    }
    if let Some(r) = &n.range {
        out.push_str(&format!("Range: {} to {}\n", r.low, r.high));
    }
    if n.max_values > 1 {
        out.push_str(&format!("Up to {} values.\n", n.max_values));
    }
    out.trim_end().to_owned()
}

/// The command's help, or its text with the note that there is none.
pub fn command_help(def: &CommandDefinition) -> String {
    let mut out = format!("{}\n\n", title(def));
    if def.help.trim().is_empty() {
        out.push_str(NO_HELP);
    } else {
        out.push_str(def.help.trim());
    }
    out.push_str(&format!(
        "\n\nCommand {} in library {}.",
        def.name.name, def.name.library
    ));
    out
}

/// The help popup for `text`.
pub fn help_box(title: &str, text: &str) -> MessageBox {
    MessageBox::new(title, text).buttons(&["Close"], 0, 0)
}

/// The rows the parameters take.
fn rows_needed(form: &Form, additional: bool) -> i32 {
    let mut rows = 0;
    if form.parameters().any(|p| p.show == Show::Additional) && additional {
        rows += 1;
    }
    for p in form.parameters().filter(|p| shown(p, additional)) {
        let under = form
            .def
            .nodes
            .iter()
            .filter(|n| n.parent.is_some() && n.show != Show::Never && form.top(n.index) == p.index)
            .count();
        rows += (1 + under) * form.entries(p.index);
    }
    i32::try_from(rows).unwrap_or(1000)
}

/// The rows, laid out from `y`.
struct Rows<'a> {
    form: &'a Form,
    x0: i32,
    y: i32,
    right: i32,
    tx: chan::Sender<Request>,
    fields: Vec<Field>,
    /// Each field's text widget and its node, for F1.
    focus: Vec<(fltk::app::WidgetPtr, u16)>,
}

impl Rows<'_> {
    /// Node `n`'s row in `entry`, then its qualifiers' or elements' rows.
    fn row(&mut self, n: &ParmNode, entry: usize, depth: i32, labelled: bool, plus: bool) {
        let form = self.form;
        let (x0, y, tx) = (self.x0, self.y, self.tx);
        let indent = depth * INDENT;
        let kids: Vec<&ParmNode> = form
            .children(n.index)
            .filter(|k| k.show != Show::Never)
            .collect();
        if labelled {
            let text = if n.prompt.is_empty() {
                &n.keyword
            } else {
                &n.prompt
            };
            let mut l = controls::label(x0 + indent, y, KEY_X - 6 - indent, ROW - 5, text);
            if n.required {
                l.set_label_font(theme::UI_FONT_BOLD);
                l.set_tooltip("Required");
            }
            if !n.keyword.is_empty() {
                let mut k =
                    controls::label(x0 + KEY_X, y, FIELD_X - KEY_X - 4, ROW - 5, &n.keyword);
                k.set_label_color(colors::DISABLED_TEXT);
            }
        }
        // A single value, or a group whose special values stand for all of it.
        let own = kids.is_empty() || !n.special_values.is_empty();
        let fx = x0 + FIELD_X;
        let mut after = fx;
        if own {
            let list = choices(n);
            let value = form.value(n.index, entry);
            let widget = if list.is_empty() {
                let mut i = controls::field(fx, y, field_w(n, false), ROW - 5);
                i.set_value(&value);
                self.focus.push((i.as_widget_ptr(), n.index));
                after = fx + i.w();
                Widget::Field(i)
            } else {
                let mut c = controls::editable_combo(fx, y, field_w(n, true), ROW - 5);
                for v in &list {
                    c.add(v);
                }
                c.set_value(&value);
                self.focus.push((c.input().as_widget_ptr(), n.index));
                after = fx + c.w();
                Widget::Combo(c)
            };
            self.fields.push(Field {
                node: n.index,
                entry,
                widget,
            });
        }
        if plus {
            let mut b = controls::button(after + 4, y, 22, ROW - 5, "+");
            b.set_tooltip("Add another value");
            b.emit_to(tx, Request::Add(n.index));
        }
        let mut q = controls::button(self.right - 22, y, 22, ROW - 5, "?");
        q.set_tooltip("Help for this value (F1)");
        q.emit_to(tx, Request::NodeHelp(n.index));
        self.y += ROW;
        for k in kids {
            self.row(k, entry, depth + 1, true, false);
        }
    }
}

/// Builds the dialog for `form` at `at` (centred when `None`), with the
/// additional parameters shown when `additional`. It is not shown.
fn build_with(
    form: &Form,
    additional: bool,
    tx: chan::Sender<Request>,
    at: Option<(i32, i32)>,
) -> PromptDialog {
    let (bw, bh) = (theme::BUTTON_W, theme::BUTTON_H);
    let fh = theme::FIELD_H;
    let footer = 8 + fh + 6 + MESSAGES_H + 8 + bh + 6 + bh + M;
    let (_, _, _, screen_h) = work_area();
    let want = rows_needed(form, additional).max(1) * ROW + 12;
    let scroll_h = want.min((screen_h - 80 - footer - M).max(3 * ROW));
    let h = M + scroll_h + footer;

    let mut window = Window::new(at.map_or(0, |a| a.0), at.map_or(0, |a| a.1), W, h, None);
    window.set_label(&title(&form.def));
    window.set_color(colors::FACE);
    window.make_modal(true);

    let mut scroll = Scroll::new(M, M, W - 2 * M, scroll_h, None);
    scroll.set_type(ScrollType::Vertical);
    scroll.set_color(colors::WINDOW);
    scroll.set_frame(FrameType::DownBox);
    let mut rows = Rows {
        form,
        x0: M + 8,
        y: M + 6,
        right: W - M - 2 - if want > scroll_h { 18 } else { 0 },
        tx,
        fields: Vec::new(),
        focus: Vec::new(),
    };
    let mut header = false;
    for p in form.parameters().filter(|p| shown(p, additional)) {
        if p.show == Show::Additional && !header {
            header = true;
            let mut l = controls::label(rows.x0, rows.y + 2, 300, ROW - 5, "Additional Parameters");
            l.set_label_font(theme::UI_FONT_BOLD);
            rows.y += ROW;
        }
        let n = form.entries(p.index);
        for e in 0..n {
            let plus = e + 1 == n && form.can_add(p.index);
            rows.row(p, e, 0, e == 0, plus);
        }
    }
    let (fields, focus) = (rows.fields, rows.focus);
    scroll.end();

    let mut y = M + scroll_h + 8;
    let mut label = controls::label(M, y, 110, fh, "Command string:");
    label.set_align(Align::Left | Align::Inside);
    let mut string = Output::new(M + 110, y, W - 2 * M - 110, fh, None);
    string.set_text_font(theme::MONO_FONT);
    string.set_text_size(theme::MONO_SIZE);
    string.set_color(colors::FACE);
    y += fh + 6;
    let mut messages = ListTable::new(M, y, W - 2 * M, MESSAGES_H, &COLUMNS);
    messages.on_activate(move |sel| {
        if sel.is_some() {
            tx.send(Request::Details);
        }
    });
    y += MESSAGES_H + 8;

    let mut more = controls::button(
        M,
        y,
        170,
        bh,
        if additional {
            "Hide Additional (F10)"
        } else {
            "Additional Parameters (F10)"
        },
    );
    more.emit_to(tx, Request::Additional);
    if !has_additional(form) {
        more.deactivate();
    }
    let mut show = controls::button(M + 176, y, 150, bh, "Show Command String");
    show.emit_to(tx, Request::ShowString);
    let buttons: [(&str, Request); 3] = [
        ("OK", Request::Ok),
        ("Cancel", Request::Cancel),
        ("Help", Request::Help),
    ];
    let mut x = W - M - 3 * bw - 12;
    for (name, r) in buttons {
        let mut b: Button = if r == Request::Ok {
            controls::default_button(x, y, bw, bh, name)
        } else {
            controls::button(x, y, bw, bh, name)
        };
        b.emit_to(tx, r);
        x += bw + 6;
    }
    y += bh + 6;
    let status = StatusLine::new(M, y, W - 2 * M, bh);
    window.end();
    window.set_callback(move |_| tx.send(Request::Cancel));
    // F1 asks for the help of the field with the focus, F10 for the
    // additional parameters; a field passes both keys on to the window.
    window.handle(move |_, ev| {
        if !matches!(ev, Event::KeyDown | Event::Shortcut) {
            return false;
        }
        match app::event_key() {
            k if k == Key::F10 => tx.send(Request::Additional),
            k if k == Key::F1 => {
                let at = app::focus().map(|w| w.as_widget_ptr());
                match focus.iter().find(|(p, _)| Some(*p) == at) {
                    Some((_, node)) => tx.send(Request::NodeHelp(*node)),
                    None => tx.send(Request::Help),
                }
            }
            _ => return false,
        }
        true
    });
    PromptDialog {
        window,
        string,
        messages,
        status,
        fields,
    }
}

/// Builds the dialog for `form` (for screenshots), with the additional
/// parameters shown when `additional` and the command string written. It
/// is not shown.
pub fn build(form: &Form, additional: bool) -> PromptDialog {
    let (tx, _rx) = chan::channel::<Request>();
    let mut d = build_with(form, additional, tx, None);
    d.string.set_value(&form.command_string());
    d
}

/// The form's values from the fields.
fn read(d: &PromptDialog, form: &mut Form) {
    for f in &d.fields {
        form.set(f.node, f.entry, &f.widget.value());
    }
}

/// What became of a prompt.
#[derive(Debug, PartialEq, Eq)]
pub enum Prompted {
    /// OK with a valid command: its string.
    Built(String),
    /// Cancelled.
    Cancelled,
    /// The definition could not be had: the status line's text.
    Failed(String),
}

/// The command a typed command line names: its name, and its library when
/// one is given.
pub fn named(typed: &str) -> Option<ObjectName> {
    let first = typed.split_whitespace().next()?;
    let first = first.split('(').next().unwrap_or(first);
    let (library, name) = match first.split_once('/') {
        Some((l, n)) => (l.to_ascii_uppercase(), n.to_ascii_uppercase()),
        None => ("*LIBL".to_owned(), first.to_ascii_uppercase()),
    };
    (!name.is_empty()).then_some(ObjectName { library, name })
}

/// The definition of `name` on `system`: from this session's cache, or
/// asked for and kept.
fn definition(
    state: &Shared,
    sys: &System,
    h: &Handle,
    name: ObjectName,
) -> Result<CommandDefinition, qap_client::Error> {
    let key = format!("{}/{}", name.library, name.name);
    if let Some(d) = state.borrow_mut().qap.definition(&sys.name, &key) {
        return Ok(d);
    }
    let def = qap::busy(|| lock(h).describe_command(name))?;
    state
        .borrow_mut()
        .qap
        .keep_definition(&sys.name, &key, def.clone());
    Ok(def)
}

/// Prompts the command `typed` names on `sys`, its values taken from
/// `typed`, until OK with a valid command or Cancel.
pub fn run(
    parent: Option<&dyn WindowExt>,
    state: &Shared,
    sys: &System,
    h: &Handle,
    typed: &str,
) -> Prompted {
    let Some(name) = named(typed) else {
        return Prompted::Failed("Type a command name to prompt it.".to_owned());
    };
    let def = match definition(state, sys, h, name.clone()) {
        Ok(d) => d,
        Err(e) => {
            let text = match (e.status(), e.server_message()) {
                (Some(QapStatus::NotFound), _) => {
                    qsa_msg::message(id::COMMAND_NOT_FOUND, &[&name.name]).to_string()
                }
                (Some(QapStatus::NotAuthorized), Some(m)) => format!("{}: {}", m.id, m.text),
                _ => qap::explain(sys, &e).line(),
            };
            crate::log!("Prompt {} on {}: {text}", name.name, sys.name);
            return Prompted::Failed(text);
        }
    };
    let mut form = Form::new(def);
    form.fill_from(typed);
    let mut additional = false;
    let (tx, rx) = chan::channel::<Request>();
    let mut d = build_with(&form, additional, tx, None);
    center_on(&mut d.window, parent);
    show_placed(&mut d.window);
    let mut checked: Vec<ServerMessage> = Vec::new();
    let mut status: Option<(String, Status)> = None;
    while d.window.shown() {
        app::wait();
        let Some(r) = rx.recv() else { continue };
        match r {
            Request::Cancel => {
                d.window.hide();
                return Prompted::Cancelled;
            }
            Request::Additional | Request::Add(_) => {
                read(&d, &mut form);
                let changed = match r {
                    Request::Add(p) => form.add_entry(p),
                    _ if has_additional(&form) => {
                        additional = !additional;
                        true
                    }
                    _ => false,
                };
                if changed {
                    // Laid out again where it is, with what it showed.
                    let at = (d.window.x(), d.window.y());
                    let string = d.string.value();
                    d.window.hide();
                    d = build_with(&form, additional, tx, Some(at));
                    d.string.set_value(&string);
                    d.messages.set_rows(message_rows(&checked));
                    if let Some((t, s)) = &status {
                        d.status.set(t, *s);
                    }
                    show_placed(&mut d.window);
                }
            }
            Request::ShowString => {
                read(&d, &mut form);
                d.string.set_value(&form.command_string());
            }
            Request::Help => {
                help_box(&format!("Help - {}", form.name()), &command_help(&form.def))
                    .run(Some(&d.window));
            }
            Request::NodeHelp(i) => {
                let kwd = form
                    .node(form.top(i))
                    .map(|p| p.keyword.clone())
                    .unwrap_or_default();
                help_box(&format!("Help - {kwd}"), &node_help(&form, i)).run(Some(&d.window));
            }
            Request::Details => {
                if let Some(m) = d.messages.selected().and_then(|i| checked.get(i)) {
                    msgdetails::show_system_message(Some(&d.window), None, &qap::shown(m));
                }
            }
            Request::Ok => {
                read(&d, &mut form);
                let command = form.command_string();
                d.string.set_value(&command);
                d.status.set(
                    &format!("Checking {}...", command_name(&command)),
                    Status::Neutral,
                );
                match qap::busy(|| lock(h).check_command(&command)) {
                    Ok(CheckOutcome {
                        status: QapStatus::Ok,
                        ..
                    }) => {
                        d.window.hide();
                        return Prompted::Built(command);
                    }
                    Ok(o) => {
                        checked = o.messages;
                        d.messages.set_rows(message_rows(&checked));
                        let mid = if o.status == QapStatus::NotFound {
                            id::COMMAND_NOT_FOUND
                        } else {
                            id::COMMAND_HAS_ERRORS
                        };
                        let m = qsa_msg::message(mid, &[form.name()]);
                        crate::log!("{m}");
                        status = Some((m.to_string(), Status::Error));
                        d.status.set(&m.to_string(), Status::Error);
                    }
                    Err(e) => {
                        let x = qap::explain(sys, &e);
                        crate::log!("{}", x.line());
                        status = Some((x.line(), Status::Error));
                        d.status.set(&x.line(), Status::Error);
                    }
                }
            }
        }
    }
    Prompted::Cancelled
}

/// The check's messages as rows, as the system sent them.
fn message_rows(messages: &[ServerMessage]) -> Vec<Vec<qsa_ui::listtable::Cell>> {
    let lines: Vec<Line> = messages.iter().cloned().map(Line::Message).collect();
    runcmd::rows(&lines)
}

/// Builds the dialog for `form` showing `messages` from a check and its
/// status (for screenshots). It is not shown.
pub fn build_checked(form: &Form, messages: &[ServerMessage]) -> PromptDialog {
    let mut d = build(form, false);
    d.messages.set_rows(message_rows(messages));
    let m = qsa_msg::message(id::COMMAND_HAS_ERRORS, &[form.name()]);
    d.status.set(&m.to_string(), Status::Error);
    d
}

#[cfg(test)]
mod tests {
    use qap_client::proto::ops::command::Role;
    use qapd::testing::commands::{definition, find};

    use super::*;

    #[test]
    fn titles_and_names() {
        let def = definition(find("CRTLIB").unwrap());
        assert_eq!(title(&def), "Create Library (CRTLIB)");
        let n = named("qgpl/mycmd parm(1)").unwrap();
        assert_eq!((n.library.as_str(), n.name.as_str()), ("QGPL", "MYCMD"));
        let n = named("crtlib").unwrap();
        assert_eq!((n.library.as_str(), n.name.as_str()), ("*LIBL", "CRTLIB"));
        assert!(named("   ").is_none());
    }

    #[test]
    fn help_without_help_text_says_so() {
        let f = Form::new(definition(find("CRTLIB").unwrap()));
        let ty = f.parameters().find(|p| p.keyword == "TYPE").unwrap().index;
        let h = node_help(&f, ty);
        assert!(h.starts_with("Library type (TYPE)\n\n"), "{h}");
        assert!(h.contains(NO_HELP));
        assert!(h.contains("Default: *PROD"));
        assert!(h.contains("Values allowed: *PROD, *TEST"));
        let c = command_help(&f.def);
        assert!(c.starts_with("Create Library (CRTLIB)\n\n"));
        assert!(c.contains(NO_HELP));
        assert!(c.ends_with("Command CRTLIB in library QSYS."));
    }

    #[test]
    fn a_qualifier_help_names_its_parameter() {
        let f = Form::new(definition(find("DSPOBJD").unwrap()));
        let obj = f.parameters().find(|p| p.keyword == "OBJ").unwrap().index;
        let lib = f.children(obj).nth(1).unwrap();
        assert_eq!(lib.role, Role::Qualifier);
        let h = node_help(&f, lib.index);
        assert!(h.starts_with("Library (OBJ)"), "{h}");
        assert!(h.contains("Special values: *LIBL, *CURLIB"));
    }

    #[test]
    fn rows_count_the_additional_parameters_only_when_shown() {
        let f = Form::new(definition(find("CRTPF").unwrap()));
        // FILE and its two qualifiers.
        assert_eq!(rows_needed(&f, false), 3);
        // The heading, SIZE and its three elements.
        assert_eq!(rows_needed(&f, true), 3 + 1 + 4);
    }
}
