// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! Data Transfer from the system: a table or a query to a file.
//!
//! - Source: a table (Browse... picks one from the catalog) with a Where
//!   clause, or a query with Check (`Prepare`, then `Release`).
//! - Columns: the source's columns, those to write in their order (Add,
//!   Remove, Up, Down; none chosen writes them all), and a row limit.
//! - Output: CSV or TSV (delimiter, quote, header row, UTF-8 with or
//!   without BOM), XLSX or ODS (sheet name, header row), or fixed width
//!   (widths from the columns' lengths, editable); dates in ISO 8601 or the
//!   job's format; the decimal point; the file, and what to do when it is
//!   there (replace, ask, or append for CSV, TSV and fixed width).
//! - Run: the rows streamed to a temporary file renamed at the end, with
//!   progress and Cancel; a cancel or failure leaves nothing behind.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use fltk::app;
use fltk::button::{CheckButton, RadioRoundButton};
use fltk::enums::Align;
use fltk::group::Group;
use fltk::input::{Input, MultilineInput};
use fltk::menu::Choice;
use fltk::prelude::*;
use fltk::window::Window;
use qap_client::proto::ops::sql::Column;
use qsa_config::System;
use qsa_transfer::{
    Dates, Download, Encoding, Existing, Failure, FileOptions, Format, Source, download,
};
use qsa_ui::chan::{self, Emit};
use qsa_ui::listtable::{Cell, ListTable};
use qsa_ui::statusbar::{Status, StatusLine};
use qsa_ui::theme::{self, colors};
use qsa_ui::{Tabs, controls, msgdetails};

use crate::app::Shared;
use crate::qap::{self, Handle};

/// Window width and height.
pub const W: i32 = 660;
pub const H: i32 = 500;

/// What the controls ask for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Act {
    SourceKind,
    Browse,
    Check,
    Columns,
    Add,
    Remove,
    Up,
    Down,
    Format,
    WidthSelected,
    SetWidth,
    BrowseFile,
    OpenRequest,
    SaveRequest,
    Run,
    Close,
}

/// The window's widgets.
pub struct Widgets {
    pub window: Window,
    pub tabs: Tabs,
    pub from_table: RadioRoundButton,
    pub from_query: RadioRoundButton,
    pub schema: Input,
    pub table: Input,
    pub where_clause: Input,
    pub query: MultilineInput,
    pub available: ListTable,
    pub chosen: ListTable,
    pub limit: Input,
    pub format: Choice,
    pub delimiter: Input,
    pub quote: Input,
    pub header: CheckButton,
    pub encoding: Choice,
    pub sheet: Input,
    pub widths: ListTable,
    pub width: Input,
    pub dates: Choice,
    pub point: Choice,
    pub file: Input,
    pub existing: Choice,
    pub status: StatusLine,
    text_group: Group,
    sheet_group: Group,
    fixed_group: Group,
}

/// The formats offered, in [`Format::ALL`]'s order.
fn format_index(f: Format) -> i32 {
    Format::ALL
        .iter()
        .position(|x| *x == f)
        .map_or(0, |i| i as i32)
}

const EXISTING: [(&str, Existing); 3] = [
    ("Replace it", Existing::Replace),
    ("Ask", Existing::Ask),
    ("Append to it", Existing::Append),
];

fn build_widgets(system: &str) -> (Widgets, chan::Receiver<Act>) {
    let (tx, rx) = chan::channel::<Act>();
    let mut window = Window::new(0, 0, W, H, None);
    window.set_label(&qap::window_title(
        "Data Transfer from System",
        system,
        None,
    ));
    window.set_icon(Some(qsa_ui::glyph::app_icon()));
    window.set_color(colors::FACE);
    let m = 10;
    let fh = theme::FIELD_H;
    let (bw, bh) = (theme::BUTTON_W, theme::BUTTON_H);
    let mut tabs = Tabs::new(m, m, W - 2 * m, H - 3 * m - bh - 24);
    let mut parts = None;
    tabs.add_page("Source", |r| {
        let mut from_table = controls::radio(r.x, r.y, 120, fh, "A table");
        from_table.set_value(true);
        from_table.emit_to(tx, Act::SourceKind);
        let lx = r.x + 20;
        controls::label(lx, r.y + 28, 60, fh, "Schema:").set_align(Align::Left | Align::Inside);
        let schema = controls::field(lx + 64, r.y + 28, 140, fh);
        controls::label(lx + 214, r.y + 28, 44, fh, "Table:")
            .set_align(Align::Left | Align::Inside);
        let table = controls::field(lx + 258, r.y + 28, 160, fh);
        let mut browse = controls::button(lx + 426, r.y + 27, bw, bh, "Browse...");
        browse.emit_to(tx, Act::Browse);
        controls::label(lx, r.y + 56, 60, fh, "Where:").set_align(Align::Left | Align::Inside);
        let mut where_clause = controls::field(lx + 64, r.y + 56, r.w - 84, fh);
        where_clause
            .set_tooltip("A condition for the rows, without the word WHERE; blank for every row.");
        let mut from_query = controls::radio(r.x, r.y + 92, 160, fh, "An SQL query");
        from_query.emit_to(tx, Act::SourceKind);
        let mut query = MultilineInput::new(lx, r.y + 118, r.w - 20, r.h - 118 - bh - 8, None);
        query.set_text_font(theme::MONO_FONT);
        query.set_text_size(theme::MONO_SIZE);
        let mut check = controls::button(lx, r.y + r.h - bh, bw, bh, "Check");
        check.set_tooltip("Prepares the query on the system without running it.");
        check.emit_to(tx, Act::Check);
        parts = Some((from_table, from_query, schema, table, where_clause, query));
    });
    let (from_table, from_query, schema, table, where_clause, query) = parts.expect("built");
    let mut parts = None;
    tabs.add_page("Columns", |r| {
        let lw = (r.w - bw - 20) / 2;
        let mut get = controls::button(r.x, r.y, bw + 30, bh, "Get Columns");
        get.set_tooltip("Reads the source's columns from the system.");
        get.emit_to(tx, Act::Columns);
        let ly = r.y + bh + 6;
        let lh = r.h - (ly - r.y) - fh - 10;
        let mut available = ListTable::new(r.x, ly, lw, lh, &[("Available", 120), ("Type", 110)]);
        available.set_multi(true);
        let bx = r.x + lw + 10;
        for (i, (label, a)) in [
            ("Add >", Act::Add),
            ("< Remove", Act::Remove),
            ("Up", Act::Up),
            ("Down", Act::Down),
        ]
        .into_iter()
        .enumerate()
        {
            let mut b = controls::button(bx, ly + 20 + i as i32 * (bh + 6), bw, bh, label);
            b.emit_to(tx, a);
        }
        let chosen = ListTable::new(
            bx + bw + 10,
            ly,
            lw,
            lh,
            &[("Columns to write", 120), ("Type", 110)],
        );
        let yy = r.y + r.h - fh;
        controls::label(r.x, yy, 70, fh, "Row limit:").set_align(Align::Left | Align::Inside);
        let mut limit = controls::field(r.x + 72, yy, 90, fh);
        limit.set_tooltip("Blank for every row.");
        let mut note = controls::label(r.x + 170, yy, 300, fh, "None chosen writes every column.");
        note.set_align(Align::Left | Align::Inside);
        parts = Some((available, chosen, limit));
    });
    let (available, chosen, limit) = parts.expect("built");
    let mut parts = None;
    tabs.add_page("Output", |r| {
        let lw = 96;
        let row = |i: i32| r.y + i * (fh + 8);
        let label = |y: i32, t: &str| {
            let mut l = controls::label(r.x, y, lw, fh, t);
            l.set_align(Align::Left | Align::Inside);
        };
        label(row(0), "Format:");
        let mut format = controls::combo(r.x + lw, row(0), 230, fh);
        for f in Format::ALL {
            format.add_choice(f.label());
        }
        format.set_value(0);
        format.emit_to(tx, Act::Format);
        // The format's own options, one group shown at a time.
        let gy = row(1);
        let gh = 3 * (fh + 8);
        let text_group = Group::new(r.x, gy, r.w, gh, None);
        label(gy, "Delimiter:");
        let mut delimiter = controls::field(r.x + lw, gy, 40, fh);
        delimiter.set_value(",");
        controls::label(r.x + lw + 60, gy, 50, fh, "Quote:").set_align(Align::Left | Align::Inside);
        let mut quote = controls::field(r.x + lw + 110, gy, 40, fh);
        quote.set_value("\"");
        label(gy + fh + 8, "Encoding:");
        let mut encoding = controls::combo(r.x + lw, gy + fh + 8, 160, fh);
        encoding.add_choice("UTF-8");
        encoding.add_choice("UTF-8 with BOM");
        encoding.set_value(0);
        text_group.end();
        let sheet_group = Group::new(r.x, gy, r.w, gh, None);
        label(gy, "Sheet name:");
        let sheet = controls::field(r.x + lw, gy, 200, fh);
        sheet_group.end();
        let fixed_group = Group::new(r.x, gy, r.w, gh, None);
        let mut widths =
            ListTable::new(r.x + lw, gy, 260, gh - 4, &[("Column", 140), ("Width", 70)]);
        widths.on_select(move |_| tx.send(Act::WidthSelected));
        label(gy, "Widths:");
        let width = controls::field(r.x + lw + 270, gy, 60, fh);
        let mut set = controls::button(r.x + lw + 336, gy - 1, bw, bh, "Set");
        set.emit_to(tx, Act::SetWidth);
        fixed_group.end();
        let mut header =
            controls::check(r.x + lw, row(4), 200, fh, "Column names in the first row");
        header.set_value(true);
        label(row(5), "Dates:");
        let mut dates = controls::combo(r.x + lw, row(5), 200, fh);
        dates.add_choice("ISO 8601");
        dates.add_choice("The job's date format");
        dates.set_value(0);
        controls::label(r.x + lw + 214, row(5), 90, fh, "Decimal point:")
            .set_align(Align::Left | Align::Inside);
        let mut point = controls::combo(r.x + lw + 304, row(5), 90, fh);
        point.add_choice("Period (.)");
        point.add_choice("Comma (,)");
        point.set_value(0);
        label(row(6), "File:");
        let file = controls::field(r.x + lw, row(6), r.w - lw - bw - 8, fh);
        let mut browse = controls::button(r.x + r.w - bw, row(6) - 1, bw, bh, "Browse...");
        browse.emit_to(tx, Act::BrowseFile);
        label(row(7), "If it is there:");
        let mut existing = controls::combo(r.x + lw, row(7), 160, fh);
        for (t, _) in EXISTING {
            existing.add_choice(t);
        }
        existing.set_value(1);
        parts = Some((
            format,
            delimiter,
            quote,
            header,
            encoding,
            sheet,
            widths,
            width,
            dates,
            point,
            file,
            existing,
            text_group,
            sheet_group,
            fixed_group,
        ));
    });
    let (
        format,
        delimiter,
        quote,
        header,
        encoding,
        sheet,
        widths,
        width,
        dates,
        point,
        file,
        existing,
        text_group,
        sheet_group,
        fixed_group,
    ) = parts.expect("built");
    let mut open_request = controls::button(m, H - m - bh, bw + 34, bh, "Open Request...");
    open_request.emit_to(tx, Act::OpenRequest);
    let mut save_request =
        controls::button(m + bw + 40, H - m - bh, bw + 34, bh, "Save Request...");
    save_request.emit_to(tx, Act::SaveRequest);
    let status = StatusLine::new(m, H - m - bh - 26, W - 2 * m, 22);
    let mut run = controls::default_button(W - m - 2 * bw - 6, H - m - bh, bw, bh, "Run");
    run.emit_to(tx, Act::Run);
    let mut close = controls::button(W - m - bw, H - m - bh, bw, bh, "Close");
    close.emit_to(tx, Act::Close);
    window.end();
    window.set_callback(move |_| tx.send(Act::Close));
    let w = Widgets {
        window,
        tabs,
        from_table,
        from_query,
        schema,
        table,
        where_clause,
        query,
        available,
        chosen,
        limit,
        format,
        delimiter,
        quote,
        header,
        encoding,
        sheet,
        widths,
        width,
        dates,
        point,
        file,
        existing,
        status,
        text_group,
        sheet_group,
        fixed_group,
    };
    w.show_format();
    w.show_source_kind();
    (w, rx)
}

impl Widgets {
    fn format(&self) -> Format {
        Format::ALL
            .get(usize::try_from(self.format.value()).unwrap_or(0))
            .copied()
            .unwrap_or_default()
    }

    /// The format's own options, the others hidden.
    fn show_format(&self) {
        let f = self.format();
        let show = |g: &Group, on: bool| {
            let mut g = g.clone();
            if on {
                g.show();
            } else {
                g.hide();
            }
        };
        show(&self.text_group, matches!(f, Format::Csv | Format::Tsv));
        show(&self.sheet_group, f.has_sheets());
        show(&self.fixed_group, f == Format::Fixed);
        let mut d = self.delimiter.clone();
        if f == Format::Tsv {
            d.set_value("Tab");
            d.deactivate();
        } else {
            if d.value() == "Tab" {
                d.set_value(",");
            }
            d.activate();
        }
        let mut h = self.header.clone();
        if f == Format::Fixed {
            h.deactivate();
        } else {
            h.activate();
        }
        let mut e = self.existing.clone();
        if !f.appends() && e.value() == 2 {
            e.set_value(1);
        }
    }

    fn show_source_kind(&self) {
        let table = self.from_table.value();
        for w in [&self.schema, &self.table, &self.where_clause] {
            let mut w = w.clone();
            if table {
                w.activate();
            } else {
                w.deactivate();
            }
        }
        let mut q = self.query.clone();
        if table {
            q.deactivate();
        } else {
            q.activate();
        }
    }
}

/// The window and what it knows of the source's columns.
pub struct DownloadWindow {
    pub w: Widgets,
    rx: chan::Receiver<Act>,
    /// The source's columns, from the system.
    columns: RefCell<Vec<Column>>,
    /// The columns chosen, by name, in order.
    chosen: RefCell<Vec<String>>,
    /// Fixed-width widths set by hand, by column name.
    widths: RefCell<Vec<(String, usize)>>,
    sys: System,
    h: Option<Handle>,
    state: Option<Shared>,
}

impl DownloadWindow {
    /// The window for `system`, filled from `d`, not shown and without a
    /// connection: for screenshots, and before [`open`] shows it.
    pub fn build(system: &System, d: &Download, columns: &[Column]) -> DownloadWindow {
        let (w, rx) = build_widgets(&system.name);
        let win = DownloadWindow {
            w,
            rx,
            columns: RefCell::new(columns.to_vec()),
            chosen: RefCell::new(d.columns.clone()),
            widths: RefCell::new(Vec::new()),
            sys: system.clone(),
            h: None,
            state: None,
        };
        win.fill(d);
        win
    }

    /// Shows request `d` in the controls.
    pub fn fill(&self, d: &Download) {
        let w = &self.w;
        match &d.source {
            Source::Table {
                schema,
                table,
                where_clause,
            } => {
                w.from_table.clone().set_value(true);
                w.from_query.clone().set_value(false);
                w.schema.clone().set_value(schema);
                w.table.clone().set_value(table);
                w.where_clause.clone().set_value(where_clause);
            }
            Source::Query { sql } => {
                w.from_table.clone().set_value(false);
                w.from_query.clone().set_value(true);
                w.query.clone().set_value(sql);
            }
        }
        *self.chosen.borrow_mut() = d.columns.clone();
        w.limit
            .clone()
            .set_value(&d.limit.map(|n| n.to_string()).unwrap_or_default());
        let o = &d.options;
        w.format.clone().set_value(format_index(o.format));
        if o.format != Format::Tsv {
            w.delimiter.clone().set_value(&o.delimiter.to_string());
        }
        w.quote.clone().set_value(&o.quote.to_string());
        w.header.clone().set_value(o.header);
        w.encoding
            .clone()
            .set_value(i32::from(o.encoding == Encoding::Utf8Bom));
        w.sheet.clone().set_value(&o.sheet);
        w.dates.clone().set_value(i32::from(o.dates == Dates::Job));
        w.point.clone().set_value(i32::from(o.point == ','));
        if !d.file.as_os_str().is_empty() {
            w.file.clone().set_value(&d.file.display().to_string());
        }
        let e = EXISTING
            .iter()
            .position(|(_, x)| *x == d.existing)
            .unwrap_or(1);
        w.existing.clone().set_value(e as i32);
        let names: Vec<String> = self
            .columns
            .borrow()
            .iter()
            .map(|c| c.name.clone())
            .collect();
        *self.widths.borrow_mut() = names
            .iter()
            .zip(o.widths.iter())
            .map(|(n, w)| (n.clone(), *w))
            .collect();
        w.show_source_kind();
        w.show_format();
        self.show_columns();
    }

    /// The source as the controls say.
    fn source(&self) -> Source {
        let w = &self.w;
        if w.from_table.value() {
            Source::Table {
                schema: w.schema.value().trim().to_ascii_uppercase(),
                table: w.table.value().trim().to_ascii_uppercase(),
                where_clause: w.where_clause.value().trim().to_owned(),
            }
        } else {
            Source::Query {
                sql: w.query.value(),
            }
        }
    }

    /// The columns written, in order: those chosen, else every one.
    fn written(&self) -> Vec<Column> {
        let all = self.columns.borrow();
        let chosen = self.chosen.borrow();
        if chosen.is_empty() {
            return all.clone();
        }
        chosen
            .iter()
            .filter_map(|n| all.iter().find(|c| c.name.eq_ignore_ascii_case(n)).cloned())
            .collect()
    }

    /// The request the controls make; why not, when they make none.
    pub fn request(&self) -> Result<Download, String> {
        let w = &self.w;
        let source = self.source();
        match &source {
            Source::Table { schema, table, .. } if schema.is_empty() || table.is_empty() => {
                return Err("Name the schema and the table, or choose Browse.".into());
            }
            Source::Query { sql } if sql.trim().is_empty() => return Err("Type the query.".into()),
            _ => {}
        }
        let limit = match w.limit.value().trim() {
            "" => None,
            t => Some(t.parse::<u64>().map_err(|_| {
                "The row limit is a whole number, or blank for every row.".to_owned()
            })?),
        };
        let file = w.file.value().trim().to_owned();
        if file.is_empty() {
            return Err("Name the output file, or choose Browse.".into());
        }
        let format = w.format();
        let one = |i: &Input, what: &str| -> Result<char, String> {
            let v = i.value();
            let mut c = v.chars();
            match (c.next(), c.next()) {
                (Some(ch), None) => Ok(ch),
                _ => Err(format!("The {what} is one character.")),
            }
        };
        let delimiter = if format == Format::Tsv {
            '\t'
        } else {
            one(&w.delimiter, "delimiter")?
        };
        let quote = one(&w.quote, "quote")?;
        let written = self.written();
        let widths: Vec<usize> = if format == Format::Fixed {
            let set = self.widths.borrow();
            written
                .iter()
                .map(|c| {
                    set.iter()
                        .find(|(n, _)| n == &c.name)
                        .map_or_else(|| qsa_transfer::writer::default_width(c), |(_, w)| *w)
                })
                .collect()
        } else {
            Vec::new()
        };
        Ok(Download {
            source,
            columns: self.chosen.borrow().clone(),
            limit,
            file: PathBuf::from(file),
            options: FileOptions {
                format,
                delimiter,
                quote,
                header: w.header.value() && format != Format::Fixed,
                encoding: if w.encoding.value() == 1 {
                    Encoding::Utf8Bom
                } else {
                    Encoding::Utf8
                },
                sheet: w.sheet.value().trim().to_owned(),
                widths,
                null_indicators: true,
                dates: if w.dates.value() == 1 {
                    Dates::Job
                } else {
                    Dates::Iso
                },
                point: if w.point.value() == 1 { ',' } else { '.' },
                bit_data_as_text: false,
            },
            existing: EXISTING
                .get(usize::try_from(w.existing.value()).unwrap_or(1))
                .map_or(Existing::Ask, |(_, e)| *e),
        })
    }

    /// The Columns page and the fixed widths from what is known.
    fn show_columns(&self) {
        let all = self.columns.borrow();
        let chosen = self.chosen.borrow();
        let row = |c: &Column| {
            vec![
                Cell::from(c.name.as_str()),
                Cell::from(qsa_sql::value::type_name(c)),
            ]
        };
        self.w.available.clone().set_rows(
            all.iter()
                .filter(|c| !chosen.iter().any(|n| n.eq_ignore_ascii_case(&c.name)))
                .map(row)
                .collect(),
        );
        self.w.chosen.clone().set_rows(
            chosen
                .iter()
                .map(
                    |n| match all.iter().find(|c| c.name.eq_ignore_ascii_case(n)) {
                        Some(c) => row(c),
                        None => vec![Cell::from(n.as_str()), Cell::from("")],
                    },
                )
                .collect(),
        );
        drop((all, chosen));
        let written = self.written();
        let set = self.widths.borrow();
        self.w.widths.clone().set_rows(
            written
                .iter()
                .map(|c| {
                    let width = set
                        .iter()
                        .find(|(n, _)| n == &c.name)
                        .map_or_else(|| qsa_transfer::writer::default_width(c), |(_, w)| *w);
                    vec![Cell::from(c.name.as_str()), Cell::from(width.to_string())]
                })
                .collect(),
        );
    }

    fn report(&self, text: &str, s: Status) {
        self.w.status.clone().set(text, s);
    }

    fn parent(&self) -> Option<&dyn WindowExt> {
        Some(&self.w.window as &dyn WindowExt)
    }

    /// Takes in what the controls asked for; false once closed.
    fn pump(&self) -> bool {
        while let Some(a) = self.rx.recv() {
            self.act(a);
        }
        self.w.window.shown()
    }

    fn act(&self, a: Act) {
        match a {
            Act::SourceKind => self.w.show_source_kind(),
            Act::Format => self.w.show_format(),
            Act::Browse => {
                if let Some(h) = &self.h
                    && let Some((s, t)) = super::pick_table(self.parent(), h, "Choose a Table")
                {
                    self.w.schema.clone().set_value(&s);
                    self.w.table.clone().set_value(&t);
                    self.w.from_table.clone().set_value(true);
                    self.w.from_query.clone().set_value(false);
                    self.w.show_source_kind();
                    self.get_columns();
                }
            }
            Act::Check => self.check(),
            Act::Columns => self.get_columns(),
            Act::Add => {
                let sel = self.w.available.selected_rows();
                let names: Vec<String> = {
                    let all = self.columns.borrow();
                    let chosen = self.chosen.borrow();
                    all.iter()
                        .filter(|c| !chosen.iter().any(|n| n.eq_ignore_ascii_case(&c.name)))
                        .enumerate()
                        .filter(|(i, _)| sel.contains(i))
                        .map(|(_, c)| c.name.clone())
                        .collect()
                };
                self.chosen.borrow_mut().extend(names);
                self.show_columns();
            }
            Act::Remove => {
                if let Some(i) = self.w.chosen.selected() {
                    let mut c = self.chosen.borrow_mut();
                    if i < c.len() {
                        c.remove(i);
                    }
                    drop(c);
                    self.show_columns();
                }
            }
            Act::Up | Act::Down => {
                let Some(i) = self.w.chosen.selected() else {
                    return;
                };
                let mut c = self.chosen.borrow_mut();
                let j = if a == Act::Up {
                    i.checked_sub(1)
                } else {
                    (i + 1 < c.len()).then_some(i + 1)
                };
                if let Some(j) = j {
                    c.swap(i, j);
                    drop(c);
                    self.show_columns();
                    self.w.chosen.clone().select(Some(j));
                }
            }
            Act::WidthSelected => {
                if let Some(row) = self.w.widths.selected().and_then(|i| self.w.widths.row(i)) {
                    self.w.width.clone().set_value(&row[1].text);
                }
            }
            Act::SetWidth => self.set_width(),
            Act::BrowseFile => {
                let f = self.w.format();
                let pattern = format!("{} (*.{})\tAll Files (*)", f.label(), f.extension());
                let name = match self.source() {
                    Source::Table { table, .. } if !table.is_empty() => {
                        format!("{}.{}", table, f.extension())
                    }
                    _ => format!("result.{}", f.extension()),
                };
                if let Some(mut p) =
                    crate::files::choose_save("Data Transfer Output File", &pattern, &name)
                {
                    if p.extension().is_none() {
                        p.set_extension(f.extension());
                    }
                    self.w.file.clone().set_value(&p.display().to_string());
                }
            }
            Act::OpenRequest => {
                if let Some(st) = &self.state {
                    super::choose_request(st, self.parent());
                }
            }
            Act::SaveRequest => self.save_request(),
            Act::Run => self.run(),
            Act::Close => self.w.window.clone().hide(),
        }
    }

    /// The width field shows the selected column's; Set changes it.
    fn set_width(&self) {
        let Some(i) = self.w.widths.selected() else {
            return;
        };
        let Some(row) = self.w.widths.row(i) else {
            return;
        };
        let name = row[0].text.clone();
        let typed = self.w.width.value();
        match typed.trim().parse::<usize>() {
            Ok(n) if n > 0 => {
                let mut set = self.widths.borrow_mut();
                set.retain(|(c, _)| c != &name);
                set.push((name, n));
                drop(set);
                self.show_columns();
                self.w.widths.clone().select(Some(i));
            }
            _ => self.report("A width is a whole number of characters.", Status::Error),
        }
    }

    /// The source's columns: a table's from the catalog, a query's from
    /// `Prepare`.
    fn get_columns(&self) {
        let Some(h) = &self.h else { return };
        let source = self.source();
        let r = match &source {
            Source::Table { schema, table, .. } => {
                let (s, t) = (schema.clone(), table.clone());
                qap::busy(|| qap::lock(h).list_columns(&s, &t))
            }
            Source::Query { sql } => {
                let Some(mut rc) = super::attach(self.parent(), h, &self.sys) else {
                    return;
                };
                let sql = sql.clone();
                qap::busy(move || {
                    let p = rc.q.prepare(&sql)?;
                    let _ = rc.q.release(p.statement);
                    Ok(p.columns)
                })
            }
        };
        match r {
            Ok(cols) => {
                let n = cols.len();
                *self.columns.borrow_mut() = cols;
                self.show_columns();
                self.report(&format!("{n} columns."), Status::Neutral);
            }
            Err(e) => {
                let x = qap::explain(&self.sys, &e);
                self.report(&x.line(), Status::Error);
                qap::show(self.parent(), &x);
            }
        }
    }

    /// Check: the query prepared on the system and released, nothing run.
    fn check(&self) {
        let Some(h) = &self.h else { return };
        let Source::Query { sql } = self.source() else {
            self.report("Check is for a query.", Status::Neutral);
            return;
        };
        let Some(mut rc) = super::attach(self.parent(), h, &self.sys) else {
            return;
        };
        let r = qap::busy(move || {
            let p = rc.q.prepare(&sql)?;
            let _ = rc.q.release(p.statement);
            Ok::<_, qap_client::Error>(p.columns)
        });
        match r {
            Ok(cols) if cols.is_empty() => {
                self.report("The statement is not a query.", Status::Error)
            }
            Ok(cols) => {
                let n = cols.len();
                *self.columns.borrow_mut() = cols;
                self.show_columns();
                self.report(
                    &format!("The query is valid: {n} columns."),
                    Status::Neutral,
                );
            }
            Err(e) => {
                let x = qap::explain(&self.sys, &e);
                self.report(&x.line(), Status::Error);
                qap::show(self.parent(), &x);
            }
        }
    }

    fn run(&self) {
        let Some(h) = self.h.clone() else { return };
        let mut d = match self.request() {
            Ok(d) => d,
            Err(t) => return self.report(&t, Status::Error),
        };
        if d.file.exists() && d.existing == Existing::Ask {
            let q = format!("{} is there already. Replace it?", d.file.display());
            let mb =
                qsa_ui::MessageBox::new("Data Transfer", &q).buttons(&["Replace", "Cancel"], 1, 1);
            if mb.run(self.parent()) != 0 {
                return;
            }
            d.existing = Existing::Replace;
        }
        if self.columns.borrow().is_empty() && !d.columns.is_empty() {
            self.get_columns();
        }
        let Some(rc) = super::attach(self.parent(), &h, &self.sys) else {
            return;
        };
        let name = d
            .file
            .file_name()
            .map_or_else(|| "download".into(), |n| n.to_string_lossy().into_owned());
        crate::log!(
            "Data Transfer from {} to {}",
            self.sys.name,
            d.file.display()
        );
        let req = d.clone();
        let r = super::with_progress(self.parent(), &name, move |progress, cancel| {
            let mut rc = rc;
            let r = download::run(&mut rc.q, &req, rc.dates, rc.ccsid, progress, cancel);
            rc.q.close();
            r
        });
        self.summary(&name, &d, r);
    }

    fn summary(&self, name: &str, d: &Download, r: Result<qsa_transfer::Summary, Failure>) {
        let file = d.file.display().to_string();
        let m = match r {
            Ok(s) => super::complete(name, s.rows, s.elapsed),
            Err(Failure::Cancelled { .. }) => super::cancelled(name),
            Err(f) => {
                let why = match f.server_message() {
                    Some(sm) => format!("{}: {}", sm.id, sm.text),
                    None => match &f {
                        Failure::System(e) => qap::explain(&self.sys, e).line(),
                        f => f.to_string(),
                    },
                };
                super::failed(name, 1, 1, &file, &why)
            }
        };
        crate::log!("{m}");
        let s = if m.kind() == qsa_msg::Kind::Error {
            Status::Error
        } else {
            Status::Neutral
        };
        self.report(&m.to_string(), s);
        msgdetails::show_message(self.parent(), &m);
    }
}

thread_local! {
    static WINDOWS: RefCell<Vec<Rc<DownloadWindow>>> = const { RefCell::new(Vec::new()) };
}

/// Opens Data Transfer from the system for `system`, signing on first when
/// this program has no session on it, filled from `preset` (Schemas'
/// Transfer data from..., a `.qdx` request).
pub fn open(
    parent: Option<&dyn WindowExt>,
    state: &Shared,
    system: &str,
    preset: Option<&Download>,
) {
    let Some((sys, h)) = super::session(parent, state, system) else {
        return;
    };
    let d = preset.cloned().unwrap_or_default();
    let mut win = DownloadWindow::build(&sys, &d, &[]);
    win.h = Some(h);
    win.state = Some(state.clone());
    let win = Rc::new(win);
    let profile = qap::signed_on(state, &sys.name).unwrap_or_default();
    win.report(&format!("{}: {profile}", sys.name), Status::Neutral);
    if preset.is_some() {
        win.get_columns();
    }
    let mut w = win.w.window.clone();
    qsa_ui::propdialog::center_on(&mut w, parent);
    qsa_ui::propdialog::show_placed(&mut w);
    WINDOWS.with(|v| v.borrow_mut().push(win));
    tick();
}

fn tick() {
    thread_local! { static TICKING: std::cell::Cell<bool> = const { std::cell::Cell::new(false) }; }
    if TICKING.with(|t| t.replace(true)) {
        return;
    }
    app::add_timeout3(0.05, |h| {
        let windows: Vec<Rc<DownloadWindow>> = WINDOWS.with(|v| v.borrow().clone());
        let open: Vec<bool> = windows.iter().map(|w| w.pump()).collect();
        WINDOWS.with(|v| {
            let mut i = 0;
            v.borrow_mut().retain(|_| {
                let keep = open.get(i).copied().unwrap_or(true);
                i += 1;
                keep
            });
        });
        if WINDOWS.with(|v| !v.borrow().is_empty()) {
            app::repeat_timeout3(0.05, h);
        } else {
            TICKING.with(|t| t.set(false));
        }
    });
}

impl DownloadWindow {
    /// Save Request: the request the controls make, to a `.qdx` file.
    fn save_request(&self) {
        let r = match self.request() {
            Ok(x) => qsa_transfer::request::Request::Download {
                system: self.sys.name.clone(),
                download: x,
            },
            Err(t) => return self.report(&t, Status::Error),
        };
        match super::save_request(&r) {
            Some(Ok(m)) => self.report(&m.to_string(), Status::Neutral),
            Some(Err(t)) => self.report(&t, Status::Error),
            None => {}
        }
    }
}

/// Whether any of these windows is open.
pub fn any_open() -> bool {
    WINDOWS.with(|v| v.borrow().iter().any(|w| w.w.window.shown()))
}
