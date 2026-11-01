// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! The SCS parser fed one print job, then the text rendering.
//!
//! Input: an options byte (code page in bits 0 to 2, paper in bits 3 and
//! 4, the device font in bits 5 to 7), then the job.

#![no_main]

use libfuzzer_sys::fuzz_target;

/// Font IDs the device may start with: known pitches, bold, italic and an
/// unknown one.
const FONTS: [u16; 8] = [11, 85, 223, 281, 46, 92, 254, 9999];

fuzz_target!(|data: &[u8]| {
    let Some((&flags, job)) = data.split_first() else {
        return;
    };
    let ccsids: Vec<u16> = tn5250::codepage::supported().collect();
    let options = scs::Options {
        ccsid: ccsids[usize::from(flags & 7) % ccsids.len()],
        paper: scs::Paper::ALL[usize::from((flags >> 3) & 3) % scs::Paper::ALL.len()],
        font: FONTS[usize::from(flags >> 5)],
    };
    let doc = scs::parse(job, &options);
    // Every page needs at least one byte of the job.
    assert!(doc.pages.len() <= job.len());
    for page in &doc.pages {
        assert!(page.width > 0 && page.height > 0);
        for run in &page.runs {
            assert!(!run.text.is_empty());
            assert!(run.line >= 1 && run.col >= 1 && run.height > 0);
            let _ = run.char_x(run.text.chars().count());
        }
    }
    let text = scs::to_text(&doc);
    assert_eq!(
        text.matches('\u{000C}').count(),
        doc.pages.len().saturating_sub(1)
    );
});
