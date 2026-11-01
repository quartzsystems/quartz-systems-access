// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! The `QAD` message catalogue for QS/400 Access for Desktop.
//!
//! Every client-side message has an ID (`QAD` and four digits), a severity, a
//! one-line first-level text, and second-level text in two sections, Cause
//! then Recovery. Texts may carry substitution variables `&1` to `&9`.
//!
//! The catalogue is `messages.toml`, compiled into the binary.
//!
//! ```
//! let m = qsa_msg::message("QAD0010", &["SYS1"]);
//! assert_eq!(m.text, "System SYS1 added.");
//! assert_eq!(m.to_string(), "QAD0010: System SYS1 added.");
//! ```

use std::collections::BTreeMap;
use std::fmt;
use std::sync::OnceLock;

use serde::Deserialize;

/// The catalogue source.
pub const CATALOGUE_TOML: &str = include_str!("../messages.toml");

/// Message IDs, so that callers cannot misspell one.
pub mod id {
    pub const NOT_AVAILABLE: &str = "QAD0001";
    pub const SYSTEM_ADDED: &str = "QAD0010";
    pub const SYSTEM_CHANGED: &str = "QAD0011";
    pub const SYSTEM_DELETED: &str = "QAD0012";
    pub const SYSTEMS_IMPORTED: &str = "QAD0020";
    pub const SYSTEMS_EXPORTED: &str = "QAD0021";
    pub const SETTINGS_UNREADABLE: &str = "QAD0030";
    pub const NO_CREDENTIAL_STORE: &str = "QAD0040";
}

/// Errors in the catalogue itself.
#[derive(Debug, thiserror::Error)]
pub enum CatalogueError {
    #[error("message catalogue is not valid TOML: {0}")]
    Parse(#[from] toml::de::Error),
    #[error("message ID {0} is not QAD followed by four digits")]
    BadId(String),
    #[error("message {0}: severity {1} is above 99")]
    BadSeverity(String, u8),
    #[error("message {0}: {1} is empty")]
    Empty(String, &'static str),
    #[error("message {0}: first-level text must be one line")]
    MultiLine(String),
}

/// One catalogue entry, before substitution.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Definition {
    pub severity: u8,
    pub text: String,
    pub cause: String,
    pub recovery: String,
}

/// A message ready to show: substitution done.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Message {
    pub id: String,
    pub severity: u8,
    /// First-level text.
    pub text: String,
    /// Second-level text, Cause section.
    pub cause: String,
    /// Second-level text, Recovery section.
    pub recovery: String,
}

/// What a severity means for display.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Information,
    Warning,
    Error,
}

impl Message {
    /// How the severity should be presented.
    pub fn kind(&self) -> Kind {
        match self.severity {
            0..=9 => Kind::Information,
            10..=19 => Kind::Warning,
            _ => Kind::Error,
        }
    }
}

impl fmt::Display for Message {
    /// The status-bar form: `QAD0010: System SYS1 added.`
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.id, self.text)
    }
}

/// The parsed catalogue.
#[derive(Debug)]
pub struct Catalogue {
    entries: BTreeMap<String, Definition>,
}

impl Catalogue {
    /// Parses and checks a catalogue.
    pub fn parse(src: &str) -> Result<Catalogue, CatalogueError> {
        let entries: BTreeMap<String, Definition> = toml::from_str(src)?;
        for (id, d) in &entries {
            let digits = id.strip_prefix("QAD").unwrap_or("");
            if digits.len() != 4 || !digits.bytes().all(|b| b.is_ascii_digit()) {
                return Err(CatalogueError::BadId(id.clone()));
            }
            if d.severity > 99 {
                return Err(CatalogueError::BadSeverity(id.clone(), d.severity));
            }
            for (field, value) in [
                ("text", &d.text),
                ("cause", &d.cause),
                ("recovery", &d.recovery),
            ] {
                if value.trim().is_empty() {
                    return Err(CatalogueError::Empty(id.clone(), field));
                }
            }
            if d.text.contains('\n') {
                return Err(CatalogueError::MultiLine(id.clone()));
            }
        }
        Ok(Catalogue { entries })
    }

    /// The built-in catalogue.
    pub fn builtin() -> &'static Catalogue {
        static CAT: OnceLock<Catalogue> = OnceLock::new();
        CAT.get_or_init(|| {
            Catalogue::parse(CATALOGUE_TOML).expect("built-in message catalogue is valid")
        })
    }

    /// The definition of a message.
    pub fn get(&self, id: &str) -> Option<&Definition> {
        self.entries.get(id)
    }

    /// Every message ID, in order.
    pub fn ids(&self) -> impl Iterator<Item = &str> {
        self.entries.keys().map(String::as_str)
    }

    /// Builds a message with its variables filled in. `None` if the ID is
    /// not in the catalogue.
    pub fn format(&self, id: &str, args: &[&str]) -> Option<Message> {
        let d = self.get(id)?;
        Some(Message {
            id: id.to_owned(),
            severity: d.severity,
            text: substitute(&d.text, args),
            cause: substitute(&d.cause, args),
            recovery: substitute(&d.recovery, args),
        })
    }
}

/// Replaces `&1` to `&9` with `args[0]` to `args[8]`. A variable with no
/// argument is left as written, so a missing argument is visible rather than
/// silently dropped.
pub fn substitute(template: &str, args: &[&str]) -> String {
    let mut out = String::with_capacity(template.len());
    let mut chars = template.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '&'
            && let Some(d) = chars.peek().and_then(|d| d.to_digit(10))
            && (1..=9).contains(&d)
            && let Some(arg) = args.get(d as usize - 1)
        {
            chars.next();
            out.push_str(arg);
            continue;
        }
        out.push(c);
    }
    out
}

/// Builds a message from the built-in catalogue.
///
/// # Panics
///
/// If `id` is not in the catalogue. IDs come from [`id`], and a test checks
/// every one of them, so this is a programming error.
pub fn message(id: &str, args: &[&str]) -> Message {
    Catalogue::builtin()
        .format(id, args)
        .unwrap_or_else(|| panic!("message {id} is not in the catalogue"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SEEDS: [&str; 8] = [
        id::NOT_AVAILABLE,
        id::SYSTEM_ADDED,
        id::SYSTEM_CHANGED,
        id::SYSTEM_DELETED,
        id::SYSTEMS_IMPORTED,
        id::SYSTEMS_EXPORTED,
        id::SETTINGS_UNREADABLE,
        id::NO_CREDENTIAL_STORE,
    ];

    #[test]
    fn builtin_catalogue_parses_and_has_every_seed() {
        let cat = Catalogue::builtin();
        for id in SEEDS {
            assert!(cat.get(id).is_some(), "{id} missing");
        }
        assert_eq!(cat.ids().count(), SEEDS.len());
    }

    #[test]
    fn seed_texts() {
        assert_eq!(
            message(id::NOT_AVAILABLE, &[]).text,
            "Function not yet available."
        );
        assert_eq!(message(id::SYSTEM_ADDED, &["A"]).text, "System A added.");
        assert_eq!(
            message(id::SYSTEM_CHANGED, &["A"]).text,
            "System A changed."
        );
        assert_eq!(
            message(id::SYSTEM_DELETED, &["A"]).text,
            "System A deleted."
        );
        assert_eq!(
            message(id::SYSTEMS_IMPORTED, &["3"]).text,
            "3 systems imported."
        );
        assert_eq!(
            message(id::SYSTEMS_EXPORTED, &["3", "x.qsc"]).text,
            "3 systems exported to x.qsc."
        );
        assert_eq!(
            message(id::SETTINGS_UNREADABLE, &["s.toml", "bad"]).text,
            "Settings file s.toml could not be read."
        );
        assert_eq!(
            message(id::NO_CREDENTIAL_STORE, &[]).text,
            "Credential store not available; you will be prompted each time."
        );
    }

    #[test]
    fn substitution_reaches_second_level_text() {
        let m = message(id::SETTINGS_UNREADABLE, &["/x/settings.toml", "denied"]);
        assert!(m.cause.contains("/x/settings.toml"));
        assert!(m.cause.contains("denied"));
    }

    #[test]
    fn substitute_edge_cases() {
        assert_eq!(substitute("&1 and &2", &["a", "b"]), "a and b");
        assert_eq!(substitute("&2 &1", &["a", "b"]), "b a");
        assert_eq!(substitute("&1 &2", &["a"]), "a &2");
        assert_eq!(substitute("A & B &0 &", &["x"]), "A & B &0 &");
        assert_eq!(substitute("&1&1", &["é"]), "éé");
    }

    #[test]
    fn display_and_kind() {
        let m = message(id::SYSTEM_ADDED, &["SYS1"]);
        assert_eq!(m.to_string(), "QAD0010: System SYS1 added.");
        assert_eq!(m.kind(), Kind::Information);
        assert_eq!(message(id::NOT_AVAILABLE, &[]).kind(), Kind::Warning);
        assert_eq!(
            message(id::SETTINGS_UNREADABLE, &["a", "b"]).kind(),
            Kind::Error
        );
    }

    #[test]
    fn catalogue_checks() {
        let bad_id = "[QA001]\nseverity = 0\ntext = \"t\"\ncause = \"c\"\nrecovery = \"r\"\n";
        assert!(matches!(
            Catalogue::parse(bad_id),
            Err(CatalogueError::BadId(_))
        ));
        let empty = "[QAD0002]\nseverity = 0\ntext = \"t\"\ncause = \" \"\nrecovery = \"r\"\n";
        assert!(matches!(
            Catalogue::parse(empty),
            Err(CatalogueError::Empty(_, "cause"))
        ));
        let two_lines =
            "[QAD0002]\nseverity = 0\ntext = \"a\\nb\"\ncause = \"c\"\nrecovery = \"r\"\n";
        assert!(matches!(
            Catalogue::parse(two_lines),
            Err(CatalogueError::MultiLine(_))
        ));
        let sev = "[QAD0002]\nseverity = 100\ntext = \"t\"\ncause = \"c\"\nrecovery = \"r\"\n";
        assert!(matches!(
            Catalogue::parse(sev),
            Err(CatalogueError::BadSeverity(_, 100))
        ));
    }
}
