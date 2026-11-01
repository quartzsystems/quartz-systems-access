// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! Every macro in `fixtures/macros/NAME.qsm` is run against the recording
//! `fixtures/replay/NAME.qsr` (the recording plays the host) and must end on
//! that recording's golden screen. A macro whose recording is not there yet
//! is skipped. Prompts are answered with a stand-in value.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use qsa_config::qsm::MacroFile;
use qsaccess::macros::player::{Act, Player};
use tn5250::replay::{Golden, Host, Recording};

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../fixtures")
}

/// Runs `m` against `host`. Simulated time moves on by 100 ms whenever the
/// player waits, so a wait that cannot be met times out quickly.
fn run(m: &MacroFile, host: &mut Host) -> Result<(), String> {
    let mut p = Player::new(m, "test");
    let mut now = Instant::now();
    for _ in 0..100_000 {
        match p.poll(&host.terminal.screen, true, now) {
            Act::Type(text) => {
                for c in text.chars() {
                    host.key(tn5250::input::Key::Char(c));
                }
            }
            Act::Key(k) => {
                host.key(k);
            }
            Act::Prompt { .. } => p.answer(Some("PASSWORD".into())),
            Act::Wait => now += Duration::from_millis(100),
            Act::Done => return Ok(()),
            Act::Failed { step, reason } => return Err(format!("step {step}: {reason}")),
        }
    }
    Err("did not finish".into())
}

#[test]
fn macros_reach_the_recorded_screen() {
    let mut failures = Vec::new();
    let mut ran = 0;
    let mut names: Vec<PathBuf> = std::fs::read_dir(fixtures().join("macros"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "qsm"))
        .collect();
    names.sort();
    for path in names {
        let name = path.file_stem().unwrap().to_string_lossy().into_owned();
        let rec_path = fixtures().join("replay").join(format!("{name}.qsr"));
        if !rec_path.exists() {
            eprintln!("skipped {name}: {} not recorded yet", rec_path.display());
            continue;
        }
        let m = MacroFile::load(&path).unwrap_or_else(|e| panic!("{name}: {e}"));
        let rec = Recording::read(&rec_path).unwrap_or_else(|e| panic!("{name}: {e}"));
        let mut host = Host::new(&rec);
        if let Err(e) = run(&m, &mut host) {
            failures.push(format!("{name}: {e}"));
            continue;
        }
        ran += 1;
        failures.extend(host.errors.iter().map(|e| format!("{name}: {e}")));
        if !host.finished() {
            failures.push(format!("{name}: the recording was not played to its end"));
        }
        let golden = fixtures().join("replay").join(format!("{name}.golden"));
        let want = Golden::parse(&std::fs::read_to_string(&golden).unwrap()).unwrap();
        if let Some(d) = Golden::diff(&want, &Golden::of(&host.terminal.screen)) {
            failures.push(format!("{name}: screen differs from the golden\n{d}"));
        }
    }
    assert!(ran >= 2, "the QS/400 sign-on macros should run");
    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
}

#[test]
fn a_macro_for_another_screen_times_out_naming_its_step() {
    let mut m = MacroFile::load(&fixtures().join("macros/qs400-signon-24x80.qsm")).unwrap();
    if let qsa_config::qsm::Action::WaitText { text, .. } = &mut m.steps[3].action {
        *text = "Work with Active Jobs".into();
    }
    let rec = Recording::read(&fixtures().join("replay/qs400-signon-24x80.qsr")).unwrap();
    let err = run(&m, &mut Host::new(&rec)).unwrap_err();
    assert!(err.starts_with("step 4: timed out"), "{err}");
}
