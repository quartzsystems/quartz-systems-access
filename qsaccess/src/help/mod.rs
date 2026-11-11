// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! Help: the topics, and the window that shows them.
//!
//! - The topics are the Markdown files in `help/`, made into HTML and plain
//!   text when the program is built (`build/help.rs`) and kept in the
//!   binary. Nothing is read from disk, no browser is opened and no
//!   network is used.
//! - [`show`] opens a topic: **Help > Help Topics**, F1 in a window
//!   ([`attach`]) and a dialog's Help button all come here. Code names a
//!   topic by its constant in [`topic`], which the build makes from the
//!   topics there are, so a topic that does not exist cannot be asked for.
//! - The window has Contents (the topics as a tree) and Search (plain
//!   substring match over titles and text) on the left, the topic in
//!   FLTK's help view on the right, and Back, Forward, Home and Print
//!   above. Print writes the topic as a PDF through `qsa-print`.
//! - While a modal dialog is open, Help opens as a modal window over it
//!   (a window that is not modal would not take the keyboard or the mouse
//!   until the dialog closed).

pub mod print;

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::OnceLock;

use fltk::app;
use fltk::enums::{Align, CallbackTrigger, Event, FrameType, Key, Shortcut};
use fltk::frame::Frame;
use fltk::input::Input;
use fltk::misc::HelpView;
use fltk::prelude::*;
use fltk::tree::{Tree, TreeReason, TreeSelect};
use fltk::window::Window;
use qsa_ui::controls::{self, Switch};
use qsa_ui::listtable::{Cell as ListCell, ListTable};
use qsa_ui::propdialog::{center_on, show_placed};
use qsa_ui::statusbar::{Status, StatusLine};
use qsa_ui::theme::{self, colors};
use qsa_ui::{Glyph, MessageBox, Tabs};

use crate::files;

/// One topic.
#[derive(Debug)]
pub struct Topic {
    /// Its name: the Markdown file's, without `.md`.
    pub slug: &'static str,
    pub title: &'static str,
    /// The heading Contents lists it under; empty for none.
    pub group: &'static str,
    /// The topic for the help view. `@@` in a link's address stands for
    /// the number [`Viewer`] gives each showing.
    pub html: &'static str,
    /// The topic as plain text, 80 columns wide: a heading's line starts
    /// with [`HEADING`], a line that must not be wrapped again with
    /// [`FIXED`].
    pub text: &'static str,
    /// The anchors a link may go to: its headings', and each message ID's
    /// in the messages reference.
    pub anchors: &'static [&'static str],
}

include!(concat!(env!("OUT_DIR"), "/help.rs"));

/// Starts a heading's line in [`Topic::text`].
pub const HEADING: char = '\u{1}';
/// Starts a line of [`Topic::text`] that is laid out already.
pub const FIXED: char = '\u{2}';

/// The topic Home shows.
pub const HOME: &str = topic::GETTING_STARTED;

/// The path the help view is given for a link to another topic:
/// `/qsa-help/<showing>/<topic>/<anchor or ->/x`.
const LINK_ROOT: &str = "/qsa-help/";

/// The topic named `slug`.
pub fn find(slug: &str) -> Option<&'static Topic> {
    TOPICS.iter().find(|t| t.slug == slug)
}

/// A heading's anchor, and a link's fragment as it is matched: lower case,
/// each run of other characters one hyphen, none at the ends.
pub fn anchor(text: &str) -> String {
    let mut out = String::new();
    for c in text.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    out.trim_matches('-').to_owned()
}

/// Lower case, the layout marks gone and every run of white space one
/// space: how titles, text and what is searched for are compared.
fn folded(text: &str) -> String {
    let plain: String = text
        .chars()
        .filter(|c| *c != HEADING && *c != FIXED)
        .collect();
    plain
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// Each topic's title and text as Search compares them.
fn search_index() -> &'static [(String, String)] {
    static INDEX: OnceLock<Vec<(String, String)>> = OnceLock::new();
    INDEX.get_or_init(|| {
        TOPICS
            .iter()
            .map(|t| (folded(t.title), folded(t.text)))
            .collect()
    })
}

/// The topics whose title or text contains `query` as it is typed (plain
/// substring, case and line breaks ignored): those with it in the title
/// first, each group in Contents order. An empty query finds nothing.
pub fn search(query: &str) -> Vec<&'static Topic> {
    let q = folded(query);
    if q.is_empty() {
        return Vec::new();
    }
    let index = search_index();
    let in_title = |i: &usize| index[*i].0.contains(&q);
    let in_text = |i: &usize| !index[*i].0.contains(&q) && index[*i].1.contains(&q);
    let all = 0..TOPICS.len();
    all.clone()
        .filter(in_title)
        .chain(all.filter(in_text))
        .map(|i| &TOPICS[i])
        .collect()
}

/// Where a link the help view was given leads: the topic and the anchor,
/// when `directory` (what the view says it last loaded from) is a link of
/// showing `showing`.
fn link_target(directory: &str, showing: u32) -> Option<(String, Option<String>)> {
    let tail = directory
        .replace('\\', "/")
        .rsplit_once(&format!("{LINK_ROOT}{showing}/"))?
        .1
        .to_owned();
    let mut parts = tail.split('/');
    let slug = parts.next().filter(|s| !s.is_empty())?.to_owned();
    let anchor = parts.next().filter(|a| !a.is_empty() && *a != "-");
    Some((slug, anchor.map(str::to_owned)))
}

/// A topic's HTML for showing number `showing`.
fn html_for(t: &Topic, showing: u32) -> String {
    t.html.replace("@@", &showing.to_string())
}

/// Window width.
pub const W: i32 = 1000;
/// Window height.
pub const H: i32 = 640;
/// Width of the Contents and Search side.
const SIDE_W: i32 = 340;

/// Tab indexes.
pub const TAB_CONTENTS: usize = 0;
pub const TAB_SEARCH: usize = 1;

/// A place Back or Forward returns to.
#[derive(Clone, Copy)]
struct Place {
    topic: &'static Topic,
    /// How far down the topic was scrolled.
    top: i32,
}

#[derive(Default)]
struct Nav {
    current: Option<&'static Topic>,
    back: Vec<Place>,
    forward: Vec<Place>,
    /// The number of the showing on screen: it is in every link's address,
    /// so a link followed from an earlier showing is told apart.
    showing: u32,
    /// The topics Search lists.
    found: Vec<&'static Topic>,
    /// Contents' rows: the tree's path for each topic.
    paths: Vec<(String, &'static Topic)>,
}

/// The Help window.
pub struct Viewer {
    pub window: Window,
    pub tabs: Tabs,
    tree: Tree,
    query: Input,
    results: ListTable,
    view: HelpView,
    status: StatusLine,
    back: Switch,
    forward: Switch,
    nav: RefCell<Nav>,
    /// Set while Contents is being selected from here, so that its
    /// callback does not show the topic a second time.
    selecting: Cell<bool>,
}

/// A tree path's item name: `/` and `\` would start a new level.
fn tree_name(title: &str) -> String {
    title.replace('\\', "\\\\").replace('/', "\\/")
}

/// Builds the Help window, showing Home. It is not shown.
pub fn build() -> Rc<Viewer> {
    let m = 8;
    let bh = theme::BUTTON_H;
    let bw = theme::BUTTON_W;
    let mut window = Window::new(0, 0, W, H, None);
    window.set_label("Help");
    window.set_icon(Some(qsa_ui::glyph::app_icon()));
    window.set_color(colors::FACE);
    window.size_range(640, 400, 0, 0);

    // Back, Forward, Home and Print.
    let mut x = m;
    let mut tool = |label: &str, tip: &str, gap: i32| {
        let mut b = controls::button(x, m, bw, bh, label);
        b.set_tooltip(tip);
        x += bw + gap;
        b
    };
    let mut back = tool("Back", "The topic shown before this one", 4);
    let mut forward = tool("Forward", "The topic Back was selected in", 4);
    let mut home = tool("Home", "The first topic", 14);
    let mut print = tool("Print...", "Write this topic to a PDF file", 0);
    let mut filler = Frame::new(x, m, W - x - m, bh, None);
    filler.set_frame(FrameType::NoBox);

    let top = m + bh + 8;
    let bottom = H - m - bh - 4;
    let mut tabs = Tabs::new(m, top, SIDE_W, bottom - top);
    let mut tree = None;
    tabs.add_page("Contents", |r| {
        let mut t = Tree::new(r.x - 6, r.y - 6, r.w + 12, r.h + 12, None);
        t.set_frame(FrameType::FlatBox);
        t.set_color(colors::WINDOW);
        t.set_select_mode(TreeSelect::Single);
        t.set_item_label_font(theme::UI_FONT);
        t.set_item_label_size(theme::UI_SIZE);
        t.set_show_root(false);
        tree = Some(t);
    });
    let mut search = None;
    tabs.add_page("Search", |r| {
        let fh = theme::FIELD_H;
        let mut l = controls::label(r.x, r.y, r.w, 18, "Search for:");
        l.set_align(Align::Left | Align::Inside);
        let mut q = controls::field(r.x, r.y + 20, r.w, fh);
        q.set_trigger(CallbackTrigger::Changed);
        q.set_tooltip("Topics whose title or text contains exactly this");
        let ly = r.y + 20 + fh + 8;
        let list = ListTable::new(r.x, ly, r.w, r.y + r.h - ly, &[("Topic", r.w - 4)]);
        search = Some((q, list));
    });
    let tree = tree.expect("the Contents page is built");
    let (query, results) = search.expect("the Search page is built");

    let vx = m + SIDE_W + 6;
    let mut view = HelpView::new(vx, top, W - vx - m, bottom - top, None);
    view.set_frame(FrameType::DownBox);
    view.set_color(colors::WINDOW);
    view.set_text_font(theme::UI_FONT);
    view.set_text_size(theme::UI_SIZE + 1);
    view.set_text_color(colors::TEXT);
    view.set_selection_color(colors::SELECTION);

    let status = StatusLine::new(m, H - m - bh, W - 3 * m - bw, bh);
    let mut close = controls::default_button(W - m - bw, H - m - bh, bw, bh, "Close");
    window.end();
    window.resizable(&view);
    let mut wc = window.clone();
    close.set_callback(move |_| wc.hide());

    let viewer = Rc::new(Viewer {
        window,
        tabs,
        tree,
        query,
        results,
        view,
        status,
        back: Switch::for_button(&mut back),
        forward: Switch::for_button(&mut forward),
        nav: RefCell::new(Nav::default()),
        selecting: Cell::new(false),
    });
    viewer.fill_contents();

    let me = Rc::downgrade(&viewer);
    let on = move |f: fn(&Viewer)| {
        let me = me.clone();
        move || {
            if let Some(v) = me.upgrade() {
                f(&v);
            }
        }
    };
    let f = on(Viewer::go_back);
    back.set_callback(move |_| f());
    let f = on(Viewer::go_forward);
    forward.set_callback(move |_| f());
    let f = on(Viewer::go_home);
    home.set_callback(move |_| f());
    let f = on(Viewer::print);
    print.set_callback(move |_| f());
    let f = on(Viewer::contents_chosen);
    viewer.tree.clone().set_callback(move |_| f());
    let f = on(Viewer::search_now);
    viewer.query.clone().set_callback(move |_| f());
    let f = on(Viewer::result_chosen);
    viewer.results.clone().on_select(move |_| f());
    // A link was clicked if the view, once it has handled the release,
    // says it tried to load one of this showing's addresses.
    let f = on(Viewer::link_followed);
    viewer.view.clone().handle(move |_, ev| {
        if ev == Event::Released {
            let f = f.clone();
            app::add_timeout3(0.0, move |_| f());
        }
        false
    });
    // F1 in Help is Help on Help: Home.
    let f = on(Viewer::go_home);
    viewer.window.clone().handle(move |_, ev| {
        if is_f1(ev) {
            f();
            return true;
        }
        false
    });

    viewer.go(HOME, None);
    viewer
}

impl Viewer {
    /// Fills Contents: a row for each topic, under its group.
    fn fill_contents(&self) {
        let mut tree = self.tree.clone();
        let mut paths = Vec::new();
        for t in TOPICS {
            let path = if t.group.is_empty() {
                tree_name(t.title)
            } else {
                format!("{}/{}", tree_name(t.group), tree_name(t.title))
            };
            tree.add(&path);
            paths.push((path, t));
        }
        self.nav.borrow_mut().paths = paths;
        tree.redraw();
    }

    /// The topic on screen.
    pub fn current(&self) -> Option<&'static Topic> {
        self.nav.borrow().current
    }

    /// The topics Search lists.
    pub fn found(&self) -> Vec<&'static Topic> {
        self.nav.borrow().found.clone()
    }

    /// The text in the status line.
    pub fn status_text(&self) -> String {
        self.status.text()
    }

    /// Whether Back and Forward have somewhere to go.
    pub fn can_go(&self) -> (bool, bool) {
        (self.back.is_enabled(), self.forward.is_enabled())
    }

    /// Shows topic `slug` (at `anchor`, a heading's or a message ID's),
    /// remembering the topic it replaces for Back. An unknown topic is
    /// reported as `QAD0050` and nothing changes.
    pub fn go(&self, slug: &str, anchor: Option<&str>) -> bool {
        let Some(t) = find(slug) else {
            let m = qsa_msg::message(qsa_msg::id::HELP_TOPIC_NOT_FOUND, &[slug]);
            crate::log!("{m}");
            self.status.clone().set(&m.to_string(), Status::Error);
            return false;
        };
        {
            let mut nav = self.nav.borrow_mut();
            if let Some(from) = nav.current
                && !std::ptr::eq(from, t)
            {
                let top = self.view.top_line();
                nav.back.push(Place { topic: from, top });
                nav.forward.clear();
            }
        }
        self.display(t, anchor, 0);
        true
    }

    /// Puts a topic on screen.
    fn display(&self, t: &'static Topic, anchor: Option<&str>, top: i32) {
        let showing = {
            let mut nav = self.nav.borrow_mut();
            nav.current = Some(t);
            nav.showing += 1;
            nav.showing
        };
        let mut view = self.view.clone();
        view.set_value(&html_for(t, showing));
        match anchor.map(self::anchor).filter(|a| !a.is_empty()) {
            Some(a) => view.set_top_line_string(&a),
            None => view.set_top_line(top),
        }
        view.set_left_line(0);
        self.window
            .clone()
            .set_label(&format!("Help - {}", t.title));
        self.status.clone().set("", Status::Neutral);
        self.select_in_contents(t);
        self.update_buttons();
    }

    fn update_buttons(&self) {
        let nav = self.nav.borrow();
        self.back.set_enabled(!nav.back.is_empty());
        self.forward.set_enabled(!nav.forward.is_empty());
    }

    /// Marks the topic on screen in Contents.
    fn select_in_contents(&self, t: &'static Topic) {
        let path = {
            let nav = self.nav.borrow();
            nav.paths
                .iter()
                .find(|(_, p)| std::ptr::eq(*p, t))
                .map(|(path, _)| path.clone())
        };
        let mut tree = self.tree.clone();
        if let Some(item) = path.and_then(|p| tree.find_item(&p)) {
            self.selecting.set(true);
            let _ = tree.select_only(&item, false);
            self.selecting.set(false);
            tree.redraw();
        }
    }

    /// Back: the topic shown before this one, where it was scrolled to.
    pub fn go_back(&self) {
        self.step(true);
    }

    /// Forward: the topic Back was selected in.
    pub fn go_forward(&self) {
        self.step(false);
    }

    /// Back, or Forward.
    fn step(&self, back: bool) {
        let to = {
            let mut nav = self.nav.borrow_mut();
            let here = nav.current.map(|topic| Place {
                topic,
                top: self.view.top_line(),
            });
            let to = if back {
                nav.back.pop()
            } else {
                nav.forward.pop()
            };
            if let (Some(_), Some(here)) = (&to, here) {
                if back {
                    nav.forward.push(here);
                } else {
                    nav.back.push(here);
                }
            }
            to
        };
        if let Some(p) = to {
            self.display(p.topic, None, p.top);
        }
    }

    /// Home: the first topic.
    pub fn go_home(&self) {
        self.go(HOME, None);
    }

    /// The addresses of the links to other topics in the topic on screen,
    /// as the help view has them.
    pub fn link_addresses(&self) -> Vec<String> {
        let html = self.view.value().unwrap_or_default();
        let mut out = Vec::new();
        let mut rest = html.as_str();
        while let Some(i) = rest.find("href=\"") {
            rest = &rest[i + 6..];
            let Some(end) = rest.find('"') else { break };
            if rest[..end].starts_with(LINK_ROOT) {
                out.push(rest[..end].to_owned());
            }
            rest = &rest[end..];
        }
        out
    }

    /// Does what a click on a link with this address does: the help view
    /// tries to load it, and the topic it names is shown (see
    /// `link_followed`). For tests, which cannot click.
    pub fn click_link(&self, address: &str) {
        let _ = self.view.clone().load(address);
        self.link_followed();
    }

    /// A row of Contents was selected.
    fn contents_chosen(&self) {
        if self.selecting.get() || self.tree.callback_reason() != TreeReason::Selected {
            return;
        }
        let Some(item) = self.tree.callback_item() else {
            return;
        };
        let Ok(path) = self.tree.item_pathname(&item) else {
            return;
        };
        let t = {
            let nav = self.nav.borrow();
            nav.paths.iter().find(|(p, _)| *p == path).map(|(_, t)| *t)
        };
        // A group's own row shows nothing.
        if let Some(t) = t {
            self.go(t.slug, None);
        }
    }

    /// Lists the topics that contain what is typed in Search.
    pub fn search_for(&self, text: &str) {
        self.query.clone().set_value(text);
        self.search_now();
    }

    fn search_now(&self) {
        let text = self.query.value();
        let found = search(&text);
        let rows = found
            .iter()
            .map(|t| vec![ListCell::from(t.title)])
            .collect();
        let said = match (text.trim().is_empty(), found.len()) {
            (true, _) => String::new(),
            (false, 0) => format!("No topic contains \"{}\".", text.trim()),
            (false, 1) => "1 topic found.".to_owned(),
            (false, n) => format!("{n} topics found."),
        };
        self.nav.borrow_mut().found = found;
        // Filling the list clears its selection, which must not be taken
        // for a row chosen.
        self.selecting.set(true);
        self.results.clone().set_rows(rows);
        self.selecting.set(false);
        self.status.clone().set(&said, Status::Neutral);
    }

    /// A row of Search's list was selected: its topic, at the first place
    /// the text searched for is.
    fn result_chosen(&self) {
        if self.selecting.get() {
            return;
        }
        let Some(i) = self.results.selected() else {
            return;
        };
        let t = self.nav.borrow().found.get(i).copied();
        if let Some(t) = t
            && self.go(t.slug, None)
        {
            let q = self.query.value();
            if !q.trim().is_empty() {
                let _ = self.view.find(q.trim(), 0);
            }
        }
    }

    /// Follows the link the help view was just clicked on, if it was one.
    ///
    /// FLTK's help view can follow a link to another document only by
    /// loading a file, and this build of it has no hook to give it the
    /// document instead. So a link to a topic is written as a path that
    /// names no file, with the topic in it: the view fails to load it,
    /// remembers the path as where it is, and this reads that and shows
    /// the topic. Nothing is read from disk for it, and an address the
    /// view would hand to a browser is never written (the build refuses
    /// links out of Help).
    fn link_followed(&self) {
        let showing = self.nav.borrow().showing;
        let directory = self.view.directory().to_string_lossy().into_owned();
        if let Some((slug, anchor)) = link_target(&directory, showing) {
            self.follow(&slug, anchor.as_deref());
        }
    }

    /// Goes where a link leads, putting the topic it was in back on screen
    /// if the link's topic does not exist.
    pub fn follow(&self, slug: &str, anchor: Option<&str>) {
        if !self.go(slug, anchor)
            && let Some(t) = self.current()
        {
            let said = self.status.text();
            self.display(t, None, 0);
            self.status.clone().set(&said, Status::Error);
        }
    }

    /// Print: the topic as a PDF file.
    fn print(&self) {
        let Some(t) = self.current() else { return };
        let Some(path) = files::choose_save(
            "Print Help Topic to PDF",
            "PDF files (*.pdf)\tAll Files (*)",
            &files::file_name_for(t.title, "pdf"),
        ) else {
            return;
        };
        let path = files::with_extension(path, "pdf");
        if path.exists()
            && MessageBox::new(
                "Print Help Topic to PDF",
                &format!("{} already exists.\n\nReplace it?", path.display()),
            )
            .buttons(&["Replace", "Cancel"], 1, 1)
            .run(Some(&self.window))
                != 0
        {
            return;
        }
        let written = print::pdf(t)
            .map_err(|e| e.to_string())
            .and_then(|bytes| std::fs::write(&path, bytes).map_err(|e| e.to_string()));
        match written {
            Ok(()) => self.status.clone().set(
                &format!("{} printed to {}.", t.title, path.display()),
                Status::Neutral,
            ),
            Err(e) => {
                let text = format!("{} could not be written: {e}", path.display());
                crate::log!("Help: {text}");
                self.status.clone().set(&text, Status::Error);
                MessageBox::new("Help", &text)
                    .glyph(Glyph::Cross)
                    .run(Some(&self.window));
            }
        }
    }
}

thread_local! {
    static WINDOW: RefCell<Option<Rc<Viewer>>> = const { RefCell::new(None) };
}

/// Whether `ev` is F1 pressed alone.
pub fn is_f1(ev: Event) -> bool {
    matches!(ev, Event::KeyDown | Event::Shortcut)
        && app::event_key() == Key::F1
        && !app::event_state().intersects(Shortcut::Ctrl | Shortcut::Alt | Shortcut::Shift)
}

/// Makes F1 in `window` open `topic` (a constant of [`topic`]). A window
/// with a handler of its own calls [`is_f1`] and [`show`] from it instead.
pub fn attach<W: WidgetBase + WindowExt>(window: &mut W, topic: &'static str) {
    attach_at(window, topic, None);
}

/// [`attach`], at a heading of the topic.
pub fn attach_at<W: WidgetBase + WindowExt>(
    window: &mut W,
    topic: &'static str,
    anchor: Option<&'static str>,
) {
    window.handle(move |_, ev| {
        if is_f1(ev) {
            show_at(topic, anchor);
            return true;
        }
        false
    });
}

/// Gives a property dialog its help: the Help button and F1 open `topic`,
/// at `anchor` when given.
pub fn for_dialog(
    d: &mut qsa_ui::PropertyDialog,
    topic: &'static str,
    anchor: Option<&'static str>,
) {
    d.on_help(move || show_at(topic, anchor));
    attach_at(&mut d.window().clone(), topic, anchor);
}

/// The heading of Getting started that System Configurations and System
/// Properties open at.
pub const SYSTEM_CONFIGURATIONS: &str = "system-configurations";

/// Opens Help at a topic.
pub fn show(topic: &str) {
    show_at(topic, None);
}

/// Opens Help at the messages reference's entry for message `id`.
pub fn show_message(id: &str) {
    show_at(topic::MESSAGES_REFERENCE, Some(id));
}

/// Opens Help at a topic and, when given, a heading's or message's anchor
/// in it. With a modal dialog open, Help is a modal window over it, and
/// this returns when it is closed; otherwise the one Help window is shown
/// and raised.
pub fn show_at(topic: &str, anchor: Option<&str>) {
    if app::modal().is_some() {
        let v = build();
        v.go(topic, anchor);
        let mut w = v.window.clone();
        w.make_modal(true);
        center_on(&mut w, None);
        show_placed(&mut w);
        while w.shown() {
            app::wait();
        }
        return;
    }
    let existing = WINDOW.with(|w| w.borrow().clone());
    let v = match existing {
        Some(v) => v,
        None => {
            let v = build();
            center_on(&mut v.window.clone(), None);
            WINDOW.with(|slot| *slot.borrow_mut() = Some(v.clone()));
            v
        }
    };
    v.go(topic, anchor);
    show_placed(&mut v.window.clone());
}

/// Closes Help if it is open.
pub fn close() {
    if let Some(v) = WINDOW.with(|w| w.borrow_mut().take()) {
        v.window.clone().hide();
    }
}

/// Whether the Help window is open.
pub fn shown() -> bool {
    WINDOW.with(|w| w.borrow().as_ref().is_some_and(|v| v.window.shown()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every link address in a topic's HTML: (topic, anchor).
    fn links(t: &Topic) -> Vec<(String, Option<String>)> {
        let mut out = Vec::new();
        let mut rest = t.html;
        while let Some(i) = rest.find("href=\"") {
            rest = &rest[i + 6..];
            let end = rest.find('"').expect("a closed attribute");
            let href = &rest[..end];
            match href.strip_prefix('#') {
                Some(a) => out.push((t.slug.to_owned(), Some(a.to_owned()))),
                None => {
                    assert!(
                        href.starts_with(&format!("{LINK_ROOT}@@/")) && href.ends_with("/x"),
                        "{}: link {href} does not stay in Help",
                        t.slug
                    );
                    // As the view reports it once followed in showing 7.
                    let dir = href.replace("@@", "7");
                    let dir = dir.strip_suffix("/x").unwrap();
                    out.push(link_target(dir, 7).expect("a topic link"));
                }
            }
        }
        out
    }

    #[test]
    fn every_topic_named_from_code_exists() {
        // The constants are made from the topics there are; these are the
        // ones windows, dialogs and Help buttons open.
        for slug in [
            HOME,
            topic::GETTING_STARTED,
            topic::SIGNING_ON,
            topic::VERIFY_CONNECTION,
            topic::DISPLAY_5250,
            topic::PRINTER_5250,
            topic::SESSION_MANAGER,
            topic::RUN_COMMAND,
            topic::PRINTER_OUTPUT,
            topic::RUN_SQL,
            topic::SCHEMAS,
            topic::DATA_TRANSFER,
            topic::QSACCESS_RUN,
            topic::INTEGRATED_FILE_SYSTEM,
            topic::MESSAGES,
            topic::CERTIFICATE_MANAGEMENT,
            topic::INSTALLING,
            topic::KEYBOARD_REFERENCE,
            topic::MESSAGES_REFERENCE,
        ] {
            let t = find(slug).unwrap_or_else(|| panic!("no topic {slug}"));
            assert!(!t.title.is_empty() && t.html.contains("<h1>"), "{slug}");
        }
        assert_eq!(TOPICS.len(), 18);
        // And the heading System Configurations opens at.
        let start = find(topic::GETTING_STARTED).unwrap();
        assert!(start.anchors.contains(&SYSTEM_CONFIGURATIONS));
        assert!(find("no-such-topic").is_none());
    }

    #[test]
    fn every_internal_link_resolves() {
        let mut count = 0;
        for t in TOPICS {
            for (slug, a) in links(t) {
                let to = find(&slug)
                    .unwrap_or_else(|| panic!("{}: link to missing topic {slug}", t.slug));
                if let Some(a) = a {
                    assert!(
                        to.anchors.contains(&a.as_str()),
                        "{}: link to {slug}#{a}, which {slug} does not have",
                        t.slug
                    );
                }
                count += 1;
            }
        }
        assert!(count > 50, "only {count} links found");
        // No topic sends the view to a browser or to the network: every
        // link address was one of Help's own (checked in `links`), and
        // there is nothing else the view would fetch.
        for t in TOPICS {
            assert!(!t.html.contains("<img"), "{}", t.slug);
        }
    }

    #[test]
    fn every_message_is_in_the_reference_with_its_cause_and_recovery() {
        let r = find(topic::MESSAGES_REFERENCE).unwrap();
        let cat = qsa_msg::Catalogue::builtin();
        let mut n = 0;
        for id in cat.ids() {
            let d = cat.get(id).unwrap();
            assert!(r.anchors.contains(&anchor(id).as_str()), "{id}");
            assert!(
                r.html
                    .contains(&format!("<a name=\"{}\"></a><h2>{id}</h2>", anchor(id)))
            );
            let text = folded(r.text);
            for part in [&d.text, &d.cause, &d.recovery] {
                assert!(text.contains(&folded(part)), "{id}: {part}");
            }
            n += 1;
        }
        assert!(n > 130);
    }

    #[test]
    fn the_keyboard_reference_has_the_default_map() {
        let k = find(topic::KEYBOARD_REFERENCE).unwrap();
        let map = qsa_config::KeyMap::defaults();
        assert!(k.html.contains("<table border=\"1\">"));
        // A table with long rows is held to the view's width.
        let start = find(topic::GETTING_STARTED).unwrap();
        assert!(start.html.contains("<table border=\"1\" width=\"96%\">"));
        for (key, function) in &map.keys {
            let label = qsa_config::keymap::function_label(function);
            assert!(k.text.contains(key.as_str()), "{key}");
            assert!(k.html.contains(&format!(">{label}</td>")), "{label}");
        }
    }

    #[test]
    fn search_finds_a_known_phrase() {
        // A phrase of one topic's text.
        let found = search("client certificates are not used");
        let slugs: Vec<&str> = found.iter().map(|t| t.slug).collect();
        assert_eq!(slugs, [topic::CERTIFICATE_MANAGEMENT]);
        // Case and line breaks do not matter; a title match comes first.
        let found = search("  RUN   sql ");
        assert_eq!(found.first().map(|t| t.slug), Some(topic::RUN_SQL));
        assert!(found.len() > 1, "other topics mention Run SQL");
        // A message's text is found in the reference.
        let found = search("Function not yet available.");
        assert!(found.iter().any(|t| t.slug == topic::MESSAGES_REFERENCE));
        // Plain substring: no patterns, and nothing for nothing.
        assert!(search("run.*sql").is_empty());
        assert!(search("   ").is_empty());
        assert!(search("zzzz no topic says this").is_empty());
    }

    #[test]
    fn a_followed_link_is_read_from_where_the_view_says_it_is() {
        let t = |d: &str| link_target(d, 7);
        assert_eq!(
            t("/qsa-help/7/run-sql/commitment-control"),
            Some(("run-sql".into(), Some("commitment-control".into())))
        );
        assert_eq!(t("/qsa-help/7/run-sql/-"), Some(("run-sql".into(), None)));
        // As Windows may hand it back, and after an earlier link.
        assert_eq!(
            t("\\qsa-help\\6\\schemas\\-/qsa-help/7/messages/qad9001"),
            Some(("messages".into(), Some("qad9001".into())))
        );
        // A link of an earlier showing, and nothing followed yet.
        assert_eq!(t("/qsa-help/6/run-sql/-"), None);
        assert_eq!(t(""), None);
        assert_eq!(t("/qsa-help/7/"), None);
    }

    #[test]
    fn anchors_fold_as_the_build_folds_them() {
        assert_eq!(anchor("Commitment control"), "commitment-control");
        assert_eq!(anchor("QAD0040"), "qad0040");
        assert_eq!(anchor("  `.qdx` requests & runs "), "qdx-requests-runs");
        assert_eq!(folded("\u{1}Run  SQL\n\u{2}  a   b"), "run sql a b");
    }

    #[test]
    fn topics_keep_to_what_the_view_shows() {
        for t in TOPICS {
            assert!(t.html.starts_with("<html><head><title>"), "{}", t.slug);
            assert!(t.html.is_ascii(), "{}: text outside ASCII", t.slug);
            assert!(!t.text.is_empty(), "{}", t.slug);
            // Print's lines fit the page.
            for line in t.text.lines() {
                let n = line.trim_start_matches([HEADING, FIXED]).chars().count();
                assert!(n <= 100, "{}: a {n}-column line: {line}", t.slug);
            }
        }
        assert_eq!(TOPICS[0].slug, HOME);
    }
}
