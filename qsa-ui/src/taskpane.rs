// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! The task pane: on a white, scrollable pane, free rows (such as the system
//! selector), dividing rules, and collapsible groups of task links. Each
//! group is a bordered box with a tinted header band; links are compact rows
//! with a small bullet, highlighted in blue under the pointer or focus.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use fltk::app;
use fltk::draw;
use fltk::enums::{Align, Cursor, Event, FrameType, Key};
use fltk::group::{Scroll, ScrollType};
use fltk::image::RgbImage;
use fltk::prelude::*;
use fltk::widget::Widget;

use crate::glyph::{self, Glyph, Size};
use crate::theme::{self, colors};

enum Item {
    Row { widgets: Vec<(Widget, i32)>, h: i32 },
    Rule(Widget),
    Group(GroupItem),
}

struct GroupItem {
    frame: Widget,
    header: Widget,
    links: Vec<Widget>,
    collapsed: Rc<Cell<bool>>,
}

struct Layout {
    scroll: Scroll,
    items: Vec<Item>,
}

/// The task pane.
pub struct TaskPane {
    layout: Rc<RefCell<Layout>>,
}

impl TaskPane {
    /// Height of a group header band.
    pub const HEADER_H: i32 = 19;
    /// Height of a link row.
    pub const LINK_H: i32 = 16;
    /// Gap between items.
    pub const GAP: i32 = 8;
    /// Padding inside the pane.
    pub const PAD: i32 = 8;

    /// A task pane at the given place. Add rows, rules, groups and links,
    /// then call [`TaskPane::end`].
    pub fn new(x: i32, y: i32, w: i32, h: i32) -> TaskPane {
        let mut scroll = Scroll::new(x, y, w, h, None);
        scroll.set_frame(FrameType::DownBox);
        scroll.set_color(colors::WINDOW);
        scroll.set_type(ScrollType::Vertical);
        TaskPane {
            layout: Rc::new(RefCell::new(Layout {
                scroll,
                items: Vec::new(),
            })),
        }
    }

    /// Left edge and width of item content.
    pub fn content_x_w(&self) -> (i32, i32) {
        content_x_w(&self.layout.borrow().scroll)
    }

    /// Adds a free row of height `h`. `build` gets the row's x, y and width,
    /// makes the widgets, and returns them so the pane can move them when a
    /// group above is collapsed.
    pub fn add_row(&mut self, h: i32, build: impl FnOnce(i32, i32, i32) -> Vec<Widget>) {
        let (x, w) = self.content_x_w();
        let y = self.next_y();
        let widgets = build(x, y, w)
            .into_iter()
            .map(|wd| {
                let dy = wd.y() - y;
                (wd, dy)
            })
            .collect();
        self.layout
            .borrow_mut()
            .items
            .push(Item::Row { widgets, h });
    }

    /// Adds a rule across the pane, dividing one section from the next.
    pub fn add_rule(&mut self) {
        let y = self.next_y();
        let (sx, sw) = {
            let l = self.layout.borrow();
            (l.scroll.x(), l.scroll.w())
        };
        let mut rule = Widget::new(sx + 1, y, sw - 2, 2, None);
        rule.draw(|r| draw::draw_rect_fill(r.x(), r.y(), r.w(), r.h(), colors::TASK_RULE));
        self.layout.borrow_mut().items.push(Item::Rule(rule));
    }

    /// Starts a group with the given title. Links added next belong to it.
    pub fn add_group(&mut self, title: &str) {
        let (x, w) = self.content_x_w();
        let y = self.next_y();
        let mut frame = Widget::new(x, y, w, Self::HEADER_H, None);
        frame.draw(|f| {
            draw::draw_rect_fill(f.x(), f.y(), f.w(), f.h(), colors::WINDOW);
            theme::draw_panel(f.x(), f.y(), f.w(), f.h());
        });
        let mut header = Widget::new(x, y, w, Self::HEADER_H, None);
        header.set_label(title);
        let collapsed = Rc::new(Cell::new(false));
        let c = collapsed.clone();
        header.draw(move |h| draw_header(h, c.get()));
        let c = collapsed.clone();
        let weak = Rc::downgrade(&self.layout);
        header.handle(move |h, ev| {
            let toggle = match ev {
                Event::Push => {
                    let _ = h.take_focus();
                    true
                }
                Event::KeyDown => {
                    matches!(app::event_key(), Key::Enter | Key::KPEnter)
                        || app::event_text() == " "
                }
                Event::Focus | Event::Unfocus => {
                    h.redraw();
                    return true;
                }
                _ => false,
            };
            if toggle {
                c.set(!c.get());
                relayout_weak(&weak);
            }
            toggle
        });
        self.layout.borrow_mut().items.push(Item::Group(GroupItem {
            frame,
            header,
            links: Vec::new(),
            collapsed,
        }));
    }

    /// Adds a link to the current group. `cb` runs on click, Enter or Space.
    /// With no glyph, the link has a small square bullet.
    pub fn add_link(
        &mut self,
        glyph: Option<Glyph>,
        text: &str,
        mut cb: impl FnMut() + 'static,
    ) -> Widget {
        let (x, w) = self.content_x_w();
        let mut link = Widget::new(x + 1, 0, w - 2, Self::LINK_H, None);
        link.set_label(text);
        let hover = Rc::new(Cell::new(false));
        let image = RefCell::new(glyph.map(|g| glyph::image(g, Size::Small)));
        let h = hover.clone();
        link.draw(move |l| draw_link(l, h.get(), &image));
        let pressed = Rc::new(Cell::new(false));
        link.handle(move |l, ev| match ev {
            Event::Enter => {
                hover.set(true);
                set_cursor(l, Cursor::Hand);
                l.redraw();
                true
            }
            Event::Leave => {
                hover.set(false);
                pressed.set(false);
                set_cursor(l, Cursor::Default);
                l.redraw();
                true
            }
            Event::Push => {
                pressed.set(true);
                let _ = l.take_focus();
                true
            }
            Event::Released => {
                if pressed.replace(false) && hover.get() {
                    cb();
                }
                true
            }
            Event::Focus | Event::Unfocus => {
                l.redraw();
                true
            }
            Event::KeyDown
                if matches!(app::event_key(), Key::Enter | Key::KPEnter)
                    || app::event_text() == " " =>
            {
                cb();
                true
            }
            _ => false,
        });
        if let Some(Item::Group(g)) = self.layout.borrow_mut().items.last_mut() {
            g.links.push(link.clone());
        }
        self.relayout();
        link
    }

    /// Finishes the pane.
    pub fn end(&mut self) {
        self.layout.borrow_mut().scroll.end();
        self.relayout();
    }

    /// Lays the items out again from the top.
    pub fn relayout(&self) {
        relayout(&mut self.layout.borrow_mut());
    }

    /// The underlying scroll group.
    pub fn scroll(&self) -> Scroll {
        self.layout.borrow().scroll.clone()
    }

    fn next_y(&self) -> i32 {
        let l = self.layout.borrow();
        let s = &l.scroll;
        let mut y = s.y() + 1 + Self::PAD - s.yposition();
        for item in &l.items {
            y += item_height(item) + Self::GAP;
        }
        y
    }
}

fn content_x_w(s: &Scroll) -> (i32, i32) {
    (s.x() + 1 + TaskPane::PAD, s.w() - 2 - 2 * TaskPane::PAD)
}

fn item_height(item: &Item) -> i32 {
    match item {
        Item::Row { h, .. } => *h,
        Item::Rule(_) => 2,
        Item::Group(g) => {
            let mut h = TaskPane::HEADER_H;
            if !g.collapsed.get() && !g.links.is_empty() {
                h += 3 + g.links.len() as i32 * TaskPane::LINK_H + 3;
            }
            h
        }
    }
}

fn relayout_weak(weak: &Weak<RefCell<Layout>>) {
    if let Some(l) = weak.upgrade() {
        relayout(&mut l.borrow_mut());
    }
}

fn relayout(l: &mut Layout) {
    let (x, w) = content_x_w(&l.scroll);
    let s = l.scroll.clone();
    let mut y = s.y() + 1 + TaskPane::PAD - s.yposition();
    for item in &mut l.items {
        let h = item_height(item);
        match item {
            Item::Row { widgets, .. } => {
                for (wd, dy) in widgets {
                    let (wx, ww, wh) = (wd.x(), wd.w(), wd.h());
                    wd.resize(wx, y + *dy, ww, wh);
                }
            }
            Item::Rule(r) => r.resize(s.x() + 1, y, s.w() - 2, 2),
            Item::Group(g) => {
                g.frame.resize(x, y, w, h);
                g.header.resize(x, y, w, TaskPane::HEADER_H);
                let mut ly = y + TaskPane::HEADER_H + 3;
                for link in &mut g.links {
                    if g.collapsed.get() {
                        link.hide();
                    } else {
                        link.resize(x + 1, ly, w - 2, TaskPane::LINK_H);
                        link.show();
                        ly += TaskPane::LINK_H;
                    }
                }
            }
        }
        y += h + TaskPane::GAP;
    }
    l.scroll.redraw();
}

fn has_focus(w: &Widget) -> bool {
    app::focus().is_some_and(|f| f.as_widget_ptr() == w.as_widget_ptr())
}

fn draw_header(h: &Widget, collapsed: bool) {
    let (x, y, w, hh) = (h.x(), h.y(), h.w(), h.h());
    draw::draw_rect_fill(x, y, w, hh, colors::TASK_HEADER);
    theme::draw_panel(x, y, w, hh);
    // The collapse box: minus when open, plus when collapsed.
    let (bx, by) = (x + 5, y + (hh - 9) / 2);
    draw::draw_rect_fill(bx, by, 9, 9, colors::BORDER);
    draw::draw_rect_fill(bx + 1, by + 1, 7, 7, colors::WINDOW);
    draw::draw_rect_fill(bx + 2, by + 4, 5, 1, colors::TEXT);
    if collapsed {
        draw::draw_rect_fill(bx + 4, by + 2, 1, 5, colors::TEXT);
    }
    draw::set_font(theme::UI_FONT_BOLD, theme::UI_SIZE);
    draw::set_draw_color(colors::TEXT);
    let label = h.label();
    draw::draw_text2(&label, x + 20, y, w - 24, hh, Align::Left);
    if has_focus(h) {
        let tw = draw::width(&label) as i32;
        draw::draw_focus_rect(x + 18, y + 2, tw + 4, hh - 4);
    }
}

fn draw_link(l: &Widget, hover: bool, image: &RefCell<Option<RgbImage>>) {
    let (x, y, w, h) = (l.x(), l.y(), l.w(), l.h());
    draw::draw_rect_fill(x, y, w, h, colors::WINDOW);
    let mut tx = x + 24;
    if let Some(img) = image.borrow_mut().as_mut() {
        img.draw(x + 16, y + (h - 16) / 2, 16, 16);
        tx = x + 16 + 16 + 4;
    } else {
        draw::draw_rect_fill(x + 15, y + (h - 4) / 2, 4, 4, colors::BULLET);
    }
    draw::set_font(theme::UI_FONT, theme::UI_SIZE);
    let label = l.label();
    let tw = draw::width(&label) as i32;
    let focused = has_focus(l);
    if hover || focused {
        draw::draw_rect_fill(tx - 2, y, tw + 4, h, colors::HOVER);
        draw::set_draw_color(colors::HOVER_TEXT);
    } else {
        draw::set_draw_color(colors::LINK);
    }
    draw::draw_text2(&label, tx, y, w - (tx - x), h, Align::Left);
    if focused {
        draw::set_draw_color(colors::HOVER_TEXT);
        draw::draw_focus_rect(tx - 2, y, tw + 4, h);
    }
}

fn set_cursor(w: &Widget, cursor: Cursor) {
    if let Some(mut win) = w.window() {
        win.set_cursor(cursor);
    }
}
