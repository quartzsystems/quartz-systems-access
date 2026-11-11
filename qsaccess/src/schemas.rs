// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! Schemas: a system's catalog, from the main window's Database group,
//! over the session's shared connection (QAP version 4's `ListSchemas`,
//! `ListTables` and `ListColumns`).
//!
//! - The tree: the system, its schemas (a page of 100 at a time, matching
//!   the pattern), in each one Tables, Views, Aliases, Physical files and
//!   Logical files (a page at a time too), and in each object its columns.
//!   A level is read when it is first opened.
//! - The right pane: the selected object's properties, or its columns
//!   (Name, Label, Type, Length, Scale, Nullable, CCSID).
//! - Actions, from the menu, the buttons and a right-click: View Data (a
//!   Run SQL window running `SELECT * FROM schema.table`, its first 1,000
//!   rows), Copy Name, Transfer Data from... and Transfer Data to... (Data
//!   Transfer with the table set).
//! - An object the profile may not read shows the system's message, as it
//!   sent it, when opened (`NotAuthorized`, `SQL0551`).
//! - Before QAP version 4 the window does not open: `QAD5001`.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use fltk::app;
use fltk::enums::{Align, Key, Shortcut};
use fltk::input::Input;
use fltk::menu::{MenuBar, MenuFlag, MenuItem};
use fltk::prelude::*;
use fltk::tree::{Tree, TreeItem, TreeReason, TreeSelect};
use fltk::window::Window;
use qap_client::proto::ops::sql::{Column, ListSchemas, ListTables, Table, TableKind};
use qsa_config::System;
use qsa_ui::chan::{self, Emit, MenuEmit};
use qsa_ui::controls;
use qsa_ui::listtable::{Cell, ListTable};
use qsa_ui::sampler::{MENU_H, menu_bar};
use qsa_ui::statusbar::{Status, StatusBar};
use qsa_ui::theme::{self, colors};

use crate::app::Shared;
use crate::qap::{self, Handle};

/// Window width and height.
pub const W: i32 = 900;
pub const H: i32 = 600;

/// Entries a page of the tree reads.
pub const PAGE: u32 = 100;

/// The columns grid.
pub const COLUMNS: [(&str, i32); 7] = [
    ("Name", 130),
    ("Label", 120),
    ("Type", 110),
    ("Length", 60),
    ("Scale", 50),
    ("Nullable", 62),
    ("CCSID", 56),
];

/// The kinds, in the tree's order, with their folders' names.
pub const KINDS: [(TableKind, &str); 5] = [
    (TableKind::Table, "Tables"),
    (TableKind::View, "Views"),
    (TableKind::Alias, "Aliases"),
    (TableKind::PhysicalFile, "Physical files"),
    (TableKind::LogicalFile, "Logical files"),
];

/// What a tree item is.
#[derive(Clone, Debug, PartialEq)]
pub enum Node {
    System,
    Schema {
        name: String,
        text: String,
    },
    Kind {
        schema: String,
        kind: TableKind,
        loaded: u32,
        total: u32,
    },
    Object(Table),
    Column {
        table: Table,
        column: Column,
    },
    /// The next page of the parent's entries.
    More,
    /// Not read yet.
    Placeholder,
}

/// What the controls ask for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Act {
    Filter,
    Tree,
    ViewData,
    CopyName,
    TransferFrom,
    TransferTo,
    Refresh,
    Close,
}

/// The window's widgets.
pub struct Widgets {
    pub window: Window,
    pub menu: MenuBar,
    pub pattern: Input,
    pub tree: Tree,
    pub grid: ListTable,
    pub title: fltk::frame::Frame,
    pub status: StatusBar,
    view: fltk::button::Button,
    copy: fltk::button::Button,
    from: fltk::button::Button,
    to: fltk::button::Button,
}

/// A label as a tree path part: its `/` and `\` kept.
fn part(label: &str) -> String {
    label.replace('\\', "\\\\").replace('/', "\\/")
}

fn kind_name(k: TableKind) -> &'static str {
    crate::transfer::kind_name(k)
}

fn build_widgets(system: &str) -> (Widgets, chan::Receiver<Act>) {
    let (tx, rx) = chan::channel::<Act>();
    let mut window = Window::new(0, 0, W, H, None);
    crate::help::attach(&mut window, crate::help::topic::SCHEMAS);
    window.set_label(&format!("Schemas - {system}"));
    window.set_icon(Some(qsa_ui::glyph::app_icon()));
    window.set_color(colors::FACE);
    window.size_range(640, 400, 0, 0);
    let mut menu = menu_bar(0, 0, W);
    {
        let mut add = |p: &str, sc: Shortcut, a: Act, f: MenuFlag| {
            menu.add_emit_to(p, sc, f, tx, a);
        };
        let n = MenuFlag::Normal;
        add("&File/&Close", Shortcut::Alt | Key::F4, Act::Close, n);
        add(
            "&Actions/&View Data",
            Shortcut::Ctrl | 'v',
            Act::ViewData,
            n,
        );
        add(
            "&Actions/&Copy Name",
            Shortcut::Ctrl | 'c',
            Act::CopyName,
            MenuFlag::MenuDivider,
        );
        add(
            "&Actions/Transfer Data &from...",
            Shortcut::None,
            Act::TransferFrom,
            n,
        );
        add(
            "&Actions/Transfer Data &to...",
            Shortcut::None,
            Act::TransferTo,
            n,
        );
        add("&View/&Refresh", Shortcut::None | Key::F5, Act::Refresh, n);
    }
    let m = 8;
    let fh = theme::FIELD_H;
    let (bw, bh) = (theme::BUTTON_W, theme::BUTTON_H);
    let fy = MENU_H + 6;
    let mut l = controls::label(m, fy, 92, fh, "Schema pattern:");
    l.set_align(Align::Left | Align::Inside);
    let mut pattern = controls::field(m + 92, fy, 150, fh);
    pattern.set_value("%");
    pattern.set_tooltip("An SQL LIKE pattern: % for any characters, _ for one.");
    pattern.set_trigger(fltk::enums::CallbackTrigger::EnterKeyAlways);
    pattern.set_callback(move |_| tx.send(Act::Filter));
    let mut go = controls::default_button(m + 250, fy - 1, bw, bh, "Go");
    go.emit_to(tx, Act::Filter);
    let ty = fy + fh + 8;
    let th = H - ty - StatusBar::HEIGHT - bh - 14;
    let tw = 300;
    let mut tree = Tree::new(m, ty, tw, th, None);
    tree.set_select_mode(TreeSelect::Single);
    tree.set_item_label_font(theme::UI_FONT);
    tree.set_item_label_size(theme::UI_SIZE);
    tree.set_root_label(system);
    tree.set_callback(move |_| tx.send(Act::Tree));
    let mut title = fltk::frame::Frame::new(m + tw + 10, ty, W - tw - 2 * m - 10, fh, None);
    title.set_align(Align::Left | Align::Inside);
    title.set_label_font(theme::UI_FONT_BOLD);
    title.set_label_size(theme::UI_SIZE);
    let mut grid = ListTable::new(
        m + tw + 10,
        ty + fh + 4,
        W - tw - 2 * m - 10,
        th - fh - 4,
        &COLUMNS,
    );
    grid.on_context(move |_| {});
    let by = ty + th + 6;
    let mut x = m + tw + 10;
    let mut buttons = Vec::new();
    for (label, a, w) in [
        ("View Data", Act::ViewData, bw + 10),
        ("Copy Name", Act::CopyName, bw + 10),
        ("Transfer Data from...", Act::TransferFrom, bw + 60),
        ("Transfer Data to...", Act::TransferTo, bw + 50),
    ] {
        let mut b = controls::button(x, by, w, bh, label);
        b.emit_to(tx, a);
        buttons.push(b);
        x += w + 6;
    }
    let status = StatusBar::new(0, H - StatusBar::HEIGHT, W);
    window.resizable(grid.table());
    window.end();
    window.set_callback(move |_| tx.send(Act::Close));
    // A right-click in the tree: the actions.
    {
        let mut t = tree.clone();
        tree.handle(move |_, ev| {
            if ev == fltk::enums::Event::Push
                && app::event_mouse_button() == app::MouseButton::Right
            {
                if let Some(item) = t.find_clicked(false) {
                    let _ = t.select_only(&item, true);
                }
                let items = [
                    ("View Data", Act::ViewData),
                    ("Copy Name", Act::CopyName),
                    ("Transfer Data from...", Act::TransferFrom),
                    ("Transfer Data to...", Act::TransferTo),
                ];
                let labels: Vec<&str> = items.iter().map(|(l, _)| *l).collect();
                if let Some(chosen) = MenuItem::new(&labels).popup(app::event_x(), app::event_y())
                    && let Some((_, a)) = items
                        .iter()
                        .find(|(l, _)| chosen.label().as_deref() == Some(*l))
                {
                    tx.send(*a);
                }
                return true;
            }
            false
        });
    }
    let mut it = buttons.into_iter();
    let w = Widgets {
        window,
        menu,
        pattern,
        tree,
        grid,
        title,
        status,
        view: it.next().expect("four"),
        copy: it.next().expect("four"),
        from: it.next().expect("four"),
        to: it.next().expect("four"),
    };
    (w, rx)
}

/// A column's row in the grid.
pub fn column_row(c: &Column) -> Vec<Cell> {
    let label = c.label.split_whitespace().collect::<Vec<_>>().join(" ");
    vec![
        Cell::from(c.name.as_str()),
        Cell::from(label),
        Cell::from(c.sql_type.as_str()),
        Cell::from(c.length.to_string()),
        Cell::from(c.scale.to_string()),
        Cell::from(if c.nullable { "Yes" } else { "No" }),
        Cell::from(if c.ccsid == 0 {
            String::new()
        } else {
            c.ccsid.to_string()
        }),
    ]
}

/// The Schemas window.
pub struct Schemas {
    pub w: Widgets,
    rx: chan::Receiver<Act>,
    nodes: RefCell<HashMap<String, Node>>,
    sys: System,
    h: Option<Handle>,
    state: Option<Shared>,
}

impl Schemas {
    /// The window for `system`, with `schemas` under its root and an object
    /// shown with its `columns`, not shown and without a connection: for
    /// screenshots.
    pub fn build(
        system: &System,
        schemas: &[(String, String)],
        shown: Option<(&Table, &[Column])>,
    ) -> Schemas {
        let (w, rx) = build_widgets(&system.name);
        let s = Schemas {
            w,
            rx,
            nodes: RefCell::default(),
            sys: system.clone(),
            h: None,
            state: None,
        };
        s.add_schemas(schemas, schemas.len() as u32);
        if let Some((t, cols)) = shown {
            let kind = KINDS
                .iter()
                .find(|(k, _)| *k == t.kind)
                .map_or("Tables", |(_, n)| n);
            let mut tree = s.w.tree.clone();
            if let Some(folder) = tree.find_item(&format!("{}/{}", part(&t.schema), kind)) {
                s.clear(&folder);
                s.fill_objects(&folder, std::slice::from_ref(t), 1, 1);
                let _ = tree.open(&format!("{}/{}", part(&t.schema), kind), false);
                let _ = tree.open(&part(&t.schema), false);
            }
            if let Some(item) =
                tree.find_item(&format!("{}/{}/{}", part(&t.schema), kind, part(&t.name)))
            {
                s.fill_columns(&item, t, cols);
                let _ = tree.select_only(&item, false);
            }
            s.show_object(t, cols);
        }
        s
    }

    fn parent(&self) -> Option<&dyn WindowExt> {
        Some(&self.w.window as &dyn WindowExt)
    }

    fn report(&self, text: &str) {
        self.w.status.clone().set_message(text);
    }

    /// An item's path from the root, the root itself left out (as
    /// [`Tree::add`] takes paths); the root's is empty.
    fn key(&self, item: &TreeItem) -> String {
        let p = self.w.tree.item_pathname(item).unwrap_or_default();
        let root = self
            .w
            .tree
            .root()
            .and_then(|r| r.label())
            .map(|l| part(&l))
            .unwrap_or_default();
        if p == root {
            String::new()
        } else {
            p.strip_prefix(&format!("{root}/"))
                .map_or(p.clone(), str::to_owned)
        }
    }

    fn node(&self, item: &TreeItem) -> Option<Node> {
        self.nodes.borrow().get(&self.key(item)).cloned()
    }

    fn add_child(&self, parent: &TreeItem, label: &str, node: Node) -> Option<TreeItem> {
        let mut tree = self.w.tree.clone();
        let path = match self.key(parent) {
            p if p.is_empty() => part(label),
            p => format!("{p}/{}", part(label)),
        };
        let item = tree.add(&path)?;
        let closed = matches!(
            node,
            Node::Schema { .. } | Node::Kind { .. } | Node::Object(_)
        );
        self.nodes.borrow_mut().insert(self.key(&item), node);
        if closed {
            // Something to open: read when it is.
            let mut c = item.clone();
            let ph = tree.add(&format!("{path}/..."));
            if let Some(ph) = ph {
                self.nodes
                    .borrow_mut()
                    .insert(self.key(&ph), Node::Placeholder);
            }
            c.close();
        }
        Some(item)
    }

    fn clear(&self, item: &TreeItem) {
        let prefix = format!("{}/", self.key(item));
        self.nodes
            .borrow_mut()
            .retain(|k, _| !k.starts_with(&prefix));
        self.w.tree.clone().clear_children(item);
    }

    /// Schemas under the root, a "more" entry when there are more.
    fn add_schemas(&self, schemas: &[(String, String)], total: u32) {
        let Some(root) = self.w.tree.root() else {
            return;
        };
        self.nodes
            .borrow_mut()
            .insert(self.key(&root), Node::System);
        for (name, text) in schemas {
            if let Some(item) = self.add_child(
                &root,
                name,
                Node::Schema {
                    name: name.clone(),
                    text: text.clone(),
                },
            ) {
                // Its folders, ready to open.
                self.clear(&item);
                for (kind, label) in KINDS {
                    self.add_child(
                        &item,
                        label,
                        Node::Kind {
                            schema: name.clone(),
                            kind,
                            loaded: 0,
                            total: 0,
                        },
                    );
                }
                item.clone().close();
            }
        }
        let shown = self
            .nodes
            .borrow()
            .values()
            .filter(|n| matches!(n, Node::Schema { .. }))
            .count() as u32;
        if shown < total {
            self.add_child(
                &root,
                &format!("More schemas ({shown} of {total})..."),
                Node::More,
            );
        }
        self.w.tree.clone().redraw();
    }

    fn fill_objects(&self, folder: &TreeItem, tables: &[Table], loaded: u32, total: u32) {
        for t in tables {
            self.add_child(folder, &t.name, Node::Object(t.clone()));
        }
        if let Some(Node::Kind { schema, kind, .. }) = self.node(folder) {
            self.nodes.borrow_mut().insert(
                self.key(folder),
                Node::Kind {
                    schema,
                    kind,
                    loaded,
                    total,
                },
            );
        }
        if loaded < total {
            self.add_child(
                folder,
                &format!("More ({loaded} of {total})..."),
                Node::More,
            );
        }
    }

    fn fill_columns(&self, item: &TreeItem, table: &Table, cols: &[Column]) {
        self.clear(item);
        for c in cols {
            self.add_child(
                item,
                &c.name,
                Node::Column {
                    table: table.clone(),
                    column: c.clone(),
                },
            );
        }
    }

    fn pump(&self) -> bool {
        while let Some(a) = self.rx.recv() {
            self.act(a);
        }
        self.w.window.shown()
    }

    fn act(&self, a: Act) {
        match a {
            Act::Close => self.w.window.clone().hide(),
            Act::Filter | Act::Refresh => self.load_schemas(0),
            Act::Tree => self.tree_event(),
            Act::CopyName => {
                if let Some(name) = self.selected_name() {
                    app::copy(&name);
                    self.report(&format!("Copied {name}."));
                }
            }
            Act::ViewData => {
                let (Some(state), Some(t)) = (&self.state, self.selected_table()) else {
                    return;
                };
                let sql = format!(
                    "SELECT * FROM {}",
                    qsa_transfer::download::qualified(&t.schema, &t.name)
                );
                crate::runsql::open(
                    self.parent(),
                    state,
                    &self.sys.name,
                    Some(&sql),
                    true,
                    crate::runsql::VIEW_ROWS,
                );
            }
            Act::TransferFrom => {
                let (Some(state), Some(t)) = (&self.state, self.selected_table()) else {
                    return;
                };
                let d = qsa_transfer::Download {
                    source: qsa_transfer::Source::Table {
                        schema: t.schema.clone(),
                        table: t.name.clone(),
                        where_clause: String::new(),
                    },
                    ..qsa_transfer::Download::default()
                };
                crate::transfer::download::open(self.parent(), state, &self.sys.name, Some(&d));
            }
            Act::TransferTo => {
                let (Some(state), Some(t)) = (&self.state, self.selected_table()) else {
                    return;
                };
                let u = qsa_transfer::Upload {
                    schema: t.schema.clone(),
                    table: t.name.clone(),
                    ..qsa_transfer::Upload::default()
                };
                crate::transfer::upload::open(self.parent(), state, &self.sys.name, Some(&u));
            }
        }
    }

    /// The table, view, alias or file selected, or the one of the column
    /// selected.
    fn selected_table(&self) -> Option<Table> {
        let item = self.w.tree.first_selected_item()?;
        match self.node(&item)? {
            Node::Object(t) | Node::Column { table: t, .. } => Some(t),
            _ => {
                self.report("Select a table, view, alias or file.");
                None
            }
        }
    }

    /// The selected entry's name, qualified for SQL.
    fn selected_name(&self) -> Option<String> {
        let item = self.w.tree.first_selected_item()?;
        Some(match self.node(&item)? {
            Node::Schema { name, .. } => qsa_transfer::download::sql_name(&name),
            Node::Object(t) => qsa_transfer::download::qualified(&t.schema, &t.name),
            Node::Column { column, .. } => qsa_transfer::download::sql_name(&column.name),
            _ => return None,
        })
    }

    /// View Data and the transfers for an object; Copy Name for anything
    /// with a name.
    fn buttons(&self, object: bool) {
        self.w.copy.clone().activate();
        for b in [&self.w.view, &self.w.from, &self.w.to] {
            let mut b = b.clone();
            if object {
                b.activate();
            } else {
                b.deactivate();
            }
        }
    }

    fn tree_event(&self) {
        let tree = &self.w.tree;
        let Some(item) = tree.callback_item() else {
            return;
        };
        let node = self.node(&item);
        match (tree.callback_reason(), node) {
            (TreeReason::Opened, Some(Node::Kind { schema, kind, .. })) => {
                if self.children_unread(&item) {
                    self.load_objects(&item, &schema, kind, 0);
                }
            }
            (TreeReason::Opened, Some(Node::Object(t))) => {
                if self.children_unread(&item) {
                    self.load_columns(&item, &t);
                }
            }
            (TreeReason::Selected, Some(Node::More)) => {
                let Some(parent) = item.parent() else { return };
                let _ = self.w.tree.clone().remove(&item);
                match self.node(&parent) {
                    Some(Node::Kind {
                        schema,
                        kind,
                        loaded,
                        ..
                    }) => self.load_objects(&parent, &schema, kind, loaded),
                    Some(Node::System) => {
                        let n = self
                            .nodes
                            .borrow()
                            .values()
                            .filter(|n| matches!(n, Node::Schema { .. }))
                            .count() as u32;
                        self.load_schemas(n);
                    }
                    _ => {}
                }
            }
            (TreeReason::Selected, Some(n)) => self.show(&item, n),
            _ => {}
        }
    }

    fn children_unread(&self, item: &TreeItem) -> bool {
        let first = item.child(0);
        first.is_some_and(|c| self.node(&c) == Some(Node::Placeholder))
    }

    fn show(&self, item: &TreeItem, n: Node) {
        let mut grid = self.w.grid.clone();
        let mut title = self.w.title.clone();
        let props = |rows: Vec<(&str, String)>| -> Vec<Vec<Cell>> {
            rows.into_iter()
                .map(|(k, v)| vec![Cell::from(k), Cell::from(v)])
                .collect()
        };
        match n {
            Node::Object(t) => {
                // Its columns, read when first shown.
                if self.children_unread(item) {
                    self.load_columns(item, &t);
                }
                let cols: Vec<Column> = self
                    .nodes
                    .borrow()
                    .iter()
                    .filter(|(k, _)| k.starts_with(&format!("{}/", self.key(item))))
                    .filter_map(|(_, n)| match n {
                        Node::Column { column, .. } => Some(column.clone()),
                        _ => None,
                    })
                    .collect();
                // In their order in the tree.
                let order: Vec<String> = (0..item.children())
                    .filter_map(|i| item.child(i))
                    .filter_map(|c| c.label())
                    .collect();
                let mut cols = cols;
                cols.sort_by_key(|c| {
                    order
                        .iter()
                        .position(|o| *o == c.name)
                        .unwrap_or(usize::MAX)
                });
                self.show_object(&t, &cols);
                self.buttons(true);
                return;
            }
            Node::Schema { name, text } => {
                title.set_label(&format!("Schema {name}"));
                grid.set_columns(&[("Property", 120), ("Value", 400)]);
                grid.set_rows(props(vec![("Name", name), ("Description", text)]));
            }
            Node::Column { table, column } => {
                title.set_label(&format!(
                    "Column {} of {}.{}",
                    column.name, table.schema, table.name
                ));
                grid.set_columns(&[("Property", 120), ("Value", 400)]);
                grid.set_rows(props(vec![
                    ("Name", column.name.clone()),
                    (
                        "Label",
                        column
                            .label
                            .split_whitespace()
                            .collect::<Vec<_>>()
                            .join(" "),
                    ),
                    ("Type", qsa_sql::value::type_name(&column)),
                    ("Length", column.length.to_string()),
                    ("Scale", column.scale.to_string()),
                    (
                        "Nullable",
                        if column.nullable { "Yes" } else { "No" }.into(),
                    ),
                    ("CCSID", column.ccsid.to_string()),
                ]));
                self.buttons(true);
                return;
            }
            Node::Kind {
                schema,
                kind,
                loaded,
                total,
            } => {
                title.set_label(&format!(
                    "{} in {schema}",
                    KINDS
                        .iter()
                        .find(|(k, _)| *k == kind)
                        .map_or("", |(_, n)| n)
                ));
                grid.set_columns(&[("Property", 120), ("Value", 400)]);
                grid.set_rows(props(vec![("Shown", format!("{loaded} of {total}"))]));
            }
            Node::System => {
                title.set_label(&format!("System {}", self.sys.name));
                grid.set_columns(&[("Property", 120), ("Value", 400)]);
                grid.set_rows(Vec::new());
            }
            Node::More | Node::Placeholder => return,
        }
        self.buttons(false);
    }

    /// The object's properties in the title, its columns in the grid.
    fn show_object(&self, t: &Table, cols: &[Column]) {
        let text = if t.text.is_empty() {
            String::new()
        } else {
            format!(": {}", t.text)
        };
        self.w.title.clone().set_label(&format!(
            "{} {}.{}{text}",
            kind_name(t.kind),
            t.schema,
            t.name
        ));
        let mut grid = self.w.grid.clone();
        grid.set_columns(&COLUMNS);
        grid.set_rows(cols.iter().map(column_row).collect());
    }

    fn load_schemas(&self, start: u32) {
        let Some(h) = &self.h else { return };
        let pattern = match self.w.pattern.value().trim() {
            "" => "%".to_owned(),
            p => p.to_ascii_uppercase(),
        };
        let req = ListSchemas {
            pattern: pattern.clone(),
            start,
            max: PAGE,
        };
        match qap::busy(|| qap::lock(h).list_schemas(&req)) {
            Ok(page) => {
                if start == 0 {
                    let mut tree = self.w.tree.clone();
                    if let Some(root) = tree.root() {
                        tree.clear_children(&root);
                    }
                    self.nodes.borrow_mut().clear();
                }
                let list: Vec<(String, String)> =
                    page.schemas.into_iter().map(|s| (s.name, s.text)).collect();
                self.add_schemas(&list, page.total);
                self.report(&format!(
                    "{} schemas match {pattern} on {}.",
                    page.total, self.sys.name
                ));
            }
            Err(e) => self.failed(&e),
        }
    }

    fn load_objects(&self, folder: &TreeItem, schema: &str, kind: TableKind, start: u32) {
        let Some(h) = &self.h else { return };
        let req = ListTables {
            schema: schema.to_owned(),
            pattern: "%".into(),
            kinds: vec![kind],
            start,
            max: PAGE,
        };
        match qap::busy(|| qap::lock(h).list_tables(&req)) {
            Ok(page) => {
                if start == 0 {
                    self.clear(folder);
                }
                let loaded = start + page.tables.len() as u32;
                self.fill_objects(folder, &page.tables, loaded, page.total);
                self.w.tree.clone().redraw();
            }
            Err(e) => self.failed(&e),
        }
    }

    fn load_columns(&self, item: &TreeItem, t: &Table) {
        let Some(h) = &self.h else { return };
        let (s, n) = (t.schema.clone(), t.name.clone());
        match qap::busy(|| qap::lock(h).list_columns(&s, &n)) {
            Ok(cols) => {
                self.fill_columns(item, t, &cols);
                self.w.tree.clone().redraw();
            }
            Err(e) => {
                // Not authorized: the system's message as it sent it; the
                // object stays, without columns.
                self.clear(item);
                self.failed(&e);
            }
        }
    }

    fn failed(&self, e: &qap_client::Error) {
        let x = qap::explain(&self.sys, e);
        self.report(&x.line());
        qap::show(self.parent(), &x);
    }
}

thread_local! {
    static WINDOWS: RefCell<Vec<Rc<Schemas>>> = const { RefCell::new(Vec::new()) };
}

/// Opens Schemas for `system`, signing on first when this program has no
/// session on it. Before QAP version 4: `QAD5001`.
pub fn open(parent: Option<&dyn WindowExt>, state: &Shared, system: &str) {
    let Some(sys) = state.borrow().systems.get(system).cloned() else {
        return;
    };
    let Some(h) = qap::session(parent, state, &sys.name) else {
        return;
    };
    let refused = h.try_lock().ok().and_then(|mut c| {
        let has = c.version().unwrap_or(1);
        c.supports(qap_client::proto::Op::ListSchemas)
            .err()
            .map(|_| has)
    });
    if let Some(has) = refused {
        let m = qap::version_needed(
            "Schemas",
            &sys.name,
            qap_client::proto::Op::ListSchemas,
            has,
        );
        crate::log!("{m}");
        qsa_ui::msgdetails::show_message(parent, &m);
        return;
    }
    let (w, rx) = build_widgets(&sys.name);
    let s = Rc::new(Schemas {
        w,
        rx,
        nodes: RefCell::default(),
        sys: sys.clone(),
        h: Some(h),
        state: Some(state.clone()),
    });
    let mut win = s.w.window.clone();
    qsa_ui::propdialog::center_on(&mut win, parent);
    qsa_ui::propdialog::show_placed(&mut win);
    let profile = qap::signed_on(state, &sys.name).unwrap_or_default();
    s.w.status
        .clone()
        .set_status(&format!("{}: {profile}", sys.name), Status::Active);
    s.buttons(false);
    s.load_schemas(0);
    WINDOWS.with(|v| v.borrow_mut().push(s));
    tick();
}

fn tick() {
    thread_local! { static TICKING: std::cell::Cell<bool> = const { std::cell::Cell::new(false) }; }
    if TICKING.with(|t| t.replace(true)) {
        return;
    }
    app::add_timeout3(0.05, |h| {
        let windows: Vec<Rc<Schemas>> = WINDOWS.with(|v| v.borrow().clone());
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

/// Whether any of these windows is open.
pub fn any_open() -> bool {
    WINDOWS.with(|v| v.borrow().iter().any(|w| w.w.window.shown()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_and_parts() {
        let c = Column {
            name: "COST".into(),
            label: "Purchase            cost".into(),
            sql_type: "DECIMAL".into(),
            length: 11,
            scale: 2,
            nullable: true,
            ccsid: 0,
        };
        let r = column_row(&c);
        assert_eq!(r[1].text, "Purchase cost");
        assert_eq!((r[5].text.as_str(), r[6].text.as_str()), ("Yes", ""));
        assert_eq!(part("a/b\\c"), "a\\/b\\\\c");
        assert_eq!(KINDS.len(), 5);
    }
}
