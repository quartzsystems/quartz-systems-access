// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! Linux: links, desktop entries, MIME types and icons.
//!
//! | | Just me | All users (root) |
//! |---|---|---|
//! | Program | `~/.local/opt/quartz-access/` | `/opt/quartz-access/` |
//! | Links | `~/.local/bin/` | `/usr/local/bin/` |
//! | Entries | `~/.local/share/{applications,mime/packages,icons/hicolor}` | `/usr/local/share/...` |
//!
//! `update-mime-database`, `update-desktop-database` and `xdg-mime default`
//! are run when they are there; when one is missing the install warns and
//! goes on. A desktop shortcut is marked trusted with `gio` where that is
//! there. Nothing else on the system is changed.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::manifest::{Item, Manifest, Scope};
use crate::plan::{Options, home};

/// The MIME types registered: (type, glob, comment, `[qsaccess]` kind).
pub const MIME_TYPES: [(&str, &str, &str, &str); 4] = [
    (
        "application/x-qsaccess-session",
        "*.qsd",
        "QS/400 display session",
        "display",
    ),
    (
        "application/x-qsaccess-printer",
        "*.qsp",
        "QS/400 printer session",
        "printer",
    ),
    (
        "application/x-qsaccess-keymap",
        "*.qkm",
        "QS/400 keyboard map",
        "keymap",
    ),
    (
        "application/x-qsaccess-macro",
        "*.qsm",
        "QS/400 macro",
        "macro",
    ),
];

/// The main desktop entry's file name.
pub const DESKTOP: &str = "quartz-access.desktop";
/// Session Manager's desktop entry's file name.
pub const DESKTOP_SESSIONS: &str = "quartz-access-sessions.desktop";
/// The MIME package's file name.
pub const MIME_PACKAGE: &str = "quartz-access.xml";

/// Where links and entries go.
#[derive(Clone, Debug)]
pub struct Places {
    pub bin: PathBuf,
    pub share: PathBuf,
}

/// The places for `scope`.
pub fn places(scope: Scope) -> Places {
    match scope {
        Scope::User => Places {
            bin: home().join(".local/bin"),
            share: std::env::var_os("XDG_DATA_HOME")
                .map(PathBuf::from)
                .filter(|p| p.is_absolute())
                .unwrap_or_else(|| home().join(".local/share")),
        },
        Scope::Machine => Places {
            bin: PathBuf::from("/usr/local/bin"),
            share: PathBuf::from("/usr/local/share"),
        },
    }
}

/// The MIME package text: each type by its glob and by its `[qsaccess]`
/// header.
pub fn mime_xml() -> String {
    let mut s = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <mime-info xmlns=\"http://www.freedesktop.org/standards/shared-mime-info\">\n",
    );
    for (mime, glob, comment, kind) in MIME_TYPES {
        s.push_str(&format!(
            "  <mime-type type=\"{mime}\">\n\
             \x20   <comment>{comment}</comment>\n\
             \x20   <sub-class-of type=\"text/plain\"/>\n\
             \x20   <icon name=\"{icon}\"/>\n\
             \x20   <glob pattern=\"{glob}\"/>\n\
             \x20   <magic priority=\"60\">\n\
             \x20     <match type=\"string\" value=\"[qsaccess]\" offset=\"0:256\">\n\
             \x20       <match type=\"string\" value=\"kind = &quot;{kind}&quot;\" offset=\"0:512\"/>\n\
             \x20     </match>\n\
             \x20   </magic>\n\
             \x20 </mime-type>\n",
            icon = mime.replace('/', "-"),
        ));
    }
    s.push_str("</mime-info>\n");
    s
}

/// `path` quoted for an `Exec` line.
fn exec_quote(path: &Path) -> String {
    let s = path.display().to_string();
    let mut out = String::from("\"");
    for c in s.chars() {
        if matches!(c, '"' | '`' | '$' | '\\') {
            out.push('\\');
        }
        out.push(c);
    }
    out.push('"');
    out
}

/// The main desktop entry. Its `Exec` names the program by its full path,
/// so it works whether or not the links' folder is on `PATH`.
pub fn desktop_entry(dir: &Path, associations: bool) -> String {
    let mut s = format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Version=1.0\n\
         Name={}\n\
         GenericName=5250 Display\n\
         Comment=Work with QS/400 systems: 5250 display sessions, Session Manager and system configurations\n\
         Exec={} open %f\n\
         Icon=quartz-access\n\
         Terminal=false\n\
         Categories=Network;RemoteAccess;\n",
        crate::PRODUCT,
        exec_quote(&dir.join("qsaccess")),
    );
    if associations {
        let types: Vec<&str> = MIME_TYPES.iter().map(|t| t.0).collect();
        s.push_str(&format!("MimeType={};\n", types.join(";")));
    }
    s
}

/// Session Manager's desktop entry.
pub fn sessions_entry(dir: &Path) -> String {
    format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Version=1.0\n\
         Name=Session Manager\n\
         GenericName=QS/400 Session Manager\n\
         Comment=Start and manage QS/400 5250 display sessions\n\
         Exec={} --session-manager\n\
         Icon=quartz-access\n\
         Terminal=false\n\
         Categories=Network;RemoteAccess;\n",
        exec_quote(&dir.join("qsaccess")),
    )
}

/// Runs a tool. `Err` says why not: missing, or failed.
fn run(tool: &str, args: &[&str]) -> Result<(), String> {
    match Command::new(tool).args(args).output() {
        Ok(o) if o.status.success() => Ok(()),
        Ok(o) => Err(format!(
            "{tool} failed: {}",
            String::from_utf8_lossy(&o.stderr).trim()
        )),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            Err(format!("{tool} is not installed"))
        }
        Err(e) => Err(format!("{tool}: {e}")),
    }
}

fn write_file(m: &mut Manifest, path: &Path, text: &str, mode: u32) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    if let Some(d) = path.parent() {
        m.make_dirs(d)?;
    }
    std::fs::write(path, text)?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))?;
    m.file(path);
    Ok(())
}

/// Copies a folder tree's files, noting each file and folder made.
fn copy_tree(m: &mut Manifest, from: &Path, to: &Path) -> std::io::Result<()> {
    for e in std::fs::read_dir(from)? {
        let e = e?;
        let src = e.path();
        let dst = to.join(e.file_name());
        if e.file_type()?.is_dir() {
            copy_tree(m, &src, &dst)?;
        } else {
            m.make_dirs(to)?;
            std::fs::copy(&src, &dst)?;
            m.file(&dst);
        }
    }
    Ok(())
}

/// The user's `mimeapps.list`.
fn mimeapps() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| home().join(".config"))
        .join("mimeapps.list")
}

/// Adds links, desktop entries, MIME types and icons, noting each in `m`.
pub fn integrate(
    opts: &Options,
    m: &mut Manifest,
    warnings: &mut Vec<String>,
) -> std::io::Result<()> {
    let p = places(opts.scope);
    let dir = &opts.dir;

    // Links to the program and the uninstaller.
    m.make_dirs(&p.bin)?;
    for (name, target) in [
        ("qsaccess", dir.join("qsaccess")),
        (
            "quartz-access-uninstall",
            dir.join(crate::uninstaller_name()),
        ),
    ] {
        let link = p.bin.join(name);
        if let Ok(meta) = std::fs::symlink_metadata(&link) {
            if meta.file_type().is_symlink() {
                std::fs::remove_file(&link)?;
            } else {
                warnings.push(format!(
                    "{} is already there and is not a link; it was left alone",
                    link.display()
                ));
                continue;
            }
        }
        std::os::unix::fs::symlink(&target, &link)?;
        m.add(Item::Symlink { path: link });
    }

    // Icons, from the install folder into the hicolor theme.
    let icons = dir.join("icons/hicolor");
    if icons.is_dir() {
        copy_tree(m, &icons, &p.share.join("icons/hicolor"))?;
    }

    let apps = p.share.join("applications");
    if opts.associations {
        let mime = p.share.join("mime");
        let existed = mime.join("mime.cache").exists();
        write_file(
            m,
            &mime.join("packages").join(MIME_PACKAGE),
            &mime_xml(),
            0o644,
        )?;
        let mime_s = mime.display().to_string();
        match run("update-mime-database", &[&mime_s]) {
            Ok(()) => m.add(Item::Generated {
                path: mime.clone(),
                created: !existed,
                rebuild: vec!["update-mime-database".into(), mime_s],
            }),
            Err(e) => warnings.push(format!(
                "{e}; the file types will be known after the next login or update"
            )),
        }
    }
    if opts.start_menu || opts.associations {
        let cache = apps.join("mimeinfo.cache");
        let existed = cache.exists();
        write_file(
            m,
            &apps.join(DESKTOP),
            &desktop_entry(dir, opts.associations),
            0o644,
        )?;
        if opts.start_menu {
            write_file(m, &apps.join(DESKTOP_SESSIONS), &sessions_entry(dir), 0o644)?;
        }
        let apps_s = apps.display().to_string();
        match run("update-desktop-database", &[&apps_s]) {
            Ok(()) => m.add(Item::Generated {
                path: cache,
                created: !existed,
                rebuild: vec!["update-desktop-database".into(), apps_s],
            }),
            Err(e) => warnings.push(e),
        }
    }
    if opts.associations && opts.scope == Scope::User {
        let list = mimeapps();
        // xdg-mime makes the config folder if there is none; note it, so
        // that it goes again if it is left empty.
        if let Some(d) = list.parent() {
            m.make_dirs(d)?;
        }
        let existed = list.exists();
        let mut set = Vec::new();
        for (mime, ..) in MIME_TYPES {
            match run("xdg-mime", &["default", DESKTOP, mime]) {
                Ok(()) => set.push(mime),
                Err(e) => {
                    warnings.push(e);
                    break;
                }
            }
        }
        if !existed && list.exists() {
            m.add(Item::Generated {
                path: list.clone(),
                created: true,
                rebuild: Vec::new(),
            });
        }
        for mime in set {
            m.add(Item::MimeDefault {
                file: list.clone(),
                mime: mime.into(),
                desktop: DESKTOP.into(),
            });
        }
    }

    // A desktop shortcut, only where there is a desktop folder.
    if opts.desktop_shortcut {
        match directories::UserDirs::new().and_then(|u| u.desktop_dir().map(Path::to_owned)) {
            Some(desk) if desk.is_dir() => {
                let link = desk.join(DESKTOP);
                write_file(m, &link, &desktop_entry(dir, opts.associations), 0o755)?;
                let s = link.display().to_string();
                if let Err(e) = run("gio", &["set", &s, "metadata::trusted", "true"]) {
                    crate::log(&format!("desktop shortcut not marked trusted: {e}"));
                }
            }
            _ => {
                warnings.push("there is no desktop folder, so no desktop shortcut was made".into())
            }
        }
    }

    if opts.scope == Scope::User {
        let on_path =
            std::env::var_os("PATH").is_some_and(|v| std::env::split_paths(&v).any(|d| d == p.bin));
        if !on_path {
            warnings.push(format!(
                "{} is not on your PATH, so `qsaccess` cannot be typed as a command until it is",
                p.bin.display()
            ));
        }
    }
    for w in warnings.iter() {
        crate::log(&format!("warning: {w}"));
    }
    Ok(())
}

/// Takes `desktop` out of `mime`'s entries in a `mimeapps.list`.
pub fn unset_default(text: &str, mime: &str, desktop: &str) -> String {
    let mut out = String::new();
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix(&format!("{mime}=")) {
            let kept: Vec<&str> = rest
                .split(';')
                .filter(|d| !d.is_empty() && *d != desktop)
                .collect();
            if !kept.is_empty() {
                out.push_str(&format!("{mime}={};\n", kept.join(";")));
            }
            continue;
        }
        out.push_str(line);
        out.push('\n');
    }
    out
}

/// Whether a `mimeapps.list` assigns nothing at all.
fn assigns_nothing(text: &str) -> bool {
    !text
        .lines()
        .any(|l| l.contains('=') && !l.trim_start().starts_with('#'))
}

/// Whether a cache the install made holds nothing but ours, so that it can
/// go entirely.
fn only_ours(path: &Path) -> bool {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    let entries = |d: &Path| -> Vec<String> {
        std::fs::read_dir(d)
            .map(|rd| {
                rd.filter_map(Result::ok)
                    .map(|e| e.file_name().to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default()
    };
    match name {
        "mime" => entries(&path.join("packages"))
            .iter()
            .all(|f| f == MIME_PACKAGE),
        "mimeinfo.cache" => path.parent().is_some_and(|d| {
            entries(d)
                .iter()
                .all(|f| f == DESKTOP || f == DESKTOP_SESSIONS || f == "mimeinfo.cache")
        }),
        "mimeapps.list" => std::fs::read_to_string(path).map_or(true, |t| assigns_nothing(&t)),
        _ => false,
    }
}

/// Undoes a manifest. Caches the install made are removed if they hold
/// nothing but ours; otherwise they are rebuilt once our files are gone.
pub fn remove(m: &Manifest) -> Vec<String> {
    let mut rebuild: Vec<Vec<String>> = Vec::new();
    let mut errors = crate::manifest::remove_files(m, |item| match item {
        Item::MimeDefault {
            file,
            mime,
            desktop,
        } => match std::fs::read_to_string(file) {
            Ok(t) => std::fs::write(file, unset_default(&t, mime, desktop))
                .map_err(|e| format!("{}: {e}", file.display())),
            Err(_) => Ok(()),
        },
        Item::Generated {
            path,
            created,
            rebuild: cmd,
        } => {
            if *created && only_ours(path) {
                let r = if path.is_dir() {
                    std::fs::remove_dir_all(path)
                } else {
                    std::fs::remove_file(path)
                };
                match r {
                    Ok(()) => Ok(()),
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
                    Err(e) => Err(format!("{}: {e}", path.display())),
                }
            } else {
                if !cmd.is_empty() && !rebuild.contains(cmd) {
                    rebuild.push(cmd.clone());
                }
                Ok(())
            }
        }
        Item::RegKey { .. } | Item::RegValue { .. } => Ok(()),
        Item::File { .. } | Item::Dir { .. } | Item::Symlink { .. } => Ok(()),
    });
    for cmd in rebuild {
        let target = Path::new(&cmd[1]);
        // A folder that is gone needs no cache.
        if !target.exists() {
            continue;
        }
        let args: Vec<&str> = cmd[1..].iter().map(String::as_str).collect();
        if let Err(e) = run(&cmd[0], &args) {
            errors.push(e);
        }
    }
    errors
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mime_package_matches_by_glob_and_header() {
        let x = mime_xml();
        for want in [
            "type=\"application/x-qsaccess-session\"",
            "<glob pattern=\"*.qsd\"/>",
            "<glob pattern=\"*.qsp\"/>",
            "kind = &quot;printer&quot;",
            "<glob pattern=\"*.qkm\"/>",
            "<glob pattern=\"*.qsm\"/>",
            "value=\"[qsaccess]\" offset=\"0:256\"",
            "kind = &quot;macro&quot;",
        ] {
            assert!(x.contains(want), "{want}\n{x}");
        }
    }

    #[test]
    fn desktop_entries() {
        let d = desktop_entry(Path::new("/opt/quartz-access"), true);
        assert!(
            d.contains("Exec=\"/opt/quartz-access/qsaccess\" open %f\n"),
            "{d}"
        );
        assert!(d.contains(
            "MimeType=application/x-qsaccess-session;application/x-qsaccess-printer;application/x-qsaccess-keymap;application/x-qsaccess-macro;\n"
        ));
        assert!(!desktop_entry(Path::new("/x"), false).contains("MimeType"));
        assert!(
            sessions_entry(Path::new("/x y")).contains("Exec=\"/x y/qsaccess\" --session-manager")
        );
    }

    #[test]
    fn unsetting_defaults_keeps_other_apps() {
        let t = "[Default Applications]\napplication/x-qsaccess-session=quartz-access.desktop;\ntext/plain=gedit.desktop;\napplication/x-qsaccess-macro=other.desktop;quartz-access.desktop;\n";
        let out = unset_default(t, "application/x-qsaccess-session", DESKTOP);
        let out = unset_default(&out, "application/x-qsaccess-macro", DESKTOP);
        assert_eq!(
            out,
            "[Default Applications]\ntext/plain=gedit.desktop;\napplication/x-qsaccess-macro=other.desktop;\n"
        );
        assert!(assigns_nothing("[Default Applications]\n\n"));
        assert!(!assigns_nothing(&out));
    }
}
