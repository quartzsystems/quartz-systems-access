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
    pub const CONNECTING: &str = "QAD1001";
    pub const CONNECTION_REFUSED: &str = "QAD1002";
    pub const HOST_NOT_FOUND: &str = "QAD1003";
    pub const CONNECT_TIMED_OUT: &str = "QAD1004";
    pub const SESSION_ENDED: &str = "QAD1005";
    pub const DISCONNECTED: &str = "QAD1006";
    pub const TLS_FAILED: &str = "QAD1010";
    pub const CERT_UNTRUSTED: &str = "QAD1011";
    pub const CERT_TRUSTED: &str = "QAD1012";
    pub const DEVICE_IN_USE: &str = "QAD1020";
    pub const DEVICE_NOT_VALID: &str = "QAD1021";
    pub const BASIC_5250: &str = "QAD1022";
    pub const PROTOCOL_ERROR: &str = "QAD1030";
    pub const SCREEN_PRINTED: &str = "QAD1040";
    pub const SESSION_FILE_UNREADABLE: &str = "QAD2001";
    pub const SESSION_FILE_NEWER: &str = "QAD2002";
    pub const SYSTEM_NOT_CONFIGURED: &str = "QAD2003";
    pub const SESSION_SAVED: &str = "QAD2004";
    pub const KEYMAP_LOADED: &str = "QAD2010";
    pub const KEYMAPS_MIGRATED: &str = "QAD2011";
    pub const MACRO_STOPPED: &str = "QAD2020";
    pub const MACRO_RECORDED: &str = "QAD2021";
    pub const FOLDER_ADDED: &str = "QAD2030";
    pub const INSTALLED: &str = "QAD2101";
    pub const UPGRADED: &str = "QAD2102";
    pub const PAYLOAD_DAMAGED: &str = "QAD2103";
    pub const ADMIN_NEEDED: &str = "QAD2104";
    pub const REMOVED: &str = "QAD2105";
    pub const REPAIRED: &str = "QAD2106";
    pub const PRINTER_CONNECTED: &str = "QAD3001";
    pub const PRINTER_DEVICE_REQUIRED: &str = "QAD3002";
    pub const NOT_A_PRINTER_DEVICE: &str = "QAD3003";
    pub const MESSAGE_QUEUE_NOT_FOUND: &str = "QAD3004";
    pub const OUTPUT_SAVED: &str = "QAD3010";
    pub const OUTPUT_PRINTED: &str = "QAD3011";
    pub const SYSTEM_PRINTING_UNAVAILABLE: &str = "QAD3012";
    pub const PRINTER_ERROR: &str = "QAD3013";
    pub const OUTPUT_FOLDER_NOT_WRITABLE: &str = "QAD3014";
    pub const TRANSFORMED_DATA: &str = "QAD3020";
    pub const JOB_IDLE_CLOSED: &str = "QAD3021";
    pub const RECORD_REJECTED: &str = "QAD3022";
    pub const JOB_INCOMPLETE: &str = "QAD3023";
    pub const PRINTER_RECONNECTING: &str = "QAD3030";
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

    const SEEDS: [&str; 51] = [
        id::NOT_AVAILABLE,
        id::SYSTEM_ADDED,
        id::SYSTEM_CHANGED,
        id::SYSTEM_DELETED,
        id::SYSTEMS_IMPORTED,
        id::SYSTEMS_EXPORTED,
        id::SETTINGS_UNREADABLE,
        id::NO_CREDENTIAL_STORE,
        id::CONNECTING,
        id::CONNECTION_REFUSED,
        id::HOST_NOT_FOUND,
        id::CONNECT_TIMED_OUT,
        id::SESSION_ENDED,
        id::DISCONNECTED,
        id::TLS_FAILED,
        id::CERT_UNTRUSTED,
        id::CERT_TRUSTED,
        id::DEVICE_IN_USE,
        id::DEVICE_NOT_VALID,
        id::BASIC_5250,
        id::PROTOCOL_ERROR,
        id::SCREEN_PRINTED,
        id::SESSION_FILE_UNREADABLE,
        id::SESSION_FILE_NEWER,
        id::SYSTEM_NOT_CONFIGURED,
        id::SESSION_SAVED,
        id::KEYMAP_LOADED,
        id::KEYMAPS_MIGRATED,
        id::MACRO_STOPPED,
        id::MACRO_RECORDED,
        id::FOLDER_ADDED,
        id::INSTALLED,
        id::UPGRADED,
        id::PAYLOAD_DAMAGED,
        id::ADMIN_NEEDED,
        id::REMOVED,
        id::REPAIRED,
        id::PRINTER_CONNECTED,
        id::PRINTER_DEVICE_REQUIRED,
        id::NOT_A_PRINTER_DEVICE,
        id::MESSAGE_QUEUE_NOT_FOUND,
        id::OUTPUT_SAVED,
        id::OUTPUT_PRINTED,
        id::SYSTEM_PRINTING_UNAVAILABLE,
        id::PRINTER_ERROR,
        id::OUTPUT_FOLDER_NOT_WRITABLE,
        id::TRANSFORMED_DATA,
        id::JOB_IDLE_CLOSED,
        id::RECORD_REJECTED,
        id::JOB_INCOMPLETE,
        id::PRINTER_RECONNECTING,
    ];

    #[test]
    fn phase_3_texts() {
        let t = |id: &str, args: &[&str]| message(id, args).text;
        for (id, args, want) in [
            (
                id::PRINTER_CONNECTED,
                &["PRT01", "SYS"][..],
                "Printer session PRT01 connected to SYS.",
            ),
            (
                id::PRINTER_DEVICE_REQUIRED,
                &[],
                "Printer device name is required.",
            ),
            (
                id::NOT_A_PRINTER_DEVICE,
                &["DSP01", "SYS", "8903", "why"],
                "Device DSP01 is not a printer device on SYS.",
            ),
            (
                id::MESSAGE_QUEUE_NOT_FOUND,
                &["Q", "L", "SYS", "8930"],
                "Message queue Q in library L not found on SYS.",
            ),
            (
                id::OUTPUT_SAVED,
                &["PRT01", "f.pdf", "3"],
                "Output for PRT01 saved to f.pdf (3 pages).",
            ),
            (
                id::OUTPUT_PRINTED,
                &["PRT01", "Laser", "3"],
                "Output for PRT01 sent to printer Laser (3 pages).",
            ),
            (
                id::SYSTEM_PRINTING_UNAVAILABLE,
                &["the lp command was not found"],
                "System printing is not available: the lp command was not found.",
            ),
            (
                id::PRINTER_ERROR,
                &["Laser", "out of paper"],
                "Printer Laser reported an error: out of paper.",
            ),
            (
                id::OUTPUT_FOLDER_NOT_WRITABLE,
                &["D"],
                "Output folder D cannot be written.",
            ),
            (
                id::TRANSFORMED_DATA,
                &[],
                "Transformed print data received and discarded.",
            ),
            (
                id::JOB_IDLE_CLOSED,
                &[],
                "Print job closed after idle timeout.",
            ),
            (
                id::PRINTER_RECONNECTING,
                &["PRT01", "20"],
                "Reconnecting printer session PRT01 in 20 seconds.",
            ),
        ] {
            assert_eq!(t(id, args), want, "{id}");
            let m = message(id, args);
            assert!(!m.cause.is_empty() && !m.recovery.is_empty(), "{id}");
            assert!(
                !m.cause.contains('&') && !m.recovery.contains('&'),
                "{id}: {m:?}"
            );
        }
    }

    #[test]
    fn phase_1_session_texts() {
        let t = |id: &str, args: &[&str]| message(id, args).text;
        assert_eq!(
            t(id::CONNECTING, &["SYS", "23"]),
            "Connecting to SYS port 23."
        );
        assert_eq!(
            t(id::CONNECTION_REFUSED, &["SYS"]),
            "Connection to SYS refused."
        );
        assert_eq!(t(id::HOST_NOT_FOUND, &["SYS"]), "Host SYS not found.");
        assert_eq!(
            t(id::CONNECT_TIMED_OUT, &["SYS"]),
            "Connection to SYS timed out."
        );
        assert_eq!(
            t(id::SESSION_ENDED, &["SYS"]),
            "Session to SYS ended by the system."
        );
        assert_eq!(
            t(id::TLS_FAILED, &["SYS", "why"]),
            "TLS handshake with SYS failed: why."
        );
        assert_eq!(
            t(id::CERT_UNTRUSTED, &["SYS"]),
            "Certificate for SYS is not trusted."
        );
        assert_eq!(
            t(id::CERT_TRUSTED, &["SYS"]),
            "Certificate for SYS added to trusted list."
        );
        assert_eq!(
            t(id::DEVICE_IN_USE, &["DSP01"]),
            "Device name DSP01 is in use."
        );
        assert_eq!(
            t(id::DEVICE_NOT_VALID, &["DSP01"]),
            "Device name DSP01 is not valid."
        );
        assert_eq!(
            t(id::BASIC_5250, &[]),
            "System does not support TN5250E; using basic 5250."
        );
        assert_eq!(
            t(id::PROTOCOL_ERROR, &["SYS", "x"]),
            "Protocol error from SYS: x."
        );
        assert_eq!(
            t(id::SCREEN_PRINTED, &["f.pdf"]),
            "Screen printed to f.pdf."
        );
    }

    #[test]
    fn phase_2_texts() {
        let t = |id: &str, args: &[&str]| message(id, args).text;
        for (id, args, want) in [
            (
                id::SESSION_FILE_UNREADABLE,
                &["a.qsd", "why"][..],
                "Session file a.qsd could not be read: why.",
            ),
            (
                id::SESSION_FILE_NEWER,
                &["a.qsd", "2"],
                "Session file a.qsd was made by a newer version (2).",
            ),
            (
                id::SYSTEM_NOT_CONFIGURED,
                &["SYS"],
                "System SYS is not configured.",
            ),
            (id::SESSION_SAVED, &["a.qsd"], "Session saved to a.qsd."),
            (
                id::KEYMAP_LOADED,
                &["k.qkm"],
                "Keyboard map loaded from k.qkm.",
            ),
            (
                id::KEYMAPS_MIGRATED,
                &["2"],
                "Keyboard maps migrated for 2 systems.",
            ),
            (
                id::MACRO_STOPPED,
                &["M", "3", "timed out"],
                "Macro M stopped at step 3: timed out.",
            ),
            (
                id::MACRO_RECORDED,
                &["M", "7"],
                "Macro M recorded with 7 steps.",
            ),
            (
                id::FOLDER_ADDED,
                &["D"],
                "Folder D added to Session Manager.",
            ),
            (
                id::INSTALLED,
                &["D"],
                "QS/400 Access for Desktop installed in D.",
            ),
            (
                id::UPGRADED,
                &["V1R0M0", "V1R0M1"],
                "Upgrade from V1R0M0 to V1R0M1 complete.",
            ),
            (
                id::PAYLOAD_DAMAGED,
                &[],
                "Installer payload is damaged; download it again.",
            ),
            (
                id::ADMIN_NEEDED,
                &[],
                "Administrator rights are needed to install for all users.",
            ),
            (id::REMOVED, &[], "QS/400 Access for Desktop removed."),
            (id::REPAIRED, &[], "Repair complete."),
        ] {
            assert_eq!(t(id, args), want, "{id}");
        }
    }

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
