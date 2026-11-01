// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! Every recording in `fixtures/replay/` replays to its golden. A fixture
//! whose recording is not there yet is skipped. `QSA_UPDATE_GOLDEN=1`
//! writes the goldens from the replays instead of comparing.

use std::path::PathBuf;

use tn5250::replay::{Golden, Recording, replay};

/// The fixtures, by name: `NAME.qsr` and `NAME.golden`.
const FIXTURES: &[&str] = &[
    // Recorded by hand; see fixtures/README.md.
    "sign-on",
    "sign-on-wrkactjob-f3",
    "screen-27x132",
    // QS/400's sign-on display, Enter on it, and the 27×132 sign-on.
    "qs400-signon-24x80",
    "qs400-signon-27x132",
];

fn dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../fixtures/replay")
}

#[test]
fn fixtures_replay_to_their_goldens() {
    let update = std::env::var_os("QSA_UPDATE_GOLDEN").is_some();
    let mut failures = Vec::new();
    for name in FIXTURES {
        let rec_path = dir().join(format!("{name}.qsr"));
        let golden_path = dir().join(format!("{name}.golden"));
        if !rec_path.exists() {
            eprintln!("skipped {name}: {} not recorded yet", rec_path.display());
            continue;
        }
        let rec = Recording::read(&rec_path).unwrap_or_else(|e| panic!("{name}: {e}"));
        let r = replay(&rec);
        for (i, want, got) in &r.divergences {
            failures.push(format!(
                "{name}: event {i} sent {}, recording has {}",
                tn5250::log::hex(got),
                tn5250::log::hex(want)
            ));
        }
        for e in &r.errors {
            failures.push(format!("{name}: {e}"));
        }
        let got = Golden::of(&r.terminal.screen);
        if update {
            std::fs::write(&golden_path, got.to_text()).unwrap();
            eprintln!("wrote {}", golden_path.display());
            continue;
        }
        let text = std::fs::read_to_string(&golden_path)
            .unwrap_or_else(|e| panic!("{}: {e}", golden_path.display()));
        let want = Golden::parse(&text)
            .unwrap_or_else(|| panic!("{} is not a golden", golden_path.display()));
        if let Some(d) = Golden::diff(&want, &got) {
            failures.push(format!("{name}: screen differs from its golden\n{d}"));
        }
    }
    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
}
