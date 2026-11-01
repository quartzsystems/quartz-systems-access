// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! Square tabs over a page panel.
//!
//! FLTK's own tabs slant their edges; these are drawn square and flat, with
//! the active tab two pixels taller and joined to the panel below it.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use fltk::app;
use fltk::draw;
use fltk::enums::{Align, Event, FrameType, Key, Shortcut};
use fltk::group::Group;
use fltk::prelude::*;
use fltk::widget::Widget;

use crate::theme::{self, colors};

/// A rectangle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

struct Pages {
    titles: Vec<String>,
    groups: Vec<Group>,
}

/// A tabbed set of pages.
#[derive(Clone)]
pub struct Tabs {
    group: Group,
    strip: Widget,
    pages: Rc<RefCell<Pages>>,
    current: Rc<Cell<usize>>,
}

impl Tabs {
    /// Height of the tab strip.
    pub const STRIP_H: i32 = 21;
    /// Padding between the panel edge and page content.
    pub const PAGE_PAD: i32 = 10;
    const TAB_PAD: i32 = 6;

    /// A tab set at the given place. Add pages with [`Tabs::add_page`].
    pub fn new(x: i32, y: i32, w: i32, h: i32) -> Tabs {
        let mut group = Group::new(x, y, w, h, None);
        group.set_frame(FrameType::NoBox);
        let panel_y = y + Self::STRIP_H - 1;
        let mut panel = Widget::new(x, panel_y, w, h - (panel_y - y), None);
        panel.draw(|p| {
            draw::draw_rect_fill(p.x() + 1, p.y() + 1, p.w() - 2, p.h() - 2, colors::WINDOW);
            theme::draw_panel(p.x(), p.y(), p.w(), p.h());
        });
        let mut strip = Widget::new(x, y, w, Self::STRIP_H, None);
        group.end();
        group.resizable(&panel);

        let pages = Rc::new(RefCell::new(Pages {
            titles: Vec::new(),
            groups: Vec::new(),
        }));
        let current = Rc::new(Cell::new(0usize));

        let (p, c) = (pages.clone(), current.clone());
        strip.draw(move |s| draw_strip(s, &p.borrow().titles, c.get()));

        let (p, c) = (pages.clone(), current.clone());
        let mut g = group.clone();
        strip.handle(move |s, ev| match ev {
            Event::Push => {
                let titles = p.borrow().titles.clone();
                if let Some(i) = tab_at(s, &titles, app::event_x(), app::event_y()) {
                    select(&p, &c, &mut g, i);
                }
                true
            }
            Event::Shortcut => {
                let ctrl = app::event_state().contains(Shortcut::Ctrl);
                let n = p.borrow().titles.len();
                if !ctrl || n == 0 {
                    return false;
                }
                let i = match app::event_key() {
                    Key::PageDown => (c.get() + 1) % n,
                    Key::PageUp => (c.get() + n - 1) % n,
                    _ => return false,
                };
                select(&p, &c, &mut g, i);
                true
            }
            _ => false,
        });

        Tabs {
            group,
            strip,
            pages,
            current,
        }
    }

    fn page_rect(&self) -> Rect {
        let g = &self.group;
        let top = g.y() + Self::STRIP_H;
        Rect {
            x: g.x() + 1,
            y: top,
            w: g.w() - 2,
            h: g.h() - Self::STRIP_H - 1,
        }
    }

    /// Adds a page. `build` is called with the content area (inside the page
    /// padding) while the page is the current group.
    pub fn add_page(&mut self, title: &str, build: impl FnOnce(Rect)) -> Group {
        let r = self.page_rect();
        // Remember the current group: ending the tab group would otherwise
        // leave its window current, and the next window made would nest in it.
        let previous = Group::try_current();
        self.group.begin();
        let mut page = Group::new(r.x, r.y, r.w, r.h, None);
        page.set_frame(FrameType::FlatBox);
        page.set_color(colors::WINDOW);
        let pad = Self::PAGE_PAD;
        build(Rect {
            x: r.x + pad,
            y: r.y + pad,
            w: r.w - 2 * pad,
            h: r.h - 2 * pad,
        });
        page.end();
        self.group.end();
        Group::set_current(previous.as_ref());
        // Keep the strip last so it draws over the panel's top edge.
        self.group.add(&self.strip);
        let mut pages = self.pages.borrow_mut();
        if !pages.groups.is_empty() {
            page.hide();
        }
        pages.titles.push(title.to_owned());
        pages.groups.push(page.clone());
        page
    }

    /// Shows page `i`.
    pub fn select(&mut self, i: usize) {
        select(&self.pages, &self.current, &mut self.group, i);
    }

    /// Index of the page shown.
    pub fn current(&self) -> usize {
        self.current.get()
    }

    /// Number of pages.
    pub fn len(&self) -> usize {
        self.pages.borrow().titles.len()
    }

    /// Whether there are no pages.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The underlying group.
    pub fn group(&self) -> &Group {
        &self.group
    }
}

fn select(pages: &Rc<RefCell<Pages>>, current: &Rc<Cell<usize>>, group: &mut Group, i: usize) {
    let pages = pages.borrow();
    if i >= pages.groups.len() {
        return;
    }
    current.set(i);
    for (n, g) in pages.groups.iter().enumerate() {
        let mut g = g.clone();
        if n == i {
            g.show();
        } else {
            g.hide();
        }
    }
    group.redraw();
}

fn tab_widths(titles: &[String]) -> Vec<i32> {
    draw::set_font(theme::UI_FONT, theme::UI_SIZE);
    titles
        .iter()
        .map(|t| draw::width(t) as i32 + 2 * Tabs::TAB_PAD + 2)
        .collect()
}

fn tab_at(s: &Widget, titles: &[String], ex: i32, ey: i32) -> Option<usize> {
    if ey < s.y() || ey >= s.y() + s.h() {
        return None;
    }
    let mut x = s.x() + 2;
    for (i, w) in tab_widths(titles).into_iter().enumerate() {
        if ex >= x && ex < x + w {
            return Some(i);
        }
        x += w;
    }
    None
}

fn draw_strip(s: &Widget, titles: &[String], current: usize) {
    let (x, y, w) = (s.x(), s.y(), s.w());
    let base = y + Tabs::STRIP_H - 1;
    draw::draw_rect_fill(x, y, w, Tabs::STRIP_H - 1, colors::FACE);
    // The panel's top edge, redrawn here so a damaged strip stays whole.
    draw::draw_rect_fill(x, base, w, 1, colors::BORDER_LIGHT);

    let widths = tab_widths(titles);
    let mut tx = x + 2;
    let mut active = None;
    for (i, tw) in widths.iter().copied().enumerate() {
        if i == current {
            active = Some((tx, tw));
        } else {
            draw_tab(tx, y + 2, tw, base - (y + 2), &titles[i], false);
        }
        tx += tw;
    }
    if let Some((tx, tw)) = active {
        // Two pixels wider and taller, and open at the bottom into the panel.
        draw_tab(tx - 2, y, tw + 4, base - y + 1, &titles[current], true);
    }
}

fn draw_tab(x: i32, y: i32, w: i32, h: i32, title: &str, active: bool) {
    let body = if active { colors::WINDOW } else { colors::FACE };
    draw::draw_rect_fill(x, y, w, h, body);
    draw::draw_rect_fill(x, y, w, 1, colors::BORDER_LIGHT);
    draw::draw_rect_fill(x, y + 1, 1, h - 1, colors::BORDER_LIGHT);
    draw::draw_rect_fill(x + w - 1, y + 1, 1, h - 1, colors::BORDER_LIGHT);
    draw::set_font(theme::UI_FONT, theme::UI_SIZE);
    draw::set_draw_color(colors::TEXT);
    draw::draw_text2(title, x, y, w, h - 1, Align::Center);
}
