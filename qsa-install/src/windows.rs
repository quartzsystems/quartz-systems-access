// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! Windows: file types, shortcuts, the Uninstall entry, elevation.
//!
//! | | Just me | All users |
//! |---|---|---|
//! | Folder | `%LOCALAPPDATA%\Programs\Quartz Systems\Access` | `%ProgramFiles%\Quartz Systems\Access` |
//! | Registry | `HKCU` | `HKLM` |
//! | Start menu | the user's Programs | All Users' Programs |
//!
//! - File types: ProgIDs `QuartzSystems.Access.Session.1`,
//!   `QuartzSystems.Access.KeyMap.1` and `QuartzSystems.Access.Macro.1`
//!   under `Software\Classes`, each with `DefaultIcon` and
//!   `shell\open\command` = `"<dir>\qsaccess.exe" open "%1"`; then
//!   `SHChangeNotify(SHCNE_ASSOCCHANGED)`.
//! - Shortcuts are made with `IShellLinkW`. File names cannot hold `/`, so
//!   the program's shortcut is `QS400 Access for Desktop.lnk`; its
//!   description gives the full name.
//! - The Uninstall entry is
//!   `Software\Microsoft\Windows\CurrentVersion\Uninstall\QuartzSystemsAccess`.

use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::ptr::{null, null_mut};

use windows_sys::Win32::Foundation::{CloseHandle, ERROR_FILE_NOT_FOUND, ERROR_SUCCESS, HANDLE};
use windows_sys::Win32::Security::{
    GetTokenInformation, TOKEN_ELEVATION, TOKEN_QUERY, TokenElevation,
};
use windows_sys::Win32::System::Com::CoTaskMemFree;
use windows_sys::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ, KEY_WRITE, REG_CREATED_NEW_KEY,
    REG_DWORD, REG_OPTION_NON_VOLATILE, REG_SZ, RegCloseKey, RegCreateKeyExW, RegDeleteKeyValueW,
    RegDeleteKeyW, RegDeleteTreeW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW,
};
use windows_sys::Win32::System::Threading::{
    GetCurrentProcess, GetExitCodeProcess, INFINITE, OpenProcessToken, WaitForSingleObject,
};
use windows_sys::Win32::UI::Shell::{
    FOLDERID_CommonPrograms, FOLDERID_Desktop, FOLDERID_Programs, FOLDERID_PublicDesktop,
    SEE_MASK_NOCLOSEPROCESS, SHCNE_ASSOCCHANGED, SHCNF_IDLIST, SHChangeNotify, SHELLEXECUTEINFOW,
    SHGetKnownFolderPath, ShellExecuteExW,
};
use windows_sys::core::GUID;

use crate::manifest::{Hive, Item, Manifest, Scope};
use crate::plan::Options;

/// The file types registered: (extension, ProgID, description, icon).
pub const FILE_TYPES: [(&str, &str, &str, &str); 3] = [
    (
        ".qsd",
        "QuartzSystems.Access.Session.1",
        "QS/400 Display Session",
        "application-x-qsaccess-session",
    ),
    (
        ".qkm",
        "QuartzSystems.Access.KeyMap.1",
        "QS/400 Keyboard Map",
        "application-x-qsaccess-keymap",
    ),
    (
        ".qsm",
        "QuartzSystems.Access.Macro.1",
        "QS/400 Macro",
        "application-x-qsaccess-macro",
    ),
];

/// The Uninstall entry's key.
pub const UNINSTALL_KEY: &str =
    r"Software\Microsoft\Windows\CurrentVersion\Uninstall\QuartzSystemsAccess";
/// The Start menu folder's name.
pub const START_FOLDER: &str = "Quartz Systems";
/// The program shortcut's file name.
pub const APP_LINK: &str = "QS400 Access for Desktop.lnk";
/// Session Manager's shortcut's file name.
pub const SESSIONS_LINK: &str = "Session Manager.lnk";

fn wide(s: impl AsRef<OsStr>) -> Vec<u16> {
    s.as_ref().encode_wide().chain(std::iter::once(0)).collect()
}

fn hkey(h: Hive) -> HKEY {
    match h {
        Hive::CurrentUser => HKEY_CURRENT_USER,
        Hive::LocalMachine => HKEY_LOCAL_MACHINE,
    }
}

/// The hive for `scope`.
pub fn hive(scope: Scope) -> Hive {
    match scope {
        Scope::User => Hive::CurrentUser,
        Scope::Machine => Hive::LocalMachine,
    }
}

fn win_err(what: &str, code: u32) -> std::io::Error {
    std::io::Error::other(format!(
        "{what}: {}",
        std::io::Error::from_raw_os_error(code as i32)
    ))
}

/// An open registry key, closed when dropped.
struct Key(HKEY);

impl Drop for Key {
    fn drop(&mut self) {
        // SAFETY: a key this program opened.
        unsafe { RegCloseKey(self.0) };
    }
}

impl Key {
    /// Opens or creates `key`; true if it was created.
    fn create(h: Hive, key: &str) -> std::io::Result<(Key, bool)> {
        let mut out: HKEY = null_mut();
        let mut disp = 0u32;
        let k = wide(key);
        // SAFETY: NUL-terminated name; out pointers to locals.
        let r = unsafe {
            RegCreateKeyExW(
                hkey(h),
                k.as_ptr(),
                0,
                null(),
                REG_OPTION_NON_VOLATILE,
                KEY_WRITE | KEY_READ,
                null(),
                &mut out,
                &mut disp,
            )
        };
        if r != ERROR_SUCCESS {
            return Err(win_err(key, r));
        }
        Ok((Key(out), disp == REG_CREATED_NEW_KEY))
    }

    fn set_str(&self, name: &str, value: &str) -> std::io::Result<()> {
        let n = wide(name);
        let v = wide(value);
        // SAFETY: NUL-terminated name; data length in bytes, NUL included.
        let r = unsafe {
            RegSetValueExW(
                self.0,
                n.as_ptr(),
                0,
                REG_SZ,
                v.as_ptr().cast(),
                (v.len() * 2) as u32,
            )
        };
        if r != ERROR_SUCCESS {
            return Err(win_err(name, r));
        }
        Ok(())
    }

    fn set_dword(&self, name: &str, value: u32) -> std::io::Result<()> {
        let n = wide(name);
        let b = value.to_le_bytes();
        // SAFETY: as above, four bytes of data.
        let r = unsafe { RegSetValueExW(self.0, n.as_ptr(), 0, REG_DWORD, b.as_ptr(), 4) };
        if r != ERROR_SUCCESS {
            return Err(win_err(name, r));
        }
        Ok(())
    }
}

/// A string value, if the key and value are there.
pub fn read_str(h: Hive, key: &str, name: &str) -> Option<String> {
    let mut k: HKEY = null_mut();
    let kw = wide(key);
    // SAFETY: NUL-terminated name; out pointer to a local.
    if unsafe { RegOpenKeyExW(hkey(h), kw.as_ptr(), 0, KEY_READ, &mut k) } != ERROR_SUCCESS {
        return None;
    }
    let k = Key(k);
    let n = wide(name);
    let mut buf = vec![0u16; 1024];
    let mut len = (buf.len() * 2) as u32;
    let mut kind = 0u32;
    // SAFETY: buffer and its length in bytes.
    let r = unsafe {
        RegQueryValueExW(
            k.0,
            n.as_ptr(),
            null(),
            &mut kind,
            buf.as_mut_ptr().cast(),
            &mut len,
        )
    };
    if r != ERROR_SUCCESS || kind != REG_SZ {
        return None;
    }
    let chars = (len as usize / 2).min(buf.len());
    let s = String::from_utf16_lossy(&buf[..chars]);
    Some(s.trim_end_matches('\0').to_owned())
}

/// Whether a key exists.
pub fn key_exists(h: Hive, key: &str) -> bool {
    let mut k: HKEY = null_mut();
    let kw = wide(key);
    // SAFETY: as in read_str.
    let ok = unsafe { RegOpenKeyExW(hkey(h), kw.as_ptr(), 0, KEY_READ, &mut k) } == ERROR_SUCCESS;
    if ok {
        drop(Key(k));
    }
    ok
}

fn delete_tree(h: Hive, key: &str) -> Result<(), String> {
    let fail = |r: u32| format!("{key}: {}", std::io::Error::from_raw_os_error(r as i32));
    let k = wide(key);
    // SAFETY: NUL-terminated name. Empties the key: its values and subkeys.
    let r = unsafe { RegDeleteTreeW(hkey(h), k.as_ptr()) };
    if r != ERROR_SUCCESS && r != ERROR_FILE_NOT_FOUND {
        return Err(fail(r));
    }
    // SAFETY: as above. Removes the key itself, now empty.
    let r = unsafe { RegDeleteKeyW(hkey(h), k.as_ptr()) };
    if r != ERROR_SUCCESS && r != ERROR_FILE_NOT_FOUND {
        return Err(fail(r));
    }
    Ok(())
}

fn delete_value(h: Hive, key: &str, name: &str) -> Result<(), String> {
    let k = wide(key);
    let n = wide(name);
    // SAFETY: NUL-terminated names.
    let r = unsafe { RegDeleteKeyValueW(hkey(h), k.as_ptr(), n.as_ptr()) };
    if r == ERROR_SUCCESS || r == ERROR_FILE_NOT_FOUND {
        Ok(())
    } else {
        Err(format!(
            "{key}\\{name}: {}",
            std::io::Error::from_raw_os_error(r as i32)
        ))
    }
}

/// A known folder's path.
pub fn known_folder(id: &GUID) -> Option<PathBuf> {
    let mut p: windows_sys::core::PWSTR = null_mut();
    // SAFETY: out pointer to a local; the string is freed below.
    let hr = unsafe { SHGetKnownFolderPath(id, 0, null_mut(), &mut p) };
    if hr < 0 || p.is_null() {
        return None;
    }
    // SAFETY: a NUL-terminated string from the shell.
    let n = (0..).take_while(|&i| unsafe { *p.add(i) } != 0).count();
    let s = String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(p, n) });
    // SAFETY: allocated by SHGetKnownFolderPath.
    unsafe { CoTaskMemFree(p.cast()) };
    Some(PathBuf::from(s))
}

/// Whether this process runs with administrator rights.
pub fn is_elevated() -> bool {
    // SAFETY: plain calls on this process's token; the handle is closed.
    unsafe {
        let mut token: HANDLE = null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return false;
        }
        let mut e = TOKEN_ELEVATION { TokenIsElevated: 0 };
        let mut len = 0u32;
        let ok = GetTokenInformation(
            token,
            TokenElevation,
            (&mut e as *mut TOKEN_ELEVATION).cast(),
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut len,
        );
        CloseHandle(token);
        ok != 0 && e.TokenIsElevated != 0
    }
}

/// Runs `exe` with `args` with administrator rights (the consent prompt
/// shows) and waits for it. `None` if the user declined.
pub fn run_elevated(exe: &Path, args: &str) -> Option<i32> {
    let verb = wide("runas");
    let file = wide(exe);
    let params = wide(args);
    // SAFETY: a zeroed SHELLEXECUTEINFOW is valid; the strings outlive the
    // call; the process handle is closed.
    unsafe {
        let mut info: SHELLEXECUTEINFOW = std::mem::zeroed();
        info.cbSize = std::mem::size_of::<SHELLEXECUTEINFOW>() as u32;
        info.fMask = SEE_MASK_NOCLOSEPROCESS;
        info.lpVerb = verb.as_ptr();
        info.lpFile = file.as_ptr();
        info.lpParameters = params.as_ptr();
        info.nShow = 1; // SW_SHOWNORMAL
        if ShellExecuteExW(&mut info) == 0 || info.hProcess.is_null() {
            return None;
        }
        WaitForSingleObject(info.hProcess, INFINITE);
        let mut code = 1u32;
        GetExitCodeProcess(info.hProcess, &mut code);
        CloseHandle(info.hProcess);
        Some(code as i32)
    }
}

/// Makes a shell link with `IShellLinkW`.
fn shortcut(
    lnk: &Path,
    target: &Path,
    args: &str,
    description: &str,
    workdir: &Path,
) -> std::io::Result<()> {
    use windows::Win32::System::Com::{
        CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
        IPersistFile,
    };
    use windows::Win32::UI::Shell::{IShellLinkW, ShellLink};
    use windows::core::{HSTRING, Interface};
    let err = |e: windows::core::Error| std::io::Error::other(format!("{}: {e}", lnk.display()));
    // SAFETY: COM calls on this thread; every interface is released when
    // dropped. CoInitializeEx may report that COM is already initialised.
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let link: IShellLinkW =
            CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).map_err(err)?;
        link.SetPath(&HSTRING::from(target.as_os_str()))
            .map_err(err)?;
        link.SetArguments(&HSTRING::from(args)).map_err(err)?;
        link.SetDescription(&HSTRING::from(description))
            .map_err(err)?;
        link.SetWorkingDirectory(&HSTRING::from(workdir.as_os_str()))
            .map_err(err)?;
        link.SetIconLocation(&HSTRING::from(target.as_os_str()), 0)
            .map_err(err)?;
        let file: IPersistFile = link.cast().map_err(err)?;
        file.Save(&HSTRING::from(lnk.as_os_str()), true)
            .map_err(err)?;
    }
    Ok(())
}

/// Tells Explorer that file types changed.
pub fn notify_associations() {
    // SAFETY: no items are passed.
    unsafe { SHChangeNotify(SHCNE_ASSOCCHANGED as i32, SHCNF_IDLIST, null(), null()) };
}

/// The size of a folder's files, in KB, for the Uninstall entry.
fn size_kb(dir: &Path) -> u32 {
    fn walk(d: &Path) -> u64 {
        std::fs::read_dir(d)
            .map(|rd| {
                rd.filter_map(Result::ok)
                    .map(|e| match e.file_type() {
                        Ok(t) if t.is_dir() => walk(&e.path()),
                        _ => e.metadata().map_or(0, |m| m.len()),
                    })
                    .sum()
            })
            .unwrap_or(0)
    }
    (walk(dir) / 1024).min(u64::from(u32::MAX)) as u32
}

/// Adds file types, shortcuts and the Uninstall entry, noting each in `m`.
pub fn integrate(
    opts: &Options,
    m: &mut Manifest,
    warnings: &mut Vec<String>,
) -> std::io::Result<()> {
    let h = hive(opts.scope);
    let dir = &opts.dir;
    let exe = dir.join(crate::program_name());
    let key = |m: &mut Manifest, path: &str| -> std::io::Result<Key> {
        let existed = key_exists(h, path);
        let (k, _) = Key::create(h, path)?;
        if !existed {
            m.add(Item::RegKey {
                hive: h,
                key: path.to_owned(),
            });
        }
        Ok(k)
    };

    if opts.associations {
        for (ext, progid, desc, icon) in FILE_TYPES {
            let base = format!(r"Software\Classes\{progid}");
            // The ProgID is ours by name, so it goes whole on uninstall,
            // even if a damaged earlier install left it behind.
            let existed = key_exists(h, &base);
            let k = key(m, &base)?;
            if existed {
                m.add(Item::RegKey {
                    hive: h,
                    key: base.clone(),
                });
            }
            k.set_str("", desc)?;
            key(m, &format!(r"{base}\DefaultIcon"))?.set_str(
                "",
                &dir.join("icons")
                    .join(format!("{icon}.ico"))
                    .display()
                    .to_string(),
            )?;
            key(m, &format!(r"{base}\shell\open\command"))?
                .set_str("", &format!("\"{}\" open \"%1\"", exe.display()))?;
            let ext_key = format!(r"Software\Classes\{ext}");
            let ext_existed = key_exists(h, &ext_key);
            let k = key(m, &ext_key)?;
            k.set_str("", progid)?;
            if ext_existed {
                m.add(Item::RegValue {
                    hive: h,
                    key: ext_key,
                    name: String::new(),
                });
            }
        }
        notify_associations();
    }

    if opts.start_menu {
        let programs = match opts.scope {
            Scope::User => known_folder(&FOLDERID_Programs),
            Scope::Machine => known_folder(&FOLDERID_CommonPrograms),
        };
        match programs {
            Some(p) => {
                let folder = p.join(START_FOLDER);
                m.make_dirs(&folder)?;
                let app = folder.join(APP_LINK);
                shortcut(&app, &exe, "", crate::PRODUCT, dir)?;
                m.file(&app);
                let sm = folder.join(SESSIONS_LINK);
                shortcut(
                    &sm,
                    &exe,
                    "--session-manager",
                    "QS/400 Session Manager",
                    dir,
                )?;
                m.file(&sm);
            }
            None => warnings.push("the Start menu folder could not be found".into()),
        }
    }
    if opts.desktop_shortcut {
        let desk = match opts.scope {
            Scope::User => known_folder(&FOLDERID_Desktop),
            Scope::Machine => known_folder(&FOLDERID_PublicDesktop),
        };
        match desk {
            Some(d) => {
                let l = d.join(APP_LINK);
                shortcut(&l, &exe, "", crate::PRODUCT, dir)?;
                m.file(&l);
            }
            None => warnings.push("the desktop folder could not be found".into()),
        }
    }

    // The Uninstall entry (Settings > Apps).
    let un = dir.join(crate::uninstaller_name());
    let k = key(m, UNINSTALL_KEY)?;
    m.add(Item::RegKey {
        hive: h,
        key: UNINSTALL_KEY.into(),
    });
    k.set_str("DisplayName", crate::PRODUCT)?;
    k.set_str("DisplayVersion", crate::VERSION)?;
    k.set_str("Publisher", crate::PUBLISHER)?;
    k.set_str("InstallLocation", &dir.display().to_string())?;
    k.set_str("DisplayIcon", &format!("{},0", exe.display()))?;
    k.set_str(
        "UninstallString",
        &format!("\"{}\" --uninstall", un.display()),
    )?;
    k.set_str(
        "QuietUninstallString",
        &format!("\"{}\" --uninstall --silent", un.display()),
    )?;
    k.set_str("ModifyPath", &format!("\"{}\" --repair", un.display()))?;
    k.set_dword("EstimatedSize", size_kb(dir))?;
    k.set_dword("NoModify", 0)?;
    k.set_dword("NoRepair", 1)?;
    Ok(())
}

/// Undoes a manifest: registry first-to-last as noted, last first.
pub fn remove(m: &Manifest) -> Vec<String> {
    let mut assoc = false;
    let errors = crate::manifest::remove_files(m, |item| match item {
        Item::RegKey { hive, key } => {
            assoc |= key.starts_with(r"Software\Classes");
            delete_tree(*hive, key)
        }
        Item::RegValue { hive, key, name } => {
            assoc |= key.starts_with(r"Software\Classes");
            delete_value(*hive, key, name)
        }
        _ => Ok(()),
    });
    if assoc {
        notify_associations();
    }
    errors
}

/// The install folder an Uninstall entry names, for an install in `scope`.
pub fn installed_dir(scope: Scope) -> Option<PathBuf> {
    read_str(hive(scope), UNINSTALL_KEY, "InstallLocation").map(PathBuf::from)
}

/// Deletes `path` once this process has exited: a hidden `cmd` tries every
/// second, for up to 20 minutes, until the file is gone. If that cannot
/// start, the file goes at the next restart (with administrator rights) or
/// stays in the temporary folder.
pub fn delete_after_exit(path: &Path) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    const DETACHED_PROCESS: u32 = 0x0000_0008;
    let p = path.display();
    let cmd = format!(
        "for /L %i in (1,1,1200) do @(ping -n 2 127.0.0.1 >nul & del /f /q \"{p}\" 2>nul & if not exist \"{p}\" exit /b 0)"
    );
    let started = std::process::Command::new("cmd.exe")
        .args(["/d", "/c"])
        .raw_arg(&cmd)
        .creation_flags(CREATE_NO_WINDOW | DETACHED_PROCESS)
        .spawn()
        .is_ok();
    if !started {
        let p = wide(path);
        // SAFETY: NUL-terminated path; no new name.
        unsafe {
            windows_sys::Win32::Storage::FileSystem::MoveFileExW(
                p.as_ptr(),
                null(),
                windows_sys::Win32::Storage::FileSystem::MOVEFILE_DELAY_UNTIL_REBOOT,
            )
        };
    }
}
