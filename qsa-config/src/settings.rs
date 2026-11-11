// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! Application settings (`settings.toml`).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// How many sessions File > Recent Sessions lists.
pub const RECENT_MAX: usize = 10;

/// Position and size of a window.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Geometry {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

/// Screen size of a 5250 display session.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ScreenSize {
    #[default]
    #[serde(rename = "24x80")]
    S24x80,
    #[serde(rename = "27x132")]
    S27x132,
}

impl ScreenSize {
    /// Rows and columns.
    pub fn rows_cols(self) -> (usize, usize) {
        match self {
            ScreenSize::S24x80 => (24, 80),
            ScreenSize::S27x132 => (27, 132),
        }
    }

    /// As shown to the user: `24 x 80`.
    pub fn label(self) -> &'static str {
        match self {
            ScreenSize::S24x80 => "24 x 80",
            ScreenSize::S27x132 => "27 x 132",
        }
    }
}

/// Colour map of a 5250 display session.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ColorMap {
    /// The QS/400 service console's colours.
    #[default]
    Console,
    BlackOnWhite,
}

/// The CCSIDs a 5250 display session can use.
pub const CCSIDS: &[u16] = &[37, 273, 285, 297, 500, 1140];

/// Checks a device name: empty, or 1 to 10 of A-Z, 0-9, #, $, @ and _,
/// starting with a letter, #, $ or @. Returns it in upper case.
pub fn check_device(name: &str) -> Result<String, String> {
    let n = name.trim().to_ascii_uppercase();
    if n.is_empty() {
        return Ok(n);
    }
    let ok = n.len() <= 10
        && n.chars()
            .all(|c| c.is_ascii_alphanumeric() || "#$@_".contains(c))
        && n.chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || "#$@".contains(c));
    if ok {
        Ok(n)
    } else {
        Err(
            "Device name must be 1 to 10 letters, digits, #, $, @ or _, starting with a letter, #, $ or @."
                .into(),
        )
    }
}

/// 5250 display settings for one system: the Start 5250 Display dialog's
/// last values.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Display {
    pub screen_size: ScreenSize,
    pub ccsid: u16,
    /// Empty: the system chooses.
    pub device_name: String,
    pub color_map: ColorMap,
    /// A keyboard map kept per system by an earlier release. It is read so
    /// that [`crate::migrate`] can move it into session files, and is never
    /// written back once that is done.
    #[serde(rename = "keymap", skip_serializing_if = "Option::is_none")]
    pub legacy_keymap: Option<crate::KeyMap>,
}

impl Default for Display {
    fn default() -> Display {
        Display {
            screen_size: ScreenSize::S24x80,
            ccsid: 37,
            device_name: String::new(),
            color_map: ColorMap::Console,
            legacy_keymap: None,
        }
    }
}

/// Application settings. Every field has a default, so older and partial
/// files load.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub version: u32,
    /// The system selected when the program last closed.
    pub last_system: String,
    /// Main window geometry when the program last closed.
    pub window: Option<Geometry>,
    /// 5250 display settings by system name.
    pub display: std::collections::BTreeMap<String, Display>,
    /// Session files opened or saved most recently, newest first.
    pub recent_sessions: Vec<PathBuf>,
    /// Folders Session Manager lists besides the Sessions folder.
    pub session_folders: Vec<PathBuf>,
    /// When each session file was last started, as Unix seconds, by path
    /// (see [`path_key`]).
    pub last_used: std::collections::BTreeMap<String, i64>,
    /// The keyboard map new sessions start from, as changes from the
    /// built-in defaults. `None`: the defaults.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub new_session_keymap: Option<crate::qsd::KeymapOverrides>,
    /// Commands run with Run Command, by system name in lower case, oldest
    /// first, at most [`COMMAND_HISTORY_MAX`] each. A command that names a
    /// password is never kept ([`Settings::add_command`]).
    #[serde(skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub command_history: std::collections::BTreeMap<String, Vec<String>>,
    /// Printer Output's list and output.
    pub printer_output: PrinterOutput,
    /// Run SQL's history, recent scripts and options.
    pub run_sql: RunSql,
    /// What Messages watches, and what opens Message Arrived, per system.
    pub messages: Messages,
}

/// What a message arriving on a watched queue does.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Arrival {
    /// An inquiry waiting for a reply opens Message Arrived.
    #[default]
    InquiriesOnly,
    /// Every message opens it (a sender's copy and a reply never do).
    All,
    /// Nothing opens it; Messages still shows the message.
    None,
}

impl Arrival {
    /// Every choice, in the order Options lists them.
    pub const ALL: [Arrival; 3] = [Arrival::InquiriesOnly, Arrival::All, Arrival::None];

    /// Its label in Options.
    pub fn label(self) -> &'static str {
        match self {
            Arrival::InquiriesOnly => "Inquiries only",
            Arrival::All => "All messages",
            Arrival::None => "None",
        }
    }
}

/// One system's message options.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct MessageOptions {
    /// What arrivals on the signed-on profile's own queue do. The own
    /// queue is always watched while signed on.
    pub own_arrival: Arrival,
    /// Watch `QSYSOPR` too. Off by default; offered only when the profile
    /// may read it, and turned off when it may not.
    pub operator: bool,
    /// What arrivals on `QSYSOPR` do while it is watched.
    pub operator_arrival: Arrival,
}

impl Default for MessageOptions {
    fn default() -> MessageOptions {
        MessageOptions {
            own_arrival: Arrival::InquiriesOnly,
            operator: false,
            operator_arrival: Arrival::InquiriesOnly,
        }
    }
}

/// `[messages]`: each system's [`MessageOptions`], by system name in lower
/// case; a system with the defaults is not written.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Messages {
    #[serde(skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub systems: std::collections::BTreeMap<String, MessageOptions>,
}

impl Messages {
    /// `system`'s options.
    pub fn for_system(&self, system: &str) -> MessageOptions {
        self.systems
            .get(&system.to_ascii_lowercase())
            .copied()
            .unwrap_or_default()
    }

    /// Sets `system`'s options.
    pub fn set(&mut self, system: &str, o: MessageOptions) {
        let k = system.to_ascii_lowercase();
        if o == MessageOptions::default() {
            self.systems.remove(&k);
        } else {
            self.systems.insert(k, o);
        }
    }
}

/// Statements Run SQL keeps per system.
pub const SQL_HISTORY_MAX: usize = 100;

/// `[run_sql]`: what Run SQL keeps between runs.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RunSql {
    /// Statements run, by system name in lower case, oldest first, at most
    /// [`SQL_HISTORY_MAX`] each. One that names a password is never kept.
    #[serde(skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub history: std::collections::BTreeMap<String, Vec<String>>,
    /// `.sql` files opened or saved most recently, newest first.
    pub recent: Vec<PathBuf>,
    /// Systems, in lower case, whose CCSID 65535 character data shows as
    /// text in the job's CCSID rather than as hex.
    #[serde(skip_serializing_if = "std::collections::BTreeSet::is_empty")]
    pub bit_data_as_text: std::collections::BTreeSet<String>,
    /// Run All stops at the first statement that fails.
    pub stop_on_error: bool,
}

impl RunSql {
    /// `system`'s statements, oldest first.
    pub fn history(&self, system: &str) -> &[String] {
        self.history
            .get(&system.to_ascii_lowercase())
            .map_or(&[], Vec::as_slice)
    }

    /// Keeps `statement` as `system`'s newest, once, dropping the oldest
    /// past [`SQL_HISTORY_MAX`]. A statement that names a password (the
    /// word `PASSWORD`, or a CL password parameter in a command it runs)
    /// is not kept. False when nothing was kept.
    pub fn add(&mut self, system: &str, statement: &str) -> bool {
        let s = statement.trim();
        if s.is_empty() || names_password(s) || s.to_ascii_uppercase().contains("PASSWORD") {
            return false;
        }
        let list = self.history.entry(system.to_ascii_lowercase()).or_default();
        list.retain(|c| c != s);
        list.push(s.to_owned());
        let over = list.len().saturating_sub(SQL_HISTORY_MAX);
        list.drain(..over);
        true
    }

    /// Puts `path` at the top of the recent scripts, keeping at most
    /// [`RECENT_MAX`].
    pub fn add_recent(&mut self, path: &Path) {
        self.recent.retain(|p| !same_path(p, path));
        self.recent.insert(0, path.to_owned());
        self.recent.truncate(RECENT_MAX);
    }

    /// Whether `system` shows CCSID 65535 data as text.
    pub fn bit_data_as_text(&self, system: &str) -> bool {
        self.bit_data_as_text.contains(&system.to_ascii_lowercase())
    }

    /// Sets whether `system` shows CCSID 65535 data as text.
    pub fn set_bit_data_as_text(&mut self, system: &str, on: bool) {
        let k = system.to_ascii_lowercase();
        if on {
            self.bit_data_as_text.insert(k);
        } else {
            self.bit_data_as_text.remove(&k);
        }
    }
}

/// Rows a page of Printer Output's lists may show.
pub const ROWS_PER_PAGE: [u32; 3] = [25, 50, 100];

/// Printer Output's auto-refresh choices, in seconds; 0 is off.
pub const AUTO_REFRESH: [u32; 4] = [0, 15, 30, 60];

/// `[printer_output]`: how Printer Output lists spooled files and lays
/// them out.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PrinterOutput {
    /// Rows a page of the list shows: one of [`ROWS_PER_PAGE`].
    pub rows_per_page: u32,
    /// Seconds between refreshes of the page shown while the window is
    /// visible: one of [`AUTO_REFRESH`], 0 for off.
    pub auto_refresh_secs: u32,
    /// Paper for files whose data sets no page size, as a 5250 printer
    /// session's `[output] paper`.
    pub paper: crate::qsp::Paper,
}

impl Default for PrinterOutput {
    fn default() -> PrinterOutput {
        PrinterOutput {
            rows_per_page: 25,
            auto_refresh_secs: 30,
            paper: crate::qsp::Paper::Letter,
        }
    }
}

impl PrinterOutput {
    /// Rows per page, the nearest allowed value to what the file says.
    pub fn rows(&self) -> u32 {
        nearest(&ROWS_PER_PAGE, self.rows_per_page)
    }

    /// The auto-refresh interval, the nearest allowed value; 0 for off.
    pub fn refresh_secs(&self) -> u32 {
        nearest(&AUTO_REFRESH, self.auto_refresh_secs)
    }
}

/// The value of `allowed` nearest to `v`, the smaller on a tie.
fn nearest(allowed: &[u32], v: u32) -> u32 {
    allowed
        .iter()
        .copied()
        .min_by_key(|a| a.abs_diff(v))
        .unwrap_or(v)
}

/// Commands Run Command keeps per system.
pub const COMMAND_HISTORY_MAX: usize = 50;

/// Whether a CL command string gives a parameter that holds a password
/// (`PASSWORD(...)`, `PWD(...)`, `NEWPWD(...)`, `CURPWD(...)`, ...): its
/// keyword contains `PASSWORD` or ends in `PWD` or `PW`.
pub fn names_password(command: &str) -> bool {
    password_keywords(command).next().is_some()
}

/// The byte ranges of the values of `command`'s password parameters.
fn password_keywords(command: &str) -> impl Iterator<Item = (usize, usize)> + '_ {
    let b = command.as_bytes();
    let mut i = 0;
    std::iter::from_fn(move || {
        while i < b.len() {
            if b[i] == b'(' {
                let mut k = i;
                while k > 0 && (b[k - 1].is_ascii_alphanumeric() || b[k - 1] == b'_') {
                    k -= 1;
                }
                let kwd = command[k..i].to_ascii_uppercase();
                let open = i;
                // The value ends at the matching parenthesis, quotes kept.
                let (mut depth, mut quoted, mut j) = (0usize, false, i);
                while j < b.len() {
                    match b[j] {
                        b'\'' => quoted = !quoted,
                        b'(' if !quoted => depth += 1,
                        b')' if !quoted => {
                            depth -= 1;
                            if depth == 0 {
                                break;
                            }
                        }
                        _ => {}
                    }
                    j += 1;
                }
                i = j + 1;
                if kwd.contains("PASSWORD") || kwd.ends_with("PWD") || kwd.ends_with("PW") {
                    return Some((open + 1, j.min(b.len())));
                }
            } else {
                i += 1;
            }
        }
        None
    })
}

/// `command` with the values of its password parameters replaced by `*`,
/// for logs.
pub fn redact_command(command: &str) -> String {
    let mut out = String::with_capacity(command.len());
    let mut at = 0;
    for (from, to) in password_keywords(command) {
        out.push_str(&command[at..from]);
        out.push('*');
        at = to;
    }
    out.push_str(&command[at..]);
    out
}

impl Default for Settings {
    fn default() -> Settings {
        Settings {
            version: 1,
            last_system: String::new(),
            window: None,
            display: Default::default(),
            recent_sessions: Vec::new(),
            session_folders: Vec::new(),
            last_used: Default::default(),
            new_session_keymap: None,
            command_history: Default::default(),
            printer_output: PrinterOutput::default(),
            run_sql: RunSql::default(),
            messages: Messages::default(),
        }
    }
}

/// A path as a key of [`Settings::last_used`]: lower case on Windows, where
/// case does not matter.
pub fn path_key(p: &Path) -> String {
    let s = p.display().to_string();
    if cfg!(windows) { s.to_lowercase() } else { s }
}

/// Whether two paths name the same file, ignoring case on Windows.
pub fn same_path(a: &Path, b: &Path) -> bool {
    if cfg!(windows) {
        a.to_string_lossy()
            .eq_ignore_ascii_case(&b.to_string_lossy())
    } else {
        a == b
    }
}

impl Settings {
    /// `system`'s Run Command history, oldest first.
    pub fn commands(&self, system: &str) -> &[String] {
        self.command_history
            .get(&system.to_ascii_lowercase())
            .map_or(&[], Vec::as_slice)
    }

    /// Keeps `command` as `system`'s newest, once, dropping the oldest past
    /// [`COMMAND_HISTORY_MAX`]. A command that names a password is not kept.
    /// False when nothing was kept.
    pub fn add_command(&mut self, system: &str, command: &str) -> bool {
        let command = command.trim();
        if command.is_empty() || names_password(command) {
            return false;
        }
        let list = self
            .command_history
            .entry(system.to_ascii_lowercase())
            .or_default();
        list.retain(|c| c != command);
        list.push(command.to_owned());
        let over = list.len().saturating_sub(COMMAND_HISTORY_MAX);
        list.drain(..over);
        true
    }

    /// Gives every path kept here that was in one of `documents`' earlier
    /// folders ([`crate::documents::EARLIER`]) its place in the one folder,
    /// when the file or folder is there now: Recent Sessions, Session
    /// Manager's folders, when each session was last used, and Run SQL's
    /// recent scripts. True when anything changed.
    pub fn reroot_documents(&mut self, documents: &Path) -> bool {
        let moved = |p: &Path| crate::documents::reroot(p, documents).filter(|n| n.exists());
        let mut changed = false;
        for p in self
            .recent_sessions
            .iter_mut()
            .chain(self.session_folders.iter_mut())
            .chain(self.run_sql.recent.iter_mut())
        {
            if let Some(new) = moved(p) {
                *p = new;
                changed = true;
            }
        }
        let keys: Vec<String> = self.last_used.keys().cloned().collect();
        for key in keys {
            if let Some(new) = moved(Path::new(&key))
                && let Some(when) = self.last_used.remove(&key)
            {
                self.last_used.insert(path_key(&new), when);
                changed = true;
            }
        }
        changed
    }

    /// Puts `path` at the top of the recent sessions, keeping at most
    /// [`RECENT_MAX`].
    pub fn add_recent(&mut self, path: &Path) {
        self.recent_sessions.retain(|p| !same_path(p, path));
        self.recent_sessions.insert(0, path.to_owned());
        self.recent_sessions.truncate(RECENT_MAX);
    }

    /// Notes that a session file was started at `when` (Unix seconds).
    pub fn mark_used(&mut self, path: &Path, when: i64) {
        self.last_used.insert(path_key(path), when);
    }

    /// When a session file was last started.
    pub fn last_used(&self, path: &Path) -> Option<i64> {
        self.last_used.get(&path_key(path)).copied()
    }

    /// Moves what is kept about a session file to its new path (after a
    /// rename), or forgets it (`to` is `None`, after a delete).
    pub fn moved(&mut self, from: &Path, to: Option<&Path>) {
        let used = self.last_used.remove(&path_key(from));
        if let (Some(t), Some(to)) = (used, to) {
            self.last_used.insert(path_key(to), t);
        }
        for p in self.recent_sessions.iter_mut() {
            if same_path(p, from)
                && let Some(to) = to
            {
                *p = to.to_owned();
            }
        }
        if to.is_none() {
            self.recent_sessions.retain(|p| !same_path(p, from));
        }
    }

    /// Adds a folder to Session Manager. False if it is already listed.
    pub fn add_session_folder(&mut self, dir: &Path) -> bool {
        if self.session_folders.iter().any(|d| same_path(d, dir)) {
            return false;
        }
        self.session_folders.push(dir.to_owned());
        true
    }

    /// The keyboard map a new session starts from.
    pub fn new_session_keymap(&self) -> crate::KeyMap {
        self.new_session_keymap
            .as_ref()
            .and_then(|k| k.to_map().ok())
            .unwrap_or_default()
    }

    /// A system's display settings (the defaults if it has none).
    pub fn display_for(&self, system: &str) -> Display {
        self.display
            .get(&system.to_ascii_lowercase())
            .cloned()
            .unwrap_or_default()
    }

    /// Keeps a system's display settings. A keyboard map not yet migrated
    /// stays with them.
    pub fn set_display(&mut self, system: &str, mut d: Display) {
        let key = system.to_ascii_lowercase();
        if d.legacy_keymap.is_none() {
            d.legacy_keymap = self
                .display
                .get(&key)
                .and_then(|old| old.legacy_keymap.clone());
        }
        self.display.insert(key, d);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn message_options_per_system() {
        let mut s = Settings::default();
        assert_eq!(s.messages.for_system("SYS1"), MessageOptions::default());
        assert_eq!(
            MessageOptions::default().own_arrival,
            Arrival::InquiriesOnly
        );
        assert!(!MessageOptions::default().operator, "QSYSOPR is opt-in");
        let o = MessageOptions {
            own_arrival: Arrival::All,
            operator: true,
            operator_arrival: Arrival::None,
        };
        s.messages.set("Sys1", o);
        assert_eq!(s.messages.for_system("sys1"), o);
        let text = toml::to_string(&s).unwrap();
        assert!(text.contains("[messages.systems.sys1]"), "{text}");
        assert!(text.contains("own_arrival = \"all\""), "{text}");
        assert!(text.contains("operator_arrival = \"none\""), "{text}");
        let back: Settings = toml::from_str(&text).unwrap();
        assert_eq!(back.messages, s.messages);
        // Back to the defaults: not written.
        s.messages.set("SYS1", MessageOptions::default());
        assert!(!toml::to_string(&s).unwrap().contains("[messages.systems"));
        let read: Settings = toml::from_str(
            "[messages.systems.sys2]\noperator = true\noperator_arrival = \"inquiries-only\"\n",
        )
        .unwrap();
        let o = read.messages.for_system("SYS2");
        assert!(o.operator);
        assert_eq!(o.own_arrival, Arrival::InquiriesOnly);
    }

    #[test]
    fn run_sql_settings() {
        let mut s = Settings::default();
        assert!(s.run_sql.add("Sys1", " SELECT * FROM HLINV.ASSETS "));
        assert!(s.run_sql.add("SYS1", "VALUES 1"));
        assert!(s.run_sql.add("sys1", "SELECT * FROM HLINV.ASSETS"));
        assert_eq!(
            s.run_sql.history("SYS1"),
            ["VALUES 1", "SELECT * FROM HLINV.ASSETS"]
        );
        assert!(
            !s.run_sql
                .add("sys1", "CALL QSYS2.QCMDEXC('CHGUSRPRF X PASSWORD(ABC)')")
        );
        assert!(!s.run_sql.add("sys1", "SET ENCRYPTION PASSWORD = 'x'"));
        for n in 0..150 {
            s.run_sql.add("sys1", &format!("VALUES {n}"));
        }
        assert_eq!(s.run_sql.history("sys1").len(), SQL_HISTORY_MAX);
        assert_eq!(s.run_sql.history("sys1")[99], "VALUES 149");
        s.run_sql.add_recent(Path::new("/q/a.sql"));
        s.run_sql.add_recent(Path::new("/q/b.sql"));
        s.run_sql.add_recent(Path::new("/q/a.sql"));
        assert_eq!(
            s.run_sql.recent,
            [Path::new("/q/a.sql"), Path::new("/q/b.sql")]
        );
        assert!(!s.run_sql.bit_data_as_text("SYS1"));
        s.run_sql.set_bit_data_as_text("SYS1", true);
        assert!(s.run_sql.bit_data_as_text("sys1"));
        s.run_sql.stop_on_error = true;
        let back: Settings = toml::from_str(&toml::to_string(&s).unwrap()).unwrap();
        assert_eq!(back.run_sql, s.run_sql);
        // An older file without the section loads with the defaults.
        let old: Settings = toml::from_str("version = 1\n").unwrap();
        assert_eq!(old.run_sql, RunSql::default());
        s.run_sql.set_bit_data_as_text("sys1", false);
        assert!(!s.run_sql.bit_data_as_text("SYS1"));
    }

    #[test]
    fn printer_output_settings() {
        let d = Settings::default();
        assert_eq!(
            (d.printer_output.rows(), d.printer_output.refresh_secs()),
            (25, 30)
        );
        let s: Settings = toml::from_str(
            "[printer_output]
rows_per_page = 100
auto_refresh_secs = 0
",
        )
        .unwrap();
        assert_eq!(
            (s.printer_output.rows(), s.printer_output.refresh_secs()),
            (100, 0)
        );
        assert_eq!(s.printer_output.paper, crate::qsp::Paper::Letter);
        let odd = PrinterOutput {
            rows_per_page: 60,
            auto_refresh_secs: 20,
            paper: crate::qsp::Paper::A4,
        };
        assert_eq!((odd.rows(), odd.refresh_secs()), (50, 15));
        let text = toml::to_string(&Settings {
            printer_output: odd.clone(),
            ..Settings::default()
        })
        .unwrap();
        assert!(text.contains("paper = \"a4\""), "{text}");
        let back: Settings = toml::from_str(&text).unwrap();
        assert_eq!(back.printer_output, odd);
    }

    #[test]
    fn display_settings_round_trip_per_system() {
        let mut s = Settings::default();
        let d = Display {
            screen_size: ScreenSize::S27x132,
            ccsid: 1140,
            device_name: "QSADSP01".into(),
            color_map: ColorMap::BlackOnWhite,
            ..Display::default()
        };
        s.set_display("QS400.example", d.clone());
        let text = toml::to_string(&s).unwrap();
        assert!(text.contains("screen_size = \"27x132\""), "{text}");
        assert!(!text.contains("keymap"), "{text}");
        let back: Settings = toml::from_str(&text).unwrap();
        assert_eq!(back.display_for("qs400.EXAMPLE"), d);
        assert_eq!(back.display_for("other"), Display::default());
        // Older files without the table still load.
        let old: Settings = toml::from_str("version = 1\nlast_system = \"a\"\n").unwrap();
        assert!(old.display.is_empty());
    }

    #[test]
    fn earlier_per_system_maps_are_read_and_kept_until_migrated() {
        let text =
            "[display.sys1]\nccsid = 37\n\n[display.sys1.keymap]\n\"Right Ctrl\" = \"FieldExit\"\n";
        let mut s: Settings = toml::from_str(text).unwrap();
        let map = s.display_for("sys1").legacy_keymap.expect("read");
        assert_eq!(map.function("Right Ctrl"), Some("FieldExit"));
        // The Start dialog saving its values does not drop it.
        s.set_display(
            "SYS1",
            Display {
                ccsid: 285,
                ..Display::default()
            },
        );
        assert!(s.display_for("sys1").legacy_keymap.is_some());
        assert_eq!(s.display_for("sys1").ccsid, 285);
    }

    #[test]
    fn recent_sessions_newest_first_without_repeats() {
        let mut s = Settings::default();
        for n in 0..12 {
            s.add_recent(Path::new(&format!("/s/{n}.qsd")));
        }
        assert_eq!(s.recent_sessions.len(), RECENT_MAX);
        assert_eq!(s.recent_sessions[0], Path::new("/s/11.qsd"));
        s.add_recent(Path::new("/s/5.qsd"));
        assert_eq!(s.recent_sessions[0], Path::new("/s/5.qsd"));
        assert_eq!(s.recent_sessions.len(), RECENT_MAX);
        assert_eq!(
            s.recent_sessions
                .iter()
                .filter(|p| p.ends_with("5.qsd"))
                .count(),
            1
        );
        let text = toml::to_string(&s).unwrap();
        let back: Settings = toml::from_str(&text).unwrap();
        assert_eq!(back.recent_sessions, s.recent_sessions);
    }

    #[test]
    fn last_used_follows_renames_and_deletes() {
        let mut s = Settings::default();
        let (a, b) = (Path::new("/s/A.qsd"), Path::new("/s/B.qsd"));
        s.mark_used(a, 100);
        s.add_recent(a);
        s.moved(a, Some(b));
        assert_eq!(s.last_used(a), None);
        assert_eq!(s.last_used(b), Some(100));
        assert_eq!(s.recent_sessions, [b]);
        s.moved(b, None);
        assert_eq!(s.last_used(b), None);
        assert!(s.recent_sessions.is_empty());
        assert!(s.add_session_folder(Path::new("/x")));
        assert!(!s.add_session_folder(Path::new("/x")));
        let back: Settings = toml::from_str(&toml::to_string(&s).unwrap()).unwrap();
        assert_eq!(back.session_folders, [Path::new("/x")]);
    }

    #[test]
    fn new_session_keymap_defaults_until_set() {
        let mut s = Settings::default();
        assert_eq!(s.new_session_keymap(), crate::KeyMap::defaults());
        let mut m = crate::KeyMap::defaults();
        m.assign("Ctrl+Enter", "FieldExit");
        s.new_session_keymap = Some(crate::qsd::KeymapOverrides::from_map(&m));
        let back: Settings = toml::from_str(&toml::to_string(&s).unwrap()).unwrap();
        assert_eq!(back.new_session_keymap(), m);
    }

    #[test]
    fn device_names() {
        assert_eq!(check_device(" qsadsp01 ").as_deref(), Ok("QSADSP01"));
        assert_eq!(check_device("").as_deref(), Ok(""));
        assert_eq!(check_device("#DSP$@_1").as_deref(), Ok("#DSP$@_1"));
        assert!(check_device("1DSP").is_err());
        assert!(check_device("BAD.NAME").is_err());
        assert!(check_device("ELEVENCHARS").is_err());
    }

    #[test]
    fn command_history_per_system() {
        let mut s = Settings::default();
        assert!(s.add_command("SYS1", "CRTLIB LIB(A)"));
        assert!(s.add_command("sys1", "DSPLIB A"));
        assert!(s.add_command("SYS1", "CRTLIB LIB(A)"));
        assert_eq!(s.commands("Sys1"), ["DSPLIB A", "CRTLIB LIB(A)"]);
        assert!(s.commands("other").is_empty());
        for i in 0..60 {
            s.add_command("SYS1", &format!("DLYJOB DLY({i})"));
        }
        assert_eq!(s.commands("SYS1").len(), COMMAND_HISTORY_MAX);
        assert_eq!(s.commands("SYS1")[49], "DLYJOB DLY(59)");
        assert_eq!(s.commands("SYS1")[0], "DLYJOB DLY(10)");
        let text = toml::to_string(&s).unwrap();
        let back: Settings = toml::from_str(&text).unwrap();
        assert_eq!(back.commands("sys1"), s.commands("sys1"));
    }

    #[test]
    fn a_command_that_names_a_password_is_not_kept_and_is_redacted() {
        let mut s = Settings::default();
        assert!(!s.add_command("SYS1", "CHGUSRPRF USRPRF(BOB) PASSWORD(SECRET1)"));
        assert!(!s.add_command("SYS1", "CHGPWD CURPWD(A) NEWPWD(B)"));
        assert!(!s.add_command("SYS1", "  "));
        assert!(s.commands("SYS1").is_empty());
        assert_eq!(
            redact_command("CHGUSRPRF USRPRF(BOB) PASSWORD('a (b)') TEXT('x')"),
            "CHGUSRPRF USRPRF(BOB) PASSWORD(*) TEXT('x')"
        );
        assert_eq!(
            redact_command("chgpwd curpwd(A) newpwd(B)"),
            "chgpwd curpwd(*) newpwd(*)"
        );
        assert_eq!(redact_command("CRTLIB LIB(QPWD)"), "CRTLIB LIB(QPWD)");
        assert!(!names_password("CRTLIB LIB(QPWD)"));
    }
}
