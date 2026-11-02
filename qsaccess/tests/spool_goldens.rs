// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! Page-model goldens for Printer Output: QS/400's test system's SCS files
//! `QPJOBLOG`, `QSYSPRT`, `LANDSCAPE` and `MIXED`, read through qap-client
//! in batches of 25 pages and written as received (each later batch's
//! leading state included), must lay out exactly as the same files read
//! whole, as a 5250 printer session lays them out:
//!
//! - the same page model and text, checked also against
//!   `goldens/spool/<name>.pages` and `goldens/spool/<name>.txt`;
//! - the same PDF pages: with `pdftoppm` (`QSA_PDFTOPPM`, else the `PATH`;
//!   required where `QSA_REQUIRE_PDFTOPPM=1`, as in CI's Linux job) both
//!   PDFs are rasterised at 50 dpi and compared page for page. The PDFs
//!   differ in their creation time only.
//!
//! `QSA_BLESS=1` writes the text and page-model goldens. The PDFs are left
//! in `target/spool-goldens/`.

#[path = "../../qap-client/tests/common/mod.rs"]
mod common;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::AtomicBool;

use common::{cfg, start};
use qap_client::proto::ops::spool::{ListSpooledFiles, ReadFormat, ReadSpooledFile, SpooledData};
use qap_client::spool::{self, ReadEnd};
use qap_client::{Client, Secret};
use qsaccess::splf::model;

const FILES: [&str; 4] = ["QPJOBLOG", "QSYSPRT", "LANDSCAPE", "MIXED"];

fn goldens() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/goldens/spool")
}

fn out_dir() -> PathBuf {
    let d = Path::new(env!("CARGO_MANIFEST_DIR")).join("../target/spool-goldens");
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// The page model as text, as qsa-print's PDF goldens write it.
fn page_model(doc: &scs::Document) -> String {
    use std::fmt::Write as _;
    let mut out = String::new();
    for (i, p) in doc.pages.iter().enumerate() {
        let m = &p.margins;
        let _ = writeln!(
            out,
            "page {} {}x{} {:?} margins {} {} {} {}",
            i + 1,
            p.width,
            p.height,
            p.orientation,
            m.left,
            m.top,
            m.right,
            m.bottom
        );
        for r in &p.runs {
            let _ = writeln!(
                out,
                "  {},{} h{} p{} f{}{}{}{} L{} C{} |{}|",
                r.x,
                r.y,
                r.height,
                r.pitch.0,
                r.font_id,
                if r.bold { " bold" } else { "" },
                if r.italic { " italic" } else { "" },
                if r.underline { " underline" } else { "" },
                r.line,
                r.col,
                r.text
            );
        }
    }
    out
}

fn check_golden(name: &str, got: &str) {
    let path = goldens().join(name);
    if std::env::var_os("QSA_BLESS").is_some() {
        std::fs::create_dir_all(goldens()).unwrap();
        std::fs::write(&path, got).unwrap();
        return;
    }
    let want = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{}: {e}; bless with QSA_BLESS=1", path.display()))
        .replace("\r\n", "\n");
    assert!(want == got, "{name} differs from its golden");
}

/// `pdftoppm`, if there is one.
fn pdftoppm() -> Option<PathBuf> {
    let exe = std::env::var_os("QSA_PDFTOPPM")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("pdftoppm"));
    let ok = Command::new(&exe)
        .arg("-v")
        .output()
        .is_ok_and(|o| o.status.success() || !o.stderr.is_empty());
    if !ok && std::env::var_os("QSA_REQUIRE_PDFTOPPM").is_some() {
        panic!("QSA_REQUIRE_PDFTOPPM is set but pdftoppm cannot be run");
    }
    ok.then_some(exe)
}

/// The pages of `pdf`, rasterised at 50 dpi in grey: their PGM bytes.
fn raster(exe: &Path, pdf: &Path) -> Vec<Vec<u8>> {
    let prefix = pdf.with_extension("");
    let st = Command::new(exe)
        .args(["-r", "50", "-gray"])
        .arg(pdf)
        .arg(&prefix)
        .status()
        .unwrap();
    assert!(st.success(), "pdftoppm {}", pdf.display());
    let stem = prefix.file_name().unwrap().to_string_lossy().into_owned();
    let mut pages: Vec<PathBuf> = std::fs::read_dir(pdf.parent().unwrap())
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            let n = p.file_name().unwrap().to_string_lossy();
            n.starts_with(&format!("{stem}-")) && n.ends_with(".pgm")
        })
        .collect();
    pages.sort();
    pages.iter().map(|p| std::fs::read(p).unwrap()).collect()
}

#[test]
fn batches_lay_out_as_the_whole_file() {
    let s = start(cfg());
    let mut c = Client::new(s.target());
    c.sign_on("BOB", &Secret::from("SECRET1")).unwrap();
    let all = c
        .all_spooled_files(&ListSpooledFiles {
            user: "*CURRENT".into(),
            ..ListSpooledFiles::default()
        })
        .unwrap();
    let options = model::scs_options(37, qsa_config::qsp::Paper::Letter);
    let pdf_options = |name: &str| qsa_print::pdf::Options {
        title: name.to_owned(),
        margin: None,
        paper: options.paper,
    };
    let exe = pdftoppm();
    let out = out_dir();
    for name in FILES {
        let f = all.iter().find(|f| f.id.file == name).unwrap();
        // Whole, in one request.
        let req = ReadSpooledFile {
            id: f.id.clone(),
            format: ReadFormat::Scs,
            first_page: 1,
            last_page: 0,
        };
        let whole = c
            .read_spooled_file(&req)
            .unwrap()
            .wait_done()
            .unwrap()
            .decode::<SpooledData>()
            .unwrap()
            .data;
        // In batches, written as received.
        let mut batched = Vec::new();
        let end = spool::read(
            |r| c.read_spooled_file(r),
            &f.id,
            ReadFormat::Scs,
            spool::BATCH_PAGES,
            &mut batched,
            |_| {},
            &AtomicBool::new(false),
        )
        .unwrap();
        assert!(matches!(end, ReadEnd::Done { .. }), "{name}");
        if f.pages > spool::BATCH_PAGES {
            assert!(
                batched.len() > whole.len(),
                "{name}: a later batch's state is there"
            );
        }
        let a = scs::parse(&whole, &options);
        let b = scs::parse(&batched, &options);
        assert_eq!(
            a.pages.len(),
            f.pages as usize,
            "{name}: the system's page count"
        );
        let (ma, mb) = (page_model(&a), page_model(&b));
        assert!(ma == mb, "{name}: the batched page model differs");
        let (ta, tb) = (scs::to_text(&a), scs::to_text(&b));
        assert_eq!(ta, tb, "{name}");
        check_golden(&format!("{name}.pages"), &ma);
        check_golden(&format!("{name}.txt"), &ta);

        let pa = qsa_print::pdf::render(&a, &pdf_options(name)).unwrap();
        let pb = qsa_print::pdf::render(&b, &pdf_options(name)).unwrap();
        let (wa, wb) = (
            out.join(format!("{name}-whole.pdf")),
            out.join(format!("{name}-batched.pdf")),
        );
        std::fs::write(&wa, &pa).unwrap();
        std::fs::write(&wb, &pb).unwrap();
        if let Some(exe) = &exe {
            let (ra, rb) = (raster(exe, &wa), raster(exe, &wb));
            assert_eq!(ra.len(), a.pages.len(), "{name}: PDF pages");
            assert!(ra == rb, "{name}: the batched PDF's pages differ");
        }
    }
    // The landscape file is laid out landscape, and MIXED changes pitch.
    let land = all.iter().find(|f| f.id.file == "LANDSCAPE").unwrap();
    let doc = {
        let mut sink = Vec::new();
        spool::read(
            |r| c.read_spooled_file(r),
            &land.id,
            ReadFormat::Scs,
            1,
            &mut sink,
            |_| {},
            &AtomicBool::new(false),
        )
        .unwrap();
        scs::parse(&sink, &options)
    };
    assert!(doc.pages.iter().all(|p| p.width > p.height));
}
