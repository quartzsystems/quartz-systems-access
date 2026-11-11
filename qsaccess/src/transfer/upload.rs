// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! Data Transfer to the system: a file's rows into a table.
//!
//! - Input: CSV, TSV, XLSX or ODS (with its sheet), or fixed width; a
//!   header row; the encoding of a text file; and a preview of the first
//!   50 rows.
//! - Target: a table (Browse... picks one), and Append or Replace (DELETE
//!   then insert, in the same transaction under commitment control;
//!   without it the delete cannot be undone, which the window says first).
//! - Mapping: each target column's source, by name or position (Map by
//!   Name, Map by Position, or chosen by hand); unmapped columns get their
//!   defaults; the first value of each column that does not fit, from the
//!   preview (`QAD7035`).
//! - Commit: one transaction (the default), a commit every N batches, or
//!   none; the row to resume from.
//! - Run: an `INSERT` prepared once and `Execute` over batches under 1 MiB,
//!   with progress and Cancel. A failure names the row, its line in the
//!   file and the system's message (`QAD7031`); when rows were kept, it
//!   offers to resume from the first row not kept (`QAD7033`). A table that
//!   cannot use commitment control (`SQL7008`) offers to upload without it
//!   (`QAD7034`).

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use fltk::app;
use fltk::button::{CheckButton, RadioRoundButton};
use fltk::enums::Align;
use fltk::input::Input;
use fltk::menu::Choice;
use fltk::prelude::*;
use fltk::window::Window;
use qap_client::proto::ops::sql::Column;
use qsa_config::System;
use qsa_msg::id;
use qsa_transfer::reader::{Sheet, Value};
use qsa_transfer::upload::{self, Failure, Stop};
use qsa_transfer::{Commit, Dates, Encoding, FileOptions, Format, Map, Mode, SourceColumn, Upload};
use qsa_ui::chan::{self, Emit};
use qsa_ui::listtable::{Cell, ListTable};
use qsa_ui::statusbar::{Status, StatusLine};
use qsa_ui::theme::{self, colors};
use qsa_ui::{Tabs, controls, msgdetails};

use crate::app::Shared;
use crate::qap::{self, Handle};

/// Window width and height.
pub const W: i32 = 700;
pub const H: i32 = 520;

/// Rows the preview shows.
pub const PREVIEW_ROWS: usize = 50;

/// What the controls ask for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Act {
    BrowseFile,
    Format,
    Preview,
    Browse,
    Columns,
    MapByName,
    MapByPosition,
    MappingSelected,
    SetMapping,
    OpenRequest,
    SaveRequest,
    Run,
    Close,
}

/// The window's widgets.
pub struct Widgets {
    pub window: Window,
    pub tabs: Tabs,
    pub file: Input,
    pub format: Choice,
    pub encoding: Choice,
    pub delimiter: Input,
    pub quote: Input,
    pub header: CheckButton,
    pub sheet: Choice,
    pub preview: ListTable,
    pub schema: Input,
    pub table: Input,
    pub append: RadioRoundButton,
    pub replace: RadioRoundButton,
    pub mapping: ListTable,
    pub source: Choice,
    pub once: RadioRoundButton,
    pub every: RadioRoundButton,
    pub batches: Input,
    pub without: RadioRoundButton,
    pub resume: Input,
    pub dates: Choice,
    pub point: Choice,
    pub bits: Choice,
    pub status: StatusLine,
}

/// The mapping grid's columns.
pub const MAPPING_COLUMNS: [(&str, i32); 4] = [
    ("Target column", 140),
    ("Type", 120),
    ("Source", 140),
    ("First problem", 220),
];

fn build_widgets(system: &str) -> (Widgets, chan::Receiver<Act>) {
    let (tx, rx) = chan::channel::<Act>();
    let mut window = Window::new(0, 0, W, H, None);
    crate::help::attach(&mut window, crate::help::topic::DATA_TRANSFER);
    window.set_label(&qap::window_title("Data Transfer to System", system, None));
    window.set_icon(Some(qsa_ui::glyph::app_icon()));
    window.set_color(colors::FACE);
    let m = 10;
    let fh = theme::FIELD_H;
    let (bw, bh) = (theme::BUTTON_W, theme::BUTTON_H);
    let label = |x: i32, y: i32, w: i32, t: &str| {
        let mut l = controls::label(x, y, w, fh, t);
        l.set_align(Align::Left | Align::Inside);
    };
    let mut tabs = Tabs::new(m, m, W - 2 * m, H - 3 * m - bh - 24);
    let mut p = None;
    tabs.add_page("Input", |r| {
        let lw = 80;
        label(r.x, r.y, lw, "File:");
        let file = controls::field(r.x + lw, r.y, r.w - lw - bw - 8, fh);
        let mut browse = controls::button(r.x + r.w - bw, r.y - 1, bw, bh, "Browse...");
        browse.emit_to(tx, Act::BrowseFile);
        let y2 = r.y + fh + 8;
        label(r.x, y2, lw, "Format:");
        let mut format = controls::combo(r.x + lw, y2, 220, fh);
        for f in Format::ALL {
            format.add_choice(f.label());
        }
        format.set_value(0);
        format.emit_to(tx, Act::Format);
        label(r.x + lw + 236, y2, 60, "Sheet:");
        let sheet = controls::combo(r.x + lw + 296, y2, 160, fh);
        let y3 = y2 + fh + 8;
        label(r.x, y3, lw, "Encoding:");
        let mut encoding = controls::combo(r.x + lw, y3, 140, fh);
        for e in Encoding::READ {
            encoding.add_choice(e.label());
        }
        encoding.set_value(0);
        label(r.x + lw + 156, y3, 64, "Delimiter:");
        let mut delimiter = controls::field(r.x + lw + 220, y3, 36, fh);
        delimiter.set_value(",");
        label(r.x + lw + 270, y3, 44, "Quote:");
        let mut quote = controls::field(r.x + lw + 314, y3, 36, fh);
        quote.set_value("\"");
        let y4 = y3 + fh + 8;
        let mut header = controls::check(r.x + lw, y4, 240, fh, "Column names in the first row");
        header.set_value(true);
        let mut show = controls::button(r.x + r.w - bw, y4 - 1, bw, bh, "Preview");
        show.emit_to(tx, Act::Preview);
        let py = y4 + fh + 10;
        let preview = ListTable::new(r.x, py, r.w, r.y + r.h - py, &[("", 80)]);
        p = Some((
            file, format, encoding, delimiter, quote, header, sheet, preview,
        ));
    });
    let (file, format, encoding, delimiter, quote, header, sheet, preview) = p.expect("built");
    let mut p = None;
    tabs.add_page("Target", |r| {
        let lw = 80;
        label(r.x, r.y, lw, "Schema:");
        let schema = controls::field(r.x + lw, r.y, 150, fh);
        label(r.x + lw + 166, r.y, 50, "Table:");
        let table = controls::field(r.x + lw + 216, r.y, 170, fh);
        let mut browse = controls::button(r.x + lw + 396, r.y - 1, bw, bh, "Browse...");
        browse.emit_to(tx, Act::Browse);
        let y2 = r.y + fh + 16;
        let mut append = controls::radio(r.x + lw, y2, 300, fh, "Append the rows to the table");
        append.set_value(true);
        let replace = controls::radio(r.x + lw, y2 + fh + 6, 420, fh, "Replace the table's rows (DELETE, then insert)");
        let mut note = controls::label(
            r.x + lw + 18,
            y2 + 2 * fh + 10,
            r.w - lw - 18,
            2 * fh,
            "Under commitment control the DELETE is undone if the upload fails.\nWithout it, the DELETE cannot be undone.",
        );
        note.set_align(Align::Left | Align::Inside | Align::Top | Align::Wrap);
        note.set_label_color(colors::DISABLED_TEXT);
        p = Some((schema, table, append, replace));
    });
    let (schema, table, append, replace) = p.expect("built");
    let mut p = None;
    tabs.add_page("Mapping", |r| {
        let mut get = controls::button(r.x, r.y, bw + 30, bh, "Get Columns");
        get.emit_to(tx, Act::Columns);
        let mut by_name = controls::button(r.x + bw + 40, r.y, bw + 20, bh, "Map by Name");
        by_name.emit_to(tx, Act::MapByName);
        let mut by_pos = controls::button(r.x + 2 * bw + 70, r.y, bw + 34, bh, "Map by Position");
        by_pos.emit_to(tx, Act::MapByPosition);
        let ly = r.y + bh + 8;
        let lh = r.h - (ly - r.y) - fh - 10;
        let mut mapping = ListTable::new(r.x, ly, r.w, lh, &MAPPING_COLUMNS);
        mapping.on_select(move |_| tx.send(Act::MappingSelected));
        let yy = r.y + r.h - fh;
        label(r.x, yy, 54, "Source:");
        let source = controls::combo(r.x + 54, yy, 200, fh);
        let mut set = controls::button(r.x + 262, yy - 1, bw, bh, "Set");
        set.emit_to(tx, Act::SetMapping);
        let mut note = controls::label(
            r.x + 350,
            yy,
            r.w - 350,
            fh,
            "Unmapped columns get their defaults.",
        );
        note.set_align(Align::Left | Align::Inside);
        p = Some((mapping, source));
    });
    let (mapping, source) = p.expect("built");
    let mut p = None;
    tabs.add_page("Commit", |r| {
        let mut once = controls::radio(
            r.x,
            r.y,
            460,
            fh,
            "One transaction: a failure keeps nothing",
        );
        once.set_value(true);
        let every = controls::radio(r.x, r.y + fh + 6, 106, fh, "Commit every");
        let mut batches = controls::field(r.x + 108, r.y + fh + 6, 50, fh);
        batches.set_value("10");
        label(
            r.x + 164,
            r.y + fh + 6,
            330,
            "batches: a failure keeps the batches committed",
        );
        let without = controls::radio(
            r.x,
            r.y + 2 * (fh + 6),
            r.w,
            fh,
            "Without commitment control: a failure leaves every earlier row in place",
        );
        let y = r.y + 3 * (fh + 6) + 12;
        let lw = 130;
        label(r.x, y, lw, "Resume from row:");
        let mut resume = controls::field(r.x + lw, y, 80, fh);
        resume.set_tooltip("Blank to start at the first row.");
        label(r.x, y + fh + 8, lw, "Dates:");
        let mut dates = controls::combo(r.x + lw, y + fh + 8, 200, fh);
        dates.add_choice("ISO 8601");
        dates.add_choice("The job's date format");
        dates.set_value(0);
        label(r.x, y + 2 * (fh + 8), lw, "Decimal point:");
        let mut point = controls::combo(r.x + lw, y + 2 * (fh + 8), 120, fh);
        point.add_choice("Period (.)");
        point.add_choice("Comma (,)");
        point.set_value(0);
        label(r.x, y + 3 * (fh + 8), lw, "CCSID 65535 data:");
        let mut bits = controls::combo(r.x + lw, y + 3 * (fh + 8), 200, fh);
        bits.add_choice("Hex");
        bits.add_choice("Text in the job's CCSID");
        bits.set_value(0);
        p = Some((once, every, batches, without, resume, dates, point, bits));
    });
    let (once, every, batches, without, resume, dates, point, bits) = p.expect("built");
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
    (
        Widgets {
            window,
            tabs,
            file,
            format,
            encoding,
            delimiter,
            quote,
            header,
            sheet,
            preview,
            schema,
            table,
            append,
            replace,
            mapping,
            source,
            once,
            every,
            batches,
            without,
            resume,
            dates,
            point,
            bits,
            status,
        },
        rx,
    )
}

/// The window, the target's columns, the preview and the mapping.
pub struct UploadWindow {
    pub w: Widgets,
    rx: chan::Receiver<Act>,
    columns: RefCell<Vec<Column>>,
    preview: RefCell<Sheet>,
    mapping: RefCell<Vec<Map>>,
    sys: System,
    h: Option<Handle>,
    state: Option<Shared>,
}

impl UploadWindow {
    /// The window for `system`, filled from `u`, with the target's
    /// `columns` and a `preview`, not shown and without a connection: for
    /// screenshots, and before [`open`] shows it.
    pub fn build(system: &System, u: &Upload, columns: &[Column], preview: Sheet) -> UploadWindow {
        let (w, rx) = build_widgets(&system.name);
        let win = UploadWindow {
            w,
            rx,
            columns: RefCell::new(columns.to_vec()),
            preview: RefCell::new(preview),
            mapping: RefCell::new(u.mapping.clone()),
            sys: system.clone(),
            h: None,
            state: None,
        };
        win.fill(u);
        win.show_preview();
        win.show_mapping();
        win
    }

    /// Shows request `u` in the controls.
    pub fn fill(&self, u: &Upload) {
        let w = &self.w;
        if !u.file.as_os_str().is_empty() {
            w.file.clone().set_value(&u.file.display().to_string());
        }
        let o = &u.options;
        w.format
            .clone()
            .set_value(Format::ALL.iter().position(|f| *f == o.format).unwrap_or(0) as i32);
        w.encoding.clone().set_value(
            Encoding::READ
                .iter()
                .position(|e| *e == o.encoding)
                .unwrap_or(0) as i32,
        );
        if o.format != Format::Tsv {
            w.delimiter.clone().set_value(&o.delimiter.to_string());
        }
        w.quote.clone().set_value(&o.quote.to_string());
        w.header.clone().set_value(o.header);
        if !o.sheet.is_empty() {
            let mut s = w.sheet.clone();
            s.clear();
            s.add_choice(&o.sheet);
            s.set_value(0);
        }
        w.schema.clone().set_value(&u.schema);
        w.table.clone().set_value(&u.table);
        w.append.clone().set_value(u.mode == Mode::Append);
        w.replace.clone().set_value(u.mode == Mode::Replace);
        w.once.clone().set_value(u.commit == Commit::Once);
        w.without.clone().set_value(u.commit == Commit::Without);
        if let Commit::Every { batches } = u.commit {
            w.every.clone().set_value(true);
            w.batches.clone().set_value(&batches.to_string());
        } else {
            w.every.clone().set_value(false);
        }
        w.resume
            .clone()
            .set_value(&u.resume_row.map(|r| r.to_string()).unwrap_or_default());
        w.dates.clone().set_value(i32::from(o.dates == Dates::Job));
        w.point.clone().set_value(i32::from(o.point == ','));
        w.bits.clone().set_value(i32::from(o.bit_data_as_text));
        *self.mapping.borrow_mut() = u.mapping.clone();
        self.show_format();
    }

    fn format(&self) -> Format {
        Format::ALL
            .get(usize::try_from(self.w.format.value()).unwrap_or(0))
            .copied()
            .unwrap_or_default()
    }

    fn show_format(&self) {
        let f = self.format();
        let on = |w: &mut dyn WidgetExt, yes: bool| {
            if yes {
                w.activate();
            } else {
                w.deactivate();
            }
        };
        on(&mut self.w.encoding.clone(), f.is_text());
        on(&mut self.w.delimiter.clone(), f == Format::Csv);
        on(
            &mut self.w.quote.clone(),
            matches!(f, Format::Csv | Format::Tsv),
        );
        on(&mut self.w.sheet.clone(), f.has_sheets());
        on(&mut self.w.header.clone(), f != Format::Fixed);
    }

    /// The request the controls make; why not, when they make none.
    pub fn request(&self) -> Result<Upload, String> {
        let w = &self.w;
        let file = w.file.value().trim().to_owned();
        if file.is_empty() {
            return Err("Name the file to upload, or choose Browse.".into());
        }
        let (schema, table) = (
            w.schema.value().trim().to_ascii_uppercase(),
            w.table.value().trim().to_ascii_uppercase(),
        );
        if schema.is_empty() || table.is_empty() {
            return Err("Name the target schema and table, or choose Browse.".into());
        }
        let format = self.format();
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
        let commit = if w.without.value() {
            Commit::Without
        } else if w.every.value() {
            let n: u32 = w
                .batches
                .value()
                .trim()
                .parse()
                .ok()
                .filter(|n| *n > 0)
                .ok_or("Commit every is a whole number of batches.")?;
            Commit::Every { batches: n }
        } else {
            Commit::Once
        };
        let resume_row = match w.resume.value().trim() {
            "" => None,
            t => Some(
                t.parse::<u64>()
                    .ok()
                    .filter(|n| *n > 0)
                    .ok_or("The resume row is a row number, from 1, or blank.")?,
            ),
        };
        let encoding = Encoding::READ
            .get(usize::try_from(w.encoding.value()).unwrap_or(0))
            .copied()
            .unwrap_or_default();
        Ok(Upload {
            file: PathBuf::from(file),
            options: FileOptions {
                format,
                delimiter,
                quote: one(&w.quote, "quote")?,
                header: w.header.value() && format != Format::Fixed,
                encoding,
                sheet: w.sheet.choice().unwrap_or_default(),
                widths: Vec::new(),
                null_indicators: true,
                dates: if w.dates.value() == 1 {
                    Dates::Job
                } else {
                    Dates::Iso
                },
                point: if w.point.value() == 1 { ',' } else { '.' },
                bit_data_as_text: w.bits.value() == 1,
            },
            schema,
            table,
            mode: if w.replace.value() {
                Mode::Replace
            } else {
                Mode::Append
            },
            mapping: self.mapping.borrow().clone(),
            commit,
            batch_bytes: 0,
            resume_row,
        })
    }

    /// The source columns' names: the header's, else `Column 1`, ...
    fn source_names(&self) -> Vec<String> {
        let p = self.preview.borrow();
        match &p.header {
            Some(h) => h.clone(),
            None => {
                let n = p.rows.iter().map(|r| r.values.len()).max().unwrap_or(0);
                (1..=n).map(|i| format!("Column {i}")).collect()
            }
        }
    }

    fn show_preview(&self) {
        let names = self.source_names();
        let p = self.preview.borrow();
        let cols: Vec<(String, i32)> = names.iter().map(|n| (n.clone(), 110)).collect();
        let cols: Vec<(&str, i32)> = cols.iter().map(|(n, w)| (n.as_str(), *w)).collect();
        let mut list = self.w.preview.clone();
        list.set_columns(&cols);
        list.set_rows(
            p.rows
                .iter()
                .map(|r| {
                    (0..names.len())
                        .map(|i| match r.values.get(i) {
                            Some(Value::Null) | None => Cell {
                                text: "-".into(),
                                status: Status::Muted,
                            },
                            Some(v) => Cell::from(v.shown()),
                        })
                        .collect()
                })
                .collect(),
        );
    }

    /// The mapping grid: each target column, its source, and its first
    /// problem in the preview.
    fn show_mapping(&self) {
        let columns = self.columns.borrow();
        let names = self.source_names();
        let req = self.request().ok();
        let mapping = self.mapping.borrow().clone();
        let p = self.preview.borrow();
        let width = p.rows.iter().map(|r| r.values.len()).max().unwrap_or(0);
        let map = upload::resolve(&columns, p.header.as_deref(), &mapping, width);
        let style = req.as_ref().map_or_else(qsa_sql::Style::default, |r| {
            r.options.style(qsa_sql::DateStyle::Iso, 37)
        });
        let problems = upload::type_check(&columns, &map, &p.rows, &style);
        let rows = columns
            .iter()
            .enumerate()
            .map(|(t, c)| {
                let source = map
                    .iter()
                    .find(|(tt, _)| *tt == t)
                    .and_then(|(_, s)| names.get(*s).cloned())
                    .unwrap_or_else(|| "(default)".into());
                let problem = problems.iter().find(|(tt, _)| *tt == t).map(|(_, m)| {
                    let text = qsa_msg::message(
                        id::VALUE_DOES_NOT_FIT,
                        &[&m.column, &m.value, &m.sql_type],
                    )
                    .text;
                    // The row names the column.
                    let short = text
                        .split_once(": ")
                        .map_or(text.as_str(), |(_, rest)| rest);
                    let mut s = short.to_owned();
                    if let Some(c) = s.get_mut(..1) {
                        c.make_ascii_uppercase();
                    }
                    s
                });
                vec![
                    Cell::from(c.name.as_str()),
                    Cell::from(qsa_sql::value::type_name(c)),
                    Cell::from(source),
                    Cell {
                        text: problem.clone().unwrap_or_default(),
                        status: if problem.is_some() {
                            Status::Error
                        } else {
                            Status::Neutral
                        },
                    },
                ]
            })
            .collect();
        self.w.mapping.clone().set_rows(rows);
        let mut s = self.w.source.clone();
        s.clear();
        s.add_choice("(default)");
        for n in &names {
            s.add_choice(&n.replace('/', "\\/").replace('&', "&&"));
        }
        s.set_value(0);
    }

    fn report(&self, text: &str, s: Status) {
        self.w.status.clone().set(text, s);
    }

    fn parent(&self) -> Option<&dyn WindowExt> {
        Some(&self.w.window as &dyn WindowExt)
    }

    fn pump(&self) -> bool {
        while let Some(a) = self.rx.recv() {
            self.act(a);
        }
        self.w.window.shown()
    }

    fn act(&self, a: Act) {
        match a {
            Act::Format => self.show_format(),
            Act::BrowseFile => {
                let pattern = "Transfer files (*.csv,*.tsv,*.xlsx,*.ods,*.txt)\tAll Files (*)";
                if let Some(p) = crate::files::choose_open("File to Upload", pattern) {
                    self.w.file.clone().set_value(&p.display().to_string());
                    if let Some(f) = Format::from_path(&p) {
                        self.w
                            .format
                            .clone()
                            .set_value(Format::ALL.iter().position(|x| *x == f).unwrap_or(0) as i32);
                    }
                    self.show_format();
                    self.load_sheets();
                    self.load_preview();
                }
            }
            Act::Preview => self.load_preview(),
            Act::Browse => {
                if let Some(h) = &self.h
                    && let Some((s, t)) =
                        super::pick_table(self.parent(), h, "Choose the Target Table")
                {
                    self.w.schema.clone().set_value(&s);
                    self.w.table.clone().set_value(&t);
                    self.get_columns();
                }
            }
            Act::Columns => self.get_columns(),
            Act::MapByName | Act::MapByPosition => {
                let by_name = a == Act::MapByName;
                let names = self.source_names();
                let cols = self.columns.borrow().clone();
                *self.mapping.borrow_mut() = cols
                    .iter()
                    .enumerate()
                    .filter_map(|(i, c)| {
                        let from = if by_name {
                            names
                                .iter()
                                .find(|n| n.trim().eq_ignore_ascii_case(&c.name))
                                .map(|n| SourceColumn::Name(n.clone()))
                        } else {
                            (i < names.len()).then_some(SourceColumn::Position(i))
                        }?;
                        Some(Map {
                            target: c.name.clone(),
                            from,
                        })
                    })
                    .collect();
                self.show_mapping();
            }
            Act::MappingSelected => {
                let Some(i) = self.w.mapping.selected() else {
                    return;
                };
                let Some(row) = self.w.mapping.row(i) else {
                    return;
                };
                let names = self.source_names();
                let at = names
                    .iter()
                    .position(|n| *n == row[2].text)
                    .map_or(0, |p| p + 1);
                self.w.source.clone().set_value(at as i32);
            }
            Act::SetMapping => {
                let Some(i) = self.w.mapping.selected() else {
                    return;
                };
                let Some(target) = self.columns.borrow().get(i).map(|c| c.name.clone()) else {
                    return;
                };
                let choice = usize::try_from(self.w.source.value()).unwrap_or(0);
                let has_header = self.preview.borrow().header.is_some();
                let names = self.source_names();
                let mut m = self.mapping.borrow_mut();
                // Writing a mapping by hand starts from what is in use.
                if m.is_empty() {
                    let p = self.preview.borrow();
                    let width = p.rows.iter().map(|r| r.values.len()).max().unwrap_or(0);
                    let cols = self.columns.borrow();
                    *m = upload::resolve(&cols, p.header.as_deref(), &[], width)
                        .into_iter()
                        .map(|(t, s)| Map {
                            target: cols[t].name.clone(),
                            from: if has_header {
                                SourceColumn::Name(names[s].clone())
                            } else {
                                SourceColumn::Position(s)
                            },
                        })
                        .collect();
                }
                m.retain(|x| !x.target.eq_ignore_ascii_case(&target));
                if choice > 0 {
                    let s = choice - 1;
                    m.push(Map {
                        target,
                        from: if has_header {
                            SourceColumn::Name(names[s].clone())
                        } else {
                            SourceColumn::Position(s)
                        },
                    });
                }
                drop(m);
                self.show_mapping();
                self.w.mapping.clone().select(Some(i));
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

    fn load_sheets(&self) {
        if !self.format().has_sheets() {
            return;
        }
        let path = PathBuf::from(self.w.file.value().trim());
        match qsa_transfer::reader::sheets(&path) {
            Ok(names) => {
                let mut s = self.w.sheet.clone();
                s.clear();
                for n in &names {
                    s.add_choice(&n.replace('/', "\\/").replace('&', "&&"));
                }
                s.set_value(0);
            }
            Err(e) => self.report(&format!("{}: {e}", path.display()), Status::Error),
        }
    }

    /// The first 50 rows, and the mapping checked against them.
    fn load_preview(&self) {
        let u = match self.request() {
            Ok(u) => u,
            Err(_) => Upload {
                file: PathBuf::from(self.w.file.value().trim()),
                options: FileOptions {
                    format: self.format(),
                    header: self.w.header.value(),
                    ..FileOptions::default()
                },
                ..Upload::default()
            },
        };
        let cols = self.columns.borrow().clone();
        match upload::read(&u, &cols, Some(PREVIEW_ROWS)) {
            Ok(sheet) => {
                let n = sheet.rows.len();
                *self.preview.borrow_mut() = sheet;
                self.show_preview();
                self.show_mapping();
                self.report(
                    &format!("The first {n} rows of {}.", u.file.display()),
                    Status::Neutral,
                );
            }
            Err(e) => self.report(&format!("{}: {e}", u.file.display()), Status::Error),
        }
    }

    fn get_columns(&self) {
        let Some(h) = &self.h else { return };
        let (s, t) = (
            self.w.schema.value().trim().to_ascii_uppercase(),
            self.w.table.value().trim().to_ascii_uppercase(),
        );
        match qap::busy(|| qap::lock(h).list_columns(&s, &t)) {
            Ok(cols) => {
                let n = cols.len();
                *self.columns.borrow_mut() = cols;
                self.show_mapping();
                self.report(&format!("{s}.{t}: {n} columns."), Status::Neutral);
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
        let u = match self.request() {
            Ok(u) => u,
            Err(t) => return self.report(&t, Status::Error),
        };
        if u.mode == Mode::Replace && u.commit == Commit::Without {
            let q = format!(
                "Without commitment control the DELETE of {}.{}'s rows cannot be undone if the upload fails. Upload?",
                u.schema, u.table
            );
            let mb =
                qsa_ui::MessageBox::new("Data Transfer", &q).buttons(&["Upload", "Cancel"], 1, 1);
            if mb.run(self.parent()) != 0 {
                return;
            }
        }
        self.run_request(&h, u);
    }

    fn run_request(&self, h: &Handle, u: Upload) {
        let Some(rc) = super::attach(self.parent(), h, &self.sys) else {
            return;
        };
        let name = u
            .file
            .file_name()
            .map_or_else(|| "upload".into(), |n| n.to_string_lossy().into_owned());
        crate::log!(
            "Data Transfer to {}.{} on {} from {}",
            u.schema,
            u.table,
            self.sys.name,
            u.file.display()
        );
        let req = u.clone();
        let r = super::with_progress(self.parent(), &name, move |progress, cancel| {
            let mut rc = rc;
            let r = upload::run(&mut rc.q, &req, rc.dates, rc.ccsid, progress, cancel);
            rc.q.close();
            r
        });
        match r {
            Ok(s) => {
                let m = super::complete(&name, s.rows, s.elapsed);
                crate::log!("{m}");
                self.report(&m.to_string(), Status::Neutral);
                self.w.resume.clone().set_value("");
                msgdetails::show_message(self.parent(), &m);
            }
            Err(f) => self.failed(h, &name, u, f),
        }
    }

    fn failed(&self, h: &Handle, name: &str, mut u: Upload, f: Failure) {
        crate::log!("upload {name}: {}", f.reason);
        if f.stop == Stop::NoCommitmentControl {
            let table = format!("{}.{}", u.schema, u.table);
            let context = qsa_msg::message(id::NO_COMMITMENT_CONTROL, &[&table]);
            let buttons = ["Upload Without Commitment Control", "Cancel"];
            let choice = match &f.message {
                Some(sm) => msgdetails::ask_system_message(
                    self.parent(),
                    Some(&context),
                    &qap::shown(sm),
                    &buttons,
                ),
                None => msgdetails::ask_message(self.parent(), &context, &buttons),
            };
            if choice == 0 {
                self.w.without.clone().set_value(true);
                self.w.once.clone().set_value(false);
                self.w.every.clone().set_value(false);
                u.commit = Commit::Without;
                if u.mode == Mode::Replace {
                    return self.run();
                }
                self.run_request(h, u);
            }
            return;
        }
        let file = u.file.display().to_string();
        let m = if f.stop == Stop::Cancelled && f.kept == 0 {
            super::cancelled(name)
        } else {
            super::failed(
                name,
                f.row.unwrap_or(0),
                f.line.map_or(0, |l| l as u64),
                &file,
                &f.reason,
            )
        };
        self.report(&m.to_string(), Status::Error);
        match f.resume {
            Some(x) => {
                let more = qsa_msg::message(
                    id::TRANSFER_RESUMABLE,
                    &[name, &f.kept.to_string(), &x.to_string()],
                );
                let text = format!("{}\n\n{}", m.text, more.text);
                let both = qsa_msg::Message {
                    text,
                    cause: format!("{}\n\n{}", m.cause, more.cause),
                    recovery: more.recovery.clone(),
                    ..m.clone()
                };
                let label = format!("Resume from Row {x}");
                if msgdetails::ask_message(self.parent(), &both, &[&label, "Close"]) == 0 {
                    self.w.resume.clone().set_value(&x.to_string());
                    self.w.tabs.clone().select(3);
                    let line = f.line.map(|l| format!("line {l} of ")).unwrap_or_default();
                    self.report(
                        &format!("Correct {line}{file} if needed, then select Run to resume from row {x}."),
                        Status::Neutral,
                    );
                }
            }
            None => match &f.message {
                Some(sm) => {
                    msgdetails::show_system_message(self.parent(), Some(&m), &qap::shown(sm))
                }
                None => msgdetails::show_message(self.parent(), &m),
            },
        }
    }
}

thread_local! {
    static WINDOWS: RefCell<Vec<Rc<UploadWindow>>> = const { RefCell::new(Vec::new()) };
}

/// Opens Data Transfer to the system for `system`, signing on first when
/// this program has no session on it, filled from `preset` (Schemas'
/// Transfer data to..., a `.qdx` request).
pub fn open(parent: Option<&dyn WindowExt>, state: &Shared, system: &str, preset: Option<&Upload>) {
    let Some((sys, h)) = super::session(parent, state, system) else {
        return;
    };
    let u = preset.cloned().unwrap_or_default();
    let mut win = UploadWindow::build(&sys, &u, &[], Sheet::default());
    win.h = Some(h);
    win.state = Some(state.clone());
    let win = Rc::new(win);
    if !u.schema.is_empty() {
        win.get_columns();
    }
    if !u.file.as_os_str().is_empty() {
        win.load_sheets();
        win.load_preview();
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
        let windows: Vec<Rc<UploadWindow>> = WINDOWS.with(|v| v.borrow().clone());
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

impl UploadWindow {
    /// Save Request: the request the controls make, to a `.qdx` file.
    fn save_request(&self) {
        let r = match self.request() {
            Ok(x) => qsa_transfer::request::Request::Upload {
                system: self.sys.name.clone(),
                upload: x,
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
