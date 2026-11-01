// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! Running a macro.
//!
//! [`Player`] is a state machine. The session calls [`Player::poll`] with
//! the screen as it is now; the player answers with what to do next: type
//! text, press a key, ask the user for a value, wait, or stop. The session
//! does it and polls again. Nothing here touches a window or the network,
//! so the same player runs against a live session and against a recording
//! ([`tn5250::replay::Host`]) in the tests.
//!
//! A macro stops (`QAD2020`, naming the step) when a wait times out, when
//! the screen is not the size it was recorded at, when the session is not
//! connected, or when the system locks the keyboard with an error.

use std::time::{Duration, Instant};

use qsa_config::qsm::{Action, MacroFile};
use qsa_config::settings::ScreenSize;
use tn5250::input::Key;
use tn5250::replay::key_from_name;
use tn5250::screen::{Keyboard, Screen};

/// What the session is to do next.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Act {
    /// Type this text at the cursor, then poll again.
    Type(String),
    /// Press this key, then poll again.
    Key(Key),
    /// Ask the user for a value; give it to [`Player::answer`].
    Prompt { label: String, hidden: bool },
    /// Nothing yet; poll again soon.
    Wait,
    /// Every step has run.
    Done,
    /// Stopped at step `step` (from 1) for `reason`.
    Failed { step: usize, reason: String },
}

/// A macro being run.
#[derive(Debug)]
pub struct Player {
    pub name: String,
    steps: Vec<Action>,
    size: ScreenSize,
    /// The step to run next, from 0.
    index: usize,
    /// When the current step began waiting.
    since: Option<Instant>,
    /// The answer to the prompt at the current step, once given.
    answer: Option<Option<String>>,
    asked: bool,
    finished: bool,
}

fn secs(ms: u32) -> String {
    if ms % 1000 == 0 {
        format!("{} s", ms / 1000)
    } else {
        format!("{ms} ms")
    }
}

/// The text on `row`, from `col` (both from 1), `len` characters long.
pub fn text_at(s: &Screen, row: usize, col: usize, len: usize) -> Option<String> {
    if row == 0 || col == 0 || row > s.rows() || col > s.cols() {
        return None;
    }
    let end = (col - 1 + len).min(s.cols());
    Some((col - 1..end).map(|c| s.cell(row - 1, c).ch).collect())
}

impl Player {
    pub fn new(m: &MacroFile, name: &str) -> Player {
        Player {
            name: name.to_owned(),
            steps: m.steps.iter().map(|s| s.action.clone()).collect(),
            size: m.info.screen_size,
            index: 0,
            since: None,
            answer: None,
            asked: false,
            finished: false,
        }
    }

    /// The step running, from 1.
    pub fn step(&self) -> usize {
        self.index + 1
    }

    /// The number of steps.
    pub fn len(&self) -> usize {
        self.steps.len()
    }

    /// Whether the macro has no steps.
    pub fn is_empty(&self) -> bool {
        self.steps.is_empty()
    }

    /// The answer to the prompt asked; `None` cancels the macro.
    pub fn answer(&mut self, value: Option<String>) {
        self.answer = Some(value);
    }

    /// Stops the macro where it is, at the user's request.
    pub fn stop(&mut self) -> Act {
        self.fail("stopped at your request".into())
    }

    fn fail(&mut self, reason: String) -> Act {
        self.finished = true;
        Act::Failed {
            step: self.step().min(self.len().max(1)),
            reason,
        }
    }

    fn next(&mut self) {
        self.index += 1;
        self.since = None;
        self.asked = false;
        self.answer = None;
    }

    /// What to do now, given the screen, whether the session is
    /// connected, and the time.
    pub fn poll(&mut self, s: &Screen, connected: bool, now: Instant) -> Act {
        if self.finished {
            return Act::Done;
        }
        if self.index == 0 && self.since.is_none() && !self.asked {
            let (rows, cols) = self.size.rows_cols();
            if (s.rows(), s.cols()) != (rows, cols) {
                return self.fail(format!(
                    "the screen is {} x {}, but the macro was recorded at {rows} x {cols}",
                    s.rows(),
                    s.cols()
                ));
            }
        }
        loop {
            let Some(action) = self.steps.get(self.index).cloned() else {
                self.finished = true;
                return Act::Done;
            };
            if !connected {
                return self.fail("the session is not connected".into());
            }
            let since = *self.since.get_or_insert(now);
            let waited = now.saturating_duration_since(since);
            match action {
                Action::Type { text } => {
                    if !s.keyboard.is_open() {
                        return self
                            .fail("the keyboard is locked, so the text cannot be typed".into());
                    }
                    self.next();
                    return Act::Type(text);
                }
                Action::Key { key } => {
                    let Some(k) = key_from_name(&key) else {
                        return self.fail(format!("{key} is not a 5250 key"));
                    };
                    self.next();
                    return Act::Key(k);
                }
                Action::WaitUnlock { timeout_ms } => match s.keyboard {
                    Keyboard::Open => self.next(),
                    Keyboard::HostError | Keyboard::OperatorError(_) => {
                        let line = s
                            .text_rows()
                            .get(s.error_row.min(s.rows().saturating_sub(1)))
                            .map(|l| l.trim().to_owned())
                            .unwrap_or_default();
                        return self.fail(if line.is_empty() {
                            "the keyboard is locked with an error".into()
                        } else {
                            format!("the system reported \"{line}\"")
                        });
                    }
                    Keyboard::System => {
                        if waited >= Duration::from_millis(timeout_ms.into()) {
                            return self.fail(format!(
                                "timed out after {} waiting for the keyboard",
                                secs(timeout_ms)
                            ));
                        }
                        return Act::Wait;
                    }
                },
                Action::WaitText {
                    row,
                    col,
                    text,
                    timeout_ms,
                } => {
                    let (r, c) = (usize::from(row), usize::from(col));
                    let Some(found) = text_at(s, r, c, text.chars().count()) else {
                        return self.fail(format!(
                            "the screen is {} x {}, so position {r},{c} is not on it",
                            s.rows(),
                            s.cols()
                        ));
                    };
                    if found == text {
                        self.next();
                    } else if waited >= Duration::from_millis(timeout_ms.into()) {
                        return self.fail(format!(
                            "timed out after {} waiting for \"{text}\" at {r},{c}",
                            secs(timeout_ms)
                        ));
                    } else {
                        return Act::Wait;
                    }
                }
                Action::Pause { ms } => {
                    if waited >= Duration::from_millis(ms.into()) {
                        self.next();
                    } else {
                        return Act::Wait;
                    }
                }
                Action::Prompt { label, hidden } => match self.answer.take() {
                    Some(Some(v)) => {
                        self.next();
                        return Act::Type(v);
                    }
                    Some(None) => return self.fail("no value was given".into()),
                    None if self.asked => return Act::Wait,
                    None => {
                        self.asked = true;
                        return Act::Prompt { label, hidden };
                    }
                },
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use qsa_config::qsm::Step;
    use tn5250::negotiate::ScreenSize as TnSize;

    fn mac(steps: Vec<Action>) -> MacroFile {
        let mut m = MacroFile::new("T", ScreenSize::S24x80);
        m.steps = steps.into_iter().map(Step::new).collect();
        m
    }

    fn screen() -> Screen {
        Screen::default_24x80()
    }

    #[test]
    fn wrong_size_and_disconnected_stop_at_once() {
        // A terminal able to show 27 x 132 still starts at 24 x 80, until
        // the system switches it.
        let mut wide = MacroFile::new("W", ScreenSize::S27x132);
        wide.steps.push(Step::new(Action::Pause { ms: 1 }));
        let mut p = Player::new(&wide, "W");
        let s = Screen::new(TnSize::S27x132, tn5250::codepage::default_page());
        let now = Instant::now();
        assert!(matches!(
            p.poll(&s, true, now),
            Act::Failed { step: 1, reason }
                if reason == "the screen is 24 x 80, but the macro was recorded at 27 x 132"
        ));
        let mut p = Player::new(&mac(vec![Action::Pause { ms: 1 }]), "T");
        assert!(matches!(
            p.poll(&screen(), false, now),
            Act::Failed { step: 1, reason } if reason.contains("not connected")
        ));
    }

    #[test]
    fn waits_time_out_naming_the_step() {
        let mut s = screen();
        s.keyboard = Keyboard::System;
        let mut p = Player::new(
            &mac(vec![
                Action::Pause { ms: 100 },
                Action::WaitUnlock { timeout_ms: 2000 },
            ]),
            "T",
        );
        let t0 = Instant::now();
        assert_eq!(p.poll(&s, true, t0), Act::Wait);
        assert_eq!(p.poll(&s, true, t0 + Duration::from_millis(150)), Act::Wait);
        assert_eq!(p.step(), 2);
        assert!(matches!(
            p.poll(&s, true, t0 + Duration::from_millis(2200)),
            Act::Failed { step: 2, reason } if reason.contains("2 s")
        ));
    }

    #[test]
    fn prompts_then_types_the_answer() {
        let mut p = Player::new(
            &mac(vec![Action::Prompt {
                label: "Password".into(),
                hidden: true,
            }]),
            "T",
        );
        let now = Instant::now();
        assert_eq!(
            p.poll(&screen(), true, now),
            Act::Prompt {
                label: "Password".into(),
                hidden: true
            }
        );
        assert_eq!(p.poll(&screen(), true, now), Act::Wait);
        p.answer(Some("secret".into()));
        assert_eq!(p.poll(&screen(), true, now), Act::Type("secret".into()));
        assert_eq!(p.poll(&screen(), true, now), Act::Done);
    }

    #[test]
    fn a_cancelled_prompt_stops() {
        let mut p = Player::new(
            &mac(vec![Action::Prompt {
                label: "Password".into(),
                hidden: true,
            }]),
            "T",
        );
        let now = Instant::now();
        let _ = p.poll(&screen(), true, now);
        p.answer(None);
        assert!(matches!(
            p.poll(&screen(), true, now),
            Act::Failed { step: 1, .. }
        ));
    }

    #[test]
    fn wait_text_outside_the_screen_is_a_size_error() {
        let mut p = Player::new(
            &mac(vec![Action::WaitText {
                row: 24,
                col: 80,
                text: "xy".into(),
                timeout_ms: 10,
            }]),
            "T",
        );
        // Two characters from column 80 run off the screen: the text read
        // is short, so it does not match and the wait times out.
        let t0 = Instant::now();
        assert_eq!(p.poll(&screen(), true, t0), Act::Wait);
        assert!(matches!(
            p.poll(&screen(), true, t0 + Duration::from_millis(20)),
            Act::Failed { step: 1, .. }
        ));
        assert_eq!(text_at(&screen(), 25, 1, 1), None);
    }
}
