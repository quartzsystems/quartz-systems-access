// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! Print Screen: the screen as a one-page PDF, drawn with the console font
//! as vector rectangles traced from its bitmaps, so it stays sharp at any
//! zoom. The grid, colours and attributes are those of the display.

use std::fmt::Write as _;

use qsa_ui::console::{self, Canvas, GLYPH_H, GLYPH_W, Layout, Look, Paint};
use tn5250::screen::Screen;

/// A4 landscape, in points.
const PAGE_W: f32 = 842.0;
const PAGE_H: f32 = 595.0;
const MARGIN: f32 = 36.0;

/// The screen's cells as the display paints them (cursor not drawn).
pub fn paints(s: &Screen) -> Vec<Paint> {
    (0..s.rows())
        .flat_map(|r| (0..s.cols()).map(move |c| (r, c)))
        .map(|(r, c)| Paint::of(s.cell(r, c)))
        .collect()
}

/// One rectangle per run of same-coloured pixels on a scanline, runs that
/// repeat on the next scanlines merged into one taller rectangle. Pixels of
/// the background colour are left out (the page is filled with it first).
/// `(x, y, w, h, rgb)` in glyph pixels from the top left.
pub fn trace(c: &Canvas, bg: u32) -> Vec<(usize, usize, usize, usize, u32)> {
    let mut open: Vec<(usize, usize, usize, usize, u32)> = Vec::new();
    let mut done = Vec::new();
    for y in 0..c.h {
        let mut runs = Vec::new();
        let mut x = 0;
        while x < c.w {
            let col = c.at(x, y);
            let start = x;
            while x < c.w && c.at(x, y) == col {
                x += 1;
            }
            if col != bg {
                runs.push((start, x - start, col));
            }
        }
        let mut next = Vec::with_capacity(runs.len());
        for (rx, rw, col) in runs {
            match open
                .iter()
                .position(|r| r.0 == rx && r.2 == rw && r.4 == col && r.1 + r.3 == y)
            {
                Some(i) => {
                    let mut r = open.swap_remove(i);
                    r.3 += 1;
                    next.push(r);
                }
                None => next.push((rx, y, rw, 1, col)),
            }
        }
        done.append(&mut open);
        open = next;
    }
    done.append(&mut open);
    done
}

fn rgb(c: u32) -> String {
    let f = |v: u32| f32::from((v & 0xFF) as u8) / 255.0;
    format!("{:.3} {:.3} {:.3} rg", f(c >> 16), f(c >> 8), f(c))
}

/// The PDF for a screen.
pub fn pdf(s: &Screen, look: Look, title: &str) -> Vec<u8> {
    let (rows, cols) = (s.rows(), s.cols());
    let (gw, gh) = (cols * GLYPH_W, rows * GLYPH_H);
    let mut canvas = Canvas::new(gw, gh, look.scheme.background());
    let l = Layout::fit(gw, gh, rows, cols, 0);
    console::render(&mut canvas, l, &paints(s), None, look);
    let k = ((PAGE_W - 2.0 * MARGIN) / gw as f32).min((PAGE_H - 2.0 * MARGIN) / gh as f32);
    let (ox, oy) = (
        (PAGE_W - gw as f32 * k) / 2.0,
        (PAGE_H - gh as f32 * k) / 2.0,
    );
    let bg = look.scheme.background();
    let mut content = String::new();
    let _ = writeln!(content, "{}", rgb(bg));
    let _ = writeln!(
        content,
        "{ox:.3} {oy:.3} {:.3} {:.3} re f",
        gw as f32 * k,
        gh as f32 * k
    );
    let mut last = None;
    for (x, y, w, h, col) in trace(&canvas, bg) {
        if last != Some(col) {
            let _ = writeln!(content, "{}", rgb(col));
            last = Some(col);
        }
        let px = ox + x as f32 * k;
        let py = PAGE_H - oy - (y + h) as f32 * k;
        let _ = writeln!(
            content,
            "{px:.3} {py:.3} {:.3} {:.3} re f",
            w as f32 * k,
            h as f32 * k
        );
    }
    let title: String = title
        .chars()
        .filter(|c| c.is_ascii() && !matches!(c, '(' | ')' | '\\'))
        .collect();
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {PAGE_W} {PAGE_H}] /Contents 4 0 R /Resources << >> >>"
        ),
        format!(
            "<< /Length {} >>\nstream\n{content}endstream",
            content.len()
        ),
        format!("<< /Title ({title}) /Producer (QS/400 Access for Desktop) >>"),
    ];
    let mut out = b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let mut offsets = Vec::new();
    for (i, o) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n{o}\nendobj\n", i + 1).as_bytes());
    }
    let xref = out.len();
    let mut tail = format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1);
    for o in offsets {
        let _ = writeln!(tail, "{o:010} 00000 n ");
    }
    let _ = write!(
        tail,
        "trailer\n<< /Size {} /Root 1 0 R /Info 5 0 R >>\nstartxref\n{xref}\n%%EOF\n",
        objects.len() + 1
    );
    out.extend_from_slice(tail.as_bytes());
    out
}

/// Where Print Screen writes: the Documents folder (or the home folder),
/// `QS400 Access/Screens/<system> <date> <time>.pdf`, in UTC.
pub fn output_path(system: &str) -> std::path::PathBuf {
    let base = directories::UserDirs::new()
        .and_then(|u| {
            u.document_dir()
                .map(|d| d.to_owned())
                .or(Some(u.home_dir().to_owned()))
        })
        .unwrap_or_else(std::env::temp_dir);
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let safe: String = system
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    base.join("QS400 Access")
        .join("Screens")
        .join(format!("{safe} {}.pdf", stamp(secs)))
}

/// `YYYY-MM-DD HHMMSS` for seconds since 1970, UTC.
fn stamp(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    // Days to a civil date (proleptic Gregorian).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}-{m:02}-{d:02} {:02}{:02}{:02}",
        rem / 3600,
        rem / 60 % 60,
        rem % 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use tn5250::negotiate::ScreenSize;
    use tn5250::record::Record;
    use tn5250::terminal::Terminal;

    #[test]
    fn traced_rectangles_cover_exactly_the_lit_pixels() {
        let mut c = Canvas::new(GLYPH_W * 3, GLYPH_H, 0);
        let l = Layout::fit(GLYPH_W * 3, GLYPH_H, 1, 3, 0);
        let p = [
            Paint::text('H', console::Color::Green),
            Paint::text('#', console::Color::White),
            Paint::text('_', console::Color::Red),
        ];
        console::render(&mut c, l, &p, None, Look::default());
        let rects = trace(&c, 0);
        let mut back = Canvas::new(c.w, c.h, 0);
        for &(x, y, w, h, col) in &rects {
            back.fill(x, y, w, h, col);
        }
        assert_eq!(back, c);
        let lit = c.px.chunks(3).filter(|p| p != &[0, 0, 0]).count();
        assert!(rects.len() < lit, "runs are merged");
    }

    #[test]
    fn a_pdf_of_a_screen() {
        let mut t = Terminal::new(ScreenSize::S24x80, 37);
        let page = tn5250::codepage::default_page();
        let mut d = vec![0x04, 0x40, 0x04, 0x11, 0, 0, 0x11, 1, 1, 0x22];
        d.extend(page.encode("Sign On"));
        t.receive(&Record::new(3, d));
        let out = pdf(&t.screen, Look::default(), "QS400 (main)");
        let text = String::from_utf8_lossy(&out);
        assert!(text.starts_with("%PDF-1.4"));
        assert!(text.ends_with("%%EOF\n"));
        assert!(text.contains("/Title (QS400 main)"));
        assert!(text.contains("0.902 0.902 0.902 rg"), "white text");
        assert!(text.matches(" re f").count() > 20);
        // The xref offsets point at the objects.
        let xref = text.find("xref\n").unwrap();
        let first = text[xref..].lines().nth(3).unwrap();
        let off: usize = first[..10].parse().unwrap();
        assert!(out[off..].starts_with(b"1 0 obj"), "offsets count bytes");
    }

    #[test]
    fn stamps() {
        assert_eq!(stamp(0), "1970-01-01 000000");
        assert_eq!(stamp(1_793_534_400), "2026-11-01 120000");
        assert_eq!(stamp(951_782_400), "2000-02-29 000000");
    }
}
