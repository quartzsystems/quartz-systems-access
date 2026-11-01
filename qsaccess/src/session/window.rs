// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! The 5250 Display window: menu bar, toolbar, the terminal, an optional
//! keypad, and a status bar. Each session is its own window in the one
//! `qsaccess` process. The network runs on a worker thread
//! ([`super::worker`]); the screen is kept and drawn here, on the GUI thread.
//!
//! A window always has a [`SessionFile`]: the one it was opened from, or a
//! new one for a session started from the Start 5250 Display dialog. Save
//! Session writes it.

use qsa_ui::chan::{self, MenuEmit};
use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::{Rc, Weak};

use fltk::app;
use fltk::button::Button;
use fltk::enums::{Event, Key, Shortcut};
use fltk::group::Group;
use fltk::menu::{MenuBar, MenuFlag};
use fltk::prelude::*;
use fltk::window::Window;
use qsa_config::qkm::KeymapFile;
use qsa_config::qsd::{self, SessionFile};
use qsa_config::qsm::MacroFile;
use qsa_config::settings::{ColorMap, Geometry, ScreenSize as CfgSize};
use qsa_config::system::CaCertificate;
use qsa_config::{AddressLookup, KeyMap, System};
use qsa_msg::{Message, id};
use qsa_ui::console::{Look, Oia, Scheme};
use qsa_ui::glyph::Glyph;
use qsa_ui::sampler::{MENU_H, menu_bar};
use qsa_ui::terminal::TerminalView;
use qsa_ui::theme::colors;
use qsa_ui::{MessageBox, Status, StatusBar, Toolbar, controls, msgdetails};
use tn5250::input::Key as K5250;
use tn5250::negotiate::{Mode, Outcome, Request, ScreenSize};
use tn5250::record::Record;
use tn5250::replay::Recorder;
use tn5250::session::Target;
use tn5250::telnet::tls::{TlsError, TlsOptions, Trust};
use tn5250::telnet::{CONNECT_TIMEOUT, ConnectError, Security};
use tn5250::terminal::Terminal;

use super::keys::{Action, Decoder};
use super::start::StartOptions;
use super::worker::{Ended, FromNet, ToNet, Worker};
use super::{cert, keymapdlg, pdf, props};
use crate::app::Shared;
use crate::macros::player::{Act, Player};
use crate::macros::recorder::Recorder as MacroRecorder;
use crate::{files, recent};

/// Width of the keypad panel.
pub const KEYPAD_W: i32 = 3 * 54 + 4 * 4;

/// The keypad's buttons, three to a row.
pub const KEYPAD: &[(&str, K5250)] = &[
    ("F1", K5250::F(1)),
    ("F2", K5250::F(2)),
    ("F3", K5250::F(3)),
    ("F4", K5250::F(4)),
    ("F5", K5250::F(5)),
    ("F6", K5250::F(6)),
    ("F7", K5250::F(7)),
    ("F8", K5250::F(8)),
    ("F9", K5250::F(9)),
    ("F10", K5250::F(10)),
    ("F11", K5250::F(11)),
    ("F12", K5250::F(12)),
    ("F13", K5250::F(13)),
    ("F14", K5250::F(14)),
    ("F15", K5250::F(15)),
    ("F16", K5250::F(16)),
    ("F17", K5250::F(17)),
    ("F18", K5250::F(18)),
    ("F19", K5250::F(19)),
    ("F20", K5250::F(20)),
    ("F21", K5250::F(21)),
    ("F22", K5250::F(22)),
    ("F23", K5250::F(23)),
    ("F24", K5250::F(24)),
    ("Enter", K5250::Enter),
    ("Reset", K5250::Reset),
    ("Attn", K5250::Attention),
    ("SysRq", K5250::SysReq),
    ("PgUp", K5250::PageUp),
    ("PgDn", K5250::PageDown),
    ("FldExit", K5250::FieldExit),
    ("Field+", K5250::FieldPlus),
    ("Field-", K5250::FieldMinus),
    ("Help", K5250::Help),
    ("Print", K5250::Print),
    ("Clear", K5250::Clear),
];

/// Path of the Recent Sessions submenu.
const RECENT_MENU: &str = "&File/Recent Se&ssions";
/// Path of the Record item, relabelled while recording.
const RECORD_ITEM: &str = "&Macro/&Record...";
/// Path of the submenu of the session's own macros.
const RUN_MENU: &str = "&Macro/R&un";

/// Picking a screen position for the Macro editor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pick {
    Off,
    Waiting,
    Got(usize, usize),
    Cancelled,
}

/// What menus, toolbar and keypad ask for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Cmd {
    Open,
    Save,
    SaveAs,
    Recent(usize),
    Properties,
    LoadKeymap,
    ExportKeymap,
    MacroRecord,
    MacroPlay,
    MacroStop,
    MacroEdit,
    MacroRun(usize),
    Connect,
    Disconnect,
    Copy,
    Paste,
    Print,
    Keypad,
    KeyMap,
    Look(ColorMap),
    Key(K5250),
    About,
    Close,
}

/// The state of the connection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Conn {
    /// Never connected.
    Idle,
    Connecting,
    Connected,
    /// Was connected or tried; the last screen stays, greyed.
    Disconnected,
}

struct State {
    /// The connection's settings, from the session file.
    opts: StartOptions,
    /// The session file's contents; written back on Save Session.
    doc: SessionFile,
    /// Where it was read from or last saved to.
    file: Option<PathBuf>,
    terminal: Terminal,
    worker: Option<Worker>,
    conn: Conn,
    security: Security,
    mode: Option<Mode>,
    device: Option<String>,
    decoder: Decoder,
    keymap: KeyMap,
    recorder: Option<Recorder>,
    /// Record the next connection to this file.
    record_to: Option<PathBuf>,
    keypad: bool,
    /// A macro being recorded, and one being run.
    macro_rec: Option<MacroRecorder>,
    player: Option<Player>,
    pick: Pick,
}

/// One session window.
pub struct SessionWindow {
    pub window: Window,
    term: TerminalView,
    keypad: Group,
    toolbar: Group,
    status: StatusBar,
    menu: MenuBar,
    rx: chan::Receiver<Cmd>,
    tx: chan::Sender<Cmd>,
    state: Shared,
    st: RefCell<State>,
    /// The recent sessions generation the menu shows.
    recent_gen: Cell<Option<u64>>,
}

fn tn_size(s: CfgSize) -> ScreenSize {
    match s {
        CfgSize::S24x80 => ScreenSize::S24x80,
        CfgSize::S27x132 => ScreenSize::S27x132,
    }
}

fn scheme(c: ColorMap) -> Scheme {
    match c {
        ColorMap::Console => Scheme::Console,
        ColorMap::BlackOnWhite => Scheme::BlackOnWhite,
    }
}

/// The connection settings a session file gives.
pub fn options_for(doc: &SessionFile) -> StartOptions {
    StartOptions {
        system: doc.system.name.clone(),
        size: doc.session.screen_size,
        ccsid: doc.session.ccsid,
        device: doc.session.device_name.clone(),
        tls: doc.system.tls,
        color: doc.session.colour_map,
    }
}

/// A new session file for options chosen in the Start 5250 Display dialog.
/// Its keyboard map is the one new sessions start from.
pub fn file_for(state: &Shared, opts: &StartOptions) -> SessionFile {
    let st = state.borrow();
    let mut f = match st.systems.get(&opts.system) {
        Some(sys) => SessionFile::new(sys),
        None => SessionFile::new(&System::new(&opts.system)),
    };
    if let Some(sys) = st.systems.get(&opts.system) {
        f.system.telnet_port = if opts.tls {
            sys.ports.telnet_tls
        } else {
            sys.ports.telnet
        };
    }
    f.system.tls = opts.tls;
    f.session.screen_size = opts.size;
    f.session.ccsid = opts.ccsid;
    f.session.device_name = opts.device.clone();
    f.session.colour_map = opts.color;
    f.keymap = st.settings.new_session_keymap.clone();
    f
}

impl SessionWindow {
    /// Builds a session window for `opts` without connecting. `scale`
    /// fixes the terminal's scale (for screenshots); otherwise it is chosen
    /// for the screen.
    pub fn build(state: &Shared, opts: StartOptions, scale: Option<usize>) -> Rc<SessionWindow> {
        let doc = file_for(state, &opts);
        SessionWindow::build_file(state, doc, None, scale)
    }

    /// Builds a session window for a session file without connecting.
    pub fn build_file(
        state: &Shared,
        doc: SessionFile,
        file: Option<PathBuf>,
        scale: Option<usize>,
    ) -> Rc<SessionWindow> {
        let opts = options_for(&doc);
        let keymap = doc.keymap();
        let (tx, rx) = chan::channel::<Cmd>();
        let size = tn_size(opts.size);
        let chrome_h = MENU_H + Toolbar::HEIGHT + StatusBar::HEIGHT;
        // The screen's own size (scale 1) unless asked otherwise; a resize
        // picks the largest scale that fits.
        let s = scale.unwrap_or(1);
        let (tw, th) = TerminalView::size_for(size.rows(), size.cols(), s);
        let (w, h) = (tw, chrome_h + th);
        let mut window = Window::new(0, 0, w, h, None);
        window.set_label(&window_title(&doc, file.as_deref()));
        window.set_icon(Some(qsa_ui::glyph::app_icon()));
        window.set_color(colors::FACE);
        window.size_range(320, 240, 0, 0);

        let mut menu = menu_bar(0, 0, w);
        let mut add = |path: &str, sc: Shortcut, c: Cmd, flag: MenuFlag| {
            menu.add_emit_to(path, sc, flag, tx, c);
        };
        let n = MenuFlag::Normal;
        add("&File/&Open Session...", Shortcut::Ctrl | 'o', Cmd::Open, n);
        add("&File/&Save Session", Shortcut::Ctrl | 's', Cmd::Save, n);
        add("&File/Save Session &As...", Shortcut::None, Cmd::SaveAs, n);
        add(
            &format!("{RECENT_MENU}/(none)"),
            Shortcut::None,
            Cmd::Recent(0),
            MenuFlag::Inactive,
        );
        add(
            "&File/P&roperties...",
            Shortcut::None,
            Cmd::Properties,
            MenuFlag::MenuDivider,
        );
        add(
            "&File/&Print Screen",
            Shortcut::Ctrl | 'p',
            Cmd::Print,
            MenuFlag::MenuDivider,
        );
        add("&File/&Close", Shortcut::Alt | Key::F4, Cmd::Close, n);
        add("&Edit/&Copy", Shortcut::Ctrl | 'c', Cmd::Copy, n);
        add("&Edit/&Paste", Shortcut::Ctrl | 'v', Cmd::Paste, n);
        add(
            "&View/&Keypad",
            Shortcut::None,
            Cmd::Keypad,
            MenuFlag::Toggle,
        );
        add(
            "&View/Colour &Map/QS\\/400 &console",
            Shortcut::None,
            Cmd::Look(ColorMap::Console),
            MenuFlag::Radio,
        );
        add(
            "&View/Colour &Map/&Black on white",
            Shortcut::None,
            Cmd::Look(ColorMap::BlackOnWhite),
            MenuFlag::Radio,
        );
        add("&Communication/&Connect", Shortcut::None, Cmd::Connect, n);
        add(
            "&Communication/&Disconnect",
            Shortcut::None,
            Cmd::Disconnect,
            n,
        );
        add("&Communication/&Reconnect", Shortcut::None, Cmd::Connect, n);
        add("&Actions/&Enter", Shortcut::None, Cmd::Key(K5250::Enter), n);
        add("&Actions/&Reset", Shortcut::None, Cmd::Key(K5250::Reset), n);
        add(
            "&Actions/&Attention",
            Shortcut::None,
            Cmd::Key(K5250::Attention),
            n,
        );
        add(
            "&Actions/&System Request",
            Shortcut::None,
            Cmd::Key(K5250::SysReq),
            n,
        );
        add("&Actions/&Help", Shortcut::None, Cmd::Key(K5250::Help), n);
        add("&Actions/C&lear", Shortcut::None, Cmd::Key(K5250::Clear), n);
        add(
            "&Actions/&Keyboard Map...",
            Shortcut::None,
            Cmd::KeyMap,
            MenuFlag::MenuDivider,
        );
        add(
            "&Actions/&Load Keyboard Map...",
            Shortcut::None,
            Cmd::LoadKeymap,
            n,
        );
        add(
            "&Actions/E&xport Keyboard Map...",
            Shortcut::None,
            Cmd::ExportKeymap,
            n,
        );
        add(RECORD_ITEM, Shortcut::None, Cmd::MacroRecord, n);
        add("&Macro/&Play...", Shortcut::None, Cmd::MacroPlay, n);
        add(
            "&Macro/&Stop",
            Shortcut::None,
            Cmd::MacroStop,
            MenuFlag::MenuDivider,
        );
        add(
            "&Macro/&Edit...",
            Shortcut::None,
            Cmd::MacroEdit,
            MenuFlag::MenuDivider,
        );
        add(
            &format!("{RUN_MENU}/(none)"),
            Shortcut::None,
            Cmd::MacroRun(0),
            MenuFlag::Inactive,
        );
        add(
            "&Help/&About QS\\/400 Access for Desktop...",
            Shortcut::None,
            Cmd::About,
            n,
        );

        let mut tb = Toolbar::new(0, MENU_H, w);
        for (g, tip, c) in [
            (Glyph::Connect, "Connect", Cmd::Connect),
            (Glyph::Disconnect, "Disconnect", Cmd::Disconnect),
        ] {
            tb.add(g, tip, move || tx.send(c));
        }
        tb.separator();
        for (g, tip, c) in [
            (Glyph::Copy, "Copy", Cmd::Copy),
            (Glyph::Paste, "Paste", Cmd::Paste),
            (Glyph::Printer, "Print Screen", Cmd::Print),
        ] {
            tb.add(g, tip, move || tx.send(c));
        }
        tb.separator();
        for (g, tip, c) in [
            (Glyph::Keypad, "Keypad", Cmd::Keypad),
            (Glyph::Keyboard, "Keyboard Map", Cmd::KeyMap),
        ] {
            tb.add(g, tip, move || tx.send(c));
        }
        tb.end();
        let toolbar = tb.group().clone();

        let top = MENU_H + Toolbar::HEIGHT;
        let term = TerminalView::new(0, top, tw, th);
        let mut keypad = Group::new(tw, top, KEYPAD_W, th, None);
        keypad.set_frame(fltk::enums::FrameType::FlatBox);
        keypad.set_color(colors::FACE);
        for (i, (label, k)) in KEYPAD.iter().enumerate() {
            let (col, row) = ((i % 3) as i32, (i / 3) as i32);
            let gap = if i >= 24 { 8 } else { 0 };
            let mut b: Button =
                controls::button(tw + 4 + col * 58, top + 4 + row * 26 + gap, 54, 22, label);
            b.clear_visible_focus();
            let k = *k;
            b.set_callback(move |_| tx.send(Cmd::Key(k)));
        }
        keypad.end();
        keypad.make_resizable(false);
        keypad.hide();
        let status = StatusBar::new(0, h - StatusBar::HEIGHT, w);
        window.end();
        window.resizable(term.widget());
        window.set_callback(move |_| {
            if app::event() == Event::Close {
                tx.send(Cmd::Close);
            }
        });

        let record_to = state.borrow_mut().record_next.take();
        let terminal = Terminal::new(size, opts.ccsid);
        let sw = Rc::new_cyclic(|me: &Weak<SessionWindow>| {
            let me1 = me.clone();
            term.on_key(move |p| me1.upgrade().is_some_and(|s| s.key_event(p)));
            let me2 = me.clone();
            term.on_click(move |r, c| {
                if let Some(s) = me2.upgrade() {
                    s.click(r, c);
                }
            });
            let me4 = me.clone();
            term.on_hover(move |cell| {
                me4.upgrade()
                    .zip(cell)
                    .is_some_and(|(s, (r, c))| s.hotspot_at(r, c).is_some())
            });
            let me3 = me.clone();
            term.on_paste(move |text| {
                if let Some(s) = me3.upgrade() {
                    s.paste(text);
                }
            });
            SessionWindow {
                window,
                term,
                keypad,
                toolbar,
                status,
                menu,
                rx,
                tx,
                state: state.clone(),
                recent_gen: Cell::new(None),
                st: RefCell::new(State {
                    opts,
                    doc,
                    file,
                    terminal,
                    worker: None,
                    conn: Conn::Idle,
                    security: Security::Plain,
                    mode: None,
                    device: None,
                    decoder: Decoder::default(),
                    keymap,
                    recorder: None,
                    record_to,
                    keypad: false,
                    macro_rec: None,
                    player: None,
                    pick: Pick::Off,
                }),
            }
        });
        let me = Rc::downgrade(&sw);
        sw.window.clone().resize_callback(move |_, _, _, _, _| {
            if let Some(s) = me.upgrade() {
                s.layout();
            }
        });
        let color = sw.st.borrow().opts.color;
        sw.set_color_map(color);
        sw.refresh_recent();
        sw.refresh_macros();
        sw.refresh();
        sw
    }

    /// Rebuilds Macro > Run from the session file's macro list.
    fn refresh_macros(&self) {
        let files = self.st.borrow().doc.macros.files.clone();
        let tx = self.tx;
        let mut menu = self.menu.clone();
        let idx = menu.find_index(RUN_MENU);
        if idx >= 0 {
            let _ = menu.clear_submenu(idx);
        }
        if files.is_empty() {
            menu.add_emit_to(
                &format!("{RUN_MENU}/(none)"),
                Shortcut::None,
                MenuFlag::Inactive,
                tx,
                Cmd::MacroRun(0),
            );
            return;
        }
        for (i, p) in files.iter().enumerate() {
            let name = MacroFile::load(p)
                .map(|m| m.display_name(Some(p)))
                .unwrap_or_else(|_| {
                    p.file_stem()
                        .map(|s| s.to_string_lossy().into_owned())
                        .unwrap_or_default()
                });
            let label = format!("&{} {}", i + 1, recent::escape(&name));
            menu.add_emit_to(
                &format!("{RUN_MENU}/{label}"),
                Shortcut::None,
                MenuFlag::Normal,
                tx,
                Cmd::MacroRun(i),
            );
        }
    }

    /// Whether a macro is running in this session.
    pub fn playing(&self) -> bool {
        self.st.borrow().player.is_some()
    }

    /// Starts running a macro read from `path`.
    pub fn play(&self, m: &MacroFile, path: &Path) {
        if self.st.borrow().macro_rec.is_some() {
            self.status
                .clone()
                .set_message("Stop recording before running a macro.");
            return;
        }
        let name = m.display_name(Some(path));
        crate::log!("running macro {name} from {}", path.display());
        self.st.borrow_mut().player = Some(Player::new(m, &name));
        self.status
            .clone()
            .set_message(&format!("Running macro {name}."));
        self.run_macro();
    }

    fn play_file(&self, path: &Path) {
        match MacroFile::load(path) {
            Ok(m) => self.play(&m, path),
            Err(e) => self.report(&e.message(path), true),
        }
    }

    /// Carries out what the running macro asks, until it has to wait.
    fn run_macro(&self) {
        loop {
            let (act, name) = {
                let mut st = self.st.borrow_mut();
                let st = &mut *st;
                let connected = st.conn == Conn::Connected;
                let Some(p) = st.player.as_mut() else { return };
                (
                    p.poll(&st.terminal.screen, connected, std::time::Instant::now()),
                    p.name.clone(),
                )
            };
            match act {
                Act::Type(text) => {
                    for c in text.chars() {
                        self.key(K5250::Char(c));
                    }
                }
                Act::Key(k) => self.key(k),
                Act::Prompt { label, hidden } => {
                    let v = qsa_ui::InputBox::new(&format!("Macro {name}"), &format!("{label}:"))
                        .hidden(hidden)
                        .run(Some(&self.window));
                    if let Some(p) = self.st.borrow_mut().player.as_mut() {
                        p.answer(v);
                    }
                }
                Act::Wait => {
                    super::tick_while_playing();
                    return;
                }
                Act::Done => {
                    self.st.borrow_mut().player = None;
                    crate::log!("macro {name} ended");
                    self.status
                        .clone()
                        .set_message(&format!("Macro {name} ended."));
                    return;
                }
                Act::Failed { step, reason } => {
                    self.st.borrow_mut().player = None;
                    let m =
                        qsa_msg::message(id::MACRO_STOPPED, &[&name, &step.to_string(), &reason]);
                    self.report(&m, !reason.contains("your request"));
                    return;
                }
            }
        }
    }

    fn stop_macro(&self) {
        let act = self.st.borrow_mut().player.as_mut().map(Player::stop);
        if act.is_some() {
            let name = self
                .st
                .borrow_mut()
                .player
                .take()
                .map(|p| p.name)
                .unwrap_or_default();
            if let Some(Act::Failed { step, reason }) = act {
                let m = qsa_msg::message(id::MACRO_STOPPED, &[&name, &step.to_string(), &reason]);
                self.report(&m, false);
            }
        }
    }

    fn toggle_recording(&self) {
        let rec = self.st.borrow_mut().macro_rec.take();
        let menu = self.menu.clone();
        let Some(rec) = rec else {
            if self.playing() {
                self.status
                    .clone()
                    .set_message("Wait for the macro to end, or stop it, before recording.");
                return;
            }
            self.st.borrow_mut().macro_rec = Some(MacroRecorder::new());
            if let Some(mut item) = menu.find_item(RECORD_ITEM) {
                item.set_label("Stop &Recording");
            }
            self.status
                .clone()
                .set_message("Recording a macro. Select Macro > Stop Recording when done.");
            return;
        };
        let idx = menu.find_index("&Macro/Stop &Recording");
        if idx >= 0
            && let Some(mut item) = menu.at(idx)
        {
            item.set_label("&Record...");
        }
        let steps = rec.finish();
        if steps.is_empty() {
            self.status.clone().set_message("Nothing was recorded.");
            return;
        }
        let Some(name) = qsa_ui::InputBox::new("Record Macro", "Macro name:")
            .value("Macro")
            .run(Some(&self.window))
            .map(|n| n.trim().to_owned())
            .filter(|n| !n.is_empty())
        else {
            self.status.clone().set_message("The macro was not kept.");
            return;
        };
        let file = files::file_name_for(&name, qsa_config::qsm::EXTENSION);
        let Some(path) = files::choose_save_in(
            &files::sessions_dir(),
            "Save Macro",
            files::MACRO_PATTERN,
            &file,
        ) else {
            self.status.clone().set_message("The macro was not kept.");
            return;
        };
        let path = files::with_extension(path, qsa_config::qsm::EXTENSION);
        let size = if self.st.borrow().terminal.screen.rows() > 24 {
            CfgSize::S27x132
        } else {
            CfgSize::S24x80
        };
        let mut m = MacroFile::new(&name, size);
        m.steps = steps;
        let n = m.steps.len();
        if let Err(e) = m.save(&path) {
            MessageBox::new(
                "Record Macro",
                &format!("{} could not be written: {e}", path.display()),
            )
            .glyph(Glyph::Cross)
            .run(Some(&self.window));
            return;
        }
        let abs = std::path::absolute(&path).unwrap_or(path);
        {
            let mut st = self.st.borrow_mut();
            if !st
                .doc
                .macros
                .files
                .iter()
                .any(|p| qsa_config::settings::same_path(p, &abs))
            {
                st.doc.macros.files.push(abs.clone());
            }
        }
        self.refresh_macros();
        self.report(
            &qsa_msg::message(id::MACRO_RECORDED, &[&name, &n.to_string()]),
            false,
        );
    }

    /// Starts waiting for a click on the screen (for the Macro editor).
    pub fn begin_pick(&self) {
        self.st.borrow_mut().pick = Pick::Waiting;
        self.status
            .clone()
            .set_message("Click the text to wait for. Press Esc to cancel.");
    }

    /// Where picking has got to; ends it once there is an answer.
    pub fn take_pick(&self) -> Pick {
        let mut st = self.st.borrow_mut();
        let p = st.pick;
        if !matches!(p, Pick::Waiting) {
            st.pick = Pick::Off;
        }
        p
    }

    /// Stops waiting for a click.
    pub fn end_pick(&self) {
        self.st.borrow_mut().pick = Pick::Off;
        self.status.clone().set_message("");
    }

    /// The word at a screen position (from 0): its column (from 1) and text.
    pub fn word_at(&self, row: usize, col: usize) -> (usize, String) {
        let st = self.st.borrow();
        let line: Vec<char> = st
            .terminal
            .screen
            .text_rows()
            .get(row)
            .map(|l| l.chars().collect())
            .unwrap_or_default();
        if line.get(col).is_none_or(|c| c.is_whitespace()) {
            return (col + 1, String::new());
        }
        let mut a = col;
        while a > 0 && !line[a - 1].is_whitespace() {
            a -= 1;
        }
        let mut b = col;
        while b + 1 < line.len() && !line[b + 1].is_whitespace() {
            b += 1;
        }
        (a + 1, line[a..=b].iter().collect())
    }

    /// Puts the window where the session file says, with its keypad, or
    /// centres it on `parent`. Call before showing.
    pub fn place(&self, parent: Option<&dyn WindowExt>) {
        let (geometry, keypad) = {
            let st = self.st.borrow();
            (st.doc.session.window, st.doc.session.keypad_visible)
        };
        let mut w = self.window.clone();
        match geometry {
            Some(g) if g.w > 100 && g.h > 100 => {
                let (sx, sy, sw, sh) = app::screen_work_area(0);
                let x = g.x.clamp(sx, (sx + sw - 100).max(sx));
                let y = g.y.clamp(sy, (sy + sh - 100).max(sy));
                w.resize(x, y, g.w, g.h);
                self.layout();
            }
            _ => qsa_ui::propdialog::center_on(&mut w, parent),
        }
        if keypad {
            self.set_keypad(true);
        }
    }

    /// The session's title, as Session Manager and the window show it.
    pub fn title(&self) -> String {
        let st = self.st.borrow();
        st.doc.display_title(st.file.as_deref())
    }

    /// The session file it was opened from or saved to.
    pub fn file(&self) -> Option<PathBuf> {
        self.st.borrow().file.clone()
    }

    /// The name of the system the session connects to.
    pub fn system_name(&self) -> String {
        self.st.borrow().opts.system.clone()
    }

    /// Whether the session file says to connect when it opens.
    pub fn auto_connect(&self) -> bool {
        self.st.borrow().doc.session.auto_connect
    }

    /// Redraws the Recent Sessions submenu if the list has changed.
    fn refresh_recent(&self) {
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
            m.add_emit_to(path, Shortcut::None, MenuFlag::Normal, tx, Cmd::Recent(i));
        });
    }

    fn set_title(&self) {
        let t = {
            let st = self.st.borrow();
            window_title(&st.doc, st.file.as_deref())
        };
        self.window.clone().set_label(&t);
    }

    /// The session file brought up to date with the window: its settings,
    /// keyboard map, keypad and geometry.
    fn current_file(&self) -> SessionFile {
        let st = self.st.borrow();
        let mut f = st.doc.clone();
        f.system.name = st.opts.system.clone();
        f.system.tls = st.opts.tls;
        f.session.screen_size = st.opts.size;
        f.session.ccsid = st.opts.ccsid;
        f.session.device_name = st.opts.device.clone();
        f.session.colour_map = st.opts.color;
        f.session.keypad_visible = st.keypad;
        let w = &self.window;
        let kw = if st.keypad { KEYPAD_W } else { 0 };
        f.session.window = Some(Geometry {
            x: w.x(),
            y: w.y(),
            w: w.w() - kw,
            h: w.h(),
        });
        if f.keymap.is_some() || st.keymap != KeyMap::defaults() {
            f.set_keymap(&st.keymap);
        }
        f
    }

    /// Saves to the session's file, or asks for one.
    fn save(&self) {
        match self.file() {
            Some(path) => self.save_to(&path),
            None => self.save_as(),
        }
    }

    fn save_as(&self) {
        let (dir, name) = {
            let st = self.st.borrow();
            let dir = st
                .file
                .as_deref()
                .and_then(Path::parent)
                .map(Path::to_owned)
                .unwrap_or_else(files::sessions_dir);
            let name =
                files::file_name_for(&st.doc.display_title(st.file.as_deref()), qsd::EXTENSION);
            (dir, name)
        };
        let Some(path) =
            files::choose_save_in(&dir, "Save Session As", files::DISPLAY_PATTERN, &name)
        else {
            return;
        };
        self.save_to(&files::with_extension(path, qsd::EXTENSION));
    }

    fn save_to(&self, path: &Path) {
        let f = self.current_file();
        match f.save(path) {
            Ok(()) => {
                let abs = std::path::absolute(path).unwrap_or_else(|_| path.to_owned());
                {
                    let mut st = self.st.borrow_mut();
                    st.doc = f;
                    st.file = Some(abs.clone());
                }
                self.set_title();
                self.state.borrow_mut().note_recent(&abs);
                self.report(
                    &qsa_msg::message(id::SESSION_SAVED, &[&abs.display().to_string()]),
                    false,
                );
            }
            Err(e) => {
                crate::log!("{}: {e}", path.display());
                MessageBox::new(
                    "Save Session",
                    &format!("{} could not be written: {e}", path.display()),
                )
                .glyph(Glyph::Cross)
                .run(Some(&self.window));
            }
        }
    }

    fn properties(&self) {
        let (title, p) = {
            let f = self.current_file();
            let st = self.st.borrow();
            (
                format!(
                    "Session Properties - {}",
                    f.display_title(st.file.as_deref())
                ),
                props::Props::from_file(&f),
            )
        };
        let edited = {
            let app = self.state.borrow();
            props::edit(Some(&self.window), &title, &p, &app.systems)
        };
        let Some(p) = edited else { return };
        {
            let systems = self.state.borrow().systems.clone();
            let mut st = self.st.borrow_mut();
            let mut f = st.doc.clone();
            p.apply(&mut f, &systems);
            st.opts.system = f.system.name.clone();
            st.opts.tls = f.system.tls;
            st.opts.size = f.session.screen_size;
            st.opts.ccsid = f.session.ccsid;
            st.opts.device = f.session.device_name.clone();
            st.doc = f;
        }
        self.set_color_map(p.color);
        self.set_title();
    }

    /// Takes a keyboard map into the session, as loaded from `from`.
    pub fn load_keymap(&self, map: KeyMap, from: &Path) {
        self.st.borrow_mut().keymap = map;
        self.report(
            &qsa_msg::message(id::KEYMAP_LOADED, &[&from.display().to_string()]),
            false,
        );
    }

    fn choose_keymap(&self) {
        let Some(path) = files::choose_open_in(
            &files::sessions_dir(),
            "Load Keyboard Map",
            files::KEYMAP_PATTERN,
        ) else {
            return;
        };
        match KeymapFile::load(&path) {
            Ok(k) => self.load_keymap(k.map, &path),
            Err(e) => self.report(&e.message(&path), true),
        }
    }

    fn export_keymap(&self) {
        let (map, name) = {
            let st = self.st.borrow();
            (
                st.keymap.clone(),
                files::file_name_for(
                    &st.doc.display_title(st.file.as_deref()),
                    qsa_config::qkm::EXTENSION,
                ),
            )
        };
        let Some(path) = files::choose_save_in(
            &files::sessions_dir(),
            "Export Keyboard Map",
            files::KEYMAP_PATTERN,
            &name,
        ) else {
            return;
        };
        let path = files::with_extension(path, qsa_config::qkm::EXTENSION);
        if let Err(e) = KeymapFile::new(map).save(&path) {
            crate::log!("{}: {e}", path.display());
            MessageBox::new(
                "Export Keyboard Map",
                &format!("{} could not be written: {e}", path.display()),
            )
            .glyph(Glyph::Cross)
            .run(Some(&self.window));
        }
    }

    /// Puts a terminal in the window as if connected, for screenshots and
    /// replays.
    pub fn show_terminal(&self, t: Terminal, conn: Conn) {
        {
            let mut st = self.st.borrow_mut();
            st.terminal = t;
            st.conn = conn;
        }
        self.refresh();
    }

    /// Shows or hides the keypad, widening the window for it.
    pub fn set_keypad(&self, on: bool) {
        let was = std::mem::replace(&mut self.st.borrow_mut().keypad, on);
        let mut win = self.window.clone();
        let mut kp = self.keypad.clone();
        if on != was {
            // The terminal keeps its width; the window widens for the keypad.
            let dw = if on { KEYPAD_W } else { -KEYPAD_W };
            let (x, y, w, h) = (win.x(), win.y(), win.w(), win.h());
            win.resize(x, y, w + dw, h);
        }
        self.layout();
        if on {
            kp.show();
        } else {
            kp.hide();
        }
        if let Some(mut item) = self.menu.find_item("&View/&Keypad") {
            if on {
                item.set();
            } else {
                item.clear();
            }
        }
        win.redraw();
    }

    /// Places every part for the window's size: bars across the top and
    /// bottom, the keypad (when shown) at the right, the terminal in the
    /// rest.
    fn layout(&self) {
        let (w, h) = (self.window.w(), self.window.h());
        let top = MENU_H + Toolbar::HEIGHT;
        let body = (h - top - StatusBar::HEIGHT).max(1);
        let kw = if self.st.borrow().keypad { KEYPAD_W } else { 0 };
        self.menu.clone().resize(0, 0, w, MENU_H);
        self.toolbar.clone().resize(0, MENU_H, w, Toolbar::HEIGHT);
        self.term
            .widget()
            .clone()
            .resize(0, top, (w - kw).max(1), body);
        self.keypad.clone().resize(w - kw, top, KEYPAD_W, body);
        self.status
            .group()
            .clone()
            .resize(0, h - StatusBar::HEIGHT, w, StatusBar::HEIGHT);
    }

    fn set_color_map(&self, c: ColorMap) {
        self.st.borrow_mut().opts.color = c;
        let path = match c {
            ColorMap::Console => "&View/Colour &Map/QS\\/400 &console",
            ColorMap::BlackOnWhite => "&View/Colour &Map/&Black on white",
        };
        if let Some(mut item) = self.menu.find_item(path) {
            item.set();
        }
        self.refresh();
    }

    /// Redraws the terminal and the status line from the state.
    fn refresh(&self) {
        let mut st = self.st.borrow_mut();
        let connected = st.conn == Conn::Connected;
        if st.terminal.screen.alarm {
            st.terminal.screen.alarm = false;
            if connected {
                fltk::dialog::beep(fltk::dialog::BeepType::Default);
            }
        }
        let s = &st.terminal.screen;
        let (r, c) = s.rc(s.cursor);
        let state = match st.conn {
            Conn::Idle => "Not connected",
            Conn::Connecting => "Connecting",
            Conn::Connected => "Connected",
            Conn::Disconnected => "Disconnected",
        };
        let system = match &st.device {
            Some(d) => format!("{} {d}", st.opts.system),
            None => st.opts.system.clone(),
        };
        let oia = Oia {
            system,
            state: state.into(),
            keyboard: connected.then_some(s.keyboard),
            insert: s.insert_mode,
            message_waiting: s.message_waiting,
            tls: matches!(st.security, Security::Tls { .. }) && st.conn != Conn::Idle,
            cursor: (r + 1, c + 1),
        };
        self.term.set_look(Look {
            scheme: scheme(st.opts.color),
            greyed: st.conn == Conn::Disconnected,
        });
        self.term.show_screen(s, &oia, connected);
        let mut bar = self.status.clone();
        let (text, kind) = match st.conn {
            Conn::Connected => ("Connected", Status::Active),
            Conn::Connecting => ("Connecting", Status::Neutral),
            Conn::Idle | Conn::Disconnected => ("Not connected", Status::Error),
        };
        bar.set_status(&format!("{}: {text}", st.opts.system), kind);
    }

    fn report(&self, m: &Message, modal: bool) {
        crate::log!("{m}");
        let mut bar = self.status.clone();
        bar.set_message(&m.to_string());
        if modal {
            msgdetails::show_message(Some(&self.window), m);
        }
    }

    /// Connects (again), with a fresh screen. `device` overrides the
    /// device name when given.
    pub fn connect(&self, device: Option<String>) {
        self.disconnect_quietly();
        let mut st = self.st.borrow_mut();
        let opts = st.opts.clone();
        let Some(system) = self.state.borrow().systems.get(&opts.system).cloned() else {
            drop(st);
            self.report(&qsa_msg::message(id::HOST_NOT_FOUND, &[&opts.system]), true);
            return;
        };
        let size = tn_size(opts.size);
        st.terminal = Terminal::new(size, opts.ccsid);
        st.conn = Conn::Connecting;
        st.security = Security::Plain;
        st.mode = None;
        st.device = None;
        let device = device.unwrap_or(opts.device.clone());
        let host = if system.address_lookup == AddressLookup::Never && !system.fixed_ip.is_empty() {
            system.fixed_ip.clone()
        } else {
            system.name.clone()
        };
        let port = if opts.tls {
            system.ports.telnet_tls
        } else {
            system.ports.telnet
        };
        let tls = opts.tls.then(|| TlsOptions {
            trust: trust_for(&system),
            system_roots: true,
            timeout: CONNECT_TIMEOUT,
        });
        let request = Request::display(size, Some(&device), opts.ccsid);
        st.worker = Some(Worker::spawn(Target { host, port, tls }, request));
        if let Some(path) = st.record_to.take() {
            match Recorder::create(&path, size, opts.ccsid) {
                Ok(r) => {
                    crate::log!("recording this session to {}", path.display());
                    st.recorder = Some(r);
                }
                Err(e) => crate::log!("recording not started: {}: {e}", path.display()),
            }
        }
        drop(st);
        self.report(
            &qsa_msg::message(id::CONNECTING, &[&opts.system, &port.to_string()]),
            false,
        );
        self.refresh();
    }

    fn disconnect_quietly(&self) {
        let mut st = self.st.borrow_mut();
        if let Some(mut w) = st.worker.take() {
            w.close();
        }
        st.recorder = None;
        if st.conn != Conn::Idle {
            st.conn = Conn::Disconnected;
        }
    }

    /// Disconnects; the last screen stays, greyed.
    pub fn disconnect(&self) {
        self.disconnect_quietly();
        self.refresh();
    }

    /// Disconnects at the user's request, and says so.
    fn user_disconnect(&self) {
        let (was, system) = {
            let st = self.st.borrow();
            (st.worker.is_some(), st.opts.system.clone())
        };
        self.disconnect();
        if was {
            self.report(&qsa_msg::message(id::DISCONNECTED, &[&system]), false);
        }
    }

    fn send(&self, st: &mut State, rec: &Record) {
        if let Some(r) = st.recorder.as_mut() {
            r.outbound(&st.terminal.screen, rec);
        }
        if let Some(w) = &st.worker {
            let _ = w.tx.send(ToNet::Send(rec.clone()));
        }
    }

    fn key(&self, k: K5250) {
        let mut st = self.st.borrow_mut();
        if st.conn != Conn::Connected {
            return;
        }
        let before = st.recorder.is_some().then(|| st.terminal.screen.clone());
        if let (Some(b), Some(r)) = (before, st.recorder.as_mut()) {
            r.key(&b, k);
        }
        {
            let st = &mut *st;
            if let Some(m) = st.macro_rec.as_mut() {
                m.key(&st.terminal.screen, k);
            }
        }
        let out = st.terminal.key(k);
        if out.error.is_some() {
            fltk::dialog::beep(fltk::dialog::BeepType::Error);
        }
        for rec in &out.send {
            self.send(&mut st, rec);
        }
        drop(st);
        self.refresh();
    }

    fn key_event(&self, p: &qsa_ui::terminal::KeyPress) -> bool {
        if self.st.borrow().pick == Pick::Waiting {
            if p.key == Key::Escape {
                self.st.borrow_mut().pick = Pick::Cancelled;
            }
            return true;
        }
        let action = {
            let mut st = self.st.borrow_mut();
            let map = st.keymap.clone();
            st.decoder.decode(p, &map)
        };
        match action {
            Action::Key(k) => self.key(k),
            Action::Type(c) => self.key(K5250::Char(c)),
            Action::Pass => return false,
            Action::Swallow => {}
        }
        true
    }

    /// The hotspot that a click at a cell (from 0) would act on: only while
    /// connected with the keyboard open, so that a click always does what
    /// the hand pointer promises.
    pub fn hotspot_at(&self, r: usize, c: usize) -> Option<super::hotspots::Hotspot> {
        let st = self.st.borrow();
        if st.conn != Conn::Connected
            || !st.terminal.screen.keyboard.is_open()
            || st.pick != Pick::Off
        {
            return None;
        }
        let list = super::hotspots::detect(&st.terminal.screen, &st.doc.hotspots);
        super::hotspots::at(&list, r, c)
    }

    fn click(&self, r: usize, c: usize) {
        if let Some(h) = self.hotspot_at(r, c) {
            crate::log!("hotspot at {},{}: {:?}", r + 1, c + 1, h.key);
            self.key(h.key);
            return;
        }
        let mut st = self.st.borrow_mut();
        if st.pick == Pick::Waiting {
            st.pick = Pick::Got(r, c);
            return;
        }
        if st.conn == Conn::Connected && st.terminal.screen.keyboard.is_open() {
            let a = st.terminal.screen.addr(r, c);
            st.terminal.screen.cursor = a;
            drop(st);
            self.refresh();
        }
    }

    fn paste(&self, text: &str) {
        let mut st = self.st.borrow_mut();
        if st.conn != Conn::Connected {
            return;
        }
        {
            let st = &mut *st;
            if let Some(m) = st.macro_rec.as_mut() {
                m.paste(&st.terminal.screen, text);
            }
        }
        let out = st.terminal.paste(text);
        if out.error.is_some() {
            fltk::dialog::beep(fltk::dialog::BeepType::Error);
        }
        for rec in &out.send {
            self.send(&mut st, rec);
        }
        drop(st);
        self.refresh();
    }

    fn copy(&self) {
        let st = self.st.borrow();
        let s = &st.terminal.screen;
        let text = self
            .term
            .selected_text(s)
            .unwrap_or_else(|| qsa_ui::terminal::block_text(s, 0, 0, s.rows() - 1, s.cols() - 1));
        app::copy(&text);
        self.term.set_selection(None);
    }

    fn print(&self) {
        let (bytes, path) = {
            let st = self.st.borrow();
            let look = Look {
                scheme: scheme(st.opts.color),
                greyed: false,
            };
            let title = format!("{} - 5250 Display", st.opts.system);
            (
                pdf::pdf(&st.terminal.screen, look, &title),
                pdf::output_path(&st.opts.system),
            )
        };
        let result = path
            .parent()
            .map_or(Ok(()), std::fs::create_dir_all)
            .and_then(|()| std::fs::write(&path, bytes));
        match result {
            Ok(()) => self.report(
                &qsa_msg::message(id::SCREEN_PRINTED, &[&path.display().to_string()]),
                false,
            ),
            Err(e) => {
                crate::log!("{}: {e}", path.display());
                MessageBox::new(
                    "Print Screen",
                    &format!("{} could not be written: {e}", path.display()),
                )
                .glyph(Glyph::Cross)
                .run(Some(&self.window));
            }
        }
    }

    fn keymap(&self) {
        let (title, map) = {
            let st = self.st.borrow();
            (
                format!("Keyboard Map - {}", st.opts.system),
                st.keymap.clone(),
            )
        };
        if let Some(map) = keymapdlg::run(Some(&self.window), &title, &map) {
            self.st.borrow_mut().keymap = map;
        }
    }

    /// Handles everything waiting: commands, then network events. Returns
    /// false once the window is closed.
    pub fn pump(&self) -> bool {
        self.refresh_recent();
        while let Some(c) = self.rx.recv() {
            match c {
                Cmd::Open => {
                    if let Some(p) = files::choose_open_in(
                        &files::sessions_dir(),
                        "Open Session",
                        files::SESSION_PATTERN,
                    ) {
                        crate::open::open_path(&self.state, &p, Some(&self.window));
                    }
                }
                Cmd::Save => self.save(),
                Cmd::SaveAs => self.save_as(),
                Cmd::Recent(i) => {
                    let p = self.state.borrow().settings.recent_sessions.get(i).cloned();
                    if let Some(p) = p {
                        crate::open::open_path(&self.state, &p, Some(&self.window));
                    }
                }
                Cmd::Properties => self.properties(),
                Cmd::LoadKeymap => self.choose_keymap(),
                Cmd::ExportKeymap => self.export_keymap(),
                Cmd::MacroRecord => self.toggle_recording(),
                Cmd::MacroPlay => {
                    if let Some(p) = files::choose_open_in(
                        &files::sessions_dir(),
                        "Play Macro",
                        files::MACRO_PATTERN,
                    ) {
                        self.play_file(&p);
                    }
                }
                Cmd::MacroStop => self.stop_macro(),
                Cmd::MacroEdit => {
                    if let Some(p) = files::choose_open_in(
                        &files::sessions_dir(),
                        "Edit Macro",
                        files::MACRO_PATTERN,
                    ) {
                        crate::macros::editor::open(&p, Some(&self.window));
                    }
                }
                Cmd::MacroRun(i) => {
                    let p = self.st.borrow().doc.macros.files.get(i).cloned();
                    if let Some(p) = p {
                        self.play_file(&p);
                    }
                }
                Cmd::Connect => self.connect(None),
                Cmd::Disconnect => self.user_disconnect(),
                Cmd::Copy => self.copy(),
                Cmd::Paste => app::paste_text(self.term.widget()),
                Cmd::Print => self.print(),
                Cmd::Keypad => {
                    let on = !self.st.borrow().keypad;
                    self.set_keypad(on);
                }
                Cmd::KeyMap => self.keymap(),
                Cmd::Look(c) => self.set_color_map(c),
                Cmd::Key(k) => {
                    self.key(k);
                    let _ = self.term.widget().clone().take_focus();
                }
                Cmd::About => crate::about::show(Some(&self.window)),
                Cmd::Close => {
                    self.st.borrow_mut().player = None;
                    self.disconnect_quietly();
                    self.window.clone().hide();
                }
            }
        }
        loop {
            let ev = {
                let st = self.st.borrow();
                match &st.worker {
                    Some(w) => w.rx.try_recv().ok(),
                    None => None,
                }
            };
            let Some(ev) = ev else { break };
            self.net(ev);
        }
        if self.playing() {
            self.run_macro();
        }
        self.window.shown()
    }

    fn net(&self, ev: FromNet) {
        let system = self.st.borrow().opts.system.clone();
        match ev {
            FromNet::Connected(sec) => {
                let mut st = self.st.borrow_mut();
                st.security = sec;
                st.conn = Conn::Connected;
                drop(st);
                self.status.clone().set_message("");
            }
            FromNet::Ready(mode) => {
                self.st.borrow_mut().mode = Some(mode);
                if mode == Mode::Basic {
                    self.report(&qsa_msg::message(id::BASIC_5250, &[]), false);
                }
            }
            FromNet::Startup(s) => match s.outcome() {
                Outcome::Started => self.st.borrow_mut().device = s.device,
                Outcome::InUse | Outcome::NotValid => {
                    let device = s.device.clone().unwrap_or_default();
                    let mid = if s.outcome() == Outcome::InUse {
                        id::DEVICE_IN_USE
                    } else {
                        id::DEVICE_NOT_VALID
                    };
                    let m = qsa_msg::message(mid, &[&device, &s.code, s.meaning()]);
                    self.disconnect();
                    self.report(&m, false);
                    let choice = MessageBox::new(
                        "QS/400 Access for Desktop",
                        &format!("{}\n\nMessage ID: {}", m.text, m.id),
                    )
                    .glyph(Glyph::Cross)
                    .buttons(&["Retry without device name", "Close", "Details..."], 0, 1)
                    .run(Some(&self.window));
                    match choice {
                        0 => self.connect(Some(String::new())),
                        2 => msgdetails::show_details(Some(&self.window), &m),
                        _ => {}
                    }
                }
                Outcome::Failed => {
                    let why = format!("start-up response {} ({})", s.code, s.meaning());
                    self.report(
                        &qsa_msg::message(id::PROTOCOL_ERROR, &[&system, &why]),
                        false,
                    );
                }
            },
            FromNet::Record(r) => {
                let mut st = self.st.borrow_mut();
                if let Some(rec) = st.recorder.as_mut() {
                    rec.inbound(&r);
                }
                let applied = st.terminal.receive(&r);
                for reply in &applied.replies {
                    self.send(&mut st, reply);
                }
                drop(st);
                if let Some(e) = applied.errors.first() {
                    self.report(
                        &qsa_msg::message(id::PROTOCOL_ERROR, &[&system, &e.to_string()]),
                        false,
                    );
                }
            }
            FromNet::ProtocolError(e) => {
                self.report(&qsa_msg::message(id::PROTOCOL_ERROR, &[&system, &e]), false);
            }
            FromNet::Ended(end) => self.ended(end, &system),
        }
        self.refresh();
    }

    fn ended(&self, end: Ended, system: &str) {
        {
            let mut st = self.st.borrow_mut();
            st.worker = None;
            st.recorder = None;
            st.conn = Conn::Disconnected;
        }
        self.refresh();
        let msg = |mid: &str| qsa_msg::message(mid, &[system]);
        match end {
            Ended::ByUser => {}
            Ended::Closed => self.report(&msg(id::SESSION_ENDED), true),
            Ended::Io(e) => {
                crate::log!("session to {system}: {e}");
                self.report(&msg(id::SESSION_ENDED), true);
            }
            Ended::Connect(ConnectError::NotFound(_)) => {
                self.report(&msg(id::HOST_NOT_FOUND), true)
            }
            Ended::Connect(ConnectError::Refused(_)) => {
                self.report(&msg(id::CONNECTION_REFUSED), true)
            }
            Ended::Connect(ConnectError::TimedOut(_)) => {
                self.report(&msg(id::CONNECT_TIMED_OUT), true)
            }
            Ended::Connect(ConnectError::Io { source, .. }) => {
                crate::log!("connecting to {system}: {source}");
                let mid = if source.kind() == std::io::ErrorKind::TimedOut {
                    id::CONNECT_TIMED_OUT
                } else {
                    id::CONNECTION_REFUSED
                };
                self.report(&msg(mid), true);
            }
            Ended::Connect(ConnectError::Tls(TlsError::Handshake(why))) => {
                self.report(&qsa_msg::message(id::TLS_FAILED, &[system, &why]), true);
            }
            Ended::Connect(ConnectError::Tls(TlsError::Untrusted {
                reason,
                certificate,
                info,
            })) => {
                self.report(
                    &qsa_msg::message(id::CERT_UNTRUSTED, &[system, &reason]),
                    false,
                );
                if cert::ask(Some(&self.window), system, &reason, &info)
                    && self.trust(system, &certificate)
                {
                    self.report(&qsa_msg::message(id::CERT_TRUSTED, &[system]), false);
                    self.connect(None);
                }
            }
        }
    }

    /// Adds a certificate to the system's CA list and saves it.
    fn trust(&self, system: &str, der: &[u8]) -> bool {
        let mut app = self.state.borrow_mut();
        let Some(mut sys) = app.systems.get(system).cloned() else {
            return false;
        };
        sys.ca_certificates
            .push(CaCertificate::Pem(tn5250::telnet::tls::to_pem(der)));
        app.systems.upsert(sys);
        match app.save_systems() {
            Ok(()) => true,
            Err(e) => {
                crate::log!("systems not saved: {e}");
                false
            }
        }
    }
}

/// What a system's CA list says to trust: PEM text kept in the
/// configuration, and PEM files read now.
pub(crate) fn trust_for(system: &System) -> Trust {
    let mut pems = Vec::new();
    for ca in &system.ca_certificates {
        match ca {
            CaCertificate::Pem(p) => pems.push(p.clone()),
            CaCertificate::Path(path) => match std::fs::read_to_string(path) {
                Ok(p) => pems.push(p),
                Err(e) => crate::log!("CA certificate {}: {e}", path.display()),
            },
        }
    }
    Trust::from_pem(pems.iter().map(String::as_str))
}

/// `<title> - 5250 Display`.
fn window_title(doc: &SessionFile, file: Option<&Path>) -> String {
    format!("{} - 5250 Display", doc.display_title(file))
}
