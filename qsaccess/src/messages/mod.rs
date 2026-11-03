// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! Messages (the main window's General group): a system's message queues
//! over its QAP session (version 6's `Messages` service).
//!
//! - Queue: My messages (`*Own`), System operator (`QSYSOPR`, offered only
//!   when the profile may read it), and Other queue... (library and name).
//! - The list is the queue newest first, 100 messages a page, with
//!   Previous and Next: Sent, Type ("Sender copy" for a sender's copy),
//!   Severity, ID, the first-level text, From user and From job, inquiries
//!   waiting for a reply in bold. A column heading sorts the page shown.
//!   While the queue is watched, new messages come in at the top as they
//!   arrive, and a reply marks what it answers as answered.
//! - The detail pane shows the selected message whole: its text, its
//!   second-level text as sent, who sent it, and its reply once answered.
//!   A reply leads to the message it answers; an answered inquiry or
//!   sender's copy to its reply.
//! - Reply: the reply field and Reply, for an inquiry waiting; an empty
//!   reply sends the message's default. A reply the message does not take
//!   shows the system's message (`CPF2420`) as it was sent.
//! - Remove the selected messages, or every answered and informational
//!   message on the queue, each after a confirmation with the count;
//!   inquiries not answered yet need a confirmation of their own, which
//!   says their senders get no reply.
//! - A double-click on a message, or Details..., shows it as the system
//!   sent it, as everywhere else.
//!
//! Before QAP version 6 Messages is not offered (`QAD5001`). The network
//! never runs on the GUI thread: a page is listed on a thread of its own,
//! which holds the shared client only to send.

pub mod arrived;
pub mod model;
pub mod options;
pub mod send;
pub mod watching;

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::mpsc;

use fltk::app;
use fltk::button::Button;
use fltk::enums::{Align, CallbackTrigger, FrameType, Key as KeyCode, Shortcut};
use fltk::frame::Frame;
use fltk::input::Input;
use fltk::menu::{Choice, MenuBar, MenuFlag};
use fltk::prelude::*;
use fltk::text::{TextBuffer, TextDisplay, WrapMode};
use fltk::window::Window;
use qap_client::messages::{PAGE, needs_reply};
use qap_client::proto::Op;
use qap_client::proto::ops::messages::{Key, ListMessages, Messages, Queue, QueuedMessage};
use qap_client::spool::Paging;
use qap_client::{Error, WatchEvent};
use qsa_config::System;
use qsa_msg::{Message, id};
use qsa_ui::chan::{self, Emit, MenuEmit};
use qsa_ui::inputbox::InputBox;
use qsa_ui::listtable::ListTable;
use qsa_ui::msgdetails::{self, SystemMessage};
use qsa_ui::sampler::{MENU_H, menu_bar};
use qsa_ui::theme::{self, colors};
use qsa_ui::{MessageBox, Status, StatusBar, controls};

use crate::app::Shared;
use crate::qap::{self, Handle, lock};

use model::Link;

/// What the main window and messages call this function.
pub const FUNCTION: &str = "Messages";

/// Window width and height.
pub const W: i32 = 960;
pub const H: i32 = 640;

/// How often the window looks at its workers.
const POLL: f64 = 0.1;

/// Messages per request when the whole queue is looked through.
const WHOLE_PAGE: u32 = 500;

/// Keys per `RemoveMessages`: well under a frame.
const REMOVE_BATCH: usize = 1000;

/// What menus, buttons and the list ask for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cmd {
    QueueChanged,
    Refresh,
    Previous,
    Next,
    Sort(usize),
    Selected,
    Details,
    Reply,
    Remove,
    RemoveAnswered,
    Link,
    Send,
    Options,
    Close,
    About,
}

/// The window's widgets.
pub struct Widgets {
    pub window: Window,
    pub menu: MenuBar,
    pub queue: Choice,
    pub list: ListTable,
    pub paging: Frame,
    pub previous: Button,
    pub next: Button,
    pub detail: TextDisplay,
    pub detail_text: TextBuffer,
    pub link: Button,
    pub reply: Input,
    pub reply_button: Button,
    pub status: StatusBar,
}

/// A look through the whole queue for a link's message: its place,
/// newest first, when it is there.
type Finding = mpsc::Receiver<Result<Option<u32>, Error>>;

/// What the window keeps.
struct St {
    system: System,
    state: Option<Shared>,
    handle: Option<Handle>,
    /// The signed-on profile, for `Own`'s name.
    profile: String,
    /// The selector's queues, in order; "Other queue..." follows them.
    queues: Vec<Queue>,
    queue: Queue,
    paging: Paging,
    /// The page shown, in the order shown.
    msgs: Vec<QueuedMessage>,
    sort: Option<(usize, bool)>,
    listing: Option<mpsc::Receiver<Result<Messages, Error>>>,
    /// List again once the request in flight is done.
    again: bool,
    /// Select where this leads once the page is in.
    select_after: Option<Link>,
    /// A link's message being looked for in the whole queue: its place,
    /// newest first.
    finding: Option<(Link, Finding)>,
}

/// One Messages window.
pub struct MessagesWindow {
    pub w: Widgets,
    rx: chan::Receiver<Cmd>,
    st: RefCell<St>,
    /// The messages selected before a refill, by key.
    keep: RefCell<Vec<Key>>,
}

thread_local! {
    static WINDOWS: RefCell<Vec<Rc<MessagesWindow>>> = const { RefCell::new(Vec::new()) };
    static TICKING: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Why Messages is not available for `system`, if it is not: `QAD5001`
/// when its QAP version has no `Messages` service. A version not known does
/// not stop it: opening it finds out.
pub fn blocked(system: &str, version: Option<u16>) -> Option<Message> {
    let op = Op::ListMessages;
    match version {
        Some(v) if v < op.since() => Some(qap::version_needed(FUNCTION, system, op, v)),
        _ => None,
    }
}

fn build_widgets(system: &str) -> (Widgets, chan::Receiver<Cmd>) {
    let (tx, rx) = chan::channel::<Cmd>();
    let mut window = Window::new(0, 0, W, H, None);
    window.set_label(&format!("{FUNCTION} - {system}"));
    window.set_icon(Some(qsa_ui::glyph::app_icon()));
    window.set_color(colors::FACE);
    window.size_range(640, 440, 0, 0);

    let mut menu = menu_bar(0, 0, W);
    let n = MenuFlag::Normal;
    let d = MenuFlag::MenuDivider;
    let mut add = |p: &str, sc: Shortcut, c: Cmd, f: MenuFlag| {
        menu.add_emit_to(p, sc, f, tx, c);
    };
    add("&File/&Send Message...", Shortcut::Ctrl | 'n', Cmd::Send, d);
    add("&File/&Close", Shortcut::Ctrl | 'w', Cmd::Close, n);
    add(
        "&View/&Refresh",
        Shortcut::None | KeyCode::F5,
        Cmd::Refresh,
        n,
    );
    add("&Messages/&Details...", Shortcut::None, Cmd::Details, d);
    add("&Messages/Re&ply", Shortcut::Ctrl | 'r', Cmd::Reply, n);
    add(
        "&Messages/&Remove",
        Shortcut::None | KeyCode::Delete,
        Cmd::Remove,
        n,
    );
    add(
        "&Messages/Remove All &Answered and Informational...",
        Shortcut::None,
        Cmd::RemoveAnswered,
        d,
    );
    add("&Messages/&Options...", Shortcut::None, Cmd::Options, n);
    add("&Help/&About...", Shortcut::None, Cmd::About, n);

    let (m, fh) = (8, theme::FIELD_H);
    let (bw, bh) = (theme::BUTTON_W, theme::BUTTON_H);
    let top = MENU_H + 8;
    let mut l = controls::label(m, top, 50, fh, "Queue:");
    l.set_align(Align::Left | Align::Inside);
    let mut queue = controls::combo(m + 50, top, 280, fh);
    queue.set_callback(move |_| tx.send(Cmd::QueueChanged));

    let status_y = H - StatusBar::HEIGHT;
    let reply_y = status_y - 8 - bh;
    let detail_h = 150;
    let detail_y = reply_y - 6 - detail_h;
    let py = detail_y - 8 - bh;
    let ly = top + fh + 8;
    let lh = py - 4 - ly;

    let mut list = ListTable::new(m - 2, ly, W - 2 * m + 4, lh, &model::COLUMNS);
    list.set_multi(true);
    list.on_select(move |_| tx.send(Cmd::Selected));
    list.on_activate(move |r| {
        if r.is_some() {
            tx.send(Cmd::Details);
        }
    });
    list.on_header_click(move |c| tx.send(Cmd::Sort(c)));

    let mut previous = controls::button(m, py, bw, bh, "Previous");
    previous.emit_to(tx, Cmd::Previous);
    let mut next = controls::button(m + bw + 6, py, bw, bh, "Next");
    next.emit_to(tx, Cmd::Next);
    let mut paging = Frame::new(m + 2 * bw + 18, py, 320, bh, None);
    paging.set_align(Align::Left | Align::Inside);
    paging.set_label_size(theme::UI_SIZE);

    let detail_text = TextBuffer::default();
    let mut detail = TextDisplay::new(m, detail_y, W - 2 * m, detail_h, None);
    detail.set_buffer(Some(detail_text.clone()));
    detail.set_frame(FrameType::BorderBox);
    detail.set_text_font(theme::UI_FONT);
    detail.set_text_size(theme::UI_SIZE);
    detail.wrap_mode(WrapMode::AtBounds, 0);

    let link_w = 220;
    let mut link = controls::button(m, reply_y, link_w, bh, "");
    link.emit_to(tx, Cmd::Link);
    link.hide();
    let rw = 260;
    let rx_ = W - m - bw - 6 - rw;
    let mut rl = controls::label(rx_ - 156, reply_y, 150, bh, "Reply (empty for default):");
    rl.set_align(Align::Right | Align::Inside);
    let mut reply = controls::field(rx_, reply_y + (bh - fh) / 2, rw, fh);
    reply.set_trigger(CallbackTrigger::EnterKeyAlways);
    reply.set_callback(move |_| tx.send(Cmd::Reply));
    let mut reply_button = controls::button(W - m - bw, reply_y, bw, bh, "Reply");
    reply_button.emit_to(tx, Cmd::Reply);
    reply.deactivate();
    reply_button.deactivate();

    let status = StatusBar::new(0, status_y, W);
    window.resizable(list.table());
    window.end();
    window.set_callback(move |_| tx.send(Cmd::Close));
    (
        Widgets {
            window,
            menu,
            queue,
            list,
            paging,
            previous,
            next,
            detail,
            detail_text,
            link,
            reply,
            reply_button,
            status,
        },
        rx,
    )
}

impl MessagesWindow {
    /// The window for `system` as `profile`, offering `QSYSOPR` when
    /// `operator`; not shown and not listing (screenshots use it so).
    pub fn build(system: &System, profile: &str, operator: bool) -> Rc<MessagesWindow> {
        let (w, rx) = build_widgets(&system.name);
        let mut queues = vec![Queue::Own];
        if operator {
            queues.push(Queue::SystemOperator);
        }
        let win = Rc::new(MessagesWindow {
            w,
            rx,
            keep: RefCell::new(Vec::new()),
            st: RefCell::new(St {
                system: system.clone(),
                state: None,
                handle: None,
                profile: profile.to_owned(),
                queues,
                queue: Queue::Own,
                paging: Paging::new(PAGE),
                msgs: Vec::new(),
                sort: None,
                listing: None,
                again: false,
                select_after: None,
                finding: None,
            }),
        });
        win.fill_selector();
        win
    }

    /// Shows `msgs` as the page from `start` of `total` on `queue`, without
    /// a system (screenshots), times in `sent`.
    pub fn show_messages(
        &self,
        queue: &Queue,
        start: u32,
        msgs: Vec<QueuedMessage>,
        total: u32,
        sent: &dyn Fn(i64) -> String,
    ) {
        {
            let mut st = self.st.borrow_mut();
            if !st.queues.contains(queue) {
                st.queues.push(queue.clone());
            }
            st.queue = queue.clone();
            st.paging.start = start;
            st.paging.update(total);
            st.msgs = msgs;
        }
        self.fill_selector();
        self.fill(sent);
    }

    /// Selects row `i`, scrolled into view, and shows it in the detail pane
    /// (screenshots).
    pub fn select_row(&self, i: usize, sent: &dyn Fn(i64) -> String) {
        let mut list = self.w.list.clone();
        list.select_rows(&[i]);
        list.table().clone().set_row_position(i as i32);
        self.show_detail(sent);
    }

    /// The system's name.
    pub fn system(&self) -> String {
        self.st.borrow().system.name.clone()
    }

    /// The queue shown.
    pub fn queue(&self) -> Queue {
        self.st.borrow().queue.clone()
    }

    pub fn window(&self) -> &Window {
        &self.w.window
    }

    fn parent(&self) -> Option<&dyn WindowExt> {
        Some(&self.w.window as &dyn WindowExt)
    }

    fn queue_name(&self) -> String {
        let st = self.st.borrow();
        model::queue_name(&st.queue, &st.profile)
    }

    fn report(&self, text: &str) {
        crate::log!("{}: {text}", self.system());
        self.w.status.clone().set_message(text);
    }

    fn fill_selector(&self) {
        let st = self.st.borrow();
        let mut c = self.w.queue.clone();
        c.clear();
        for q in &st.queues {
            c.add_choice(&model::queue_choice(q, &st.profile).replace('/', "\\/"));
        }
        c.add_choice("Other queue...");
        let at = st.queues.iter().position(|q| *q == st.queue).unwrap_or(0);
        c.set_value(at as i32);
    }

    /// The rows from the page kept, keeping the selection by key.
    fn fill(&self, sent: &dyn Fn(i64) -> String) {
        let st = self.st.borrow();
        let mut list = self.w.list.clone();
        let rows = st.msgs.iter().map(|m| model::row(m, sent)).collect();
        list.set_rows(rows);
        list.set_bold(model::bold(&st.msgs));
        let keep = self.keep.borrow();
        let again: Vec<usize> = st
            .msgs
            .iter()
            .enumerate()
            .filter(|(_, m)| keep.contains(&m.key))
            .map(|(i, _)| i)
            .collect();
        list.select_rows(&again);
        list.set_sort(st.sort);
        self.w
            .paging
            .clone()
            .set_label(&model::paging_line(&st.paging, st.msgs.len()));
        for (b, on) in [
            (&self.w.previous, st.paging.has_previous()),
            (&self.w.next, st.paging.has_next()),
        ] {
            let mut b = b.clone();
            if on {
                b.activate();
            } else {
                b.deactivate();
            }
        }
        let sorted = st.sort.map(|(c, _)| model::COLUMNS[c].0);
        let line = model::list_line(
            st.paging.total,
            &model::queue_name(&st.queue, &st.profile),
            sorted,
        );
        drop(st);
        drop(keep);
        self.w.status.clone().set_message(&line);
        self.show_detail(sent);
        self.w.window.clone().redraw();
    }

    /// The messages selected, in the order shown.
    fn selected(&self) -> Vec<QueuedMessage> {
        let st = self.st.borrow();
        self.w
            .list
            .selected_rows()
            .into_iter()
            .filter_map(|i| st.msgs.get(i).cloned())
            .collect()
    }

    /// The message the detail pane, Reply and Details act on: the first
    /// selected.
    fn current(&self) -> Option<QueuedMessage> {
        let st = self.st.borrow();
        self.w.list.selected().and_then(|i| st.msgs.get(i).cloned())
    }

    fn keep_selection(&self) {
        *self.keep.borrow_mut() = self.selected().into_iter().map(|m| m.key).collect();
    }

    /// The detail pane, the link and Reply for the current message.
    fn show_detail(&self, sent: &dyn Fn(i64) -> String) {
        let m = self.current();
        let mut buf = self.w.detail_text.clone();
        buf.set_text(
            &m.as_ref()
                .map(|m| model::detail(m, sent))
                .unwrap_or_default(),
        );
        let mut link = self.w.link.clone();
        match m.as_ref().and_then(Link::of) {
            Some(l) => {
                link.set_label(l.label());
                link.show();
            }
            None => link.hide(),
        }
        let waiting = m.as_ref().is_some_and(needs_reply);
        let mut reply = self.w.reply.clone();
        let mut button = self.w.reply_button.clone();
        if waiting {
            reply.activate();
            button.activate();
        } else {
            reply.set_value("");
            reply.deactivate();
            button.deactivate();
        }
    }

    /// Asks for the page in view, unless a request is in flight (then once
    /// it is done).
    pub fn list(&self) {
        let mut st = self.st.borrow_mut();
        let Some(h) = st.handle.clone() else { return };
        if st.listing.is_some() {
            st.again = true;
            return;
        }
        let req = ListMessages {
            queue: st.queue.clone(),
            newest_first: true,
            start: st.paging.start,
            max: st.paging.per_page,
        };
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(qap::call_unlocked(&h, Op::ListMessages, &req));
            app::awake();
        });
        st.listing = Some(rx);
    }

    /// Gives the window its waiting work; false once it is closed.
    fn pump(&self) -> bool {
        while let Some(c) = self.rx.recv() {
            self.command(c);
            if !self.w.window.shown() {
                return false;
            }
        }
        self.take_listing();
        self.take_finding();
        self.w.window.shown()
    }

    fn take_listing(&self) {
        let r = {
            let st = self.st.borrow();
            match st.listing.as_ref().map(mpsc::Receiver::try_recv) {
                Some(Ok(r)) => r,
                Some(Err(mpsc::TryRecvError::Empty)) | None => return,
                Some(Err(mpsc::TryRecvError::Disconnected)) => {
                    Err(Error::Closed(qap_client::Closed::ByClient))
                }
            }
        };
        let again = {
            let mut st = self.st.borrow_mut();
            st.listing = None;
            std::mem::take(&mut st.again)
        };
        match r {
            Ok(page) => {
                let (moved, select) = {
                    let mut st = self.st.borrow_mut();
                    let moved = st.paging.update(page.total);
                    let mut msgs = page.messages;
                    if let Some((c, asc)) = st.sort {
                        model::sort_page(&mut msgs, c, asc);
                    }
                    st.msgs = msgs;
                    (moved, st.select_after.take())
                };
                if let Some(l) = select {
                    let st = self.st.borrow();
                    *self.keep.borrow_mut() = st
                        .msgs
                        .iter()
                        .filter(|m| l.leads_to(m))
                        .map(|m| m.key)
                        .collect();
                }
                self.fill(&crate::splf::model::local_time);
                if moved || again {
                    self.list();
                }
            }
            Err(e) => {
                let x = qap::explain(&self.st.borrow().system, &e);
                self.report(&x.line());
                if x.server.is_some() && !again {
                    qap::show(self.parent(), &x);
                }
                if again {
                    self.list();
                }
            }
        }
    }

    fn take_finding(&self) {
        let (link, r) = {
            let st = self.st.borrow();
            match st.finding.as_ref().map(|(l, rx)| (*l, rx.try_recv())) {
                Some((l, Ok(r))) => (l, r),
                _ => return,
            }
        };
        self.st.borrow_mut().finding = None;
        match r {
            Ok(Some(at)) => {
                {
                    let mut st = self.st.borrow_mut();
                    st.paging.start = at / st.paging.per_page * st.paging.per_page;
                    st.sort = None;
                    st.select_after = Some(link);
                }
                self.list();
            }
            Ok(None) => self.report("That message is no longer on the queue."),
            Err(e) => {
                let x = qap::explain(&self.st.borrow().system, &e);
                self.report(&x.line());
            }
        }
    }

    /// A message the watch reported: on the first page it comes in at the
    /// top; a reply marks what it answers on the page as answered.
    pub fn arrived(&self, queue: &Queue, m: &QueuedMessage, sent: &dyn Fn(i64) -> String) {
        {
            let mut st = self.st.borrow_mut();
            if st.queue != *queue {
                return;
            }
            if let Some(k) = m.answers
                && let Some(t) = st.msgs.iter_mut().find(|x| x.key == k)
            {
                t.needs_reply = false;
                t.reply.get_or_insert_with(|| m.text.clone());
            }
            if !st.msgs.iter().any(|x| x.key == m.key) {
                st.paging.total = st.paging.total.saturating_add(1);
                if st.paging.start == 0 {
                    st.msgs.insert(0, m.clone());
                    if let Some((c, asc)) = st.sort {
                        model::sort_page(&mut st.msgs, c, asc);
                    }
                    let per = st.paging.per_page as usize;
                    st.msgs.truncate(per);
                }
            }
        }
        self.keep_selection();
        self.fill(sent);
    }

    fn command(&self, c: Cmd) {
        match c {
            Cmd::QueueChanged => self.queue_changed(),
            Cmd::Refresh => {
                self.keep_selection();
                self.list();
            }
            Cmd::Previous | Cmd::Next => {
                let moved = {
                    let mut st = self.st.borrow_mut();
                    if c == Cmd::Next {
                        st.paging.next_page()
                    } else {
                        st.paging.previous_page()
                    }
                };
                if moved {
                    self.keep.borrow_mut().clear();
                    self.w.list.table().clone().set_row_position(0);
                    self.list();
                }
            }
            Cmd::Sort(col) => {
                self.keep_selection();
                {
                    let mut st = self.st.borrow_mut();
                    let asc = match st.sort {
                        Some((c, asc)) if c == col => !asc,
                        _ => false,
                    };
                    st.sort = Some((col, asc));
                    model::sort_page(&mut st.msgs, col, asc);
                }
                self.fill(&crate::splf::model::local_time);
            }
            Cmd::Selected => self.show_detail(&crate::splf::model::local_time),
            Cmd::Details => {
                if let Some(m) = self.current() {
                    details(self.parent(), &m);
                }
            }
            Cmd::Reply => self.reply(),
            Cmd::Remove => self.remove_selected(),
            Cmd::RemoveAnswered => self.remove_answered(),
            Cmd::Link => self.follow_link(),
            Cmd::Send => {
                let (state, system) = {
                    let st = self.st.borrow();
                    (st.state.clone(), st.system.name.clone())
                };
                if let Some(state) = state {
                    send::open(self.parent(), &state, &system, None);
                }
            }
            Cmd::Options => {
                let (state, system) = {
                    let st = self.st.borrow();
                    (st.state.clone(), st.system.name.clone())
                };
                if let Some(state) = state {
                    options::open(self.parent(), &state, &system);
                }
            }
            Cmd::Close => self.w.window.clone().hide(),
            Cmd::About => crate::about::show(self.parent()),
        }
    }

    fn queue_changed(&self) {
        let at = usize::try_from(self.w.queue.value()).unwrap_or(0);
        let n = self.st.borrow().queues.len();
        let q = if at < n {
            self.st.borrow().queues[at].clone()
        } else {
            let typed = InputBox::new("Other Queue", "Message queue (library/name):")
                .value("")
                .run(self.parent());
            match typed.as_deref().and_then(model::named_queue) {
                Some(q) => {
                    let mut st = self.st.borrow_mut();
                    if !st.queues.contains(&q) {
                        st.queues.push(q.clone());
                    }
                    q
                }
                None => {
                    if typed.is_some_and(|t| !t.trim().is_empty()) {
                        self.report("Type a message queue as LIBRARY/NAME.");
                    }
                    self.fill_selector();
                    return;
                }
            }
        };
        {
            let mut st = self.st.borrow_mut();
            st.queue = q;
            st.paging.start = 0;
            st.sort = None;
            st.msgs.clear();
        }
        self.keep.borrow_mut().clear();
        self.fill_selector();
        self.fill(&crate::splf::model::local_time);
        self.list();
    }

    fn handle_now(&self) -> Option<Handle> {
        self.st.borrow().handle.clone()
    }

    fn failed(&self, e: &Error) {
        let x = qap::explain(&self.st.borrow().system, e);
        self.report(&x.line());
        qap::show(self.parent(), &x);
    }

    fn reply(&self) {
        let Some(m) = self.current().filter(needs_reply) else {
            return;
        };
        let Some(h) = self.handle_now() else { return };
        let text = self.w.reply.value().trim().to_owned();
        let queue = self.queue();
        let key = m.key;
        let t = text.clone();
        match qap::busy(|| lock(&h).reply_message(&queue, key, &t)) {
            Ok(()) => {
                let what = if m.id.is_empty() {
                    format!("the inquiry from {}", m.from_user.trim())
                } else {
                    m.id.clone()
                };
                self.report(&qsa_msg::message(id::REPLY_SENT, &[&what]).to_string());
                {
                    let mut st = self.st.borrow_mut();
                    if let Some(x) = st.msgs.iter_mut().find(|x| x.key == key) {
                        x.needs_reply = false;
                        x.reply = Some(if text.is_empty() {
                            "(default)".into()
                        } else {
                            text
                        });
                    }
                }
                self.w.reply.clone().set_value("");
                self.keep_selection();
                self.fill(&crate::splf::model::local_time);
                self.list();
            }
            Err(e) => self.failed(&e),
        }
    }

    fn remove_selected(&self) {
        let msgs = self.selected();
        if msgs.is_empty() {
            return;
        }
        let waiting = msgs.iter().filter(|m| !model::removable(m)).count();
        let q = self.queue_name();
        let asked = if waiting > 0 {
            ask_remove_waiting(self.parent(), waiting, msgs.len())
        } else {
            ask(
                self.parent(),
                "Remove",
                &format!("Remove {} from {q}?", count(msgs.len())),
            )
        };
        if asked {
            let keys: Vec<Key> = msgs.iter().map(|m| m.key).collect();
            self.remove(keys);
        }
    }

    fn remove_answered(&self) {
        let Some(h) = self.handle_now() else { return };
        let queue = self.queue();
        let found = qap::busy(|| whole_queue(&h, &queue));
        let all = match found {
            Ok(all) => all,
            Err(e) => return self.failed(&e),
        };
        let keys: Vec<Key> = all
            .iter()
            .filter(|m| model::removable(m))
            .map(|m| m.key)
            .collect();
        let q = self.queue_name();
        if keys.is_empty() {
            self.report(&format!("No answered or informational messages on {q}."));
            return;
        }
        let stay = all.len() - keys.len();
        let mut text = format!(
            "Remove {} answered and informational from {q}?",
            count(keys.len())
        );
        if stay > 0 {
            text.push_str(&format!(
                " {} waiting for a reply {} on the queue.",
                count(stay),
                if stay == 1 { "stays" } else { "stay" }
            ));
        }
        if ask(
            self.parent(),
            "Remove All Answered and Informational",
            &text,
        ) {
            self.remove(keys);
        }
    }

    /// Removes `keys` from the queue, in batches, all or none of each.
    fn remove(&self, keys: Vec<Key>) {
        let Some(h) = self.handle_now() else { return };
        let queue = self.queue();
        let n = keys.len();
        let r = qap::busy(|| {
            for batch in keys.chunks(REMOVE_BATCH) {
                lock(&h).remove_messages(&queue, batch)?;
            }
            Ok::<(), Error>(())
        });
        match r {
            Ok(()) => {
                let q = self.queue_name();
                self.report(
                    &qsa_msg::message(id::MESSAGES_REMOVED, &[&n.to_string(), &q]).to_string(),
                );
                self.keep.borrow_mut().clear();
            }
            Err(e) => self.failed(&e),
        }
        self.list();
    }

    fn follow_link(&self) {
        let Some(link) = self.current().as_ref().and_then(Link::of) else {
            return;
        };
        let at = self.st.borrow().msgs.iter().position(|m| link.leads_to(m));
        if let Some(i) = at {
            let mut list = self.w.list.clone();
            list.select_rows(&[i]);
            list.table().clone().set_row_position(i as i32);
            self.show_detail(&crate::splf::model::local_time);
            return;
        }
        let Some(h) = self.handle_now() else { return };
        let queue = self.queue();
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let r = whole_queue(&h, &queue)
                .map(|all| all.iter().position(|m| link.leads_to(m)).map(|i| i as u32));
            let _ = tx.send(r);
            app::awake();
        });
        self.st.borrow_mut().finding = Some((link, rx));
        self.report("Looking for the message on the queue...");
    }
}

/// "1 message", "3 messages".
fn count(n: usize) -> String {
    if n == 1 {
        "1 message".into()
    } else {
        format!("{n} messages")
    }
}

fn ask(parent: Option<&dyn WindowExt>, title: &str, text: &str) -> bool {
    MessageBox::new(title, text)
        .buttons(&["Remove", "Cancel"], 1, 1)
        .run(parent)
        == 0
}

/// The confirmation of its own for removing inquiries not answered yet.
pub fn remove_waiting_box(waiting: usize, of: usize) -> MessageBox {
    let what = if waiting == of {
        if waiting == 1 {
            "The message is an inquiry".to_owned()
        } else {
            format!("All {waiting} messages are inquiries")
        }
    } else {
        format!("{waiting} of the {of} messages are inquiries")
    };
    MessageBox::new(
        "Remove Unanswered Inquiries",
        &format!(
            "{what} not answered yet. Their senders get no reply: a program waiting for one \
             goes on waiting until it ends some other way. Remove them anyway?"
        ),
    )
    .buttons(&["Remove", "Cancel"], 1, 1)
}

fn ask_remove_waiting(parent: Option<&dyn WindowExt>, waiting: usize, of: usize) -> bool {
    remove_waiting_box(waiting, of).run(parent) == 0
}

/// A message's Details, as the system sent it.
pub fn details(parent: Option<&dyn WindowExt>, m: &QueuedMessage) {
    let id = if m.id.is_empty() {
        model::type_word(m.kind)
    } else {
        m.id.as_str()
    };
    msgdetails::show_system_message(
        parent,
        None,
        &SystemMessage {
            id,
            severity: m.severity,
            text: &m.text,
            help: &m.help,
        },
    );
}

/// Every message of `queue`, newest first, holding the client only to
/// send each page.
pub fn whole_queue(h: &Handle, queue: &Queue) -> Result<Vec<QueuedMessage>, Error> {
    let mut all = Vec::new();
    loop {
        let req = ListMessages {
            queue: queue.clone(),
            newest_first: true,
            start: u32::try_from(all.len()).unwrap_or(u32::MAX),
            max: WHOLE_PAGE,
        };
        let page: Messages = qap::call_unlocked(h, Op::ListMessages, &req)?;
        let n = page.messages.len();
        all.extend(page.messages);
        if n == 0 || all.len() >= page.total as usize {
            return Ok(all);
        }
    }
}

/// Opens Messages for `system`, signing on first when this program has no
/// session on it. An open window for the system comes to the front.
pub fn open(parent: Option<&dyn WindowExt>, state: &Shared, system: &str) {
    let existing = WINDOWS.with(|v| {
        v.borrow()
            .iter()
            .find(|w| w.w.window.shown() && w.system().eq_ignore_ascii_case(system))
            .cloned()
    });
    if let Some(w) = existing {
        w.w.window.clone().show();
        return;
    }
    let Some(sys) = state.borrow().systems.get(system).cloned() else {
        return;
    };
    let Some(h) = qap::session(parent, state, &sys.name) else {
        return;
    };
    let unsupported = h.try_lock().ok().and_then(|mut c| {
        let has = c.version().unwrap_or(1);
        c.supports(Op::ListMessages).err().map(|_| has)
    });
    if let Some(has) = unsupported {
        let m = qap::version_needed(FUNCTION, &sys.name, Op::ListMessages, has);
        crate::log!("{m}");
        msgdetails::show_message(parent, &m);
        return;
    }
    let operator = match qap::busy(|| lock(&h).may_read_queue(&Queue::SystemOperator)) {
        Ok(may) => may,
        Err(e) => {
            crate::log!("QSYSOPR on {}: {e}", sys.name);
            false
        }
    };
    let profile = qap::signed_on(state, &sys.name).unwrap_or_default();
    let w = MessagesWindow::build(&sys, &profile, operator);
    {
        let mut st = w.st.borrow_mut();
        st.state = Some(state.clone());
        st.handle = Some(h);
    }
    let mut win = w.w.window.clone();
    qsa_ui::propdialog::center_on(&mut win, parent);
    qsa_ui::propdialog::show_placed(&mut win);
    w.w.status
        .clone()
        .set_status(&format!("{}: {profile}", sys.name), Status::Active);
    w.list();
    WINDOWS.with(|v| v.borrow_mut().push(w));
    tick();
}

/// What `system`'s watches reported, for its Messages windows and Send
/// Message dialogs.
pub fn watch_events(system: &str, events: &[WatchEvent]) {
    send::watch_events(system, events);
    let windows: Vec<Rc<MessagesWindow>> = WINDOWS.with(|v| {
        v.borrow()
            .iter()
            .filter(|w| w.w.window.shown() && w.system().eq_ignore_ascii_case(system))
            .cloned()
            .collect()
    });
    for w in windows {
        for e in events {
            if let WatchEvent::Arrived { queue, message, .. } = e {
                w.arrived(queue, message, &crate::splf::model::local_time);
            }
        }
    }
}

fn tick() {
    if TICKING.with(|t| t.replace(true)) {
        return;
    }
    app::add_timeout3(POLL, |h| {
        pump();
        if any_open() {
            app::repeat_timeout3(POLL, h);
        } else {
            TICKING.with(|t| t.set(false));
        }
    });
}

/// Gives every Messages window its waiting work and forgets closed ones.
pub fn pump() {
    let windows: Vec<Rc<MessagesWindow>> = WINDOWS.with(|v| v.borrow().clone());
    let open: Vec<bool> = windows.iter().map(|w| w.pump()).collect();
    send::pump();
    arrived::pump();
    WINDOWS.with(|v| {
        let mut i = 0;
        v.borrow_mut().retain(|_| {
            let keep = open.get(i).copied().unwrap_or(true);
            i += 1;
            keep
        });
    });
}

/// Whether any Messages window, Send Message dialog or Message Arrived is
/// open.
pub fn any_open() -> bool {
    WINDOWS.with(|v| v.borrow().iter().any(|w| w.w.window.shown()))
        || send::any_open()
        || arrived::any_open()
}
