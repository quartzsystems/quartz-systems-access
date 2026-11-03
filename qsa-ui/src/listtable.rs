// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! A list with column headers: 18-pixel rows, white background, selection
//! in the palette blue.
//!
//! - One row is selected at a time, unless [`ListTable::set_multi`] allows
//!   more: then Ctrl+click adds or removes a row, Shift+click and
//!   Shift+arrow select a range, and Ctrl+A selects every row.
//! - A click on a column header calls [`ListTable::on_header_click`]; the
//!   caller sorts and shows which way with [`ListTable::set_sort`].
//! - [`ListTable::set_icons`] puts a 16-pixel glyph at the start of each
//!   row.

use std::cell::RefCell;
use std::collections::BTreeSet;
use std::ffi::{CStr, CString};
use std::rc::Rc;

use fltk::app;
use fltk::draw;
use fltk::enums::{Align, Color, Event, FrameType, Key, Shortcut};
use fltk::misc::Tooltip;
use fltk::prelude::*;
use fltk::table::{TableContext, TableRow, TableRowSelectMode};

use crate::statusbar::Status;
use crate::theme::{self, colors};

/// One cell: its text and, for status words, its colour.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cell {
    pub text: String,
    pub status: Status,
}

impl From<&str> for Cell {
    fn from(text: &str) -> Cell {
        Cell {
            text: text.to_owned(),
            status: Status::Neutral,
        }
    }
}

impl From<String> for Cell {
    fn from(text: String) -> Cell {
        Cell {
            text,
            status: Status::Neutral,
        }
    }
}

type Callback = Box<dyn FnMut(Option<usize>)>;

struct Model {
    headers: Vec<String>,
    rows: Vec<Vec<Cell>>,
    /// A glyph before each row's first cell.
    icons: Vec<Option<crate::glyph::Glyph>>,
    /// The row with the focus: the one selected, or the last one clicked.
    selected: Option<usize>,
    /// Multiple selection: allowed, every selected row, and where a Shift
    /// range starts.
    multi: bool,
    marked: BTreeSet<usize>,
    anchor: Option<usize>,
    /// Sorted column and whether ascending, for the header arrow.
    sort: Option<(usize, bool)>,
    /// A tooltip for each column header, if any.
    header_tips: Vec<Option<&'static CStr>>,
    /// The column of the last click in the rows.
    clicked_col: Option<usize>,
    on_select: Option<Callback>,
    on_activate: Option<Callback>,
    on_context: Option<Callback>,
    on_header: Option<Box<dyn FnMut(usize)>>,
}

impl Model {
    fn is_selected(&self, row: usize) -> bool {
        if self.multi {
            self.marked.contains(&row)
        } else {
            self.selected == Some(row)
        }
    }
}

/// A small triangle, up for ascending, for a sorted column's header.
fn sort_arrow(x: i32, cy: i32, ascending: bool) {
    for i in 0..4 {
        let (w, y) = if ascending {
            (1 + 2 * i, cy - 2 + i)
        } else {
            (7 - 2 * i, cy - 2 + i)
        };
        draw::draw_rect_fill(x + 3 - w / 2, y, w, 1, colors::DISABLED_TEXT);
    }
}

/// The list.
#[derive(Clone)]
pub struct ListTable {
    table: TableRow,
    model: Rc<RefCell<Model>>,
}

impl ListTable {
    /// Header height.
    pub const HEADER_H: i32 = 20;

    /// A list with the given columns: (title, width).
    pub fn new(x: i32, y: i32, w: i32, h: i32, columns: &[(&str, i32)]) -> ListTable {
        let mut table = TableRow::new(x, y, w, h, None);
        table.set_frame(FrameType::BorderBox);
        table.set_color(colors::WINDOW);
        table.set_type(TableRowSelectMode::Single);
        table.set_rows(0);
        table.set_cols(columns.len() as i32);
        table.set_col_header(true);
        table.set_col_header_height(Self::HEADER_H);
        table.set_row_header(false);
        table.set_row_height_all(theme::ROW_H);
        table.set_col_resize(true);
        for (i, (_, cw)) in columns.iter().enumerate() {
            table.set_col_width(i as i32, *cw);
        }
        table.end();
        crate::theme::style_scrollbar(&mut table.scrollbar());
        crate::theme::style_scrollbar(&mut table.hscrollbar());

        let model = Rc::new(RefCell::new(Model {
            headers: columns.iter().map(|(t, _)| (*t).to_owned()).collect(),
            icons: Vec::new(),
            rows: Vec::new(),
            selected: None,
            multi: false,
            marked: BTreeSet::new(),
            anchor: None,
            sort: None,
            header_tips: Vec::new(),
            clicked_col: None,
            on_select: None,
            on_activate: None,
            on_context: None,
            on_header: None,
        }));

        let m = model.clone();
        table.draw_cell(move |t, ctx, row, col, x, y, w, h| match ctx {
            TableContext::ColHeader => {
                let m = m.borrow();
                let text = m.headers.get(col as usize).map_or("", |s| s.as_str());
                draw::push_clip(x, y, w, h);
                draw::draw_rect_fill(x, y, w, h, colors::WINDOW);
                draw::draw_rect_fill(x + w - 1, y + 2, 1, h - 4, colors::HEADER_RULE);
                draw::draw_rect_fill(x, y + h - 1, w, 1, colors::HEADER_RULE);
                draw::set_font(theme::UI_FONT, theme::UI_SIZE);
                draw::set_draw_color(colors::TEXT);
                draw::draw_text2(
                    text,
                    x + theme::CELL_PAD,
                    y,
                    w - 2 * theme::CELL_PAD,
                    h,
                    Align::Left,
                );
                if let Some((c, asc)) = m.sort
                    && c == col as usize
                {
                    sort_arrow(x + w - 14, y + h / 2, asc);
                }
                draw::pop_clip();
            }
            TableContext::Cell => {
                let m = m.borrow();
                let selected = m.is_selected(row as usize);
                let cell = m.rows.get(row as usize).and_then(|r| r.get(col as usize));
                draw::push_clip(x, y, w, h);
                let (bg, fg): (Color, Color) = if selected {
                    (colors::SELECTION, colors::SELECTION_TEXT)
                } else {
                    (
                        colors::WINDOW,
                        cell.map_or(colors::TEXT, |c| c.status.color()),
                    )
                };
                draw::draw_rect_fill(x, y, w, h, bg);
                let mut tx = x + theme::CELL_PAD;
                if col == 0
                    && let Some(Some(g)) = m.icons.get(row as usize)
                {
                    let mut img = crate::glyph::image(*g, crate::glyph::Size::Small);
                    img.draw(tx, y + (h - 16) / 2, 16, 16);
                    tx += 20;
                }
                if let Some(c) = cell {
                    draw::set_font(theme::UI_FONT, theme::UI_SIZE);
                    draw::set_draw_color(fg);
                    draw::draw_text2(
                        &c.text,
                        tx,
                        y,
                        w - (tx - x) - theme::CELL_PAD,
                        h,
                        Align::Left,
                    );
                }
                if m.selected == Some(row as usize) && col == 0 && t.has_focus() {
                    draw::set_draw_color(colors::SELECTION_TEXT);
                    draw::draw_focus_rect(x + 1, y + 1, t.w() - 6, h - 2);
                }
                draw::pop_clip();
            }
            _ => {}
        });

        let m = model.clone();
        table.handle(move |t, ev| match ev {
            Event::Push => {
                if let Some((TableContext::ColHeader, _, col, resize)) = t.cursor2rowcol() {
                    // Near a column edge the table resizes the column.
                    if resize != fltk::table::TableResizeFlag::None {
                        return false;
                    }
                    fire_header(&m, col as usize);
                    return true;
                }
                let row = t.callback_row();
                let in_rows = t.callback_context() == TableContext::Cell;
                if in_rows {
                    m.borrow_mut().clicked_col = usize::try_from(t.callback_col()).ok();
                }
                let double = app::event_clicks();
                let _ = t.take_focus();
                if in_rows {
                    let state = app::event_state();
                    let multi = m.borrow().multi;
                    let row = row as usize;
                    if multi && state.contains(Shortcut::Ctrl) {
                        toggle(t, &m, row);
                    } else if multi && state.contains(Shortcut::Shift) {
                        extend(t, &m, row);
                    } else {
                        set_selection(t, &m, Some(row));
                    }
                    if double {
                        fire(&m, true);
                    }
                }
                // The right button asks for the row's context menu, once the
                // row under the pointer is selected.
                if app::event_mouse_button() == app::MouseButton::Right {
                    let cb = m.borrow_mut().on_context.take();
                    if let Some(mut cb) = cb {
                        let sel = m.borrow().selected.filter(|_| in_rows);
                        cb(sel);
                        m.borrow_mut().on_context = Some(cb);
                    }
                }
                true
            }
            Event::KeyDown => {
                let n = m.borrow().rows.len();
                let cur = m.borrow().selected;
                let multi = m.borrow().multi;
                let state = app::event_state();
                if multi
                    && state.contains(Shortcut::Ctrl)
                    && app::event_key() == Key::from_char('a')
                {
                    {
                        let mut mm = m.borrow_mut();
                        mm.marked = (0..n).collect();
                        mm.anchor = Some(0);
                    }
                    t.redraw();
                    fire(&m, false);
                    return true;
                }
                let next = match app::event_key() {
                    Key::Up => Some(cur.map_or(0, |c| c.saturating_sub(1))),
                    Key::Down => Some(cur.map_or(0, |c| (c + 1).min(n.saturating_sub(1)))),
                    Key::Home => Some(0),
                    Key::End => Some(n.saturating_sub(1)),
                    Key::Enter | Key::KPEnter => {
                        if cur.is_some() {
                            fire(&m, true);
                            return true;
                        }
                        return false;
                    }
                    _ => None,
                };
                match next {
                    Some(i) if n > 0 => {
                        if multi && state.contains(Shortcut::Shift) {
                            extend(t, &m, i);
                        } else {
                            set_selection(t, &m, Some(i));
                        }
                        t.set_top_row(t.top_row().min(i as i32));
                        let visible = (t.h() - Self::HEADER_H - 4) / theme::ROW_H;
                        if i as i32 >= t.top_row() + visible {
                            t.set_top_row(i as i32 - visible + 1);
                        }
                        true
                    }
                    _ => false,
                }
            }
            Event::Focus | Event::Unfocus => {
                t.redraw();
                true
            }
            Event::Move => {
                if let Some((TableContext::ColHeader, _, col, _)) = t.cursor2rowcol()
                    && let Some(Some(tip)) = m.borrow().header_tips.get(col as usize)
                    && let Some((x, y, w, h)) = t.find_cell(TableContext::ColHeader, 0, col)
                {
                    Tooltip::enter_area(t, x, y, w, h, tip);
                }
                false
            }
            _ => false,
        });
        table.super_handle_first(false);

        ListTable { table, model }
    }

    /// A tooltip for each column header (`None`: no tooltip). Their texts
    /// are kept for the life of the program, each distinct one once.
    pub fn set_header_tooltips(&mut self, tips: &[Option<String>]) {
        self.model.borrow_mut().header_tips =
            tips.iter().map(|t| t.as_deref().map(intern)).collect();
    }

    /// The column of the last click in the rows.
    pub fn clicked_column(&self) -> Option<usize> {
        self.model.borrow().clicked_col
    }

    /// Row `row`'s cells.
    pub fn row(&self, row: usize) -> Option<Vec<Cell>> {
        self.model.borrow().rows.get(row).cloned()
    }

    /// Adds rows at the end, keeping the selection and scroll position.
    pub fn append_rows(&mut self, rows: Vec<Vec<Cell>>) {
        let n = {
            let mut m = self.model.borrow_mut();
            m.rows.extend(rows);
            m.rows.len()
        };
        self.table.set_rows(n as i32);
        self.table.set_row_height_all(theme::ROW_H);
        self.table.redraw();
    }

    /// Allows more than one row to be selected.
    pub fn set_multi(&mut self, on: bool) {
        let mut m = self.model.borrow_mut();
        m.multi = on;
        m.marked = m.selected.into_iter().collect();
    }

    /// Shows an arrow in a column's header: the list is sorted by it,
    /// ascending or not. `None` shows none.
    pub fn set_sort(&mut self, sort: Option<(usize, bool)>) {
        self.model.borrow_mut().sort = sort;
        self.table.redraw();
    }

    /// Called with the column when a column header is clicked.
    pub fn on_header_click(&mut self, cb: impl FnMut(usize) + 'static) {
        self.model.borrow_mut().on_header = Some(Box::new(cb));
    }

    /// Every selected row, in order.
    pub fn selected_rows(&self) -> Vec<usize> {
        let m = self.model.borrow();
        if m.multi {
            m.marked.iter().copied().collect()
        } else {
            m.selected.into_iter().collect()
        }
    }

    /// Selects exactly `rows` (the first gets the focus). With single
    /// selection only the first is kept.
    pub fn select_rows(&mut self, rows: &[usize]) {
        let n = self.len();
        let rows: Vec<usize> = rows.iter().copied().filter(|r| *r < n).collect();
        if !self.model.borrow().multi {
            self.select(rows.first().copied());
            return;
        }
        {
            let mut m = self.model.borrow_mut();
            m.marked = rows.iter().copied().collect();
            m.selected = rows.first().copied();
            m.anchor = m.selected;
        }
        self.table.redraw();
        fire(&self.model, false);
    }

    /// Sets the glyph shown at the start of each row (by row, as set).
    pub fn set_icons(&mut self, icons: Vec<Option<crate::glyph::Glyph>>) {
        self.model.borrow_mut().icons = icons;
        self.table.redraw();
    }

    /// Replaces every row. The selection is cleared.
    pub fn set_rows(&mut self, rows: Vec<Vec<Cell>>) {
        let n = rows.len() as i32;
        {
            let mut m = self.model.borrow_mut();
            m.rows = rows;
            m.selected = None;
            m.marked.clear();
            m.anchor = None;
        }
        self.table.set_rows(n);
        self.table.set_row_height_all(theme::ROW_H);
        self.table.redraw();
        fire(&self.model, false);
    }

    /// Number of rows.
    pub fn len(&self) -> usize {
        self.model.borrow().rows.len()
    }

    /// Whether the list is empty.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The selected row.
    pub fn selected(&self) -> Option<usize> {
        self.model.borrow().selected
    }

    /// Selects a row, or clears the selection.
    pub fn select(&mut self, row: Option<usize>) {
        let row = row.filter(|r| *r < self.len());
        set_selection(&mut self.table, &self.model, row);
    }

    /// Called when the selection changes.
    pub fn on_select(&mut self, cb: impl FnMut(Option<usize>) + 'static) {
        self.model.borrow_mut().on_select = Some(Box::new(cb));
    }

    /// Called on double-click or Enter on a row.
    pub fn on_activate(&mut self, cb: impl FnMut(Option<usize>) + 'static) {
        self.model.borrow_mut().on_activate = Some(Box::new(cb));
    }

    /// Calls `cb` with the row under the pointer (selected first), or
    /// `None` outside the rows, when the right mouse button is pressed.
    pub fn on_context(&mut self, cb: impl FnMut(Option<usize>) + 'static) {
        self.model.borrow_mut().on_context = Some(Box::new(cb));
    }

    /// The underlying table.
    pub fn table(&self) -> &TableRow {
        &self.table
    }
}

fn set_selection(t: &mut TableRow, m: &Rc<RefCell<Model>>, row: Option<usize>) {
    let changed = {
        let mut m = m.borrow_mut();
        let marked: BTreeSet<usize> = row.into_iter().collect();
        let changed = m.selected != row || (m.multi && m.marked != marked);
        m.selected = row;
        m.anchor = row;
        if m.multi {
            m.marked = marked;
        }
        changed
    };
    t.redraw();
    if changed {
        fire(m, false);
    }
}

/// Ctrl+click: adds or removes one row.
fn toggle(t: &mut TableRow, m: &Rc<RefCell<Model>>, row: usize) {
    {
        let mut mm = m.borrow_mut();
        if !mm.marked.remove(&row) {
            mm.marked.insert(row);
        }
        mm.selected = Some(row);
        mm.anchor = Some(row);
    }
    t.redraw();
    fire(m, false);
}

/// Shift+click or Shift+arrow: selects from the anchor to `row`.
fn extend(t: &mut TableRow, m: &Rc<RefCell<Model>>, row: usize) {
    {
        let mut mm = m.borrow_mut();
        let a = mm.anchor.unwrap_or(row);
        mm.marked = (a.min(row)..=a.max(row)).collect();
        mm.selected = Some(row);
    }
    t.redraw();
    fire(m, false);
}

/// Runs the header callback without holding the model borrowed.
fn fire_header(m: &Rc<RefCell<Model>>, col: usize) {
    let cb = m.borrow_mut().on_header.take();
    if let Some(mut cb) = cb {
        cb(col);
        let mut mm = m.borrow_mut();
        if mm.on_header.is_none() {
            mm.on_header = Some(cb);
        }
    }
}

/// Runs a callback without holding the model borrowed, so the callback may
/// call back into the list.
fn fire(m: &Rc<RefCell<Model>>, activate: bool) {
    let (cb, sel) = {
        let mut mm = m.borrow_mut();
        let sel = mm.selected;
        let cb = if activate {
            mm.on_activate.take()
        } else {
            mm.on_select.take()
        };
        (cb, sel)
    };
    if let Some(mut cb) = cb {
        cb(sel);
        let mut mm = m.borrow_mut();
        let slot = if activate {
            &mut mm.on_activate
        } else {
            &mut mm.on_select
        };
        if slot.is_none() {
            *slot = Some(cb);
        }
    }
}

/// `text` as a C string that lives as long as the program, each distinct
/// text made once: FLTK keeps the pointer an area tooltip shows.
fn intern(text: &str) -> &'static CStr {
    static TIPS: std::sync::Mutex<Vec<&'static CStr>> = std::sync::Mutex::new(Vec::new());
    let mut tips = TIPS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let clean: String = text.chars().filter(|c| *c != '\0').collect();
    if let Some(t) = tips.iter().find(|t| t.to_bytes() == clean.as_bytes()) {
        return t;
    }
    let t: &'static CStr = Box::leak(CString::new(clean).expect("no NUL left").into_boxed_c_str());
    tips.push(t);
    t
}
