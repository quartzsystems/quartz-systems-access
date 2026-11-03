// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! A script's statements: split on `;` outside string constants (`'...'`,
//! `''` inside one), delimited identifiers (`"..."`, `""` inside one), line
//! comments (`--` to the end of the line) and bracketed comments (`/* */`,
//! which nest). A statement keeps where it is in the script, so Run
//! Statement finds the one at the cursor and a failure names its line.

/// One statement of a script.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Statement {
    /// The text without the `;` and the blanks and comments around it.
    pub text: String,
    /// Byte offsets in the script: where its text starts and ends.
    pub start: usize,
    pub end: usize,
    /// Its first line, from 1.
    pub line: usize,
}

/// Byte spans of a script between `;`s, each up to (not including) its
/// `;`, or to the end.
fn spans(script: &str) -> Vec<(usize, usize)> {
    let b = script.as_bytes();
    let mut out = Vec::new();
    let (mut i, mut from) = (0, 0);
    while i < b.len() {
        match b[i] {
            b'\'' | b'"' => {
                let q = b[i];
                i += 1;
                while i < b.len() {
                    if b[i] == q {
                        if b.get(i + 1) == Some(&q) {
                            i += 2;
                            continue;
                        }
                        break;
                    }
                    i += 1;
                }
                i += 1;
            }
            b'-' if b.get(i + 1) == Some(&b'-') => {
                while i < b.len() && b[i] != b'\n' {
                    i += 1;
                }
            }
            b'/' if b.get(i + 1) == Some(&b'*') => {
                let mut depth = 0usize;
                while i < b.len() {
                    if b[i] == b'/' && b.get(i + 1) == Some(&b'*') {
                        depth += 1;
                        i += 2;
                    } else if b[i] == b'*' && b.get(i + 1) == Some(&b'/') {
                        depth -= 1;
                        i += 2;
                        if depth == 0 {
                            break;
                        }
                    } else {
                        i += 1;
                    }
                }
            }
            b';' => {
                out.push((from, i));
                i += 1;
                from = i;
            }
            _ => i += 1,
        }
    }
    out.push((from, b.len().max(from)));
    out
}

/// The part of `script[from..to]` that is a statement: without leading and
/// trailing blanks and comments. `None` when there is nothing else.
fn trimmed(script: &str, from: usize, to: usize) -> Option<(usize, usize)> {
    let b = script.as_bytes();
    let mut s = from;
    loop {
        while s < to && b[s].is_ascii_whitespace() {
            s += 1;
        }
        if s + 1 < to && b[s] == b'-' && b[s + 1] == b'-' {
            while s < to && b[s] != b'\n' {
                s += 1;
            }
        } else if s + 1 < to && b[s] == b'/' && b[s + 1] == b'*' {
            let mut depth = 0usize;
            while s < to {
                if s + 1 < to && b[s] == b'/' && b[s + 1] == b'*' {
                    depth += 1;
                    s += 2;
                } else if s + 1 < to && b[s] == b'*' && b[s + 1] == b'/' {
                    depth -= 1;
                    s += 2;
                    if depth == 0 {
                        break;
                    }
                } else {
                    s += 1;
                }
            }
        } else {
            break;
        }
    }
    if s >= to {
        return None;
    }
    // Trailing blanks, and a trailing line comment that runs to `to`.
    let mut e = to;
    while e > s && b[e - 1].is_ascii_whitespace() {
        e -= 1;
    }
    let text = &script[s..e];
    let last_line_start = text.rfind('\n').map_or(0, |n| n + 1);
    if let Some(c) = comment_start(&text[last_line_start..]) {
        e = s + last_line_start + c;
        while e > s && b[e - 1].is_ascii_whitespace() {
            e -= 1;
        }
    }
    (e > s).then_some((s, e))
}

/// Where a line comment starts in `line`, outside quotes.
fn comment_start(line: &str) -> Option<usize> {
    let b = line.as_bytes();
    let mut i = 0;
    let mut quote: Option<u8> = None;
    while i < b.len() {
        match (quote, b[i]) {
            (Some(q), c) if c == q => quote = None,
            (None, b'\'' | b'"') => quote = Some(b[i]),
            (None, b'-') if b.get(i + 1) == Some(&b'-') => return Some(i),
            _ => {}
        }
        i += 1;
    }
    None
}

fn line_of(script: &str, at: usize) -> usize {
    script[..at].bytes().filter(|&c| c == b'\n').count() + 1
}

/// Every statement of `script`, in order.
pub fn split(script: &str) -> Vec<Statement> {
    spans(script)
        .into_iter()
        .filter_map(|(a, b)| trimmed(script, a, b))
        .map(|(s, e)| Statement {
            text: script[s..e].to_owned(),
            start: s,
            end: e,
            line: line_of(script, s),
        })
        .collect()
}

/// The statement at byte `cursor`: the one whose span (up to and including
/// its `;`) holds it, else the one before it.
pub fn at(script: &str, cursor: usize) -> Option<Statement> {
    let all = split(script);
    let mut best = None;
    for (a, b) in spans(script) {
        if let Some((s, e)) = trimmed(script, a, b) {
            let found = all.iter().find(|st| st.start == s && st.end == e).cloned();
            if cursor <= b {
                return found.or(best);
            }
            best = found;
        }
    }
    best
}

/// The statements of the text from `from` to `to` (a selection), with
/// their places in the whole script.
pub fn selected(script: &str, from: usize, to: usize) -> Vec<Statement> {
    let (from, to) = (from.min(to), from.max(to).min(script.len()));
    let part = &script[from..to];
    let base_line = line_of(script, from) - 1;
    split(part)
        .into_iter()
        .map(|s| Statement {
            start: s.start + from,
            end: s.end + from,
            line: s.line + base_line,
            text: s.text,
        })
        .collect()
}

/// The statement's first word, upper case: `SELECT`, `INSERT`.
pub fn verb(text: &str) -> String {
    text.split(|c: char| c.is_whitespace() || c == '(')
        .find(|w| !w.is_empty())
        .unwrap_or("")
        .to_ascii_uppercase()
}

/// Whether a statement changes rows (its count is "rows changed").
pub fn changes_rows(text: &str) -> bool {
    matches!(
        verb(text).as_str(),
        "INSERT" | "UPDATE" | "DELETE" | "MERGE"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(script: &str) -> Vec<String> {
        split(script).into_iter().map(|s| s.text).collect()
    }

    #[test]
    fn splits_outside_strings_identifiers_and_comments() {
        let script = "SELECT 'a;b', \"x;\"\"y\" FROM T;\n\
                      -- a comment; with a semicolon\n\
                      /* block; /* nested; */ still */ VALUES 1 ;\n\
                      INSERT INTO T VALUES('it''s;') -- trailing; comment\n\
                      ;;\n   ";
        assert_eq!(
            texts(script),
            [
                "SELECT 'a;b', \"x;\"\"y\" FROM T",
                "VALUES 1",
                "INSERT INTO T VALUES('it''s;')",
            ]
        );
        let all = split(script);
        assert_eq!(all.iter().map(|s| s.line).collect::<Vec<_>>(), [1, 3, 4]);
        assert_eq!(&script[all[1].start..all[1].end], "VALUES 1");
        assert!(split("  -- only a comment\n").is_empty());
        assert_eq!(texts("VALUES 1"), ["VALUES 1"]);
    }

    #[test]
    fn the_statement_at_the_cursor() {
        let script = "VALUES 1;\nVALUES 2;\n\nVALUES 3";
        assert_eq!(at(script, 0).unwrap().text, "VALUES 1");
        assert_eq!(at(script, 8).unwrap().text, "VALUES 1", "on its ;");
        assert_eq!(at(script, 12).unwrap().text, "VALUES 2");
        assert_eq!(at(script, script.len()).unwrap().text, "VALUES 3");
        // In the blank lines after a statement: the one before.
        let s = "VALUES 1;\n\n\n";
        assert_eq!(at(s, s.len()).unwrap().text, "VALUES 1");
        assert_eq!(at("", 0), None);
    }

    #[test]
    fn selections_keep_their_place() {
        let script = "VALUES 0;\nVALUES 1;\nVALUES 2;";
        let sel = selected(script, 10, script.len());
        assert_eq!(sel.len(), 2);
        assert_eq!((sel[0].text.as_str(), sel[0].line), ("VALUES 1", 2));
        assert_eq!(&script[sel[1].start..sel[1].end], "VALUES 2");
        assert_eq!(verb("  select * from t"), "SELECT");
        assert!(changes_rows("delete from t") && !changes_rows("SELECT 1"));
    }
}
