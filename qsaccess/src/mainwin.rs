// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! The main window: menu bar, toolbar, task pane with the system selector,
//! a welcome panel, and the status bar.

use std::cell::Cell;
use std::rc::Rc;

use fltk::draw;
use fltk::enums::{Align, FrameType, Key, Shortcut};
use fltk::menu::{Choice, MenuBar, MenuFlag};
use fltk::prelude::*;
use fltk::widget::Widget;
use fltk::window::Window;
use qsa_ui::glyph::{self, Glyph, Size};
use qsa_ui::msgdetails;
use qsa_ui::sampler::{MENU_H, menu_bar};
use qsa_ui::theme::{self, colors};
use qsa_ui::{Status, StatusBar, TaskPane, Toolbar, controls};

use crate::app::Shared;
use crate::{PRODUCT, VERSION, about, files, recent};

/// Path of the Recent Sessions submenu.
const RECENT_MENU: &str = "&File/Recent Se&ssions";

/// Default width of the main window.
pub const W: i32 = 980;
/// Default height of the main window.
pub const H: i32 = 660;
/// Width of the task pane.
pub const PANE_W: i32 = 268;

/// What a task entry does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Task {
    DataTransfer,
    Display5250,
    IntegratedFileSystem,
    PrinterOutput,
    RunCommand,
    Schemas,
    RunSql,
    Console5250,
    ControlPanel,
    SystemConfigurations,
    SessionManager,
    KeyManagement,
}

impl Task {
    /// The label shown in menus and the task pane.
    pub fn label(self) -> &'static str {
        match self {
            Task::DataTransfer => "Data Transfer",
            Task::Display5250 => "5250 Display",
            Task::IntegratedFileSystem => "Integrated File System",
            Task::PrinterOutput => "Printer Output",
            Task::RunCommand => "Run Command",
            Task::Schemas => "Schemas",
            Task::RunSql => "Run SQL",
            Task::Console5250 => "5250 Console",
            Task::ControlPanel => "Control Panel",
            Task::SystemConfigurations => "System Configurations",
            Task::SessionManager => "Session Manager",
            Task::KeyManagement => "Key and Certificate Management",
        }
    }
}

/// The task pane, group by group. A `None` group is a dividing rule.
pub const TASK_GROUPS: &[(Option<&str>, &[Task])] = &[
    (
        Some("General"),
        &[
            Task::DataTransfer,
            Task::Display5250,
            Task::IntegratedFileSystem,
            Task::PrinterOutput,
            Task::RunCommand,
        ],
    ),
    (Some("Database"), &[Task::Schemas, Task::RunSql]),
    (Some("Console"), &[Task::Console5250, Task::ControlPanel]),
    (None, &[]),
    (
        Some("Management"),
        &[
            Task::SystemConfigurations,
            Task::SessionManager,
            Task::KeyManagement,
        ],
    ),
];

/// Handles to the parts of the main window that callbacks update.
#[derive(Clone)]
pub struct Ui {
    pub window: Window,
    pub status: StatusBar,
    pub system: Choice,
    pub state: Shared,
    menu: MenuBar,
    tx: fltk::app::Sender<Action>,
    /// The recent sessions generation the menu shows.
    recent_gen: Rc<Cell<Option<u64>>>,
}

impl Ui {
    /// Runs a task.
    pub fn run(&self, task: Task) {
        match task {
            Task::SystemConfigurations => crate::sysconfig::open(self),
            Task::SessionManager => crate::manager::open(&self.state, self.parent()),
            Task::Display5250 => {
                if self.state.borrow().systems.is_empty() {
                    crate::sysconfig::open(self);
                }
                let system = self.selected_system();
                crate::session::start(&self.window, &self.state, system.as_deref());
            }
            _ => self.not_available(),
        }
    }

    /// Reports `QAD0001` in the status bar and a message box.
    pub fn not_available(&self) {
        let m = qsa_msg::message(qsa_msg::id::NOT_AVAILABLE, &[]);
        self.report(&m);
        msgdetails::show_message(Some(&self.window), &m);
    }

    /// Shows a message in the status bar.
    pub fn report(&self, m: &qsa_msg::Message) {
        let mut s = self.status.clone();
        s.set_message(&m.to_string());
    }

    /// Refills the system selector from the configured systems, keeping the
    /// selection where possible.
    pub fn refresh_systems(&self) {
        let st = self.state.borrow();
        let mut c = self.system.clone();
        let wanted = c
            .choice()
            .filter(|n| st.systems.contains(n))
            .unwrap_or_else(|| st.settings.last_system.clone());
        c.clear();
        for s in st.systems.iter() {
            c.add_choice(&s.name);
        }
        let index = st
            .systems
            .iter()
            .position(|s| s.name.eq_ignore_ascii_case(&wanted))
            .or(if st.systems.is_empty() { None } else { Some(0) });
        c.set_value(index.map_or(-1, |i| i as i32));
        c.redraw();
        let mut status = self.status.clone();
        match index {
            Some(i) => {
                let name = st.systems.as_slice()[i].name.clone();
                status.set_status(&format!("{name}: Not connected"), Status::Error);
            }
            None => status.set_status("No system configured", Status::Neutral),
        }
    }

    /// The selected system's name.
    pub fn selected_system(&self) -> Option<String> {
        self.system.choice()
    }

    /// Redraws the Recent Sessions submenu if the list has changed.
    pub fn refresh_recent(&self) {
        let (gen_now, list) = {
            let st = self.state.borrow();
            (st.recent_gen, st.settings.recent_sessions.clone())
        };
        if self.recent_gen.get() == Some(gen_now) {
            return;
        }
        self.recent_gen.set(Some(gen_now));
        let tx = self.tx;
        recent::fill(&mut self.menu.clone(), RECENT_MENU, &list, |m, path, i| {
            m.add_emit(
                path,
                Shortcut::None,
                MenuFlag::Normal,
                tx,
                Action::Recent(i),
            );
        });
    }

    /// The window to put dialogs over: this one if it is shown.
    fn parent(&self) -> Option<&dyn WindowExt> {
        self.window
            .shown()
            .then_some(&self.window as &dyn WindowExt)
    }
}

fn welcome_panel(x: i32, y: i32, w: i32, h: i32) -> Widget {
    let mut p = Widget::new(x, y, w, h, None);
    let mut icon = Some(glyph::image(Glyph::System, Size::Large));
    p.draw(move |p| {
        let (x, y, w, h) = (p.x(), p.y(), p.w(), p.h());
        draw::draw_box(FrameType::DownBox, x, y, w, h, colors::WINDOW);
        draw::push_clip(x + 1, y + 1, w - 2, h - 2);
        if let Some(img) = icon.as_mut() {
            img.draw(x + 16, y + 16, 32, 32);
        }
        let tx = x + 60;
        let tw = w - 60 - 20;
        draw::set_draw_color(colors::TEXT);
        draw::set_font(theme::UI_FONT_BOLD, 16);
        draw::draw_text2(
            &format!("Welcome to {PRODUCT}"),
            tx,
            y + 14,
            tw,
            22,
            Align::Left,
        );
        draw::set_font(theme::UI_FONT, theme::UI_SIZE);
        draw::draw_text2(
            &format!("Version {VERSION}"),
            tx,
            y + 36,
            tw,
            16,
            Align::Left,
        );
        let body = "The task pane on the left gathers the most common tasks for using and \
            managing a QS/400 system. Choose the system to work with in the System list \
            at the top of the pane, then select a task: click it, or move to it with the \
            Tab and arrow keys and press Enter.\n\n\
            To get started, add a system configuration for each QS/400 system you use. \
            Select System Configurations in the Management group.";
        draw::draw_text2(
            body,
            tx,
            y + 64,
            tw,
            h - 80,
            Align::Left | Align::Top | Align::Wrap,
        );
        draw::pop_clip();
    });
    p
}

/// Everything the main window can be asked to do. Menus, the toolbar and
/// the task pane send these through an FLTK channel; [`Ui::handle`] does them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Task(Task),
    Open,
    Recent(usize),
    NotAvailable,
    Refresh,
    About,
    Exit,
    SystemChanged,
}

impl Ui {
    /// Does one action.
    pub fn handle(&self, action: Action) {
        match action {
            Action::Task(t) => self.run(t),
            Action::Open => {
                if let Some(p) =
                    files::choose_open_in(&files::sessions_dir(), "Open", files::ANY_PATTERN)
                {
                    crate::open::open_path(&self.state, &p, self.parent());
                }
            }
            Action::Recent(i) => {
                let p = self.state.borrow().settings.recent_sessions.get(i).cloned();
                if let Some(p) = p {
                    crate::open::open_path(&self.state, &p, self.parent());
                }
            }
            Action::NotAvailable => self.not_available(),
            Action::Refresh => reload(self),
            Action::About => about::show(Some(&self.window)),
            Action::Exit => self.window.clone().hide(),
            Action::SystemChanged => {
                if let Some(name) = self.system.choice() {
                    self.state.borrow_mut().settings.last_system = name;
                }
                self.refresh_systems();
            }
        }
    }
}

/// Builds the main window. It is not shown. Actions arrive on the receiver.
pub fn build(state: Shared) -> (Ui, fltk::app::Receiver<Action>) {
    let (tx, rx) = fltk::app::channel::<Action>();
    let (w, h) = (W, H);
    let mut window = Window::new(0, 0, w, h, None);
    window.set_label(PRODUCT);
    window.set_color(colors::FACE);
    window.size_range(640, 420, 0, 0);

    // Menus.
    let mut menu = menu_bar(0, 0, w);
    let add = |menu: &mut fltk::menu::MenuBar, path: &str, sc: Shortcut, a: Action| {
        menu.add_emit(path, sc, MenuFlag::Normal, tx, a);
    };
    add(
        &mut menu,
        "&File/&Open...",
        Shortcut::Ctrl | 'o',
        Action::Open,
    );
    menu.add_emit(
        &format!("{RECENT_MENU}/(none)"),
        Shortcut::None,
        MenuFlag::Inactive,
        tx,
        Action::Recent(0),
    );
    if let Some(mut item) = menu.find_item(RECENT_MENU) {
        item.set_flag(MenuFlag::MenuDivider);
    }
    add(
        &mut menu,
        "&File/E&xit",
        Shortcut::Alt | Key::F4,
        Action::Exit,
    );
    for path in ["&Edit/Cu&t", "&Edit/&Copy", "&Edit/&Paste"] {
        add(&mut menu, path, Shortcut::None, Action::NotAvailable);
    }
    add(
        &mut menu,
        "&View/&Refresh",
        Shortcut::None | Key::F5,
        Action::Refresh,
    );
    for task in [
        Task::Display5250,
        Task::DataTransfer,
        Task::IntegratedFileSystem,
        Task::PrinterOutput,
        Task::RunSql,
        Task::RunCommand,
    ] {
        let path = format!("&Actions/{}...", task.label());
        add(&mut menu, &path, Shortcut::None, Action::Task(task));
    }
    for task in [
        Task::SystemConfigurations,
        Task::SessionManager,
        Task::KeyManagement,
    ] {
        let path = format!("&Tools/{}...", task.label());
        add(&mut menu, &path, Shortcut::None, Action::Task(task));
    }
    add(
        &mut menu,
        "&Help/&Help Topics",
        Shortcut::None | Key::F1,
        Action::NotAvailable,
    );
    let about = format!("&Help/&About {PRODUCT}...");
    add(&mut menu, &about, Shortcut::None, Action::About);

    // Toolbar.
    let mut tb = Toolbar::new(0, MENU_H, w);
    for (g, task) in [
        (Glyph::System, Task::SystemConfigurations),
        (Glyph::Terminal, Task::Display5250),
        (Glyph::Disk, Task::DataTransfer),
        (Glyph::Folder, Task::IntegratedFileSystem),
        (Glyph::Printer, Task::PrinterOutput),
        (Glyph::Table, Task::RunSql),
    ] {
        tb.add(g, task.label(), move || tx.send(Action::Task(task)));
        if task == Task::SystemConfigurations {
            tb.separator();
        }
    }
    tb.end();

    // Task pane: system selector, then the task groups.
    let top = MENU_H + Toolbar::HEIGHT + 4;
    let bottom = h - StatusBar::HEIGHT - 4;
    let mut pane = TaskPane::new(4, top, PANE_W, bottom - top);
    let mut system = None;
    pane.add_row(theme::FIELD_H, |x, y, w| {
        let lw = 48;
        let bw = 26;
        let l = controls::label(x, y, lw, theme::FIELD_H, "System:");
        let mut c = controls::combo(x + lw, y, w - lw - bw - 4, theme::FIELD_H);
        c.emit(tx, Action::SystemChanged);
        let mut b = controls::button(x + w - bw, y, bw, theme::FIELD_H, "");
        b.set_image(Some(glyph::image(Glyph::System, Size::Small)));
        b.set_align(Align::Center | Align::Inside | Align::ImageBackdrop);
        b.set_tooltip("System Configurations");
        b.emit(tx, Action::Task(Task::SystemConfigurations));
        system = Some(c.clone());
        vec![l.as_base_widget(), c.as_base_widget(), b.as_base_widget()]
    });
    let system = system.expect("system row is built");
    pane.add_rule();
    for (title, tasks) in TASK_GROUPS {
        match title {
            Some(t) => {
                pane.add_group(t);
                for task in *tasks {
                    let task = *task;
                    pane.add_link(None, task.label(), move || tx.send(Action::Task(task)));
                }
            }
            None => pane.add_rule(),
        }
    }
    pane.end();

    let panel = welcome_panel(PANE_W + 8, top, w - PANE_W - 12, bottom - top);
    let mut status = StatusBar::new(0, h - StatusBar::HEIGHT, w);
    window.end();
    window.resizable(&panel);

    status.set_message("Ready");
    let ui = Ui {
        window,
        status,
        system,
        state,
        menu,
        tx,
        recent_gen: Rc::new(Cell::new(None)),
    };
    ui.refresh_systems();
    ui.refresh_recent();
    (ui, rx)
}

/// Reloads systems from disk (View > Refresh).
pub fn reload(ui: &Ui) {
    {
        let mut st = ui.state.borrow_mut();
        if let Some(dir) = st.dir.clone() {
            let (systems, err) = dir.load_systems();
            if err.is_none() {
                st.systems = systems;
            }
        }
    }
    ui.refresh_systems();
    let mut s = ui.status.clone();
    s.set_message("Ready");
}

/// Reports files that could not be read at start-up.
pub fn report_startup_errors(ui: &Ui) {
    let messages = std::mem::take(&mut ui.state.borrow_mut().startup_messages);
    for m in messages {
        ui.report(&m);
    }
    let errors = std::mem::take(&mut ui.state.borrow_mut().startup_errors);
    for e in errors {
        let m = e.message();
        crate::log!("{m}");
        ui.report(&m);
        msgdetails::show_message(Some(&ui.window), &m);
    }
}
