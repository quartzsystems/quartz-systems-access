// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! `.qsd` files: a saved 5250 display session.
//!
//! ```toml
//! [qsaccess]
//! kind = "display"
//! version = 1
//!
//! [system]
//! name = "qs400.example.net"   # the System Configuration to use
//! host = "10.42.70.150"        # fallback, for "Create system from session file"
//! telnet_port = 23
//! tls = false
//!
//! [session]
//! title = "Payroll"
//! screen_size = "24x80"
//! ccsid = 37
//! device_name = ""
//! colour_map = "console"
//! auto_connect = true
//! keypad_visible = false
//! window = { x = 40, y = 40, w = 900, h = 600 }
//!
//! [keymap]
//! base = "defaults-v1"
//! [keymap.overrides]
//! "Ctrl+Enter" = "FieldExit"
//! "Pause" = ""                 # "" takes the key's function away
//!
//! [hotspots]
//! enabled = true
//! kinds = ["function-keys", "paging"]
//!
//! [macros]
//! files = ["signon.qsm", "../Macros/wrkactjob.qsm"]
//! ```
//!
//! A session file never holds a password or certificate trust: trust is
//! kept only in the System Configuration. A missing `[keymap]` table means
//! the session has no map of its own yet and uses the defaults.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::doc::{self, Extra, FileError, Header};
use crate::keymap::{self, KeyMap};
use crate::settings::{self, ColorMap, Geometry, ScreenSize};
use crate::system::System;

/// File extension, without the dot.
pub const EXTENSION: &str = "qsd";

/// Hotspot kind: function-key labels such as `F3=Exit`.
pub const HOTSPOT_FUNCTION_KEYS: &str = "function-keys";
/// Hotspot kind: `More...` and `Bottom` page labels.
pub const HOTSPOT_PAGING: &str = "paging";

/// The `.qsd` files directly in `dir` (not in folders below it), sorted.
pub fn files_in(dir: &Path) -> Vec<PathBuf> {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out: Vec<PathBuf> = rd
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| {
            p.is_file()
                && p.extension()
                    .is_some_and(|e| e.eq_ignore_ascii_case(EXTENSION))
        })
        .collect();
    out.sort();
    out
}

/// `[system]`: the System Configuration the session uses, and what to
/// create one from if it is missing.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SystemRef {
    pub name: String,
    /// Host name or address. Empty: the same as `name`.
    pub host: String,
    pub telnet_port: u16,
    pub tls: bool,
    #[serde(flatten)]
    pub extra: Extra,
}

impl Default for SystemRef {
    fn default() -> SystemRef {
        SystemRef {
            name: String::new(),
            host: String::new(),
            telnet_port: 23,
            tls: false,
            extra: Extra::new(),
        }
    }
}

impl SystemRef {
    /// The fields to keep for `system`.
    pub fn from_system(system: &System) -> SystemRef {
        let host = if system.fixed_ip.is_empty() {
            system.name.clone()
        } else {
            system.fixed_ip.clone()
        };
        SystemRef {
            name: system.name.clone(),
            host,
            telnet_port: if system.use_tls {
                system.ports.telnet_tls
            } else {
                system.ports.telnet
            },
            tls: system.use_tls,
            extra: Extra::new(),
        }
    }

    /// A new System Configuration from the fallback fields, for "Create
    /// system from session file". There is no default user to go on, so the
    /// Sign On dialog starts empty. It trusts nothing beyond the platform's
    /// own certificate authorities.
    pub fn to_system(&self) -> System {
        let mut s = System::new(&self.name);
        s.use_tls = self.tls;
        if self.tls {
            s.ports.telnet_tls = self.telnet_port;
        } else {
            s.ports.telnet = self.telnet_port;
        }
        let host = self.host.trim();
        if !host.is_empty()
            && !host.eq_ignore_ascii_case(&self.name)
            && host.parse::<std::net::IpAddr>().is_ok()
        {
            s.address_lookup = crate::AddressLookup::Never;
            s.fixed_ip = host.to_owned();
        }
        s
    }
}

/// `[session]`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Session {
    /// Shown in the window title and in Session Manager. Empty: the file
    /// name.
    pub title: String,
    pub screen_size: ScreenSize,
    pub ccsid: u16,
    /// Empty: the system chooses.
    pub device_name: String,
    pub colour_map: ColorMap,
    pub auto_connect: bool,
    /// Sign on automatically with a one-time QAP Telnet ticket (Telnet over
    /// TLS only, QAP version 2).
    pub auto_sign_on: bool,
    pub keypad_visible: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub window: Option<Geometry>,
    #[serde(flatten)]
    pub extra: Extra,
}

impl Default for Session {
    fn default() -> Session {
        Session {
            title: String::new(),
            screen_size: ScreenSize::S24x80,
            ccsid: 37,
            device_name: String::new(),
            colour_map: ColorMap::Console,
            auto_connect: true,
            auto_sign_on: false,
            keypad_visible: false,
            window: None,
            extra: Extra::new(),
        }
    }
}

/// `[keymap]`: the changes from a set of built-in defaults.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct KeymapOverrides {
    /// Which defaults the overrides apply to; see [`keymap::DEFAULTS_ID`].
    pub base: String,
    /// Key to function. An empty function takes the key's default away.
    #[serde(default)]
    pub overrides: BTreeMap<String, String>,
    #[serde(flatten)]
    pub extra: Extra,
}

impl KeymapOverrides {
    /// The changes that turn the defaults into `map`.
    pub fn from_map(map: &KeyMap) -> KeymapOverrides {
        let defaults = KeyMap::defaults();
        let mut overrides = BTreeMap::new();
        for (k, f) in &map.keys {
            if defaults.function(k) != Some(f.as_str()) {
                overrides.insert(k.clone(), f.clone());
            }
        }
        for k in defaults.keys.keys() {
            if !map.keys.contains_key(k) {
                overrides.insert(k.clone(), String::new());
            }
        }
        KeymapOverrides {
            base: keymap::DEFAULTS_ID.to_owned(),
            overrides,
            extra: Extra::new(),
        }
    }

    /// The defaults with the overrides applied.
    pub fn to_map(&self) -> Result<KeyMap, String> {
        if self.base != keymap::DEFAULTS_ID {
            return Err(format!(
                "keyboard map base \"{}\" is not known; this release knows \"{}\"",
                self.base,
                keymap::DEFAULTS_ID
            ));
        }
        let mut map = KeyMap::defaults();
        for (k, f) in &self.overrides {
            if f.is_empty() {
                map.keys.remove(k);
            } else if keymap::is_function(f) {
                map.keys.insert(k.clone(), f.clone());
            } else {
                return Err(format!("key {k} is given an unknown function \"{f}\""));
            }
        }
        Ok(map)
    }
}

/// `[hotspots]`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Hotspots {
    pub enabled: bool,
    /// The kinds switched on: [`HOTSPOT_FUNCTION_KEYS`], [`HOTSPOT_PAGING`].
    /// Kinds a later release adds are kept.
    pub kinds: Vec<String>,
    #[serde(flatten)]
    pub extra: Extra,
}

impl Default for Hotspots {
    fn default() -> Hotspots {
        Hotspots {
            enabled: true,
            kinds: vec![HOTSPOT_FUNCTION_KEYS.into(), HOTSPOT_PAGING.into()],
            extra: Extra::new(),
        }
    }
}

impl Hotspots {
    /// Whether hotspots of `kind` act on a click.
    pub fn on(&self, kind: &str) -> bool {
        self.enabled && self.kinds.iter().any(|k| k == kind)
    }

    /// Switches a kind on or off.
    pub fn set(&mut self, kind: &str, on: bool) {
        self.kinds.retain(|k| k != kind);
        if on {
            self.kinds.push(kind.to_owned());
            self.kinds.sort();
        }
    }
}

/// `[macros]` as written: paths relative to the file.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
struct MacrosDoc {
    files: Vec<String>,
    #[serde(flatten)]
    extra: Extra,
}

/// `[macros]`: the macros in the session's Macro menu.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Macros {
    /// Absolute once read; written relative to the session file.
    pub files: Vec<PathBuf>,
    pub extra: Extra,
}

/// The file as written.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct SessionDoc {
    qsaccess: Header,
    #[serde(default)]
    system: SystemRef,
    #[serde(default)]
    session: Session,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    keymap: Option<KeymapOverrides>,
    #[serde(default)]
    hotspots: Hotspots,
    #[serde(default)]
    macros: MacrosDoc,
    #[serde(flatten)]
    extra: Extra,
}

/// A display session file.
#[derive(Clone, Debug, PartialEq)]
pub struct SessionFile {
    pub header: Header,
    pub system: SystemRef,
    pub session: Session,
    /// `None`: no map of its own yet; the defaults apply.
    pub keymap: Option<KeymapOverrides>,
    pub hotspots: Hotspots,
    pub macros: Macros,
    pub extra: Extra,
}

impl SessionFile {
    /// A new session for `system`, with default settings.
    pub fn new(system: &System) -> SessionFile {
        SessionFile {
            header: Header::new(doc::DISPLAY),
            system: SystemRef::from_system(system),
            session: Session::default(),
            keymap: None,
            hotspots: Hotspots::default(),
            macros: Macros::default(),
            extra: Extra::new(),
        }
    }

    /// The keyboard map the session uses.
    pub fn keymap(&self) -> KeyMap {
        self.keymap
            .as_ref()
            .and_then(|k| k.to_map().ok())
            .unwrap_or_default()
    }

    /// Keeps `map` as the session's keyboard map.
    pub fn set_keymap(&mut self, map: &KeyMap) {
        let extra = self.keymap.take().map(|k| k.extra).unwrap_or_default();
        self.keymap = Some(KeymapOverrides {
            extra,
            ..KeymapOverrides::from_map(map)
        });
    }

    /// The title to show: `[session] title`, else the file's name.
    pub fn display_title(&self, path: Option<&Path>) -> String {
        if !self.session.title.trim().is_empty() {
            return self.session.title.trim().to_owned();
        }
        path.and_then(Path::file_stem)
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| self.system.name.clone())
    }

    /// Checks every value a session needs.
    pub fn validate(&self) -> Result<(), FileError> {
        let bad = |s: String| Err(FileError::Invalid(s));
        if self.system.name.trim().is_empty() {
            return bad("[system] has no name".into());
        }
        if let Err(e) = crate::system::check_name(&self.system.name) {
            return bad(format!("system name {}: {e}", self.system.name));
        }
        if self.system.telnet_port == 0 {
            return bad("Telnet port 0 is not valid".into());
        }
        if !settings::CCSIDS.contains(&self.session.ccsid) {
            return bad(format!("CCSID {} is not supported", self.session.ccsid));
        }
        if let Err(e) = settings::check_device(&self.session.device_name) {
            return bad(e.trim_end_matches('.').to_owned());
        }
        if let Some(k) = &self.keymap {
            k.to_map().map_err(FileError::Invalid)?;
        }
        Ok(())
    }

    /// Parses `.qsd` text. Relative macro paths are taken from `base`.
    pub fn parse(text: &str, base: &Path) -> Result<SessionFile, FileError> {
        let d: SessionDoc = doc::parse(text, doc::DISPLAY)?;
        let f = SessionFile {
            header: d.qsaccess,
            system: d.system,
            session: d.session,
            keymap: d.keymap,
            hotspots: d.hotspots,
            macros: Macros {
                files: d
                    .macros
                    .files
                    .iter()
                    .map(|p| doc::resolve(base, p))
                    .collect(),
                extra: d.macros.extra,
            },
            extra: d.extra,
        };
        f.validate()?;
        Ok(f)
    }

    /// The `.qsd` text, with macro paths relative to `base`.
    pub fn to_string(&self, base: &Path) -> String {
        let mut header = self.header.clone();
        header.kind = doc::DISPLAY.into();
        header.version = doc::VERSION;
        doc::to_text(&SessionDoc {
            qsaccess: header,
            system: self.system.clone(),
            session: self.session.clone(),
            keymap: self.keymap.clone(),
            hotspots: self.hotspots.clone(),
            macros: MacrosDoc {
                files: self
                    .macros
                    .files
                    .iter()
                    .map(|p| doc::relative(base, p))
                    .collect(),
                extra: self.macros.extra.clone(),
            },
            extra: self.extra.clone(),
        })
    }

    /// Reads a `.qsd` file.
    pub fn load(path: &Path) -> Result<SessionFile, FileError> {
        SessionFile::parse(&doc::read(path)?, &doc::base_dir(path))
    }

    /// Writes a `.qsd` file atomically.
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        doc::write(path, &self.to_string(&doc::base_dir(path)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::tests::temp_dir;

    fn sample() -> SessionFile {
        let mut sys = System::new("qs400.example.net");
        sys.fixed_ip = "10.42.70.150".into();
        let mut f = SessionFile::new(&sys);
        f.session = Session {
            title: "Payroll".into(),
            screen_size: ScreenSize::S27x132,
            ccsid: 285,
            device_name: "QSADSP01".into(),
            colour_map: ColorMap::BlackOnWhite,
            auto_connect: false,
            auto_sign_on: true,
            keypad_visible: true,
            window: Some(Geometry {
                x: 10,
                y: 20,
                w: 900,
                h: 700,
            }),
            extra: Extra::new(),
        };
        let mut map = KeyMap::defaults();
        map.assign("Ctrl+Enter", "FieldExit");
        map.clear_function("Clear");
        f.set_keymap(&map);
        f.hotspots.set(HOTSPOT_PAGING, false);
        f
    }

    #[test]
    fn round_trip_with_relative_macros() {
        let dir = temp_dir("qsd");
        let path = dir.join("Sessions").join("payroll.qsd");
        let mut f = sample();
        f.macros.files = vec![
            dir.join("Sessions").join("signon.qsm"),
            dir.join("Macros").join("wrkactjob.qsm"),
        ];
        f.save(&path).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(
            text.starts_with("[qsaccess]\nkind = \"display\"\nversion = 1\n"),
            "{text}"
        );
        assert!(text.contains("\"signon.qsm\""), "{text}");
        assert!(text.contains("\"../Macros/wrkactjob.qsm\""), "{text}");
        assert!(text.contains("base = \"defaults-v1\""), "{text}");
        assert!(text.contains("Pause = \"\""), "{text}");
        assert!(text.contains("colour_map = \"black-on-white\""), "{text}");
        let back = SessionFile::load(&path).unwrap();
        assert_eq!(back, f);
        let map = back.keymap();
        assert_eq!(map.function("Ctrl+Enter"), Some("FieldExit"));
        assert_eq!(map.function("Pause"), None);
        assert_eq!(map.function("F3"), Some("F:3"));
        assert!(back.hotspots.on(HOTSPOT_FUNCTION_KEYS));
        assert!(!back.hotspots.on(HOTSPOT_PAGING));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn unknown_keys_are_kept() {
        let text = r#"
future_top = "kept"

[qsaccess]
kind = "display"
version = 1
build = "V1R1M0"

[system]
name = "sys1"
host = "192.0.2.1"
telnet_port = 2323
tls = true
site = "Leeds"

[session]
title = "T"
ccsid = 37
zoom = 2

[session.extras]
deep = [1, 2]

[keymap]
base = "defaults-v1"
note = "mine"

[keymap.overrides]
"Ctrl+Enter" = "FieldExit"

[hotspots]
enabled = false
kinds = ["function-keys", "urls"]

[macros]
files = []
sort = "name"

[printer]
name = "LPT1"
"#;
        let base = Path::new("/");
        let f = SessionFile::parse(text, base).unwrap();
        assert_eq!(f.extra["future_top"].as_str(), Some("kept"));
        assert_eq!(f.extra["printer"]["name"].as_str(), Some("LPT1"));
        assert_eq!(f.header.extra["build"].as_str(), Some("V1R1M0"));
        assert_eq!(f.system.extra["site"].as_str(), Some("Leeds"));
        assert_eq!(f.session.extra["zoom"].as_integer(), Some(2));
        assert!(f.hotspots.kinds.contains(&"urls".to_owned()));
        let out = f.to_string(base);
        let again = SessionFile::parse(&out, base).unwrap();
        assert_eq!(again, f, "{out}");
        for want in [
            "future_top = \"kept\"",
            "build = \"V1R1M0\"",
            "site = \"Leeds\"",
            "zoom = 2",
            "deep = [",
            "note = \"mine\"",
            "\"urls\"",
            "sort = \"name\"",
            "[printer]",
        ] {
            assert!(out.contains(want), "{want} lost:\n{out}");
        }
        // TOML puts keys outside any table first, so an unknown one comes
        // before the header; the header is still the first table.
        let first_table = out.lines().find(|l| l.starts_with('['));
        assert_eq!(first_table, Some("[qsaccess]"), "{out}");
    }

    #[test]
    fn defaults_for_a_minimal_file() {
        let text = "[qsaccess]\nkind = \"display\"\nversion = 1\n[system]\nname = \"sys1\"\n";
        let f = SessionFile::parse(text, Path::new("/")).unwrap();
        assert!(f.session.auto_connect);
        assert_eq!(f.session.ccsid, 37);
        assert_eq!(f.system.telnet_port, 23);
        assert_eq!(f.keymap, None);
        assert_eq!(f.keymap(), KeyMap::defaults());
        assert!(f.hotspots.on(HOTSPOT_FUNCTION_KEYS) && f.hotspots.on(HOTSPOT_PAGING));
        assert_eq!(
            f.display_title(Some(Path::new("/s/Payroll.qsd"))),
            "Payroll"
        );
    }

    #[test]
    fn invalid_files_are_refused() {
        let base = Path::new("/");
        let with = |body: &str| {
            SessionFile::parse(
                &format!("[qsaccess]\nkind = \"display\"\nversion = 1\n{body}"),
                base,
            )
        };
        assert!(matches!(with(""), Err(FileError::Invalid(_))));
        assert!(matches!(
            with("[system]\nname = \"bad name\"\n"),
            Err(FileError::Invalid(_))
        ));
        assert!(matches!(
            with("[system]\nname = \"a\"\ntelnet_port = 0\n"),
            Err(FileError::Invalid(_))
        ));
        assert!(matches!(
            with("[system]\nname = \"a\"\n[session]\nccsid = 9\n"),
            Err(FileError::Invalid(s)) if s.contains("CCSID 9")
        ));
        assert!(matches!(
            with("[system]\nname = \"a\"\n[session]\ndevice_name = \"1BAD\"\n"),
            Err(FileError::Invalid(_))
        ));
        assert!(matches!(
            with("[system]\nname = \"a\"\n[keymap]\nbase = \"defaults-v9\"\n"),
            Err(FileError::Invalid(s)) if s.contains("defaults-v9")
        ));
        assert!(matches!(
            with("[system]\nname = \"a\"\n[keymap]\nbase = \"defaults-v1\"\n[keymap.overrides]\nF1 = \"Warp\"\n"),
            Err(FileError::Invalid(s)) if s.contains("Warp")
        ));
        assert!(matches!(
            with("[system]\nname = \"a\"\n[session]\nscreen_size = \"25x80\"\n"),
            Err(FileError::Parse(_))
        ));
        assert!(matches!(
            SessionFile::parse(
                "[qsaccess]\nkind = \"display\"\nversion = 2\n[system]\nname = 5\n",
                base
            ),
            Err(FileError::Newer(2))
        ));
    }

    #[test]
    fn create_system_from_fallback() {
        let r = SystemRef {
            name: "qs400.example.net".into(),
            host: "10.42.70.150".into(),
            telnet_port: 2323,
            tls: false,
            extra: Extra::new(),
        };
        let s = r.to_system();
        assert_eq!(s.name, "qs400.example.net");
        assert_eq!(s.ports.telnet, 2323);
        assert_eq!(s.fixed_ip, "10.42.70.150");
        assert_eq!(s.address_lookup, crate::AddressLookup::Never);
        assert!(s.ca_certificates.is_empty());
        assert!(s.validate().is_ok());
        let tls = SystemRef {
            name: "pub400.com".into(),
            host: "pub400.com".into(),
            telnet_port: 992,
            tls: true,
            extra: Extra::new(),
        }
        .to_system();
        assert!(tls.use_tls);
        assert_eq!(tls.ports.telnet_tls, 992);
        assert!(tls.fixed_ip.is_empty());
        assert_eq!(SystemRef::from_system(&tls).telnet_port, 992);
    }

    #[test]
    fn overrides_are_only_the_changes() {
        assert!(
            KeymapOverrides::from_map(&KeyMap::defaults())
                .overrides
                .is_empty()
        );
        let mut m = KeyMap::defaults();
        m.assign("Right Ctrl", "Enter");
        m.keys.remove("Pause");
        let o = KeymapOverrides::from_map(&m);
        assert_eq!(o.overrides.len(), 2);
        assert_eq!(o.to_map().unwrap(), m);
    }
}
