// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! The installer, uninstaller and repairer of QS/400 Access for Desktop.
//! See `qsa_install::cli::USAGE` for the command line and exit codes.

#![cfg_attr(windows, windows_subsystem = "windows")]

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use qsa_install::cli::{self, Cli};
use qsa_install::install::{self, Error};
use qsa_install::{exit, log, manifest::Scope, plan, ui};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cli = match cli::parse(&args) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{e}\n\n{}", cli::USAGE);
            return code(exit::USAGE);
        }
    };
    if cli.help {
        println!("{}", cli::USAGE);
        return code(exit::OK);
    }
    let log_file = cli
        .log
        .clone()
        .unwrap_or_else(|| std::env::temp_dir().join("qsa-install.log"));
    qsa_install::log_to(&log_file);
    log(&format!(
        "{} installer {}: {args:?}",
        qsa_install::PRODUCT,
        qsa_install::RELEASE
    ));
    let exe = match std::env::current_exe() {
        Ok(p) => p,
        Err(e) => {
            log(&format!("cannot find this program: {e}"));
            return code(exit::FAILED);
        }
    };
    let gui = !cli.silent;
    let _app = gui.then(|| {
        let a = fltk::app::App::default();
        qsa_ui::theme::pin_font_rendering();
        if let Err(e) = qsa_ui::theme::init() {
            log(&format!("fonts not loaded: {e}"));
        }
        a
    });
    let named_uninstall = exe
        .file_name()
        .is_some_and(|n| n.to_string_lossy() == qsa_install::uninstaller_name());
    let c =
        if cli.uninstall || cli.repair || (named_uninstall && !cli.silent && cli.scope.is_none()) {
            maintain(&cli, &exe)
        } else {
            install_mode(&cli, &exe)
        };
    log(&format!("exit code {c}"));
    code(c)
}

fn code(c: i32) -> ExitCode {
    ExitCode::from(c as u8)
}

fn report_error(gui: bool, e: &Error) -> i32 {
    log(&format!("error: {e}"));
    if gui {
        match e.message() {
            Some(m) => ui::tell(&m),
            None => {
                qsa_ui::MessageBox::new(
                    &format!("{} Setup", qsa_install::PRODUCT),
                    &format!("Setup did not complete: {e}."),
                )
                .glyph(qsa_ui::Glyph::Cross)
                .run(None);
            }
        }
    }
    e.exit_code()
}

fn install_mode(cli: &Cli, exe: &Path) -> i32 {
    // The checksum comes first, before anything is changed.
    if let Err(e) = install::check(exe) {
        return report_error(!cli.silent, &e);
    }
    if !cli.silent {
        return ui::run_wizard(exe);
    }
    let opts = cli.options();
    if opts.scope == Scope::Machine && !plan::is_admin() {
        return report_error(false, &Error::NeedsAdmin);
    }
    if qsa_install::running::is_running() && !(cli.close_running && qsa_install::running::close()) {
        log("QS/400 Access for Desktop is running; not installing");
        return exit::RUNNING;
    }
    let mut progress = |f: f32, t: &str| log(&format!("{:3.0}% {t}", f * 100.0));
    match install::install(exe, &opts, &mut progress) {
        Ok(r) => {
            for w in &r.warnings {
                log(&format!("warning: {w}"));
            }
            println!("{}", r.message());
            exit::OK
        }
        Err(e) => report_error(false, &e),
    }
}

/// The install folder to uninstall or repair: the one this program is in,
/// else the one found in the default places.
fn install_dir(exe: &Path) -> Option<PathBuf> {
    if let Some(d) = exe.parent()
        && plan::existing_in(d).is_some()
    {
        return Some(d.to_owned());
    }
    plan::existing().map(|m| m.dir)
}

fn maintain(cli: &Cli, exe: &Path) -> i32 {
    let gui = !cli.silent;
    let Some(dir) = install_dir(exe) else {
        return report_error(
            gui,
            &Error::NotInstalled(exe.parent().unwrap_or(exe).to_owned()),
        );
    };
    // On Windows a program cannot remove or replace itself. Run from a copy
    // in the temporary folder, wait for it, and pass on its exit code.
    #[cfg(windows)]
    if !cli.from_temp && exe.starts_with(&dir) {
        let tmp = match install::temp_copy(exe) {
            Ok(t) => t,
            Err(e) => return report_error(gui, &Error::Failed(e.to_string())),
        };
        let mut args: Vec<String> = std::env::args().skip(1).collect();
        if !args.iter().any(|a| a == "--uninstall" || a == "--repair") {
            args.push("--uninstall".into());
        }
        args.push("--from-temp".into());
        log(&format!("continuing from {}", tmp.display()));
        return match std::process::Command::new(&tmp).args(&args).status() {
            Ok(s) => s.code().unwrap_or(exit::FAILED),
            Err(e) => report_error(gui, &Error::Failed(e.to_string())),
        };
    }
    // The temporary copy goes once it has exited, however it ends.
    #[cfg(windows)]
    if cli.from_temp {
        qsa_install::windows::delete_after_exit(exe);
    }
    let manifest = match qsa_install::manifest::Manifest::load(&dir) {
        Ok(m) => m,
        Err(_) => return report_error(gui, &Error::NotInstalled(dir)),
    };
    if manifest.scope == Scope::Machine && !plan::is_admin() {
        #[cfg(windows)]
        if gui {
            let args: Vec<String> = std::env::args().skip(1).collect();
            let line = args
                .iter()
                .map(|a| cli::quote(a))
                .collect::<Vec<_>>()
                .join(" ");
            return match qsa_install::windows::run_elevated(exe, &line) {
                Some(c) => c,
                None => report_error(gui, &Error::NeedsAdmin),
            };
        }
        return report_error(gui, &Error::NeedsAdmin);
    }
    let remove_settings = if gui {
        match ui::confirm_uninstall(&dir, cli.repair) {
            Some(r) => r,
            None => return exit::CANCELLED,
        }
    } else {
        cli.remove_settings
    };
    let close_ok = if gui {
        ui::close_running(None, true)
    } else {
        !qsa_install::running::is_running() || (cli.close_running && qsa_install::running::close())
    };
    if !close_ok {
        log("QS/400 Access for Desktop is running; stopping");
        return exit::RUNNING;
    }
    // This program's own copy in the install folder is in use: move it out
    // of the way so that the folder can go.
    #[cfg(windows)]
    {
        let own = dir.join(qsa_install::uninstaller_name());
        if own.exists() && own != exe {
            let aside =
                std::env::temp_dir().join(format!("qsa-old-uninstall-{}.exe", std::process::id()));
            if std::fs::rename(&own, &aside).is_ok() {
                qsa_install::windows::delete_after_exit(&aside);
            }
        }
    }
    let result = if cli.repair {
        // The payload is read after the install is undone, so it must not
        // be read from the install folder.
        let source = if exe.starts_with(&dir) {
            match install::temp_copy(exe) {
                Ok(t) => t,
                Err(e) => return report_error(gui, &Error::Failed(e.to_string())),
            }
        } else {
            exe.to_owned()
        };
        let mut progress = |f: f32, t: &str| log(&format!("{:3.0}% {t}", f * 100.0));
        let r = install::repair(&source, &dir, &mut progress);
        if source != exe {
            let _ = std::fs::remove_file(&source);
        }
        r.map(|_| qsa_msg::message(qsa_msg::id::REPAIRED, &[]))
    } else {
        install::uninstall(&dir, remove_settings).map(|problems| {
            for p in &problems {
                log(&format!("not removed: {p}"));
            }
            qsa_msg::message(qsa_msg::id::REMOVED, &[])
        })
    };
    match result {
        Ok(m) => {
            println!("{m}");
            if gui {
                ui::tell(&m);
            }
            exit::OK
        }
        Err(e) => report_error(gui, &e),
    }
}
