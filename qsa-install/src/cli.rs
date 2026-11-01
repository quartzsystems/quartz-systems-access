// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! The installer's command line.

use std::path::{Path, PathBuf};

use crate::manifest::Scope;
use crate::plan::Options;

/// Usage, as `--help` prints it.
pub const USAGE: &str = "\
Usage:
  installer                                      the Setup wizard
  installer --silent --scope=user|machine [--dir=PATH]
            [--no-desktop-shortcut] [--no-associations] [--no-start-menu]
            [--close-running] [--log=FILE]
  uninstall --uninstall [--silent] [--remove-settings] [--close-running]
  uninstall --repair [--silent] [--close-running]

Exit codes:
  0  done
  1  arguments not understood
  2  installer payload damaged (QAD2103)
  3  administrator (root) rights needed for --scope=machine (QAD2104)
  4  cancelled
  5  a file or registry key could not be written or removed; see the log
  6  QS/400 Access for Desktop is running (use --close-running to close it)
  7  no install found to uninstall or repair
";

/// The parsed command line.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Cli {
    pub silent: bool,
    pub scope: Option<Scope>,
    pub dir: Option<PathBuf>,
    pub no_desktop_shortcut: bool,
    pub no_associations: bool,
    pub no_start_menu: bool,
    pub close_running: bool,
    pub uninstall: bool,
    pub repair: bool,
    pub remove_settings: bool,
    /// Running from a temporary copy (Windows uninstall and repair).
    pub from_temp: bool,
    pub log: Option<PathBuf>,
    pub help: bool,
}

/// Parses arguments (without the program name).
pub fn parse(args: &[String]) -> Result<Cli, String> {
    let mut c = Cli::default();
    for a in args {
        let (k, v) = match a.split_once('=') {
            Some((k, v)) => (k, Some(v)),
            None => (a.as_str(), None),
        };
        match (k, v) {
            ("--silent", None) => c.silent = true,
            ("--scope", Some("user")) => c.scope = Some(Scope::User),
            ("--scope", Some("machine")) => c.scope = Some(Scope::Machine),
            ("--dir", Some(d)) if !d.is_empty() => c.dir = Some(PathBuf::from(d)),
            ("--no-desktop-shortcut", None) => c.no_desktop_shortcut = true,
            ("--no-associations", None) => c.no_associations = true,
            ("--no-start-menu", None) => c.no_start_menu = true,
            ("--close-running", None) => c.close_running = true,
            ("--uninstall", None) => c.uninstall = true,
            ("--repair", None) => c.repair = true,
            ("--remove-settings", None) => c.remove_settings = true,
            ("--from-temp", None) => c.from_temp = true,
            ("--log", Some(f)) => c.log = Some(PathBuf::from(f)),
            ("--help" | "-h" | "/?", None) => c.help = true,
            _ => return Err(format!("argument not understood: {a}")),
        }
    }
    if c.uninstall && c.repair {
        return Err("--uninstall and --repair cannot be given together".into());
    }
    if c.silent && !c.uninstall && !c.repair && c.scope.is_none() {
        return Err("--silent needs --scope=user or --scope=machine".into());
    }
    if let Some(d) = &c.dir
        && !d.is_absolute()
    {
        return Err(format!("--dir must be a full path: {}", d.display()));
    }
    Ok(c)
}

impl Cli {
    /// The install options the command line asks for.
    pub fn options(&self) -> Options {
        let scope = self.scope.unwrap_or(Scope::User);
        let mut o = Options::new(scope);
        if let Some(d) = &self.dir {
            o.dir = d.clone();
        }
        o.desktop_shortcut = !self.no_desktop_shortcut;
        o.associations = !self.no_associations;
        o.start_menu = !self.no_start_menu;
        o
    }
}

/// Quotes one argument for a Windows command line.
pub fn quote(s: &str) -> String {
    if !s.is_empty() && !s.contains([' ', '\t', '"']) {
        return s.to_owned();
    }
    let mut out = String::from("\"");
    let mut slashes = 0;
    for c in s.chars() {
        match c {
            '\\' => slashes += 1,
            '"' => {
                out.push_str(&"\\".repeat(slashes * 2 + 1));
                out.push('"');
                slashes = 0;
                continue;
            }
            _ => {}
        }
        if c != '\\' {
            out.push_str(&"\\".repeat(slashes));
            slashes = 0;
            out.push(c);
        }
    }
    out.push_str(&"\\".repeat(slashes * 2));
    out.push('"');
    out
}

/// The silent-install command line for `opts` (for the elevated copy the
/// wizard starts for all users).
pub fn silent_args(opts: &Options, log: &Path) -> String {
    let mut a = vec![
        "--silent".to_owned(),
        format!(
            "--scope={}",
            match opts.scope {
                Scope::User => "user",
                Scope::Machine => "machine",
            }
        ),
        format!("--dir={}", opts.dir.display()),
        format!("--log={}", log.display()),
        // The wizard has already asked.
        "--close-running".to_owned(),
    ];
    if !opts.desktop_shortcut {
        a.push("--no-desktop-shortcut".into());
    }
    if !opts.associations {
        a.push("--no-associations".into());
    }
    if !opts.start_menu {
        a.push("--no-start-menu".into());
    }
    a.iter().map(|s| quote(s)).collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| (*x).to_owned()).collect()
    }

    #[test]
    fn silent_install_arguments() {
        let dir = if cfg!(windows) {
            r"C:\Apps\Q"
        } else {
            "/apps/q"
        };
        let c = parse(&s(&[
            "--silent",
            "--scope=machine",
            &format!("--dir={dir}"),
            "--no-desktop-shortcut",
            "--no-associations",
        ]))
        .unwrap();
        let o = c.options();
        assert_eq!(o.scope, Scope::Machine);
        assert_eq!(o.dir, PathBuf::from(dir));
        assert!(!o.desktop_shortcut && !o.associations && o.start_menu);
        assert_eq!(
            parse(&s(&["--silent"])).unwrap_err(),
            "--silent needs --scope=user or --scope=machine"
        );
        assert!(parse(&s(&["--scope=everyone"])).is_err());
        assert!(parse(&s(&["--dir=relative"])).is_err());
        assert!(parse(&s(&["--uninstall", "--repair"])).is_err());
        assert!(
            parse(&s(&["--uninstall", "--silent", "--remove-settings"]))
                .unwrap()
                .remove_settings
        );
    }

    #[test]
    fn windows_quoting() {
        assert_eq!(quote("plain"), "plain");
        assert_eq!(
            quote(r"--dir=C:\Program Files\Q"),
            r#""--dir=C:\Program Files\Q""#
        );
        assert_eq!(quote(r"a b\"), r#""a b\\""#);
        assert_eq!(quote(r#"say "hi""#), r#""say \"hi\"""#);
        let o = Options {
            scope: Scope::Machine,
            dir: PathBuf::from(r"C:\Program Files\Quartz Systems\Access"),
            start_menu: true,
            desktop_shortcut: false,
            associations: true,
        };
        let a = silent_args(&o, Path::new(r"C:\Temp\l.log"));
        assert!(
            a.starts_with(
                "--silent --scope=machine \"--dir=C:\\Program Files\\Quartz Systems\\Access\""
            ),
            "{a}"
        );
        assert!(a.ends_with("--no-desktop-shortcut"), "{a}");
    }
}
