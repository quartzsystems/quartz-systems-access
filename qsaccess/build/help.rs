// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! Help topics, made when the program is built: each Markdown file in
//! `help/` becomes HTML for FLTK's help view and plain text for Search and
//! Print, and all of them go into the binary as `$OUT_DIR/help.rs`. The
//! program reads no help file from disk.
//!
//! - `help/contents.txt` lists the topics in the order Contents shows them,
//!   under `= Group` lines. A topic not listed, or a listed topic with no
//!   file, fails the build.
//! - A topic links to another as `[text](other-topic.md)` or
//!   `(other-topic.md#heading)`, and within itself as `(#heading)`. A link
//!   to a topic or heading that does not exist fails the build, so no link
//!   in Help can lead nowhere. Links out of Help (`http:` and the like) and
//!   images are refused: Help opens no browser and uses no network.
//! - `{{messages}}`, alone in a paragraph, becomes every `QAD` message with
//!   its Cause and Recovery, from the catalogue; `{{default-keymap}}`
//!   becomes the default keyboard map as a table. Neither can go out of
//!   date.
//! - Each topic's slug becomes a constant (`topic::RUN_SQL`), so code can
//!   only name a topic that exists.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::Path;

use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag, TagEnd, html};

/// The line width of the plain text, for Print.
const WIDTH: usize = 80;
/// Starts a heading's line in the plain text.
const HEADING: char = '\u{1}';
/// Starts a line of the plain text that is laid out already (a table row,
/// a code line) and must not be wrapped again.
const FIXED: char = '\u{2}';

/// One topic as it goes into the binary.
struct Topic {
    slug: String,
    title: String,
    group: String,
    html: String,
    text: String,
    anchors: BTreeSet<String>,
    /// Links to check: (topic slug, anchor or empty).
    links: Vec<(String, String)>,
}

/// A heading's anchor, and a link's fragment as it is matched: lower case,
/// each run of other characters one hyphen, none at the ends.
fn anchor(text: &str) -> String {
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

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// The href the program gives the help view for a link to `slug` (and
/// `anchor`, or `-` for none). The view can only follow a link by loading
/// a file, so the program lets it try this path, which names no file, and
/// shows the topic itself (see `help::Viewer`). `@@` is replaced each time
/// a topic is shown.
fn href(slug: &str, anchor: &str) -> String {
    let anchor = if anchor.is_empty() { "-" } else { anchor };
    format!("/qsa-help/@@/{slug}/{anchor}/x")
}

/// Wraps `text` to `width`, each line after the first indented by `hang`.
fn wrap(text: &str, width: usize, hang: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut line = String::new();
    for word in text.split_whitespace() {
        let room = if lines.is_empty() {
            width
        } else {
            width.saturating_sub(hang)
        };
        if !line.is_empty() && line.chars().count() + 1 + word.chars().count() > room {
            lines.push(std::mem::take(&mut line));
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(word);
    }
    if !line.is_empty() || lines.is_empty() {
        lines.push(line);
    }
    let pad = " ".repeat(hang);
    lines
        .into_iter()
        .enumerate()
        .map(|(i, l)| if i == 0 { l } else { format!("{pad}{l}") })
        .collect()
}

/// A table as fixed lines: the columns side by side, the last one taking
/// the room that is left, cells wrapped in their column.
fn table_lines(rows: &[Vec<String>]) -> Vec<String> {
    let cols = rows.iter().map(Vec::len).max().unwrap_or(0);
    if cols == 0 {
        return Vec::new();
    }
    let natural = |c: usize| {
        rows.iter()
            .filter_map(|r| r.get(c))
            .map(|s| s.chars().count())
            .max()
            .unwrap_or(0)
    };
    let gap = 2;
    let mut widths: Vec<usize> = (0..cols).map(natural).collect();
    // Every column but the last is held to a share of the line, and the
    // last gets what is left.
    let cap = (WIDTH / cols.max(2)).max(12);
    for w in widths.iter_mut().take(cols - 1) {
        *w = (*w).min(cap);
    }
    let used: usize = widths[..cols - 1].iter().sum::<usize>() + gap * (cols - 1);
    widths[cols - 1] = widths[cols - 1].min(WIDTH.saturating_sub(used).max(12));
    let mut out = Vec::new();
    for (i, row) in rows.iter().enumerate() {
        let cells: Vec<Vec<String>> = (0..cols)
            .map(|c| wrap(row.get(c).map_or("", String::as_str), widths[c], 0))
            .collect();
        let height = cells.iter().map(Vec::len).max().unwrap_or(1);
        for l in 0..height {
            let mut line = String::new();
            for (c, cell) in cells.iter().enumerate() {
                let part = cell.get(l).map_or("", String::as_str);
                if c + 1 == cols {
                    line.push_str(part);
                } else {
                    let _ = write!(line, "{part:<w$}{}", " ".repeat(gap), w = widths[c]);
                }
            }
            out.push(format!("{FIXED}{}", line.trim_end()));
        }
        if i == 0 {
            let rule: Vec<String> = widths.iter().map(|w| "-".repeat(*w)).collect();
            out.push(format!("{FIXED}{}", rule.join(&" ".repeat(gap))));
        }
    }
    out
}

/// Builds the plain text of a topic from its Markdown events.
#[derive(Default)]
struct Plain {
    lines: Vec<String>,
    /// The inline text being gathered.
    cur: String,
    /// Open lists: the next number of an ordered one.
    lists: Vec<Option<u64>>,
    /// The prefix the next flushed text gets (a list item's mark).
    mark: Option<String>,
    in_heading: bool,
    in_code: bool,
    table: Option<Vec<Vec<String>>>,
    /// For each table so far, whether a row of it is long enough to need
    /// the whole width of the view.
    wide_tables: Vec<bool>,
}

impl Plain {
    fn blank(&mut self) {
        if self.lines.last().is_some_and(|l| !l.is_empty()) {
            self.lines.push(String::new());
        }
    }

    /// Ends the inline text gathered so far as a heading, a list item or a
    /// paragraph.
    fn flush(&mut self) {
        let text = std::mem::take(&mut self.cur);
        let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
        if text.is_empty() {
            return;
        }
        if self.in_heading {
            self.blank();
            self.lines.push(format!("{HEADING}{text}"));
            self.lines.push(String::new());
            return;
        }
        let depth = self.lists.len().saturating_sub(1);
        let indent = " ".repeat(2 * depth);
        match self.mark.take() {
            Some(mark) => {
                let hang = indent.len() + mark.len();
                for l in wrap(&format!("{indent}{mark}{text}"), WIDTH, hang) {
                    self.lines.push(l);
                }
            }
            None if !self.lists.is_empty() => {
                // More text of the same item.
                let hang = indent.len() + 2;
                for l in wrap(&format!("{}{text}", " ".repeat(hang)), WIDTH, hang) {
                    self.lines.push(l);
                }
            }
            None => {
                self.lines.extend(wrap(&text, WIDTH, 0));
                self.lines.push(String::new());
            }
        }
    }

    fn event(&mut self, e: &Event) {
        match e {
            Event::Start(Tag::Heading { .. }) => {
                self.flush();
                self.in_heading = true;
            }
            Event::End(TagEnd::Heading(_)) => {
                self.flush();
                self.in_heading = false;
            }
            Event::Start(Tag::Paragraph) => self.flush(),
            Event::End(TagEnd::Paragraph) => self.flush(),
            Event::Start(Tag::List(first)) => {
                self.flush();
                if self.lists.is_empty() {
                    self.blank();
                }
                self.lists.push(*first);
            }
            Event::End(TagEnd::List(_)) => {
                self.flush();
                self.lists.pop();
                if self.lists.is_empty() {
                    self.lines.push(String::new());
                }
            }
            Event::Start(Tag::Item) => {
                self.flush();
                self.mark = Some(match self.lists.last_mut() {
                    Some(Some(n)) => {
                        let mark = format!("{n}. ");
                        *n += 1;
                        mark
                    }
                    _ => "- ".to_owned(),
                });
            }
            Event::End(TagEnd::Item) => self.flush(),
            Event::Start(Tag::CodeBlock(_)) => {
                self.flush();
                self.blank();
                self.in_code = true;
            }
            Event::End(TagEnd::CodeBlock) => {
                self.in_code = false;
                self.lines.push(String::new());
            }
            Event::Start(Tag::Table(_)) => {
                self.flush();
                self.blank();
                self.table = Some(Vec::new());
            }
            Event::End(TagEnd::Table) => {
                if let Some(rows) = self.table.take() {
                    let longest = rows
                        .iter()
                        .map(|r| r.iter().map(|c| c.chars().count() + 3).sum::<usize>())
                        .max()
                        .unwrap_or(0);
                    self.wide_tables.push(longest > WIDTH);
                    self.lines.extend(table_lines(&rows));
                    self.lines.push(String::new());
                }
            }
            Event::Start(Tag::TableHead | Tag::TableRow) => {
                if let Some(rows) = &mut self.table {
                    rows.push(Vec::new());
                }
            }
            Event::End(TagEnd::TableCell) => {
                let text = std::mem::take(&mut self.cur);
                let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
                if let Some(row) = self.table.as_mut().and_then(|rows| rows.last_mut()) {
                    row.push(text);
                }
            }
            Event::Text(t) if self.in_code => {
                for l in t.lines() {
                    self.lines.push(format!("{FIXED}    {l}"));
                }
            }
            Event::Text(t) | Event::Code(t) => self.cur.push_str(t),
            Event::SoftBreak | Event::HardBreak => self.cur.push(' '),
            _ => {}
        }
    }

    fn finish(mut self) -> String {
        self.flush();
        while self.lines.last().is_some_and(String::is_empty) {
            self.lines.pop();
        }
        while self.lines.first().is_some_and(String::is_empty) {
            self.lines.remove(0);
        }
        self.lines.join("\n")
    }
}

/// What a placeholder paragraph becomes: HTML, plain lines and anchors.
struct Generated {
    html: String,
    lines: Vec<String>,
    anchors: Vec<String>,
}

/// Every message in the catalogue, in ID order.
fn messages() -> Generated {
    let cat = qsa_msg::Catalogue::builtin();
    let mut g = Generated {
        html: String::new(),
        lines: Vec::new(),
        anchors: Vec::new(),
    };
    for id in cat.ids() {
        let d = cat.get(id).expect("an ID the catalogue lists");
        let kind = match d.severity {
            0..=9 => "information",
            10..=19 => "warning",
            _ => "error",
        };
        let a = anchor(id);
        let _ = write!(
            g.html,
            "<a name=\"{a}\"></a><h2>{id}</h2>\n\
             <p><b>{}</b></p>\n\
             <p>Severity {:02} ({kind}).</p>\n\
             <p><b>Cause.</b> {}</p>\n\
             <p><b>Recovery.</b> {}</p>\n",
            escape(&d.text),
            d.severity,
            escape(&d.cause),
            escape(&d.recovery),
        );
        g.anchors.push(a);
        g.lines.push(String::new());
        g.lines.push(format!("{HEADING}{id}"));
        g.lines.push(String::new());
        g.lines.extend(wrap(&d.text, WIDTH, 0));
        g.lines
            .push(format!("Severity {:02} ({kind}).", d.severity));
        g.lines.push(String::new());
        g.lines
            .extend(wrap(&format!("Cause. {}", d.cause), WIDTH, 0));
        g.lines.push(String::new());
        g.lines
            .extend(wrap(&format!("Recovery. {}", d.recovery), WIDTH, 0));
        g.lines.push(String::new());
    }
    g
}

/// The default keyboard map: each 5250 function with the keys that do it.
fn default_keymap() -> Generated {
    let map = qsa_config::KeyMap::defaults();
    let mut rows = vec![vec!["5250 function".to_owned(), "Keys".to_owned()]];
    for (function, label) in qsa_config::keymap::FUNCTIONS {
        let keys: Vec<&str> = map
            .keys
            .iter()
            .filter(|(_, f)| f.as_str() == *function)
            .map(|(k, _)| k.as_str())
            .collect();
        let keys = if keys.is_empty() {
            "(no key)".to_owned()
        } else {
            keys.join(", ")
        };
        rows.push(vec![(*label).to_owned(), keys]);
    }
    let mut html = String::from("<table border=\"1\">\n");
    for (i, row) in rows.iter().enumerate() {
        let cell = if i == 0 { "th" } else { "td" };
        html.push_str("<tr>");
        for c in row {
            let _ = write!(html, "<{cell}>{}</{cell}>", escape(c));
        }
        html.push_str("</tr>\n");
    }
    html.push_str("</table>\n");
    let mut lines = table_lines(&rows);
    lines.push(String::new());
    Generated {
        html,
        lines,
        anchors: Vec::new(),
    }
}

fn generated(name: &str) -> Option<Generated> {
    match name {
        "{{messages}}" => Some(messages()),
        "{{default-keymap}}" => Some(default_keymap()),
        _ => None,
    }
}

/// Converts one topic. `slugs` are every topic's.
fn convert(slug: &str, group: &str, source: &str, slugs: &BTreeSet<String>) -> Topic {
    let fail = |why: String| -> ! { panic!("help/{slug}.md: {why}") };
    let parser = Parser::new_ext(source, Options::ENABLE_TABLES);
    let events: Vec<Event> = parser.collect();

    let mut title = None;
    let mut anchors = BTreeSet::new();
    let mut links = Vec::new();
    let mut plain = Plain::default();
    let mut out: Vec<Event> = Vec::with_capacity(events.len() + 16);
    // Placeholders found: what stands for each in the HTML, and its content.
    let mut inserts: Vec<(String, Generated)> = Vec::new();
    let mut i = 0;
    while i < events.len() {
        let e = &events[i];
        match e {
            Event::Start(Tag::Heading { .. }) => {
                // The heading's text names its anchor.
                let mut text = String::new();
                let mut j = i + 1;
                while j < events.len() && !matches!(events[j], Event::End(TagEnd::Heading(_))) {
                    if let Event::Text(t) | Event::Code(t) = &events[j] {
                        text.push_str(t);
                    }
                    j += 1;
                }
                let a = anchor(&text);
                if a.is_empty() {
                    fail("a heading has no text".into());
                }
                if title.is_none() {
                    title = Some(text.trim().to_owned());
                } else if !anchors.insert(a.clone()) {
                    fail(format!("two headings are both \"{}\"", text.trim()));
                }
                out.push(Event::Html(format!("<a name=\"{a}\"></a>").into()));
                out.push(e.clone());
                plain.event(e);
            }
            Event::Start(Tag::Paragraph)
                if matches!(
                    (events.get(i + 1), events.get(i + 2)),
                    (Some(Event::Text(t)), Some(Event::End(TagEnd::Paragraph)))
                        if generated(t).is_some()
                ) =>
            {
                let Some(Event::Text(name)) = events.get(i + 1) else {
                    unreachable!()
                };
                let g = generated(name).expect("checked");
                let mark = format!("<!--generated {}-->", inserts.len());
                out.push(Event::Html(mark.clone().into()));
                plain.flush();
                plain.lines.extend(g.lines.iter().cloned());
                anchors.extend(g.anchors.iter().cloned());
                inserts.push((mark, g));
                i += 3;
                continue;
            }
            Event::Start(Tag::Link {
                link_type,
                dest_url,
                title: link_title,
                id,
            }) => {
                let (file, fragment) = match dest_url.split_once('#') {
                    Some((f, a)) => (f, anchor(a)),
                    None => (dest_url.as_ref(), String::new()),
                };
                let dest = if file.is_empty() {
                    if fragment.is_empty() {
                        fail(format!("a link has no destination ({dest_url})"));
                    }
                    links.push((slug.to_owned(), fragment.clone()));
                    // The view goes to an anchor of the topic it shows.
                    format!("#{fragment}")
                } else {
                    let Some(to) = file.strip_suffix(".md") else {
                        fail(format!(
                            "link to {dest_url}: Help links only to its own topics, as topic.md"
                        ));
                    };
                    if !slugs.contains(to) {
                        fail(format!("link to {dest_url}: there is no topic {to}"));
                    }
                    links.push((to.to_owned(), fragment.clone()));
                    href(to, &fragment)
                };
                out.push(Event::Start(Tag::Link {
                    link_type: *link_type,
                    dest_url: dest.into(),
                    title: link_title.clone(),
                    id: id.clone(),
                }));
            }
            Event::Start(Tag::Image { dest_url, .. }) => {
                fail(format!("image {dest_url}: Help has no images"));
            }
            Event::Html(h) | Event::InlineHtml(h) => {
                fail(format!("raw HTML ({}): write Markdown", h.trim()));
            }
            Event::Start(Tag::BlockQuote(_)) | Event::FootnoteReference(_) => {
                fail("block quotes and footnotes are not shown by the help view".into());
            }
            Event::Start(Tag::CodeBlock(CodeBlockKind::Fenced(_) | CodeBlockKind::Indented)) => {
                // No language class: the view would not know it.
                out.push(Event::Start(Tag::CodeBlock(CodeBlockKind::Indented)));
                plain.event(e);
            }
            _ => {
                out.push(e.clone());
                plain.event(e);
            }
        }
        i += 1;
    }
    let Some(title) = title else {
        fail("the topic has no # title line".into());
    };
    if !matches!(events.first(), Some(Event::Start(Tag::Heading { .. }))) {
        fail("the first line must be the # title".into());
    }

    let mut body = String::new();
    html::push_html(&mut body, out.into_iter());
    for (mark, g) in &inserts {
        body = body.replace(mark, &g.html);
    }
    // The view draws a table's rules only when asked to. Left to itself it
    // also lets a table with long rows run a little past its right edge,
    // under the scroll bar, so such a table is given its width.
    let mut wide = plain.wide_tables.iter();
    let mut tabled = String::with_capacity(body.len() + 64);
    let mut rest = body.as_str();
    while let Some(i) = rest.find("<table>") {
        tabled.push_str(&rest[..i]);
        tabled.push_str(if wide.next().copied().unwrap_or(false) {
            "<table border=\"1\" width=\"96%\">"
        } else {
            "<table border=\"1\">"
        });
        rest = &rest[i + "<table>".len()..];
    }
    tabled.push_str(rest);
    let body = tabled;
    let html = format!(
        "<html><head><title>{}</title></head>\
         <body bgcolor=\"#FFFFFF\" text=\"#000000\" link=\"#1F4FCB\">\n{body}</body></html>\n",
        escape(&title)
    );
    Topic {
        slug: slug.to_owned(),
        title,
        group: group.to_owned(),
        html,
        text: plain.finish(),
        anchors,
        links,
    }
}

/// Reads `contents.txt`: (group, slug) in order.
fn contents(dir: &Path) -> Vec<(String, String)> {
    let path = dir.join("contents.txt");
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let mut group = String::new();
    let mut out = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        match line.strip_prefix('=') {
            Some(g) => g.trim().clone_into(&mut group),
            None => out.push((group.clone(), line.to_owned())),
        }
    }
    out
}

/// A slug as a constant's name.
fn constant(slug: &str) -> String {
    slug.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_uppercase()
            } else {
                '_'
            }
        })
        .collect()
}

/// Converts every topic in `dir` and writes them to `out` as Rust.
pub fn build(dir: &Path, out: &Path) {
    println!("cargo:rerun-if-changed={}", dir.display());
    let listed = contents(dir);
    let slugs: BTreeSet<String> = listed.iter().map(|(_, s)| s.clone()).collect();
    if slugs.len() != listed.len() {
        panic!("help/contents.txt lists a topic twice");
    }
    // Every file is listed, and every listed topic has its file.
    for entry in std::fs::read_dir(dir).unwrap_or_else(|e| panic!("{}: {e}", dir.display())) {
        let path = entry.expect("a directory entry").path();
        if path.extension().is_some_and(|x| x == "md") {
            let stem = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or_default();
            if !slugs.contains(stem) {
                panic!("help/{stem}.md is not listed in help/contents.txt");
            }
        }
    }
    let mut topics = Vec::new();
    for (group, slug) in &listed {
        if slug.is_empty()
            || !slug
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        {
            panic!("help/contents.txt: \"{slug}\" is not a topic name (a-z, 0-9 and -)");
        }
        let path = dir.join(format!("{slug}.md"));
        let source = std::fs::read_to_string(&path).unwrap_or_else(|e| {
            panic!(
                "help/contents.txt lists {slug}, but {}: {e}",
                path.display()
            )
        });
        // Windows line ends make no difference to what is built.
        let source = source.replace("\r\n", "\n");
        topics.push(convert(slug, group, &source, &slugs));
    }
    // Every link's heading exists in the topic it goes to.
    let anchors: BTreeMap<&str, &BTreeSet<String>> = topics
        .iter()
        .map(|t| (t.slug.as_str(), &t.anchors))
        .collect();
    for t in &topics {
        for (to, a) in &t.links {
            if !a.is_empty() && !anchors[to.as_str()].contains(a) {
                panic!(
                    "help/{}.md: link to {to}.md#{a}: {to} has no heading with that anchor",
                    t.slug
                );
            }
        }
    }

    let mut code = String::from(
        "// Made by build/help.rs from help/*.md. Not edited by hand.\n\n\
         /// Every topic, in the order Contents lists them.\n\
         pub static TOPICS: &[Topic] = &[\n",
    );
    for t in &topics {
        let anchors: Vec<&str> = t.anchors.iter().map(String::as_str).collect();
        let _ = writeln!(
            code,
            "    Topic {{ slug: {:?}, title: {:?}, group: {:?}, html: {:?}, text: {:?}, anchors: &{:?} }},",
            t.slug, t.title, t.group, t.html, t.text, anchors
        );
    }
    code.push_str(
        "];\n\n/// Each topic by name, for the windows and dialogs that open one.\n\
         pub mod topic {\n",
    );
    for t in &topics {
        let _ = writeln!(
            code,
            "    /// {}.\n    pub const {}: &str = {:?};",
            t.title,
            constant(&t.slug),
            t.slug
        );
    }
    code.push_str("}\n");
    std::fs::write(out, code).unwrap_or_else(|e| panic!("{}: {e}", out.display()));
}
