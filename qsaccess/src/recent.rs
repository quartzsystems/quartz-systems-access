// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! The Recent Sessions submenu, shared by the main window and every session
//! window. Each keeps the list generation it last drew and redraws its
//! submenu when [`crate::app::AppState::recent_gen`] moves on.

use std::path::{Path, PathBuf};

use fltk::menu::{MenuBar, MenuFlag};
use fltk::prelude::*;

/// Longest path shown in full; longer ones lose their middle.
const MAX_LABEL: usize = 64;

/// `s` with FLTK's menu path characters escaped, so it is one label.
pub fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' | '/' => {
                out.push('\\');
                out.push(c);
            }
            '&' => out.push_str("&&"),
            _ => out.push(c),
        }
    }
    out
}

/// A path shortened for a menu: the start and the end, `...` between.
pub fn shorten(p: &Path) -> String {
    let s = p.display().to_string();
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= MAX_LABEL {
        return s;
    }
    let tail = MAX_LABEL * 2 / 3;
    let head = MAX_LABEL - tail - 3;
    let a: String = chars[..head].iter().collect();
    let b: String = chars[chars.len() - tail..].iter().collect();
    format!("{a}...{b}")
}

/// The menu label of the `n`th (from 0) recent session.
pub fn label(n: usize, p: &Path) -> String {
    let key = if n < 9 {
        format!("&{}", n + 1)
    } else {
        format!("1&{}", n - 9)
    };
    format!("{key} {}", escape(&shorten(p)))
}

/// Rebuilds the submenu at `path` (for example `&File/Recent Se&ssions`)
/// from `recent`. `add` adds one item: it is given the menu, the item path
/// and the index.
pub fn fill(
    menu: &mut MenuBar,
    path: &str,
    recent: &[PathBuf],
    mut add: impl FnMut(&mut MenuBar, &str, usize),
) {
    let idx = menu.find_index(path);
    if idx >= 0 {
        let _ = menu.clear_submenu(idx);
    }
    if recent.is_empty() {
        menu.add(
            &format!("{path}/(none)"),
            fltk::enums::Shortcut::None,
            MenuFlag::Inactive,
            |_| {},
        );
        return;
    }
    for (i, p) in recent.iter().enumerate() {
        add(menu, &format!("{path}/{}", label(i, p)), i);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_escape_menu_characters() {
        assert_eq!(label(0, Path::new("C:/a&b/c.qsd")), "&1 C:\\/a&&b\\/c.qsd");
        assert_eq!(label(9, Path::new("x")), "1&0 x");
        let long = format!("/{}/end.qsd", "d".repeat(100));
        let s = shorten(Path::new(&long));
        assert_eq!(s.chars().count(), MAX_LABEL);
        assert!(s.ends_with("end.qsd") && s.contains("..."));
    }
}
