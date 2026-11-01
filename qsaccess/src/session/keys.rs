// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! From FLTK key events to 5250 keys, through the keyboard map.
//!
//! Key names are those of [`qsa_config::keymap`]. Right Ctrl is told apart
//! from left Ctrl by FLTK's key codes (`Control_R` / `Control_L`, which FLTK
//! reports on Windows, X11 and Wayland). It acts when it is released, and
//! only if no other key went down while it was held, so Right Ctrl+C still
//! copies.

use fltk::enums::{Key, Shortcut};
use qsa_config::KeyMap;
use qsa_ui::terminal::KeyPress;
use tn5250::input::Key as K5250;

const KP: i32 = 0xff80;
/// X11's Shift+Tab.
const ISO_LEFT_TAB: i32 = 0xfe20;

/// The keymap name of a key with its modifiers, or `None` for a modifier
/// alone (Right Ctrl excepted) or a key with no name.
pub fn key_name(key: Key, state: Shortcut) -> Option<String> {
    let k = key.bits();
    let mut shift = state.contains(Shortcut::Shift);
    let base: String = match key {
        Key::ControlR => return Some("Right Ctrl".into()),
        Key::ControlL
        | Key::ShiftL
        | Key::ShiftR
        | Key::AltL
        | Key::AltR
        | Key::MetaL
        | Key::MetaR
        | Key::CapsLock => return None,
        Key::Enter => "Enter".into(),
        Key::KPEnter => "Keypad Enter".into(),
        Key::Escape => "Esc".into(),
        Key::Tab => "Tab".into(),
        Key::BackSpace => "Backspace".into(),
        Key::Delete => "Delete".into(),
        Key::Insert => "Insert".into(),
        Key::Pause => "Pause".into(),
        Key::Home => "Home".into(),
        Key::End => "End".into(),
        Key::PageUp => "Page Up".into(),
        Key::PageDown => "Page Down".into(),
        Key::Left => "Left".into(),
        Key::Right => "Right".into(),
        Key::Up => "Up".into(),
        Key::Down => "Down".into(),
        Key::Print => "Print Screen".into(),
        Key::ScrollLock => "Scroll Lock".into(),
        _ if k == ISO_LEFT_TAB => {
            shift = true;
            "Tab".into()
        }
        _ if (Key::F1.bits()..=Key::F1.bits() + 23).contains(&k) => {
            format!("F{}", k - Key::F1.bits() + 1)
        }
        _ if k == KP + i32::from(b'+') => "Keypad +".into(),
        _ if k == KP + i32::from(b'-') => "Keypad -".into(),
        _ if k == KP + i32::from(b'*') => "Keypad *".into(),
        _ if k == KP + i32::from(b'/') => "Keypad /".into(),
        _ if (0x20..0x7f).contains(&k) => {
            let c = char::from(k as u8).to_ascii_uppercase();
            c.to_string()
        }
        _ => return None,
    };
    let mut name = String::new();
    if state.contains(Shortcut::Ctrl) {
        name.push_str("Ctrl+");
    }
    if shift {
        name.push_str("Shift+");
    }
    if state.contains(Shortcut::Alt) {
        name.push_str("Alt+");
    }
    name.push_str(&base);
    Some(name)
}

/// What a key press means for the session.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    /// A 5250 key.
    Key(K5250),
    /// Text to type (one character).
    Type(char),
    /// Nothing for the session; let FLTK have it (menu shortcuts).
    Pass,
    /// Swallowed: a modifier going down, a key coming up.
    Swallow,
}

/// Turns key events into actions, holding the Right Ctrl state.
#[derive(Debug, Default)]
pub struct Decoder {
    right_ctrl_alone: bool,
}

impl Decoder {
    pub fn decode(&mut self, p: &KeyPress, map: &KeyMap) -> Action {
        if p.key == Key::ControlR {
            if !p.up {
                self.right_ctrl_alone = true;
                return Action::Swallow;
            }
            let alone = std::mem::take(&mut self.right_ctrl_alone);
            if !alone {
                return Action::Swallow;
            }
        } else if !p.up {
            self.right_ctrl_alone = false;
        } else {
            return Action::Swallow;
        }
        let Some(name) = key_name(p.key, p.state) else {
            return Action::Swallow;
        };
        if let Some(f) = map.function(&name) {
            return match tn5250::replay::key_from_name(f) {
                Some(k) => Action::Key(k),
                None => Action::Swallow,
            };
        }
        // Plain typing: the text FLTK composed, without Ctrl or Alt.
        let chord = p.state.contains(Shortcut::Ctrl) || p.state.contains(Shortcut::Alt);
        let mut chars = p.text.chars();
        if let (Some(c), None, false) = (chars.next(), chars.next(), chord)
            && !c.is_control()
        {
            return Action::Type(c);
        }
        if chord { Action::Pass } else { Action::Swallow }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn press(key: Key, state: Shortcut, text: &str, up: bool) -> KeyPress {
        KeyPress {
            key,
            text: text.into(),
            state,
            up,
        }
    }

    #[test]
    fn names() {
        let n = |k, s| key_name(k, s);
        assert_eq!(n(Key::Enter, Shortcut::None).as_deref(), Some("Enter"));
        assert_eq!(
            n(Key::ControlR, Shortcut::Ctrl).as_deref(),
            Some("Right Ctrl")
        );
        assert_eq!(n(Key::ControlL, Shortcut::Ctrl), None);
        assert_eq!(n(Key::F1, Shortcut::Shift).as_deref(), Some("Shift+F1"));
        assert_eq!(
            n(Key::from_i32(Key::F1.bits() + 11), Shortcut::None).as_deref(),
            Some("F12")
        );
        assert_eq!(
            n(Key::from_i32(KP + 0x2b), Shortcut::None).as_deref(),
            Some("Keypad +")
        );
        assert_eq!(
            n(Key::from_char('r'), Shortcut::Ctrl).as_deref(),
            Some("Ctrl+R")
        );
        assert_eq!(
            n(Key::Escape, Shortcut::Shift).as_deref(),
            Some("Shift+Esc")
        );
        assert_eq!(n(Key::Home, Shortcut::Ctrl).as_deref(), Some("Ctrl+Home"));
        assert_eq!(
            n(Key::from_i32(ISO_LEFT_TAB), Shortcut::Shift).as_deref(),
            Some("Shift+Tab")
        );
    }

    #[test]
    fn the_default_map_drives_5250_keys() {
        let map = KeyMap::defaults();
        let mut d = Decoder::default();
        let mut a = |p: KeyPress| d.decode(&p, &map);
        assert_eq!(
            a(press(Key::Enter, Shortcut::None, "\r", false)),
            Action::Key(K5250::Enter)
        );
        assert_eq!(
            a(press(Key::F1, Shortcut::Shift, "", false)),
            Action::Key(K5250::F(13))
        );
        assert_eq!(
            a(press(Key::Escape, Shortcut::Shift, "\u{1b}", false)),
            Action::Key(K5250::SysReq)
        );
        assert_eq!(
            a(press(Key::Escape, Shortcut::None, "\u{1b}", false)),
            Action::Key(K5250::Attention)
        );
        assert_eq!(
            a(press(Key::from_char('a'), Shortcut::None, "a", false)),
            Action::Type('a')
        );
        assert_eq!(
            a(press(Key::from_char('c'), Shortcut::Ctrl, "\u{3}", false)),
            Action::Pass
        );
        assert_eq!(
            a(press(Key::from_char('a'), Shortcut::None, "a", true)),
            Action::Swallow
        );
    }

    #[test]
    fn right_ctrl_acts_on_release_when_alone() {
        let map = KeyMap::defaults();
        let mut d = Decoder::default();
        assert_eq!(
            d.decode(&press(Key::ControlR, Shortcut::None, "", false), &map),
            Action::Swallow
        );
        assert_eq!(
            d.decode(&press(Key::ControlR, Shortcut::Ctrl, "", true), &map),
            Action::Key(K5250::FieldExit)
        );
        // Right Ctrl+C: the C goes down while it is held; no Field Exit.
        d.decode(&press(Key::ControlR, Shortcut::None, "", false), &map);
        assert_eq!(
            d.decode(&press(Key::from_char('c'), Shortcut::Ctrl, "", false), &map),
            Action::Pass
        );
        assert_eq!(
            d.decode(&press(Key::ControlR, Shortcut::Ctrl, "", true), &map),
            Action::Swallow
        );
        // Left Ctrl does nothing alone.
        assert_eq!(
            d.decode(&press(Key::ControlL, Shortcut::None, "", false), &map),
            Action::Swallow
        );
    }
}
