// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! SCS fixtures through the page model to PDF, checked against goldens.
//!
//! - Every fixture's text rendering against `goldens/<name>.txt`.
//! - Every PDF embeds the print font subset, not whole.
//! - With `pdftoppm` (from `QSA_PDFTOPPM`, else the `PATH`), every page is
//!   rasterised at 50 dpi in grey and compared, pixel for pixel, with
//!   `goldens/<name>-<page>.png`. CI's Linux job runs a pinned poppler and
//!   sets `QSA_REQUIRE_PDFTOPPM=1`; elsewhere the raster check is skipped
//!   when `pdftoppm` is missing, since another poppler draws differently.
//! - `QSA_BLESS=1` writes what was made as the new goldens.
//!
//! The PDFs and page images are left in `target/pdf-goldens/`.

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use qsa_print::pdf;
use scs::sample::Builder;
use scs::{Options, Paper};

/// Rasterising resolution, dots per inch.
const DPI: &str = "50";
/// A pixel differs when its grey level is off by more than this.
const TOLERANCE: u8 = 40;
/// A page fails when more than this many pixels differ. With poppler
/// pinned the images should be identical; a page is mostly paper, so a
/// share of its pixels would hide a changed digit.
const MAX_PIXELS: usize = 16;

struct Fixture {
    name: &'static str,
    data: Vec<u8>,
    options: Options,
}

fn hex(s: &str) -> Vec<u8> {
    let s: String = s.split_whitespace().collect();
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
        .collect()
}

fn fixtures() -> Vec<Fixture> {
    let mut out = vec![Fixture {
        name: "test-page",
        data: scs::sample::test_page(),
        options: Options::default(),
    }];

    // RFC 4777 section 11.1's print record sets up a 13.2 by 11 inch page;
    // some text follows.
    let mut rfc = hex(
        "34C4012BD20345FF2BD2044C00022BD2040D00002BD20A8501010201030204022BD20309022BD2061100014A
         402BD20601010000012BD306F60000FFFF2BD20A48000001000000010100
         2BD10705000B0090012BD2044900F02BD206404A403DE02BD2041500F034
         C4012BD10381FF002BC8034001",
    );
    let mut b = Builder::new();
    b.text("Print Key Output")
        .nl()
        .nl()
        .text("Display Device . . . . :   QPADEV0003")
        .nl();
    b.text("User . . . . . . . . . :   QSECOFR").ff();
    rfc.extend(b.bytes());
    out.push(Fixture {
        name: "rfc4777-print-record",
        data: rfc,
        options: Options::default(),
    });

    // A three-page listing: 66 lines a page, 132 columns at 15 pitch on
    // letter paper, a heading on every page.
    let mut b = Builder::new();
    b.scd(15).svf(66, 1, 66).shf(132, 1, 132, &[]);
    for page in 1..=3 {
        b.avpp(2)
            .text("5770SS1 V7R5M0  240000                    Sample listing")
            .ahpp(120)
            .text(&format!("Page {page:>4}"))
            .nl()
            .nl();
        for line in 1..=50 {
            b.text(&format!(
                "{:>6}  QSYSPRT   LISTING  {:<40}{:>12}",
                (page - 1) * 50 + line,
                format!("Detail line {line} of page {page}"),
                line * 1234
            ))
            .nl();
        }
        b.ff();
    }
    out.push(Fixture {
        name: "listing-132",
        data: b.bytes(),
        options: Options::default(),
    });

    // A4, 12 pitch: overstrike and underline in the forms they arrive in.
    let mut b = Builder::new();
    b.scd(12).svf(70, 4, 66).avpp(4).ahpp(5);
    b.text("Total ").bus().text("1,234.56").eus().nl();
    b.text("Bold by repeat").cr().text("Bold by repeat").nl();
    b.text("Underscore by backspace: A")
        .bs()
        .text("_")
        .text("B")
        .bs()
        .text("_")
        .nl();
    b.text("Mixed: X").bs().text("O").nl();
    out.push(Fixture {
        name: "a4-overstrike",
        data: b.bytes(),
        options: Options {
            paper: Paper::A4,
            font: 85,
            ..Options::default()
        },
    });
    out
}

fn pdftoppm() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("QSA_PDFTOPPM") {
        return Some(PathBuf::from(p));
    }
    let name = if cfg!(windows) {
        "pdftoppm.exe"
    } else {
        "pdftoppm"
    };
    std::env::split_paths(&std::env::var_os("PATH")?)
        .map(|d| d.join(name))
        .find(|p| p.is_file())
}

/// The `/Length1` (uncompressed size) of every embedded TrueType font.
fn font_file_lengths(pdf: &[u8]) -> Vec<usize> {
    let key = b"/Length1";
    pdf.windows(key.len())
        .enumerate()
        .filter(|(_, w)| *w == key)
        .filter_map(|(i, _)| {
            let digits: String = pdf[i + key.len()..]
                .iter()
                .skip_while(|b| b.is_ascii_whitespace())
                .take_while(|b| b.is_ascii_digit())
                .map(|&b| char::from(b))
                .collect();
            digits.parse().ok()
        })
        .collect()
}

struct Gray {
    w: u32,
    h: u32,
    px: Vec<u8>,
}

fn read_png(path: &Path) -> Result<Gray, String> {
    let f = std::fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut dec = png::Decoder::new(std::io::BufReader::new(f));
    dec.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut r = dec.read_info().map_err(|e| e.to_string())?;
    let mut buf = vec![0; r.output_buffer_size()];
    let info = r.next_frame(&mut buf).map_err(|e| e.to_string())?;
    let n = info.color_type.samples();
    let px = buf[..info.buffer_size()]
        .chunks(n)
        .map(|p| {
            if n >= 3 {
                ((u16::from(p[0]) + u16::from(p[1]) + u16::from(p[2])) / 3) as u8
            } else {
                p[0]
            }
        })
        .collect();
    Ok(Gray {
        w: info.width,
        h: info.height,
        px,
    })
}

/// The page images pdftoppm made for `prefix`, in page order.
fn pages_made(dir: &Path, stem: &str) -> Vec<PathBuf> {
    let mut pages: Vec<(u32, PathBuf)> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            let n = name
                .strip_prefix(&format!("{stem}-"))?
                .strip_suffix(".png")?;
            Some((n.parse().ok()?, e.path()))
        })
        .collect();
    pages.sort();
    pages.into_iter().map(|(_, p)| p).collect()
}

fn main() -> ExitCode {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let goldens = root.join("tests/goldens");
    let out = root.join("../target/pdf-goldens");
    let _ = std::fs::remove_dir_all(&out);
    std::fs::create_dir_all(&out).expect("output folder");
    let bless = std::env::var("QSA_BLESS").is_ok_and(|v| v == "1");
    let required = std::env::var("QSA_REQUIRE_PDFTOPPM").is_ok_and(|v| v == "1");
    let raster = pdftoppm();
    let mut failed: Vec<String> = Vec::new();

    if raster.is_none() {
        if required {
            eprintln!("pdftoppm is required (QSA_REQUIRE_PDFTOPPM=1) but was not found");
            return ExitCode::FAILURE;
        }
        println!("pdftoppm not found: page images are not compared");
    }

    for f in fixtures() {
        let doc = scs::parse(&f.data, &f.options);
        // Text.
        let text = scs::to_text(&doc);
        let txt = goldens.join(format!("{}.txt", f.name));
        if bless {
            std::fs::write(&txt, &text).expect("text golden");
        } else {
            match std::fs::read_to_string(&txt) {
                Ok(want) if want.replace("\r\n", "\n") == text => {}
                Ok(_) => {
                    std::fs::write(out.join(format!("{}.txt", f.name)), &text).ok();
                    failed.push(format!("{}: text differs from {}", f.name, txt.display()));
                }
                Err(e) => failed.push(format!("{}: {}: {e}", f.name, txt.display())),
            }
        }

        // PDF.
        let bytes = pdf::render(
            &doc,
            &pdf::Options {
                title: f.name.to_owned(),
                ..pdf::Options::default()
            },
        )
        .expect("PDF");
        let pdf_path = out.join(format!("{}.pdf", f.name));
        std::fs::write(&pdf_path, &bytes).expect("PDF file");
        if !bytes.starts_with(b"%PDF-") {
            failed.push(format!("{}: not a PDF", f.name));
        }
        // Each embedded face is a subset: its uncompressed length is far
        // well below the smallest whole face (71 KB).
        let lengths = font_file_lengths(&bytes);
        if lengths.is_empty() || lengths.iter().any(|&l| l > 60_000) {
            failed.push(format!(
                "{}: embedded font lengths {lengths:?}; not subset",
                f.name
            ));
        }

        // Pages.
        let Some(tool) = &raster else { continue };
        let stem = format!("{}-page", f.name);
        let status = Command::new(tool)
            .args(["-r", DPI, "-gray", "-png"])
            .arg(&pdf_path)
            .arg(out.join(&stem))
            .status();
        if !status.is_ok_and(|s| s.success()) {
            failed.push(format!("{}: pdftoppm failed", f.name));
            continue;
        }
        let made = pages_made(&out, &stem);
        if made.len() != doc.pages.len().max(1) {
            failed.push(format!(
                "{}: {} page images for {} pages",
                f.name,
                made.len(),
                doc.pages.len()
            ));
        }
        for (i, page) in made.iter().enumerate() {
            let golden = goldens.join(format!("{}-{}.png", f.name, i + 1));
            if bless {
                std::fs::copy(page, &golden).expect("page golden");
                continue;
            }
            let (got, want) = match (read_png(page), read_png(&golden)) {
                (Ok(g), Ok(w)) => (g, w),
                (Err(e), _) | (_, Err(e)) => {
                    failed.push(format!("{} page {}: {e}", f.name, i + 1));
                    continue;
                }
            };
            if (got.w, got.h) != (want.w, want.h) {
                failed.push(format!(
                    "{} page {}: {}x{}, golden {}x{}",
                    f.name,
                    i + 1,
                    got.w,
                    got.h,
                    want.w,
                    want.h
                ));
                continue;
            }
            let off = got
                .px
                .iter()
                .zip(&want.px)
                .filter(|(a, b)| a.abs_diff(**b) > TOLERANCE)
                .count();
            if off > MAX_PIXELS {
                failed.push(format!("{} page {}: {off} pixels differ", f.name, i + 1));
            }
        }
    }

    if failed.is_empty() {
        println!("PDF goldens: all fixtures match");
        ExitCode::SUCCESS
    } else {
        for f in &failed {
            eprintln!("FAILED  {f}");
        }
        ExitCode::FAILURE
    }
}
