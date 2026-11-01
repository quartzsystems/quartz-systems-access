// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! Recording a macro from what is typed into a session.
//!
//! - Characters typed one after another become one Type step.
//! - Every other key becomes a Key step. After an attention key (Enter,
//!   F1 to F24, Page Up, Page Down, Clear, Help, Print, Record Backspace) a
//!   step that waits for the keyboard to unlock follows.
//! - Nothing typed into a field that is not displayed (a password) is
//!   kept: a step that asks for the value when the macro runs takes its
//!   place, once per field.

use qsa_config::qsm::{Action, DEFAULT_TIMEOUT_MS, Step};
use tn5250::input::Key;
use tn5250::replay::key_name;
use tn5250::screen::Screen;

/// Whether a key is an attention key: it sends the screen to the system
/// and locks the keyboard until the system answers.
pub fn is_aid(k: Key) -> bool {
    matches!(
        k,
        Key::Enter
            | Key::F(_)
            | Key::PageUp
            | Key::PageDown
            | Key::Clear
            | Key::Help
            | Key::Print
            | Key::RecordBackspace
    )
}

/// The label of an input field: the text before it on its row, without the
/// dot leaders, as in `Password . . . . .`.
pub fn field_label(s: &Screen, start: usize) -> String {
    let (row, col) = s.rc(start);
    let before: String = (0..col.saturating_sub(1))
        .map(|c| s.cell(row, c).ch)
        .collect();
    let label = before.trim_end().trim_end_matches(['.', ':', ' ']).trim();
    // The last run of words before the field: labels are separated from
    // other text on the row by two or more spaces.
    let label = label.rsplit("  ").next().unwrap_or(label).trim();
    if label.is_empty() {
        "a value".to_owned()
    } else {
        label.to_owned()
    }
}

/// A macro being recorded.
#[derive(Debug, Default)]
pub struct Recorder {
    steps: Vec<Step>,
    typing: String,
    /// The start of the hidden field a prompt was put in for.
    prompted: Option<usize>,
}

impl Recorder {
    pub fn new() -> Recorder {
        Recorder::default()
    }

    fn flush(&mut self) {
        if !self.typing.is_empty() {
            let text = std::mem::take(&mut self.typing);
            self.steps.push(Step::new(Action::Type { text }));
        }
    }

    /// The hidden field the cursor is in, if any.
    fn hidden_field(s: &Screen) -> Option<usize> {
        s.field_at(s.cursor)
            .map(|i| &s.fields[i])
            .filter(|f| f.nondisplay())
            .map(|f| f.start)
    }

    fn char(&mut self, before: &Screen, c: char) {
        match Recorder::hidden_field(before) {
            Some(start) => {
                if self.prompted != Some(start) {
                    self.flush();
                    self.steps.push(Step::new(Action::Prompt {
                        label: field_label(before, start),
                        hidden: true,
                    }));
                    self.prompted = Some(start);
                }
            }
            None => {
                self.prompted = None;
                self.typing.push(c);
            }
        }
    }

    /// A key, given the screen as it was before the key.
    pub fn key(&mut self, before: &Screen, k: Key) {
        if let Key::Char(c) = k {
            self.char(before, c);
            return;
        }
        self.flush();
        // Moving within a hidden field (Backspace, say) is part of typing
        // the value; the prompt covers it.
        if Recorder::hidden_field(before).is_some()
            && Recorder::hidden_field(before) == self.prompted
            && matches!(k, Key::Backspace | Key::Delete | Key::Left | Key::Right)
        {
            return;
        }
        self.prompted = None;
        self.steps.push(Step::new(Action::Key { key: key_name(k) }));
        if is_aid(k) {
            self.steps.push(Step::new(Action::WaitUnlock {
                timeout_ms: DEFAULT_TIMEOUT_MS,
            }));
        }
    }

    /// Text pasted at the cursor.
    pub fn paste(&mut self, before: &Screen, text: &str) {
        for c in text.chars().filter(|c| !c.is_control()) {
            self.char(before, c);
        }
    }

    /// The steps recorded.
    pub fn finish(mut self) -> Vec<Step> {
        self.flush();
        self.steps
    }

    /// Steps so far, counting text not yet ended.
    pub fn len(&self) -> usize {
        self.steps.len() + usize::from(!self.typing.is_empty())
    }

    /// Whether nothing has been recorded.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use tn5250::replay::{Recording, replay};

    fn signon() -> Screen {
        let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../fixtures/replay/qs400-signon-24x80.qsr");
        let rec = Recording::read(&p).unwrap();
        // The sign-on display as first sent, before Enter was pressed.
        let mut first = rec.clone();
        let cut = first
            .events
            .iter()
            .position(|(_, e)| matches!(e, tn5250::replay::Event::Key(_)))
            .unwrap_or(first.events.len());
        first.events.truncate(cut);
        replay(&first).terminal.screen
    }

    #[test]
    fn records_text_keys_waits_and_prompts_for_hidden_fields() {
        let mut t = tn5250::terminal::Terminal { screen: signon() };
        let mut r = Recorder::new();
        let mut press = |t: &mut tn5250::terminal::Terminal, k: Key| {
            r.key(&t.screen, k);
            let _ = t.key(k);
        };
        for c in "QSATEST".chars() {
            press(&mut t, Key::Char(c));
        }
        press(&mut t, Key::FieldExit);
        for c in "hunter2".chars() {
            press(&mut t, Key::Char(c));
        }
        press(&mut t, Key::Backspace);
        press(&mut t, Key::Char('x'));
        press(&mut t, Key::Enter);
        let steps: Vec<Action> = r.finish().into_iter().map(|s| s.action).collect();
        assert_eq!(
            steps,
            vec![
                Action::Type {
                    text: "QSATEST".into()
                },
                Action::Key {
                    key: "FieldExit".into()
                },
                Action::Prompt {
                    label: "Password".into(),
                    hidden: true
                },
                Action::Key {
                    key: "Enter".into()
                },
                Action::WaitUnlock {
                    timeout_ms: DEFAULT_TIMEOUT_MS
                },
            ]
        );
        let text = format!("{steps:?}");
        assert!(!text.contains("hunter"), "{text}");
    }

    #[test]
    fn labels_lose_their_dot_leaders() {
        let s = signon();
        let pw = s
            .fields
            .iter()
            .find(|f| f.nondisplay())
            .expect("password field");
        assert_eq!(field_label(&s, pw.start), "Password");
        let user = s.fields.iter().find(|f| !f.nondisplay()).unwrap();
        assert_eq!(field_label(&s, user.start), "User");
    }

    #[test]
    fn attention_keys() {
        assert!(is_aid(Key::F(24)) && is_aid(Key::PageDown) && is_aid(Key::Enter));
        assert!(!is_aid(Key::FieldExit) && !is_aid(Key::SysReq) && !is_aid(Key::Reset));
    }
}
