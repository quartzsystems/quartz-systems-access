// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! The 5250 Display window: menu bar, toolbar, the terminal, an optional
//! keypad, and a status bar. Each session is its own window in the one
//! `qsaccess` process. The network runs on a worker thread
//! ([`super::worker`]); the screen is kept and drawn here, on the GUI thread.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::{Rc, Weak};

use fltk::app;
use fltk::button::Button;
use fltk::enums::{Event, Key, Shortcut};
use fltk::group::Group;
use fltk::menu::{MenuBar, MenuFlag};
use fltk::prelude::*;
use fltk::window::Window;
use qsa_config::settings::{ColorMap, ScreenSize as CfgSize};
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
use super::{cert, keymapdlg, pdf};
use crate::app::Shared;

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

/// What menus, toolbar and keypad ask for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Cmd {
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
    opts: StartOptions,
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
}

/// One session window.
pub struct SessionWindow {
    pub window: Window,
    term: TerminalView,
    keypad: Group,
    toolbar: Group,
    status: StatusBar,
    menu: MenuBar,
    rx: app::Receiver<Cmd>,
    state: Shared,
    st: RefCell<State>,
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

/// The scale the window opens at: the largest that fits 90 % of the screen.
fn initial_scale(rows: usize, cols: usize, extra_w: i32, extra_h: i32) -> usize {
    let (_, _, sw, sh) = app::screen_xywh(0);
    let (w1, h1) = TerminalView::size_for(rows, cols, 1);
    let fit = |avail: i32, extra: i32, one: i32| ((avail * 9 / 10 - extra) / one.max(1)).max(1);
    fit(sw, extra_w, w1).min(fit(sh, extra_h, h1)).max(1) as usize
}

impl SessionWindow {
    /// Builds a session window for `opts` without connecting. `scale`
    /// fixes the terminal's scale (for screenshots); otherwise it is chosen
    /// for the screen.
    pub fn build(state: &Shared, opts: StartOptions, scale: Option<usize>) -> Rc<SessionWindow> {
        let (tx, rx) = app::channel::<Cmd>();
        let size = tn_size(opts.size);
        let chrome_h = MENU_H + Toolbar::HEIGHT + StatusBar::HEIGHT;
        let s = scale
            .unwrap_or_else(|| initial_scale(size.rows(), size.cols(), KEYPAD_W, chrome_h + 60));
        let (tw, th) = TerminalView::size_for(size.rows(), size.cols(), s);
        let (w, h) = (tw, chrome_h + th);
        let mut window = Window::new(0, 0, w, h, None);
        window.set_label(&format!("{} - 5250 Display", opts.system));
        window.set_color(colors::FACE);
        window.size_range(320, 240, 0, 0);

        let mut menu = menu_bar(0, 0, w);
        let mut add = |path: &str, sc: Shortcut, c: Cmd, flag: MenuFlag| {
            menu.add_emit(path, sc, flag, tx, c);
        };
        let n = MenuFlag::Normal;
        add("&File/&Print Screen", Shortcut::Ctrl | 'p', Cmd::Print, n);
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
                state: state.clone(),
                st: RefCell::new(State {
                    opts,
                    terminal,
                    worker: None,
                    conn: Conn::Idle,
                    security: Security::Plain,
                    mode: None,
                    device: None,
                    decoder: Decoder::default(),
                    keymap: KeyMap::defaults(),
                    recorder: None,
                    record_to,
                    keypad: false,
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
        sw.refresh();
        sw
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

    fn click(&self, r: usize, c: usize) {
        let mut st = self.st.borrow_mut();
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
        while let Some(c) = self.rx.recv() {
            match c {
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
fn trust_for(system: &System) -> Trust {
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
