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
    pub const CERT_IMPORTED: &str = "QAD0040";
    pub const CERT_REMOVED: &str = "QAD0041";
    pub const CERT_EXPORTED: &str = "QAD0042";
    pub const NOT_A_CERTIFICATE: &str = "QAD0043";
    pub const CERT_EXPIRED: &str = "QAD0044";
    pub const HELP_TOPIC_NOT_FOUND: &str = "QAD0050";
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
    pub const SIGNON_OK: &str = "QAD4020";
    pub const SIGNON_FAILED: &str = "QAD4021";
    pub const PASSWORD_EXPIRED: &str = "QAD4022";
    pub const PASSWORD_CHANGED: &str = "QAD4023";
    pub const SESSION_ENDED_BY_OPERATOR: &str = "QAD4024";
    pub const QAP_SERVER_ENDING: &str = "QAD4025";
    pub const QAP_SESSION_EXPIRED: &str = "QAD4026";
    pub const PASSWORDS_DIFFER: &str = "QAD4027";
    pub const SIGNED_OFF: &str = "QAD4028";
    pub const PASSWORD_EXPIRES_SOON: &str = "QAD4031";
    pub const AUTO_SIGNON_REFUSED: &str = "QAD4040";
    pub const VERIFIED: &str = "QAD4050";
    pub const VERIFIED_WITH_WARNINGS: &str = "QAD4051";
    pub const VERIFY_FAILED: &str = "QAD4052";
    pub const QAP_NOT_REACHED: &str = "QAD4053";
    pub const QAP_TLS_FAILED: &str = "QAD4054";
    pub const QAP_VERSION_UNSUPPORTED: &str = "QAD4055";
    pub const UNEXPECTED_REPLY: &str = "QAD4060";
    pub const QAP_VERSION_NEEDED: &str = "QAD5001";
    pub const COMMAND_COMPLETED: &str = "QAD5010";
    pub const COMMAND_FAILED: &str = "QAD5011";
    pub const COMMAND_CANCELLED: &str = "QAD5012";
    pub const COMMAND_NOT_FOUND: &str = "QAD5013";
    pub const COMMAND_HAS_ERRORS: &str = "QAD5020";
    pub const AUTO_SIGNON_NEEDS_TLS: &str = "QAD5030";
    pub const SPOOLED_FILES_LISTED: &str = "QAD6001";
    pub const SPOOLED_FILE_HELD: &str = "QAD6002";
    pub const SPOOLED_FILE_RELEASED: &str = "QAD6003";
    pub const SPOOLED_FILE_DELETED: &str = "QAD6004";
    pub const SPOOLED_FILE_MOVED: &str = "QAD6005";
    pub const SPOOL_ACTIONS_FAILED: &str = "QAD6006";
    pub const SPOOL_READING: &str = "QAD6010";
    pub const SPOOL_READ_CANCELLED: &str = "QAD6011";
    pub const SPOOLED_FILE_OPEN: &str = "QAD6012";
    pub const SPOOL_FORMAT_NOT_POSSIBLE: &str = "QAD6013";
    pub const REPLY_TOO_LARGE: &str = "QAD6014";
    pub const SPOOL_SAVED: &str = "QAD6020";
    pub const SPOOL_PRINTED: &str = "QAD6021";
    pub const ATTACHED: &str = "QAD7001";
    pub const ATTACH_NEEDS_VERSION_4: &str = "QAD7002";
    pub const NO_MORE_CONNECTIONS: &str = "QAD7003";
    pub const STATEMENT_LOST: &str = "QAD7010";
    pub const RESULT_INCOMPLETE: &str = "QAD7011";
    pub const ROWS_CHANGED: &str = "QAD7020";
    pub const UNCOMMITTED_CHANGES: &str = "QAD7021";
    pub const TRANSFER_COMPLETE: &str = "QAD7030";
    pub const TRANSFER_FAILED: &str = "QAD7031";
    pub const TRANSFER_CANCELLED: &str = "QAD7032";
    pub const TRANSFER_RESUMABLE: &str = "QAD7033";
    pub const NO_COMMITMENT_CONTROL: &str = "QAD7034";
    pub const VALUE_DOES_NOT_FIT: &str = "QAD7035";
    pub const REQUEST_SAVED: &str = "QAD7040";
    pub const ENTRIES_LISTED: &str = "QAD8001";
    pub const FOLDER_CREATED: &str = "QAD8002";
    pub const RENAMED: &str = "QAD8003";
    pub const DELETED: &str = "QAD8004";
    pub const COPIED: &str = "QAD8005";
    pub const DELETE_STOPPED: &str = "QAD8006";
    pub const ALREADY_EXISTS: &str = "QAD8010";
    pub const CHANGED_ON_SYSTEM: &str = "QAD8011";
    pub const TOO_LARGE_TO_EDIT: &str = "QAD8012";
    pub const NOT_TEXT: &str = "QAD8013";
    pub const FILE_TRANSFER_COMPLETE: &str = "QAD8020";
    pub const FILE_TRANSFER_FAILED: &str = "QAD8021";
    pub const FILE_TRANSFER_CANCELLED: &str = "QAD8022";
    pub const RESUMING: &str = "QAD8023";
    pub const SHARED_FOR_TRANSFERS: &str = "QAD8030";
    pub const FOREIGN_PEER_REFUSED: &str = "QAD8040";
    pub const MESSAGES_LISTED: &str = "QAD9001";
    pub const REPLY_SENT: &str = "QAD9002";
    pub const MESSAGES_REMOVED: &str = "QAD9003";
    pub const MESSAGE_SENT: &str = "QAD9004";
    pub const REPLY_RECEIVED: &str = "QAD9005";
    pub const WATCHING: &str = "QAD9010";
    pub const WATCH_STOPPED: &str = "QAD9011";
    pub const WATCH_NOT_AUTHORIZED: &str = "QAD9012";
    pub const CAUGHT_UP: &str = "QAD9013";
    pub const SPOOL_LIVE: &str = "QAD9020";
    pub const SPOOL_POLLING: &str = "QAD9021";
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

    const SEEDS: [&str; 135] = [
        id::NOT_AVAILABLE,
        id::SYSTEM_ADDED,
        id::SYSTEM_CHANGED,
        id::SYSTEM_DELETED,
        id::SYSTEMS_IMPORTED,
        id::SYSTEMS_EXPORTED,
        id::SETTINGS_UNREADABLE,
        id::CERT_IMPORTED,
        id::CERT_REMOVED,
        id::CERT_EXPORTED,
        id::NOT_A_CERTIFICATE,
        id::CERT_EXPIRED,
        id::HELP_TOPIC_NOT_FOUND,
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
        id::SIGNON_OK,
        id::SIGNON_FAILED,
        id::PASSWORD_EXPIRED,
        id::PASSWORD_CHANGED,
        id::SESSION_ENDED_BY_OPERATOR,
        id::QAP_SERVER_ENDING,
        id::QAP_SESSION_EXPIRED,
        id::PASSWORDS_DIFFER,
        id::SIGNED_OFF,
        id::PASSWORD_EXPIRES_SOON,
        id::AUTO_SIGNON_REFUSED,
        id::VERIFIED,
        id::VERIFIED_WITH_WARNINGS,
        id::VERIFY_FAILED,
        id::QAP_NOT_REACHED,
        id::QAP_TLS_FAILED,
        id::QAP_VERSION_UNSUPPORTED,
        id::UNEXPECTED_REPLY,
        id::QAP_VERSION_NEEDED,
        id::COMMAND_COMPLETED,
        id::COMMAND_FAILED,
        id::COMMAND_CANCELLED,
        id::COMMAND_NOT_FOUND,
        id::COMMAND_HAS_ERRORS,
        id::AUTO_SIGNON_NEEDS_TLS,
        id::SPOOLED_FILES_LISTED,
        id::SPOOLED_FILE_HELD,
        id::SPOOLED_FILE_RELEASED,
        id::SPOOLED_FILE_DELETED,
        id::SPOOLED_FILE_MOVED,
        id::SPOOL_ACTIONS_FAILED,
        id::SPOOL_READING,
        id::SPOOL_READ_CANCELLED,
        id::SPOOLED_FILE_OPEN,
        id::SPOOL_FORMAT_NOT_POSSIBLE,
        id::REPLY_TOO_LARGE,
        id::SPOOL_SAVED,
        id::SPOOL_PRINTED,
        id::ATTACHED,
        id::ATTACH_NEEDS_VERSION_4,
        id::NO_MORE_CONNECTIONS,
        id::STATEMENT_LOST,
        id::RESULT_INCOMPLETE,
        id::ROWS_CHANGED,
        id::UNCOMMITTED_CHANGES,
        id::TRANSFER_COMPLETE,
        id::TRANSFER_FAILED,
        id::TRANSFER_CANCELLED,
        id::TRANSFER_RESUMABLE,
        id::NO_COMMITMENT_CONTROL,
        id::VALUE_DOES_NOT_FIT,
        id::REQUEST_SAVED,
        id::ENTRIES_LISTED,
        id::FOLDER_CREATED,
        id::RENAMED,
        id::DELETED,
        id::COPIED,
        id::DELETE_STOPPED,
        id::ALREADY_EXISTS,
        id::CHANGED_ON_SYSTEM,
        id::TOO_LARGE_TO_EDIT,
        id::NOT_TEXT,
        id::FILE_TRANSFER_COMPLETE,
        id::FILE_TRANSFER_FAILED,
        id::FILE_TRANSFER_CANCELLED,
        id::RESUMING,
        id::SHARED_FOR_TRANSFERS,
        id::FOREIGN_PEER_REFUSED,
        id::MESSAGES_LISTED,
        id::REPLY_SENT,
        id::MESSAGES_REMOVED,
        id::MESSAGE_SENT,
        id::REPLY_RECEIVED,
        id::WATCHING,
        id::WATCH_STOPPED,
        id::WATCH_NOT_AUTHORIZED,
        id::CAUGHT_UP,
        id::SPOOL_LIVE,
        id::SPOOL_POLLING,
    ];

    #[test]
    fn phase_9_texts() {
        let t = |id: &str, args: &[&str]| message(id, args).text;
        for (id, args, want) in [
            (
                id::MESSAGES_LISTED,
                &["149", "QSYS/QSYSOPR"][..],
                "149 messages on QSYS/QSYSOPR.",
            ),
            (id::REPLY_SENT, &["CPA0701"], "Reply sent to CPA0701."),
            (
                id::MESSAGES_REMOVED,
                &["2", "QUSRSYS/BOB"],
                "2 messages removed from QUSRSYS/BOB.",
            ),
            (id::MESSAGE_SENT, &["ANN"], "Message sent to ANN."),
            (
                id::REPLY_RECEIVED,
                &["ANN", "Yes"],
                "Reply received from ANN: Yes.",
            ),
            (
                id::WATCHING,
                &["QSYS/QSYSOPR", "SYS"],
                "Watching QSYS/QSYSOPR on SYS.",
            ),
            (
                id::WATCH_STOPPED,
                &["QSYS/QSYSOPR", "SYS", "CPF9802: Not authorized"],
                "Watching QSYS/QSYSOPR on SYS stopped: CPF9802: Not authorized.",
            ),
            (
                id::WATCH_NOT_AUTHORIZED,
                &["QSYS/QSYSOPR", "SYS"],
                "Not authorized to QSYS/QSYSOPR on SYS; it will not be watched.",
            ),
            (
                id::CAUGHT_UP,
                &["3", "QUSRSYS/BOB"],
                "Caught up 3 messages on QUSRSYS/BOB after reconnecting.",
            ),
            (
                id::SPOOL_LIVE,
                &[],
                "Printer Output is updating as changes happen.",
            ),
            (
                id::SPOOL_POLLING,
                &["30"],
                "Printer Output is refreshing every 30 seconds.",
            ),
        ] {
            assert_eq!(t(id, args), want, "{id}");
            let m = message(id, args);
            assert!(!m.cause.is_empty() && !m.recovery.is_empty(), "{id}");
            assert!(!m.text.contains('&'), "{id}");
            assert!(!m.cause.contains('&') && !m.recovery.contains('&'), "{id}");
        }
        let m = |id: &str, args: &[&str]| message(id, args);
        assert!(
            m(id::MESSAGES_REMOVED, &["1", "Q"])
                .cause
                .contains("gets no reply")
        );
        assert!(m(id::WATCHING, &["Q", "S"]).cause.contains("break mode"));
        assert!(
            m(id::WATCH_NOT_AUTHORIZED, &["Q", "S"])
                .cause
                .contains("Messages > Options")
        );
        assert_eq!(m(id::WATCH_STOPPED, &["1", "2", "3"]).kind(), Kind::Error);
        assert_eq!(
            m(id::WATCH_NOT_AUTHORIZED, &["1", "2"]).kind(),
            Kind::Warning
        );
        for id in [
            id::MESSAGES_LISTED,
            id::REPLY_RECEIVED,
            id::CAUGHT_UP,
            id::SPOOL_LIVE,
        ] {
            assert_eq!(m(id, &["1", "2"]).kind(), Kind::Information, "{id}");
        }
    }

    #[test]
    fn phase_7_texts() {
        let t = |id: &str, args: &[&str]| message(id, args).text;
        for (id, args, want) in [
            (
                id::ATTACHED,
                &["SYS", "Run SQL", "000123/BOB/QAPCNN"][..],
                "Connected to SYS for Run SQL (job 000123/BOB/QAPCNN).",
            ),
            (
                id::ATTACH_NEEDS_VERSION_4,
                &["Run Command"],
                "Run Command needs its own connection, and the server is older than version 4.",
            ),
            (
                id::NO_MORE_CONNECTIONS,
                &["SYS"],
                "No more connections allowed to SYS. Close a window and try again.",
            ),
            (
                id::STATEMENT_LOST,
                &["SYS"],
                "The connection to SYS was lost; the statement must be run again.",
            ),
            (
                id::RESULT_INCOMPLETE,
                &["cancelled"],
                "Result is incomplete: cancelled.",
            ),
            (id::ROWS_CHANGED, &["12"], "12 rows changed."),
            (
                id::UNCOMMITTED_CHANGES,
                &["Run SQL - SYS"],
                "Uncommitted changes in Run SQL - SYS. Commit, roll back, or cancel.",
            ),
            (
                id::TRANSFER_COMPLETE,
                &["ASSETS.csv", "12", "0.4 s"],
                "Transfer ASSETS.csv complete: 12 rows in 0.4 s.",
            ),
            (
                id::TRANSFER_FAILED,
                &[
                    "up.qdx",
                    "7",
                    "8",
                    "a.csv",
                    "SQL0803: Duplicate key value specified",
                ],
                "Transfer up.qdx failed at row 7 (line 8 of a.csv): SQL0803: Duplicate key value specified.",
            ),
            (
                id::TRANSFER_CANCELLED,
                &["down.qdx"],
                "Transfer down.qdx was cancelled; nothing was kept.",
            ),
            (
                id::TRANSFER_RESUMABLE,
                &["up.qdx", "200", "201"],
                "200 rows committed before the failure; resume from row 201.",
            ),
            (
                id::NO_COMMITMENT_CONTROL,
                &["HLTEST.NOJRN"],
                "HLTEST.NOJRN cannot use commitment control; uploading without it.",
            ),
            (
                id::VALUE_DOES_NOT_FIT,
                &["COST", "123456789.123", "DECIMAL(9,2)"],
                "Column COST: value 123456789.123 does not fit DECIMAL(9,2).",
            ),
            (
                id::REQUEST_SAVED,
                &["C:\\x\\up.qdx"],
                "Request saved to C:\\x\\up.qdx.",
            ),
        ] {
            assert_eq!(t(id, args), want, "{id}");
            let m = message(id, args);
            assert!(!m.cause.is_empty() && !m.recovery.is_empty(), "{id}");
            assert!(!m.text.contains('&'), "{id}");
            assert!(!m.cause.contains('&') && !m.recovery.contains('&'), "{id}");
        }
        let m = |id: &str, args: &[&str]| message(id, args);
        assert!(m(id::NO_MORE_CONNECTIONS, &["S"]).cause.contains("TCP1A59"));
        assert!(m(id::NO_MORE_CONNECTIONS, &["S"]).cause.contains("16"));
        assert!(
            m(id::STATEMENT_LOST, &["S"])
                .cause
                .contains("not run again")
        );
        assert!(
            m(id::ATTACHED, &["S", "F", "J"])
                .recovery
                .contains("does not sign off")
        );
        assert!(m(id::NO_COMMITMENT_CONTROL, &["T"]).cause.contains("55019"));
        assert!(
            m(id::TRANSFER_RESUMABLE, &["T", "2", "3"])
                .cause
                .contains("first row not committed")
        );
        assert!(m(id::REQUEST_SAVED, &["F"]).cause.contains("never saved"));
        for id in [
            id::NO_MORE_CONNECTIONS,
            id::STATEMENT_LOST,
            id::TRANSFER_FAILED,
        ] {
            assert_eq!(
                m(id, &["1", "2", "3", "4", "5"]).kind(),
                Kind::Error,
                "{id}"
            );
        }
        for id in [
            id::RESULT_INCOMPLETE,
            id::UNCOMMITTED_CHANGES,
            id::NO_COMMITMENT_CONTROL,
        ] {
            assert_eq!(m(id, &["1"]).kind(), Kind::Warning, "{id}");
        }
        assert_eq!(m(id::ROWS_CHANGED, &["1"]).kind(), Kind::Information);
    }

    #[test]
    fn phase_8_texts() {
        let t = |id: &str, args: &[&str]| message(id, args).text;
        for (id, args, want) in [
            (
                id::ENTRIES_LISTED,
                &["1200", "/home/BOB/many"][..],
                "1200 entries in /home/BOB/many.",
            ),
            (
                id::FOLDER_CREATED,
                &["/home/BOB/new"],
                "Folder /home/BOB/new created.",
            ),
            (id::RENAMED, &["a.txt", "b.txt"], "a.txt renamed to b.txt."),
            (
                id::DELETED,
                &["/home/BOB/a.txt"],
                "/home/BOB/a.txt deleted.",
            ),
            (
                id::COPIED,
                &["a.txt", "/home/ANN"],
                "a.txt copied to /home/ANN.",
            ),
            (
                id::DELETE_STOPPED,
                &[
                    "/home/BOB/tree",
                    "/home/BOB/tree/src/lib/m.txt",
                    "CPFA09E: Object in use",
                ],
                "Deleting /home/BOB/tree stopped at /home/BOB/tree/src/lib/m.txt: CPFA09E: Object in use.",
            ),
            (
                id::ALREADY_EXISTS,
                &["a.txt", "/home/BOB"],
                "a.txt already exists in /home/BOB.",
            ),
            (
                id::CHANGED_ON_SYSTEM,
                &["/home/BOB/a.txt"],
                "/home/BOB/a.txt changed on the system since you opened it.",
            ),
            (
                id::TOO_LARGE_TO_EDIT,
                &["big.bin"],
                "big.bin is too large to edit here; download it instead.",
            ),
            (
                id::NOT_TEXT,
                &["binary.dat"],
                "binary.dat is not a text file; opened read-only.",
            ),
            (
                id::FILE_TRANSFER_COMPLETE,
                &["big.bin", "200.0 MiB", "12.3 s"],
                "Transfer of big.bin complete: 200.0 MiB in 12.3 s.",
            ),
            (
                id::FILE_TRANSFER_FAILED,
                &["a.txt", "CPFA09E: Object in use"],
                "Transfer of a.txt failed: CPFA09E: Object in use.",
            ),
            (
                id::FILE_TRANSFER_CANCELLED,
                &["a.txt"],
                "Transfer of a.txt cancelled.",
            ),
            (
                id::RESUMING,
                &["big.bin", "64.0 MiB"],
                "Resuming big.bin at 64.0 MiB.",
            ),
            (
                id::SHARED_FOR_TRANSFERS,
                &[],
                "No connection available for transfers; using the shared connection.",
            ),
            (
                id::FOREIGN_PEER_REFUSED,
                &[],
                "A connection to this program from another user was refused.",
            ),
        ] {
            assert_eq!(t(id, args), want, "{id}");
            let m = message(id, args);
            assert!(!m.cause.is_empty() && !m.recovery.is_empty(), "{id}");
            assert!(!m.text.contains('&'), "{id}");
            assert!(!m.cause.contains('&') && !m.recovery.contains('&'), "{id}");
        }
        let m = |id: &str, args: &[&str]| message(id, args);
        assert!(m(id::SHARED_FOR_TRANSFERS, &[]).cause.contains("TCP1A59"));
        assert!(
            m(id::FILE_TRANSFER_FAILED, &["a", "b"])
                .recovery
                .contains("starts again")
        );
        assert!(m(id::TOO_LARGE_TO_EDIT, &["a"]).cause.contains("64 MiB"));
        assert!(m(id::NOT_TEXT, &["a"]).cause.contains("8 MiB"));
        for id in [id::DELETE_STOPPED, id::FOREIGN_PEER_REFUSED] {
            assert_eq!(m(id, &["1", "2", "3"]).kind(), Kind::Error, "{id}");
        }
        for id in [
            id::ALREADY_EXISTS,
            id::CHANGED_ON_SYSTEM,
            id::NOT_TEXT,
            id::SHARED_FOR_TRANSFERS,
        ] {
            assert_eq!(m(id, &["1", "2"]).kind(), Kind::Warning, "{id}");
        }
        assert_eq!(m(id::ENTRIES_LISTED, &["1", "2"]).kind(), Kind::Information);
    }

    #[test]
    fn phase_6_texts() {
        let t = |id: &str, args: &[&str]| message(id, args).text;
        for (id, args, want) in [
            (
                id::SPOOLED_FILES_LISTED,
                &["60", "SYS"][..],
                "60 spooled files on SYS.",
            ),
            (
                id::SPOOLED_FILE_HELD,
                &["QSYSPRT"],
                "Spooled file QSYSPRT held.",
            ),
            (
                id::SPOOLED_FILE_RELEASED,
                &["QSYSPRT"],
                "Spooled file QSYSPRT released.",
            ),
            (
                id::SPOOLED_FILE_DELETED,
                &["QSYSPRT"],
                "Spooled file QSYSPRT deleted.",
            ),
            (
                id::SPOOLED_FILE_MOVED,
                &["QSYSPRT", "QGPL/PRT01"],
                "Spooled file QSYSPRT moved to QGPL/PRT01.",
            ),
            (
                id::SPOOL_ACTIONS_FAILED,
                &["2", "5"],
                "2 of 5 actions failed; see the details.",
            ),
            (
                id::SPOOL_READING,
                &["QSYSPRT", "25", "30"],
                "Reading QSYSPRT: page 25 of 30.",
            ),
            (
                id::SPOOL_READ_CANCELLED,
                &["QSYSPRT"],
                "Reading QSYSPRT cancelled.",
            ),
            (
                id::SPOOLED_FILE_OPEN,
                &["OPENFILE"],
                "Spooled file OPENFILE is still being written.",
            ),
            (
                id::SPOOL_FORMAT_NOT_POSSIBLE,
                &["AFPINV", "PDF"],
                "Spooled file AFPINV cannot be shown as PDF.",
            ),
            (
                id::REPLY_TOO_LARGE,
                &["SYS"],
                "Reply from SYS was larger than the program allows.",
            ),
            (
                id::SPOOL_SAVED,
                &["QSYSPRT", "C:\\out\\QSYSPRT.pdf"],
                "QSYSPRT saved to C:\\out\\QSYSPRT.pdf.",
            ),
            (
                id::SPOOL_PRINTED,
                &["QSYSPRT", "Office"],
                "QSYSPRT sent to printer Office.",
            ),
        ] {
            assert_eq!(t(id, args), want, "{id}");
            let m = message(id, args);
            assert!(!m.cause.is_empty() && !m.recovery.is_empty(), "{id}");
            // Every variable the text has is filled.
            assert!(!m.text.contains('&'), "{id}");
        }
        // The second-level texts say what the case is.
        let m = |id: &str, args: &[&str]| message(id, args);
        assert!(
            m(id::SPOOLED_FILES_LISTED, &["1", "S"])
                .cause
                .contains("page shown")
        );
        assert!(
            m(id::SPOOL_ACTIONS_FAILED, &["1", "2"])
                .cause
                .contains("as it sent it")
        );
        assert!(
            m(id::SPOOL_READ_CANCELLED, &["F"])
                .cause
                .contains("discarded")
        );
        assert!(m(id::REPLY_TOO_LARGE, &["S"]).cause.contains("64 MiB"));
        assert!(m(id::REPLY_TOO_LARGE, &["S"]).cause.contains("resumes"));
        assert!(
            m(id::SPOOLED_FILE_OPEN, &["F"])
                .recovery
                .contains("Refresh")
        );
        assert!(
            m(id::SPOOLED_FILE_HELD, &["F"])
                .recovery
                .contains("Release")
        );
        for id in [id::SPOOL_ACTIONS_FAILED, id::REPLY_TOO_LARGE] {
            assert_eq!(m(id, &["1", "2"]).kind(), Kind::Error, "{id}");
        }
        assert_eq!(m(id::SPOOL_READ_CANCELLED, &["F"]).kind(), Kind::Warning);
    }

    #[test]
    fn phase_5_texts() {
        let t = |id: &str, args: &[&str]| message(id, args).text;
        for (id, args, want) in [
            (
                id::QAP_VERSION_NEEDED,
                &["Run Command", "2", "SYS", "1"][..],
                "Run Command needs QAP version 2; system SYS has version 1.",
            ),
            (
                id::COMMAND_COMPLETED,
                &["CRTLIB"],
                "Command CRTLIB completed.",
            ),
            (
                id::COMMAND_FAILED,
                &["CRTLIB"],
                "Command CRTLIB ended with an error.",
            ),
            (
                id::COMMAND_CANCELLED,
                &["DLYJOB"],
                "Command DLYJOB was cancelled.",
            ),
            (
                id::COMMAND_NOT_FOUND,
                &["NOSUCH"],
                "Command NOSUCH not found.",
            ),
            (
                id::COMMAND_HAS_ERRORS,
                &["DSPOBJD"],
                "Command DSPOBJD has errors; see the messages.",
            ),
            (
                id::AUTO_SIGNON_NEEDS_TLS,
                &[],
                "Automatic sign-on needs Telnet over TLS.",
            ),
            (
                id::PASSWORD_EXPIRES_SOON,
                &["QATEST", "SYS", "3"],
                "Password for QATEST on SYS expires in 3 days.",
            ),
            (
                id::AUTO_SIGNON_REFUSED,
                &["SYS"],
                "Automatic sign-on to SYS was not accepted.",
            ),
        ] {
            assert_eq!(t(id, args), want, "{id}");
            let m = message(id, args);
            assert!(!m.cause.is_empty() && !m.recovery.is_empty(), "{id}");
        }
        let r = message(id::AUTO_SIGNON_REFUSED, &["SYS"]).recovery;
        assert!(
            r.contains("QRMTSIGN") && r.contains("*VERIFY") && r.contains("TLS"),
            "{r}"
        );
        assert!(
            message(id::PASSWORD_EXPIRES_SOON, &["U", "S", "1"])
                .recovery
                .contains("Change Password...")
        );
    }

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
                &["OLD", "NEW"],
                "Upgrade from OLD to NEW complete.",
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
    }

    #[test]
    fn certificate_management_texts() {
        let t = |id: &str, args: &[&str]| message(id, args).text;
        for (id, args, want) in [
            (
                id::CERT_IMPORTED,
                &["Example Root", "2"][..],
                "Certificate Example Root imported for 2 systems.",
            ),
            (
                id::CERT_REMOVED,
                &["Example Root", "3"],
                "Certificate Example Root removed from 3 systems.",
            ),
            (
                id::CERT_EXPORTED,
                &["Example Root", "root.pem"],
                "Certificate Example Root exported to root.pem.",
            ),
            (
                id::NOT_A_CERTIFICATE,
                &["notes.txt"],
                "File notes.txt does not contain a certificate.",
            ),
            (
                id::CERT_EXPIRED,
                &["Example Root", "2025-01-01 00:00:00 UTC"],
                "Certificate Example Root has expired.",
            ),
        ] {
            assert_eq!(t(id, args), want);
        }
        let m = message(
            id::CERT_EXPIRED,
            &["Example Root", "2025-01-01 00:00:00 UTC"],
        );
        assert_eq!(m.kind(), Kind::Warning);
        assert!(m.cause.contains("2025-01-01 00:00:00 UTC"));
        assert_eq!(message(id::NOT_A_CERTIFICATE, &["f"]).kind(), Kind::Error);
    }

    #[test]
    fn help_texts() {
        let m = message(id::HELP_TOPIC_NOT_FOUND, &["run-sql"]);
        assert_eq!(m.text, "Help topic run-sql not found.");
        assert_eq!(m.kind(), Kind::Error);
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
