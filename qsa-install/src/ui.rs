// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! The installer's windows, in the program's own look: the install wizard
//! (Welcome, Licence, Install for, Location, Options, Ready, Progress,
//! Finish), and the Uninstall and Repair dialogs.

use qsa_ui::chan::{self, Emit};
use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::mpsc;

use fltk::app;
use fltk::button::{Button, CheckButton, RadioRoundButton};
use fltk::enums::{Align, FrameType};
use fltk::frame::Frame;
use fltk::group::Group;
use fltk::input::Input;
use fltk::misc::Progress;
use fltk::prelude::*;
use fltk::text::{TextBuffer, TextDisplay};
use fltk::window::Window;
use qsa_ui::controls::{self, Switch};
use qsa_ui::glyph::{self, Glyph, Size};
use qsa_ui::propdialog::center_on;
use qsa_ui::theme::{self, colors};
use qsa_ui::{MessageBox, msgdetails};

use crate::install::{self, Report};
use crate::manifest::{Manifest, Scope};
use crate::plan::{self, Options};

/// Window width.
pub const W: i32 = 500;
/// Window height.
pub const H: i32 = 380;
const HEADER_H: i32 = 58;

/// The pages, in order: their names (for screenshots), titles and
/// subtitles.
pub const PAGES: [(&str, &str, &str); 8] = [
    (
        "welcome",
        "Welcome",
        concat!(
            "This installs QS/400 Access for Desktop V",
            env!("CARGO_PKG_VERSION_MAJOR"),
            "R",
            env!("CARGO_PKG_VERSION_MINOR"),
            "M",
            env!("CARGO_PKG_VERSION_PATCH"),
            " on this computer."
        ),
    ),
    (
        "licence",
        "Licence Agreement",
        "Read the licence terms before you install.",
    ),
    (
        "install-for",
        "Install For",
        "Choose who can use QS/400 Access for Desktop on this computer.",
    ),
    (
        "location",
        "Install Location",
        "Choose the folder to install the program in.",
    ),
    (
        "options",
        "Shortcuts and File Types",
        "Choose what Setup adds to this computer.",
    ),
    (
        "ready",
        "Ready to Install",
        "Check your choices, then select Install.",
    ),
    (
        "progress",
        "Installing",
        "Please wait while QS/400 Access for Desktop is installed.",
    ),
    ("finish", "Setup Complete", ""),
];

const PAGE_READY: usize = 5;
const PAGE_PROGRESS: usize = 6;
const PAGE_FINISH: usize = 7;

/// The project's MIT licence, as hard-wrapped in the file.
const LICENSE_FILE: &str = include_str!("../../LICENSE");

/// The licence page's text: [`LICENSE_FILE`] with each paragraph on one
/// line, so the text box wraps it to its own width.
pub fn licence() -> String {
    LICENSE_FILE
        .replace('\r', "")
        .split("\n\n")
        .map(|p| p.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// What happened, for the Finish page.
#[derive(Debug)]
pub enum Outcome {
    Done(Report),
    Failed(String, Option<qsa_msg::Message>),
}

/// Messages from the worker thread.
enum FromWork {
    Progress(f32, String),
    Finished(Outcome),
}

/// The wizard's widgets.
pub struct Wizard {
    pub window: Window,
    title: Frame,
    subtitle: Frame,
    pages: Vec<Group>,
    back: Button,
    next: Button,
    cancel: Button,
    back_sw: Switch,
    next_sw: Switch,
    accept: RadioRoundButton,
    decline: RadioRoundButton,
    just_me: RadioRoundButton,
    all_users: RadioRoundButton,
    dir: Input,
    start_menu: CheckButton,
    desktop: CheckButton,
    assoc: CheckButton,
    summary: TextBuffer,
    progress: Progress,
    progress_text: Frame,
    finish_text: TextBuffer,
    launch: CheckButton,
    page: Rc<Cell<usize>>,
    dir_edited: Rc<Cell<bool>>,
    existing: Option<Manifest>,
}

fn text_box(x: i32, y: i32, w: i32, h: i32, text: &str) -> (TextDisplay, TextBuffer) {
    let mut buf = TextBuffer::default();
    buf.set_text(text);
    let mut d = TextDisplay::new(x, y, w, h, None);
    d.set_buffer(buf.clone());
    d.set_frame(FrameType::DownBox);
    d.set_color(colors::WINDOW);
    d.set_text_font(theme::UI_FONT);
    d.set_text_size(theme::UI_SIZE);
    d.wrap_mode(fltk::text::WrapMode::AtBounds, 0);
    d.set_scrollbar_size(16);
    (d, buf)
}

fn para(x: i32, y: i32, w: i32, h: i32, text: &str) -> Frame {
    let mut f = controls::label(x, y, w, h, text);
    f.set_align(Align::Left | Align::Top | Align::Inside | Align::Wrap);
    f
}

/// What the Options page calls the start menu entries here.
fn start_menu_label() -> &'static str {
    if cfg!(windows) {
        "Start menu shortcuts (Quartz Systems folder)"
    } else {
        "Applications menu entries"
    }
}

impl Wizard {
    /// Builds the wizard, without showing it. `existing` is an install to
    /// upgrade.
    pub fn build(existing: Option<Manifest>) -> Wizard {
        let mut window = Window::new(0, 0, W, H, None);
        window.set_label(&format!("{} Setup", crate::PRODUCT));
        window.set_icon(Some(glyph::app_icon()));
        window.set_color(colors::FACE);
        window.make_modal(false);

        // Header band.
        let mut band = Frame::new(0, 0, W, HEADER_H, None);
        band.set_frame(FrameType::FlatBox);
        band.set_color(colors::WINDOW);
        let mut title = Frame::new(16, 8, W - 80, 20, None);
        title.set_label_font(theme::UI_FONT_BOLD);
        title.set_label_size(theme::UI_SIZE);
        title.set_align(Align::Left | Align::Inside);
        let mut subtitle = Frame::new(30, 28, W - 94, 24, None);
        subtitle.set_label_size(theme::UI_SIZE);
        subtitle.set_align(Align::Left | Align::Top | Align::Inside | Align::Wrap);
        let mut icon = Frame::new(W - 48, 13, 32, 32, None);
        icon.set_image(Some(glyph::image(Glyph::Terminal, Size::Large)));
        let mut rule = Frame::new(0, HEADER_H, W, 1, None);
        rule.set_frame(FrameType::FlatBox);
        rule.set_color(colors::BORDER_LIGHT);

        let (px, py, pw, ph) = (24, HEADER_H + 14, W - 48, H - HEADER_H - 14 - 56);
        let mut pages = Vec::new();
        let mut page = |build: &mut dyn FnMut()| {
            let g = Group::new(px, py, pw, ph, None);
            build();
            g.end();
            pages.push(g);
        };

        // Welcome.
        let upgrade = existing.as_ref().map(|m| {
            format!(
                "Release {} is installed in {}. Setup will upgrade it to {}. Your settings, \
                 system configurations and session files are kept.\n\n",
                m.release,
                m.dir.display(),
                crate::RELEASE
            )
        });
        page(&mut || {
            para(
                px,
                py,
                pw,
                ph,
                &format!(
                    "{}Setup will guide you through installing {}.\n\n\
                     Select Next to continue.",
                    upgrade.clone().unwrap_or_default(),
                    crate::PRODUCT
                ),
            );
        });

        // Licence.
        let mut accept = None;
        page(&mut || {
            let _ = text_box(px, py, pw, ph - 54, &licence());
            let g = Group::new(px, py + ph - 48, pw, 48, None);
            let a = controls::radio(px, py + ph - 46, pw, 20, "I accept the licence terms");
            let mut d = controls::radio(
                px,
                py + ph - 24,
                pw,
                20,
                "I do not accept the licence terms",
            );
            d.set_value(true);
            g.end();
            accept = Some((a, d));
        });
        let (accept, decline) = accept.expect("licence page");

        // Install for.
        let mut scope = None;
        page(&mut || {
            let g = Group::new(px, py, pw, ph, None);
            let mut me = controls::radio(px, py, pw, 20, "Just me");
            para(
                px + 20,
                py + 22,
                pw - 20,
                34,
                if cfg!(windows) {
                    "In your own folders, for your account only. No administrator rights are needed."
                } else {
                    "In your home folder, for your account only."
                },
            );
            let all = controls::radio(px, py + 66, pw, 20, "All users");
            para(
                px + 20,
                py + 88,
                pw - 20,
                34,
                if cfg!(windows) {
                    "In Program Files, for everyone who uses this computer. Windows asks for administrator rights."
                } else {
                    "In /opt, for everyone who uses this computer. Setup must be run as root."
                },
            );
            g.end();
            me.set_value(true);
            scope = Some((me, all));
        });
        let (just_me, all_users) = scope.expect("scope page");

        // Location.
        let mut loc = None;
        page(&mut || {
            para(
                px,
                py,
                pw,
                34,
                "Setup will install QS/400 Access for Desktop in this folder. To use another, type it or select Browse.",
            );
            let mut dir = controls::field(px, py + 44, pw - theme::BUTTON_W - 8, theme::FIELD_H);
            dir.set_value(&plan::default_dir(Scope::User).display().to_string());
            let b = controls::button(
                px + pw - theme::BUTTON_W,
                py + 44,
                theme::BUTTON_W,
                theme::BUTTON_H,
                "Browse...",
            );
            loc = Some((dir, b));
        });
        let (dir, mut browse) = loc.expect("location page");

        // Options.
        let mut opt = None;
        page(&mut || {
            let a = controls::check(px, py, pw, 20, start_menu_label());
            a.set_checked(true);
            let b = controls::check(px, py + 26, pw, 20, "A shortcut on the desktop");
            b.set_checked(true);
            let c = controls::check(
                px,
                py + 52,
                pw,
                20,
                "Open .qsd, .qsp, .qkm, .qsm and .qdx files with QS/400 Access for Desktop",
            );
            c.set_checked(true);
            opt = Some((a, b, c));
        });
        let (start_menu, desktop, assoc) = opt.expect("options page");

        // Ready.
        let mut ready = None;
        page(&mut || {
            ready = Some(text_box(px, py, pw, ph, "").1);
        });
        let summary = ready.expect("ready page");

        // Progress.
        let mut prog = None;
        page(&mut || {
            let t = controls::label(px, py + 10, pw, 20, "Preparing");
            let mut p = Progress::new(px, py + 36, pw, 18, None);
            p.set_frame(FrameType::DownBox);
            p.set_color(colors::WINDOW);
            p.set_selection_color(colors::SELECTION);
            p.set_minimum(0.0);
            p.set_maximum(1.0);
            prog = Some((p, t));
        });
        let (progress, progress_text) = prog.expect("progress page");

        // Finish.
        let mut fin = None;
        page(&mut || {
            let (_, buf) = text_box(px, py, pw, ph - 30, "");
            let l = controls::check(px, py + ph - 22, pw, 20, "Start QS/400 Access for Desktop");
            l.set_checked(true);
            fin = Some((buf, l));
        });
        let (finish_text, launch) = fin.expect("finish page");

        // Buttons.
        let mut sep = Frame::new(0, H - 46, W, 1, None);
        sep.set_frame(FrameType::FlatBox);
        sep.set_color(colors::BORDER_LIGHT);
        let (bw, bh) = (theme::BUTTON_W, theme::BUTTON_H);
        let by = H - 12 - bh;
        let mut back = controls::button(W - 12 - 3 * bw - 24, by, bw, bh, "< Back");
        let mut next = controls::default_button(W - 12 - 2 * bw - 12, by, bw, bh, "Next >");
        let cancel = controls::button(W - 12 - bw, by, bw, bh, "Cancel");
        window.end();
        let back_sw = Switch::for_button(&mut back);
        let next_sw = Switch::for_button(&mut next);

        let dir_edited = Rc::new(Cell::new(false));
        {
            let e = dir_edited.clone();
            let mut d = dir.clone();
            d.set_trigger(fltk::enums::CallbackTrigger::Changed);
            d.set_callback(move |_| e.set(true));
        }
        {
            let mut d = dir.clone();
            let e = dir_edited.clone();
            browse.set_callback(move |_| {
                let start = PathBuf::from(d.value());
                if let Some(v) = qsaccess::files::choose_folder(&start, "Install Location") {
                    d.set_value(&v.display().to_string());
                    e.set(true);
                }
            });
        }
        // A choice of scope moves the folder to that scope's default, unless
        // the user typed one.
        for (mut r, s) in [
            (just_me.clone(), Scope::User),
            (all_users.clone(), Scope::Machine),
        ] {
            let mut d = dir.clone();
            let e = dir_edited.clone();
            r.set_callback(move |_| {
                if !e.get() {
                    d.set_value(&plan::default_dir(s).display().to_string());
                }
            });
        }
        let mut w = Wizard {
            window,
            title,
            subtitle,
            pages,
            back,
            next,
            cancel,
            back_sw,
            next_sw,
            accept,
            decline,
            just_me,
            all_users,
            dir,
            start_menu,
            desktop,
            assoc,
            summary,
            progress,
            progress_text,
            finish_text,
            launch,
            page: Rc::new(Cell::new(0)),
            dir_edited,
            existing,
        };
        if let Some(m) = w.existing.clone() {
            let all = m.scope == Scope::Machine;
            w.all_users.set_value(all);
            w.just_me.set_value(!all);
            w.dir.set_value(&m.dir.display().to_string());
            w.dir_edited.set(true);
            w.start_menu.set_checked(m.start_menu);
            w.desktop.set_checked(m.desktop_shortcut);
            w.assoc.set_checked(m.associations);
        }
        w.show_page(0);
        w
    }

    /// Shows page `i`.
    pub fn show_page(&mut self, i: usize) {
        self.page.set(i);
        for (n, g) in self.pages.iter_mut().enumerate() {
            if n == i {
                g.show();
            } else {
                g.hide();
            }
        }
        let (_, t, s) = PAGES[i];
        self.title.set_label(t);
        self.subtitle.set_label(s);
        self.back_sw.set_enabled(i > 0 && i < PAGE_PROGRESS);
        self.next.set_label(match i {
            PAGE_READY => "Install",
            PAGE_FINISH => "Finish",
            _ => "Next >",
        });
        self.next_sw
            .set_enabled(i != PAGE_PROGRESS && (i != 1 || self.accept.value()));
        if i == PAGE_FINISH || i == PAGE_PROGRESS {
            self.cancel.hide();
        } else {
            self.cancel.show();
        }
        if i == PAGE_READY {
            self.summary.set_text(&self.summary_text());
        }
        self.window.redraw();
    }

    /// Sets the install folder shown on the Location page.
    pub fn set_location(&mut self, dir: &Path) {
        self.dir.set_value(&dir.display().to_string());
        self.dir_edited.set(true);
    }

    /// Sets how far the Progress page shows the install has got.
    pub fn set_progress(&mut self, fraction: f32, text: &str) {
        self.progress.set_value(f64::from(fraction));
        self.progress_text.set_label(text);
    }

    /// The choices made.
    pub fn options(&self) -> Options {
        let scope = if self.all_users.value() {
            Scope::Machine
        } else {
            Scope::User
        };
        Options {
            scope,
            dir: PathBuf::from(self.dir.value().trim()),
            start_menu: self.start_menu.is_checked(),
            desktop_shortcut: self.desktop.is_checked(),
            associations: self.assoc.is_checked(),
        }
    }

    fn summary_text(&self) -> String {
        let o = self.options();
        let yes = |b: bool| if b { "Yes" } else { "No" };
        format!(
            "{}\n\nInstall for:\n    {}\n\nFolder:\n    {}\n\n{}:\n    {}\n\nDesktop shortcut:\n    {}\n\nOpen .qsd, .qsp, .qkm, .qsm and .qdx files:\n    {}",
            match &self.existing {
                Some(m) => format!("Upgrade {} to {}.", m.release, crate::RELEASE),
                None => format!("Install {} {}.", crate::PRODUCT, crate::RELEASE),
            },
            match o.scope {
                Scope::User => "Just me",
                Scope::Machine => "All users",
            },
            o.dir.display(),
            start_menu_label(),
            yes(o.start_menu),
            yes(o.desktop_shortcut),
            yes(o.associations),
        )
    }

    /// Shows how an install went, on the Finish page.
    pub fn show_outcome(&mut self, o: &Outcome) {
        let text = match o {
            Outcome::Done(r) => {
                let mut t = r.message().text;
                if !r.warnings.is_empty() {
                    t.push_str("\n\nPlease note:\n");
                    for w in &r.warnings {
                        t.push_str(&format!("  - {w}\n"));
                    }
                }
                t
            }
            Outcome::Failed(why, m) => {
                self.launch.set_checked(false);
                self.launch.hide();
                match m {
                    Some(m) => format!("{}\n\n{}\n\nMessage ID: {}", m.text, m.recovery, m.id),
                    None => format!(
                        "QS/400 Access for Desktop was not installed: {why}.\n\nNothing was left behind. The details are in the installer log."
                    ),
                }
            }
        };
        if let Outcome::Failed(..) = o {
            self.title.set_label("Setup Did Not Complete");
        }
        self.finish_text.set_text(&text);
    }
}

/// Asks to close a running QS/400 Access for Desktop and closes it. False
/// if it is still running (the user said no, or it did not close).
pub fn close_running(parent: Option<&dyn WindowExt>, ask: bool) -> bool {
    if !crate::running::is_running() {
        return true;
    }
    if ask {
        let a = MessageBox::new(
            &format!("{} Setup", crate::PRODUCT),
            "QS/400 Access for Desktop is running. Setup must close it to go on.\n\n\
             Open sessions will be disconnected. Unsaved changes to session settings and macros are lost.",
        )
        .buttons(&["Close It", "Cancel"], 0, 1)
        .run(parent);
        if a != 0 {
            return false;
        }
    }
    crate::running::close()
}

/// Runs the wizard. Returns the exit code.
pub fn run_wizard(installer: &Path) -> i32 {
    let existing = plan::existing();
    let mut w = Wizard::build(existing);
    center_on(&mut w.window, None);
    qsa_ui::propdialog::show_placed(&mut w.window);
    let (tx, rx) = chan::channel::<u8>();
    w.back.emit_to(tx, 1);
    w.next.emit_to(tx, 2);
    w.cancel.emit_to(tx, 3);
    w.window.set_callback(move |_| tx.send(3));
    {
        let n = w.next_sw.clone();
        w.accept.clone().set_callback(move |_| n.set_enabled(true));
        let n = w.next_sw.clone();
        w.decline
            .clone()
            .set_callback(move |_| n.set_enabled(false));
    }
    let (wtx, wrx) = mpsc::channel::<FromWork>();
    let mut result = crate::exit::CANCELLED;
    while w.window.shown() {
        app::wait();
        while let Ok(m) = wrx.try_recv() {
            match m {
                FromWork::Progress(f, t) => {
                    w.progress.set_value(f64::from(f));
                    w.progress_text.set_label(&t);
                }
                FromWork::Finished(o) => {
                    result = match &o {
                        Outcome::Done(_) => crate::exit::OK,
                        Outcome::Failed(_, Some(m)) if m.id == qsa_msg::id::ADMIN_NEEDED => {
                            crate::exit::NEEDS_ADMIN
                        }
                        Outcome::Failed(_, Some(m)) if m.id == qsa_msg::id::PAYLOAD_DAMAGED => {
                            crate::exit::DAMAGED
                        }
                        Outcome::Failed(..) => crate::exit::FAILED,
                    };
                    w.show_outcome(&o);
                    w.show_page(PAGE_FINISH);
                }
            }
        }
        let Some(r) = rx.recv() else { continue };
        let page = w.page.get();
        match r {
            1 if page > 0 => w.show_page(page - 1),
            2 if page == PAGE_FINISH => {
                if result == crate::exit::OK && w.launch.is_checked() {
                    let exe = w.options().dir.join(crate::program_name());
                    let _ = std::process::Command::new(exe).spawn();
                }
                w.window.hide();
            }
            2 if page == 3 => {
                let o = w.options();
                if o.dir.as_os_str().is_empty() || !o.dir.is_absolute() {
                    MessageBox::new(&w.window.label(), "Type the full path of a folder.")
                        .glyph(Glyph::Cross)
                        .run(Some(&w.window));
                } else {
                    w.show_page(4);
                }
            }
            2 if page == 2 => {
                if w.all_users.value() && !cfg!(windows) && !plan::is_admin() {
                    let m = qsa_msg::message(qsa_msg::id::ADMIN_NEEDED, &[]);
                    msgdetails::show_message(Some(&w.window), &m);
                } else {
                    w.show_page(3);
                }
            }
            2 if page == PAGE_READY => {
                if !close_running(Some(&w.window), true) {
                    continue;
                }
                w.show_page(PAGE_PROGRESS);
                let opts = w.options();
                let installer = installer.to_owned();
                let wtx = wtx.clone();
                std::thread::spawn(move || {
                    let o = work(&installer, &opts, &wtx);
                    let _ = wtx.send(FromWork::Finished(o));
                    app::awake();
                });
            }
            2 if page < PAGE_READY => w.show_page(page + 1),
            3 => {
                if page == PAGE_FINISH {
                    w.window.hide();
                    continue;
                }
                let a = MessageBox::new(
                    &w.window.label(),
                    "Setup is not complete. Do you want to leave Setup?",
                )
                .buttons(&["Yes", "No"], 1, 1)
                .run(Some(&w.window));
                if a == 0 {
                    w.window.hide();
                }
            }
            _ => {}
        }
    }
    result
}

/// The install itself, on a worker thread.
fn work(installer: &Path, opts: &Options, tx: &mpsc::Sender<FromWork>) -> Outcome {
    let send = |f: f32, t: &str| {
        let _ = tx.send(FromWork::Progress(f, t.to_owned()));
        app::awake();
    };
    #[cfg(windows)]
    if opts.scope == Scope::Machine && !plan::is_admin() {
        send(0.0, "Waiting for administrator rights");
        let log = std::env::temp_dir().join("qsa-install-elevated.log");
        let args = crate::cli::silent_args(opts, &log);
        return match crate::windows::run_elevated(installer, &args) {
            None => Outcome::Failed(
                "administrator rights were not given".into(),
                Some(qsa_msg::message(qsa_msg::id::ADMIN_NEEDED, &[])),
            ),
            Some(0) => {
                send(1.0, "Done");
                let warnings = std::fs::read_to_string(&log)
                    .unwrap_or_default()
                    .lines()
                    .filter_map(|l| l.strip_prefix("warning: ").map(str::to_owned))
                    .collect();
                Outcome::Done(Report {
                    warnings,
                    upgraded_from: None,
                    dir: opts.dir.clone(),
                })
            }
            Some(code) => Outcome::Failed(
                format!(
                    "the installer stopped with code {code}; see {}",
                    log.display()
                ),
                (code == crate::exit::DAMAGED)
                    .then(|| qsa_msg::message(qsa_msg::id::PAYLOAD_DAMAGED, &[])),
            ),
        };
    }
    let mut p = |f: f32, t: &str| send(f, t);
    match install::install(installer, opts, &mut p) {
        Ok(r) => Outcome::Done(r),
        Err(e) => {
            let m = e.message();
            Outcome::Failed(e.to_string(), m)
        }
    }
}

/// The Uninstall dialog's widgets.
pub struct UninstallDialog {
    pub window: Window,
    pub remove_settings: CheckButton,
}

/// Builds the Uninstall dialog for the install in `dir`.
pub fn build_uninstall(dir: &Path, repair: bool) -> (UninstallDialog, chan::Receiver<bool>) {
    let sessions = qsa_config::store::default_sessions_dir()
        .map(|d| d.display().to_string())
        .unwrap_or_else(|| "your Sessions folder".into());
    build_uninstall_with(dir, &sessions, repair)
}

/// Builds the Uninstall (or Repair) dialog, naming the Sessions folder as
/// `sessions`.
pub fn build_uninstall_with(
    dir: &Path,
    sessions: &str,
    repair: bool,
) -> (UninstallDialog, chan::Receiver<bool>) {
    let (tx, rx) = chan::channel::<bool>();
    let (w, h) = (440, 232);
    let m = 14;
    let mut window = Window::new(0, 0, w, h, None);
    window.set_label(&if repair {
        format!("Repair {}", crate::PRODUCT)
    } else {
        format!("Uninstall {}", crate::PRODUCT)
    });
    window.set_icon(Some(glyph::app_icon()));
    window.set_color(colors::FACE);
    window.make_modal(true);
    let mut icon = Frame::new(m, m, 32, 32, None);
    icon.set_image(Some(glyph::image(Glyph::Terminal, Size::Large)));
    let text = if repair {
        format!(
            "Setup will put back the program files, shortcuts and file types of the install in {}.\n\n\
             Your settings, system configurations and session files are not changed.",
            dir.display()
        )
    } else {
        format!(
            "Setup will remove QS/400 Access for Desktop from {}, with its shortcuts and file types.\n\n\
             Your session files in {sessions} are kept, and so are your settings and system \
             configurations unless you tick Also remove my settings.",
            dir.display()
        )
    };
    let body = qsa_ui::msgbox::break_long_words(&text, w - 3 * m - 32, |s| {
        fltk::draw::set_font(theme::UI_FONT, theme::UI_SIZE);
        fltk::draw::width(s)
    });
    para(m + 32 + 12, m, w - 3 * m - 32, 110, &body);
    let mut remove_settings = controls::check(
        m + 32 + 12,
        h - m - theme::BUTTON_H - 34,
        w - 3 * m - 32,
        20,
        "Also remove my settings",
    );
    if repair {
        remove_settings.hide();
    }
    let bw = theme::BUTTON_W;
    let bh = theme::BUTTON_H;
    let mut ok = controls::default_button(
        w - m - 2 * bw - 6,
        h - m - bh,
        bw,
        bh,
        if repair { "Repair" } else { "Uninstall" },
    );
    ok.emit_to(tx, true);
    let mut cancel = controls::button(w - m - bw, h - m - bh, bw, bh, "Cancel");
    cancel.emit_to(tx, false);
    window.end();
    window.set_callback(move |_| tx.send(false));
    (
        UninstallDialog {
            window,
            remove_settings,
        },
        rx,
    )
}

/// Asks to confirm an uninstall (or repair). `Some(remove_settings)` to go
/// ahead.
pub fn confirm_uninstall(dir: &Path, repair: bool) -> Option<bool> {
    let (mut d, rx) = build_uninstall(dir, repair);
    center_on(&mut d.window, None);
    qsa_ui::propdialog::show_placed(&mut d.window);
    while d.window.shown() {
        app::wait();
        match rx.recv() {
            Some(true) => {
                d.window.hide();
                return Some(d.remove_settings.is_checked());
            }
            Some(false) => d.window.hide(),
            None => {}
        }
    }
    None
}

/// Tells the user how it went.
pub fn tell(m: &qsa_msg::Message) {
    msgdetails::show_message(None, m);
}
