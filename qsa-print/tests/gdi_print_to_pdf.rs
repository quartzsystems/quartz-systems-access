// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! Windows printing through GDI, checked on "Microsoft Print to PDF": the
//! test page goes through the spooler into a PDF file, which must hold both
//! pages. The file is left in `target/gdi-print/` to look at (the Windows
//! VM test rasterises it). Skipped when that printer is not installed.

#![cfg(windows)]

use qsa_print::gdi;

const PRINTER: &str = "Microsoft Print to PDF";

#[test]
fn test_page_prints_to_microsoft_print_to_pdf() {
    if !gdi::printers().iter().any(|p| p == PRINTER) {
        eprintln!("{PRINTER} is not installed; skipped");
        return;
    }
    let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../target/gdi-print");
    std::fs::create_dir_all(&out).unwrap();
    let file = out.join("test-page.pdf");
    let _ = std::fs::remove_file(&file);
    let doc = scs::parse(&scs::sample::test_page(), &scs::Options::default());
    gdi::print(
        &doc,
        Some(PRINTER),
        "QS/400 Access test page",
        None,
        Some(&file),
    )
    .unwrap();
    // The spooler writes the file after the job is handed over.
    let mut bytes = Vec::new();
    for _ in 0..100 {
        if let Ok(b) = std::fs::read(&file)
            && b.ends_with(b"%%EOF\r\n") | b.ends_with(b"%%EOF\n") | b.ends_with(b"%%EOF")
        {
            bytes = b;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    assert!(bytes.starts_with(b"%PDF-"), "no PDF at {}", file.display());
    let pages = bytes
        .windows(10)
        .filter(|w| *w == b"/Type/Page" || *w == b"/Type /Pag")
        .count();
    assert!(pages >= 2, "{pages} page objects in {}", file.display());
}
