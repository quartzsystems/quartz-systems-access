// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! `.qsm` files: a macro for a 5250 display session.
//!
//! ```toml
//! [qsaccess]
//! kind = "macro"
//! version = 1
//!
//! [macro]
//! name = "Sign on"
//! description = "Signs on and starts WRKACTJOB"
//! screen_size = "24x80"
//!
//! [[step]]
//! action = "wait-text"
//! row = 1
//! col = 36
//! text = "Sign On"
//! timeout_ms = 15000
//!
//! [[step]]
//! action = "type"
//! text = "QSATEST"
//!
//! [[step]]
//! action = "key"
//! key = "FieldExit"
//!
//! [[step]]
//! action = "prompt"
//! label = "Password"
//!
//! [[step]]
//! action = "key"
//! key = "Enter"
//!
//! [[step]]
//! action = "wait-unlock"
//! timeout_ms = 15000
//!
//! [[step]]
//! action = "pause"
//! ms = 500
//! ```
//!
//! - `key` names a 5250 function as keyboard maps do (`Enter`, `F:3`,
//!   `PageDown`, `FieldExit` ...); see [`crate::keymap::FUNCTIONS`].
//! - `row` and `col` count from 1.
//! - `prompt` asks for a value when the macro runs and types it. Recording
//!   puts one in place of anything typed into a field that is not
//!   displayed, so a password never reaches the file.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::doc::{self, Extra, FileError, Header};
use crate::keymap;
use crate::settings::ScreenSize;

/// File extension, without the dot.
pub const EXTENSION: &str = "qsm";
/// Longest wait a step may give, in milliseconds (ten minutes).
pub const MAX_WAIT_MS: u32 = 600_000;
/// The wait a new step gets, in milliseconds.
pub const DEFAULT_TIMEOUT_MS: u32 = 15_000;

/// What a step does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    /// Type text at the cursor.
    Type { text: String },
    /// Press a key or attention key, by 5250 function name.
    Key { key: String },
    /// Wait until the keyboard unlocks.
    WaitUnlock { timeout_ms: u32 },
    /// Wait until `text` appears at `row`, `col` (from 1).
    WaitText {
        row: u8,
        col: u8,
        text: String,
        timeout_ms: u32,
    },
    /// Wait a fixed time.
    Pause { ms: u32 },
    /// Ask for a value when the macro runs, and type it. `hidden` asks
    /// without echoing (for passwords).
    Prompt { label: String, hidden: bool },
}

impl Action {
    /// The name written in the file.
    pub fn name(&self) -> &'static str {
        match self {
            Action::Type { .. } => "type",
            Action::Key { .. } => "key",
            Action::WaitUnlock { .. } => "wait-unlock",
            Action::WaitText { .. } => "wait-text",
            Action::Pause { .. } => "pause",
            Action::Prompt { .. } => "prompt",
        }
    }

    /// A one-line description, as the Macro editor lists it.
    pub fn describe(&self) -> String {
        let secs = |ms: &u32| {
            if ms % 1000 == 0 {
                format!("{} s", ms / 1000)
            } else {
                format!("{ms} ms")
            }
        };
        match self {
            Action::Type { text } => format!("Type \"{text}\""),
            Action::Key { key } => format!("Press {}", keymap::function_label(key)),
            Action::WaitUnlock { timeout_ms } => {
                format!("Wait for keyboard (up to {})", secs(timeout_ms))
            }
            Action::WaitText {
                row,
                col,
                text,
                timeout_ms,
            } => format!(
                "Wait for \"{text}\" at {row},{col} (up to {})",
                secs(timeout_ms)
            ),
            Action::Pause { ms } => format!("Pause {}", secs(ms)),
            Action::Prompt { label, hidden } => {
                if *hidden {
                    format!("Ask for {label} (hidden)")
                } else {
                    format!("Ask for {label}")
                }
            }
        }
    }
}

/// One step, and any keys in it this release does not know.
#[derive(Clone, Debug, PartialEq)]
pub struct Step {
    pub action: Action,
    pub extra: Extra,
}

impl Step {
    pub fn new(action: Action) -> Step {
        Step {
            action,
            extra: Extra::new(),
        }
    }
}

impl From<Action> for Step {
    fn from(a: Action) -> Step {
        Step::new(a)
    }
}

/// A step as written: every field optional, checked by [`StepDoc::into_step`].
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
struct StepDoc {
    action: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    row: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    col: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    timeout_ms: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    ms: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    hidden: Option<bool>,
    #[serde(flatten)]
    extra: Extra,
}

impl StepDoc {
    fn from_step(s: &Step) -> StepDoc {
        let mut d = StepDoc {
            action: s.action.name().into(),
            extra: s.extra.clone(),
            ..StepDoc::default()
        };
        match &s.action {
            Action::Type { text } => d.text = Some(text.clone()),
            Action::Key { key } => d.key = Some(key.clone()),
            Action::WaitUnlock { timeout_ms } => d.timeout_ms = Some((*timeout_ms).into()),
            Action::WaitText {
                row,
                col,
                text,
                timeout_ms,
            } => {
                d.row = Some((*row).into());
                d.col = Some((*col).into());
                d.text = Some(text.clone());
                d.timeout_ms = Some((*timeout_ms).into());
            }
            Action::Pause { ms } => d.ms = Some((*ms).into()),
            Action::Prompt { label, hidden } => {
                d.label = Some(label.clone());
                d.hidden = Some(*hidden);
            }
        }
        d
    }

    fn into_step(self) -> Result<Step, String> {
        fn need<T>(v: Option<T>, what: &str) -> Result<T, String> {
            v.ok_or_else(|| format!("{what} is missing"))
        }
        fn wait(v: Option<i64>, what: &str) -> Result<u32, String> {
            let v = need(v, what)?;
            u32::try_from(v)
                .ok()
                .filter(|v| (1..=MAX_WAIT_MS).contains(v))
                .ok_or_else(|| format!("{what} {v} is not between 1 and {MAX_WAIT_MS}"))
        }
        let action = match self.action.as_str() {
            "type" => Action::Type {
                text: need(self.text, "text")?,
            },
            "key" => {
                let key = need(self.key, "key")?;
                if !keymap::is_function(&key) {
                    return Err(format!("key \"{key}\" is not a 5250 function"));
                }
                Action::Key { key }
            }
            "wait-unlock" => Action::WaitUnlock {
                timeout_ms: wait(self.timeout_ms, "timeout_ms")?,
            },
            "wait-text" => {
                let pos = |v: Option<i64>, what: &str, max: i64| -> Result<u8, String> {
                    let v = need(v, what)?;
                    if (1..=max).contains(&v) {
                        Ok(v as u8)
                    } else {
                        Err(format!("{what} {v} is not between 1 and {max}"))
                    }
                };
                let text = need(self.text, "text")?;
                if text.is_empty() {
                    return Err("text is empty".into());
                }
                Action::WaitText {
                    row: pos(self.row, "row", 27)?,
                    col: pos(self.col, "col", 132)?,
                    text,
                    timeout_ms: wait(self.timeout_ms, "timeout_ms")?,
                }
            }
            "pause" => Action::Pause {
                ms: wait(self.ms, "ms")?,
            },
            "prompt" => Action::Prompt {
                label: self.label.unwrap_or_else(|| "a value".into()),
                hidden: self.hidden.unwrap_or(true),
            },
            other => return Err(format!("action \"{other}\" is not known")),
        };
        Ok(Step {
            action,
            extra: self.extra,
        })
    }
}

/// `[macro]`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Info {
    pub name: String,
    pub description: String,
    /// The screen size it was recorded at; a run at another size stops.
    pub screen_size: ScreenSize,
    #[serde(flatten)]
    pub extra: Extra,
}

impl Default for Info {
    fn default() -> Info {
        Info {
            name: String::new(),
            description: String::new(),
            screen_size: ScreenSize::S24x80,
            extra: Extra::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct MacroDoc {
    qsaccess: Header,
    #[serde(default, rename = "macro")]
    info: Info,
    #[serde(default, rename = "step")]
    steps: Vec<StepDoc>,
    #[serde(flatten)]
    extra: Extra,
}

/// A macro file.
#[derive(Clone, Debug, PartialEq)]
pub struct MacroFile {
    pub header: Header,
    pub info: Info,
    pub steps: Vec<Step>,
    pub extra: Extra,
}

impl MacroFile {
    /// An empty macro.
    pub fn new(name: &str, screen_size: ScreenSize) -> MacroFile {
        MacroFile {
            header: Header::new(doc::MACRO),
            info: Info {
                name: name.to_owned(),
                screen_size,
                ..Info::default()
            },
            steps: Vec::new(),
            extra: Extra::new(),
        }
    }

    /// The name to show: `[macro] name`, else the file's name.
    pub fn display_name(&self, path: Option<&Path>) -> String {
        if !self.info.name.trim().is_empty() {
            return self.info.name.trim().to_owned();
        }
        path.and_then(Path::file_stem)
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Macro".into())
    }

    /// Checks a step as the editor would save it.
    pub fn check_step(step: &Step, size: ScreenSize) -> Result<(), String> {
        let s = StepDoc::from_step(step).into_step()?;
        if let Action::WaitText { row, col, .. } = s.action {
            let (rows, cols) = size.rows_cols();
            if usize::from(row) > rows || usize::from(col) > cols {
                return Err(format!(
                    "position {row},{col} is outside a {rows} x {cols} screen"
                ));
            }
        }
        Ok(())
    }

    /// Parses `.qsm` text.
    pub fn parse(text: &str) -> Result<MacroFile, FileError> {
        let d: MacroDoc = doc::parse(text, doc::MACRO)?;
        let mut steps = Vec::with_capacity(d.steps.len());
        for (i, s) in d.steps.into_iter().enumerate() {
            let step = s
                .into_step()
                .map_err(|e| FileError::Invalid(format!("step {}: {e}", i + 1)))?;
            MacroFile::check_step(&step, d.info.screen_size)
                .map_err(|e| FileError::Invalid(format!("step {}: {e}", i + 1)))?;
            steps.push(step);
        }
        Ok(MacroFile {
            header: d.qsaccess,
            info: d.info,
            steps,
            extra: d.extra,
        })
    }

    /// The `.qsm` text.
    pub fn to_text(&self) -> String {
        let mut header = self.header.clone();
        header.kind = doc::MACRO.into();
        header.version = doc::VERSION;
        doc::to_text(&MacroDoc {
            qsaccess: header,
            info: self.info.clone(),
            steps: self.steps.iter().map(StepDoc::from_step).collect(),
            extra: self.extra.clone(),
        })
    }

    /// Reads a `.qsm` file.
    pub fn load(path: &Path) -> Result<MacroFile, FileError> {
        MacroFile::parse(&doc::read(path)?)
    }

    /// Writes a `.qsm` file atomically.
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        doc::write(path, &self.to_text())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::tests::temp_dir;

    fn sign_on() -> MacroFile {
        let mut m = MacroFile::new("Sign on", ScreenSize::S24x80);
        m.info.description = "Sign on and run WRKACTJOB".into();
        m.steps = vec![
            Action::WaitText {
                row: 1,
                col: 36,
                text: "Sign On".into(),
                timeout_ms: 15_000,
            }
            .into(),
            Action::Type {
                text: "QSATEST".into(),
            }
            .into(),
            Action::Key {
                key: "FieldExit".into(),
            }
            .into(),
            Action::Prompt {
                label: "Password".into(),
                hidden: true,
            }
            .into(),
            Action::Key {
                key: "Enter".into(),
            }
            .into(),
            Action::WaitUnlock { timeout_ms: 15_000 }.into(),
            Action::Type {
                text: "WRKACTJOB".into(),
            }
            .into(),
            Action::Key { key: "F:3".into() }.into(),
            Action::Pause { ms: 500 }.into(),
        ];
        m
    }

    #[test]
    fn round_trip() {
        let dir = temp_dir("qsm");
        let path = dir.join("signon.qsm");
        let m = sign_on();
        m.save(&path).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.starts_with("[qsaccess]\nkind = \"macro\"\nversion = 1\n"));
        assert!(text.contains("[[step]]\naction = \"wait-text\""), "{text}");
        assert!(!text.contains("password ="), "{text}");
        assert_eq!(MacroFile::load(&path).unwrap(), m);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn unknown_keys_kept_in_steps_too() {
        let text = r#"[qsaccess]
kind = "macro"
version = 1

[macro]
name = "M"
author = "ops"

[[step]]
action = "type"
text = "X"
comment = "user id"

[[step]]
action = "pause"
ms = 100
"#;
        let m = MacroFile::parse(text).unwrap();
        assert_eq!(m.steps[0].extra["comment"].as_str(), Some("user id"));
        let out = m.to_text();
        assert!(out.contains("comment = \"user id\""), "{out}");
        assert!(out.contains("author = \"ops\""), "{out}");
        assert_eq!(MacroFile::parse(&out).unwrap(), m);
    }

    #[test]
    fn bad_steps_name_their_number() {
        let bad = |step: &str| {
            let t = format!(
                "[qsaccess]\nkind = \"macro\"\nversion = 1\n[[step]]\naction = \"pause\"\nms = 1\n[[step]]\n{step}"
            );
            match MacroFile::parse(&t) {
                Err(FileError::Invalid(s)) => s,
                other => panic!("{step}: {other:?}"),
            }
        };
        assert!(bad("action = \"fly\"\n").starts_with("step 2: action \"fly\""));
        assert!(bad("action = \"key\"\nkey = \"Warp\"\n").contains("Warp"));
        assert!(bad("action = \"type\"\n").contains("text is missing"));
        assert!(bad("action = \"pause\"\nms = 0\n").contains("ms 0"));
        assert!(bad("action = \"wait-unlock\"\ntimeout_ms = 9999999\n").contains("9999999"));
        assert!(
            bad("action = \"wait-text\"\nrow = 26\ncol = 1\ntext = \"x\"\ntimeout_ms = 5\n")
                .contains("outside a 24 x 80")
        );
        assert!(
            bad("action = \"wait-text\"\nrow = 1\ncol = 0\ntext = \"x\"\ntimeout_ms = 5\n")
                .contains("col 0")
        );
    }

    #[test]
    fn wide_macros_reach_row_27() {
        let mut m = MacroFile::new("W", ScreenSize::S27x132);
        m.steps.push(
            Action::WaitText {
                row: 27,
                col: 132,
                text: "x".into(),
                timeout_ms: 5,
            }
            .into(),
        );
        assert_eq!(MacroFile::parse(&m.to_text()).unwrap(), m);
    }

    #[test]
    fn descriptions() {
        let d: Vec<String> = sign_on()
            .steps
            .iter()
            .map(|s| s.action.describe())
            .collect();
        assert_eq!(d[0], "Wait for \"Sign On\" at 1,36 (up to 15 s)");
        assert_eq!(d[2], "Press Field Exit");
        assert_eq!(d[3], "Ask for Password (hidden)");
        assert_eq!(d[7], "Press F3");
        assert_eq!(d[8], "Pause 500 ms");
    }
}
