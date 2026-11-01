// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! Printing the page model through the Windows spooler with GDI.
//!
//! - The print font is loaded for this process only
//!   (`AddFontMemResourceEx`); it is never installed.
//! - Each page gets its own orientation and paper size (`ResetDCW` before
//!   the page).
//! - Text is placed as in the PDF ([`crate::pdf::fit`]): a margin kept
//!   clear on every side of the paper, text inside it where the data put
//!   it. Positions are measured from the paper's corner, so output on paper,
//!   to "Microsoft Print to PDF" and in the session's own PDFs agree. Every
//!   character's advance is given (`ExtTextOutW`), so columns stay exact
//!   whatever the font's rasterised widths.

use std::ffi::c_void;
use std::sync::OnceLock;

use scs::{Document, Orientation, Page, Pitch};
use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Gdi::{
    AddFontMemResourceEx, CLIP_DEFAULT_PRECIS, CreateDCW, CreateFontW, CreatePen, DEVMODEW,
    DM_ORIENTATION, DM_PAPERLENGTH, DM_PAPERSIZE, DM_PAPERWIDTH, DMORIENT_LANDSCAPE,
    DMORIENT_PORTRAIT, DeleteDC, DeleteObject, ETO_OPTIONS, ExtTextOutW, FF_MODERN, FIXED_PITCH,
    FONT_CHARSET, FW_BOLD, FW_NORMAL, GetDeviceCaps, HDC, HFONT, LOGPIXELSX, LOGPIXELSY, LineTo,
    MoveToEx, OUT_TT_ONLY_PRECIS, PHYSICALOFFSETX, PHYSICALOFFSETY, PROOF_QUALITY, PS_SOLID,
    ResetDCW, SelectObject, SetBkMode, SetTextAlign, TA_BASELINE, TA_LEFT, TRANSPARENT,
};
use windows::Win32::Graphics::Printing::{
    ClosePrinter, DocumentPropertiesW, EnumPrintersW, GetDefaultPrinterW, OpenPrinterW,
    PRINTER_ENUM_CONNECTIONS, PRINTER_ENUM_LOCAL, PRINTER_HANDLE, PRINTER_INFO_4W,
};
use windows::Win32::Storage::Xps::{AbortDoc, DOCINFOW, EndDoc, EndPage, StartDocW, StartPage};
use windows::Win32::UI::Controls::Dialogs::{
    DEVNAMES, PD_PRINTSETUP, PD_RETURNDEFAULT, PRINTDLGW, PrintDlgW,
};
use windows::core::{HSTRING, PCWSTR, PWSTR, w};

use crate::font::{self, FAMILY, Face};
use crate::pdf::{DEFAULT_MARGIN, fit};

/// Printing failed.
#[derive(Debug, thiserror::Error)]
pub enum PrintError {
    #[error("no printer is set up on this computer")]
    NoPrinter,
    #[error("printer {0} cannot be opened")]
    Open(String),
    #[error("the printer did not start the document")]
    StartDoc,
    #[error("page {0} could not be printed")]
    Page(usize),
}

/// The process-wide private copy of the print font.
fn load_fonts() {
    static LOADED: OnceLock<()> = OnceLock::new();
    LOADED.get_or_init(|| {
        for face in Face::ALL {
            let bytes = face.bytes();
            let mut count = 0u32;
            // SAFETY: the bytes are 'static and valid for the call; GDI
            // copies them. The handle is kept for the process's life. The
            // binding types the font count as const, but Windows writes it,
            // so the pointer comes from a mutable place.
            let h = unsafe {
                AddFontMemResourceEx(
                    bytes.as_ptr() as *const c_void,
                    bytes.len() as u32,
                    None,
                    (&raw mut count).cast_const(),
                )
            };
            if h.is_invalid() {
                tn5250::warn!("print: the {face:?} print font could not be loaded");
            }
        }
    });
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// The printers this user can print to.
pub fn printers() -> Vec<String> {
    let flags = PRINTER_ENUM_LOCAL | PRINTER_ENUM_CONNECTIONS;
    let mut needed = 0u32;
    let mut count = 0u32;
    // SAFETY: a size query with no buffer.
    let _ = unsafe { EnumPrintersW(flags, PCWSTR::null(), 4, None, &mut needed, &mut count) };
    if needed == 0 {
        return Vec::new();
    }
    let mut buf = vec![0u8; needed as usize];
    // SAFETY: the buffer is the size asked for.
    if unsafe {
        EnumPrintersW(
            flags,
            PCWSTR::null(),
            4,
            Some(&mut buf),
            &mut needed,
            &mut count,
        )
    }
    .is_err()
    {
        return Vec::new();
    }
    // SAFETY: EnumPrintersW wrote `count` PRINTER_INFO_4W at the start of
    // the buffer; their strings point into it.
    let infos = unsafe {
        std::slice::from_raw_parts(buf.as_ptr() as *const PRINTER_INFO_4W, count as usize)
    };
    infos
        .iter()
        // SAFETY: a NUL-terminated string in `buf`.
        .filter_map(|i| unsafe { i.pPrinterName.to_string() }.ok())
        .collect()
}

/// The default printer, if there is one.
pub fn default_printer() -> Option<String> {
    let mut len = 0u32;
    // SAFETY: a size query.
    let _ = unsafe { GetDefaultPrinterW(None, &mut len) };
    if len == 0 {
        return None;
    }
    let mut buf = vec![0u16; len as usize];
    // SAFETY: the buffer holds `len` characters.
    if !unsafe { GetDefaultPrinterW(Some(PWSTR(buf.as_mut_ptr())), &mut len) }.as_bool() {
        return None;
    }
    String::from_utf16(&buf[..buf.iter().position(|&c| c == 0).unwrap_or(buf.len())]).ok()
}

/// The print setup common dialog, owned by window `owner`. Returns the
/// printer chosen, or `None` when cancelled.
pub fn print_setup(owner: isize) -> Option<String> {
    let mut pd = PRINTDLGW {
        lStructSize: std::mem::size_of::<PRINTDLGW>() as u32,
        hwndOwner: HWND(owner as *mut c_void),
        Flags: PD_PRINTSETUP,
        ..Default::default()
    };
    // SAFETY: a valid PRINTDLGW.
    if !unsafe { PrintDlgW(&mut pd) }.as_bool() {
        return None;
    }
    device_name(&pd)
}

/// The printer named in a print dialog's result; frees its memory.
fn device_name(pd: &PRINTDLGW) -> Option<String> {
    use windows::Win32::Foundation::{GlobalFree, HGLOBAL};
    use windows::Win32::System::Memory::{GlobalLock, GlobalUnlock};
    let names = pd.hDevNames;
    let mut out = None;
    if !names.is_invalid() {
        // SAFETY: hDevNames is a movable global holding a DEVNAMES and its
        // strings, as the dialog documents.
        unsafe {
            let p = GlobalLock(names) as *const DEVNAMES;
            if !p.is_null() {
                let base = p as *const u16;
                let name = PCWSTR(base.add((*p).wDeviceOffset as usize));
                out = name.to_string().ok();
                let _ = GlobalUnlock(names);
            }
        }
    }
    for h in [pd.hDevNames, pd.hDevMode] {
        if !h.is_invalid() {
            // SAFETY: the dialog allocated these for us to free.
            let _ = unsafe { GlobalFree(Some(HGLOBAL(h.0))) };
        }
    }
    out.filter(|n| !n.is_empty())
}

/// Whether printing is possible at all: a default printer, or the print
/// dialog can find one.
pub fn available() -> bool {
    if default_printer().is_some() {
        return true;
    }
    let mut pd = PRINTDLGW {
        lStructSize: std::mem::size_of::<PRINTDLGW>() as u32,
        Flags: PD_RETURNDEFAULT,
        ..Default::default()
    };
    // SAFETY: a valid PRINTDLGW; PD_RETURNDEFAULT shows nothing.
    unsafe { PrintDlgW(&mut pd) }.as_bool() && device_name(&pd).is_some()
}

/// An open printer's default settings, to change page by page.
struct DevMode {
    buf: Vec<u8>,
}

impl DevMode {
    fn of(printer: &str) -> Result<DevMode, PrintError> {
        let name = HSTRING::from(printer);
        let mut h = PRINTER_HANDLE::default();
        // SAFETY: a valid name; the handle is closed below.
        unsafe { OpenPrinterW(&name, &mut h, None) }
            .map_err(|_| PrintError::Open(printer.to_owned()))?;
        // SAFETY: size query, then a buffer that size; DM_OUT_BUFFER = 2.
        let size = unsafe { DocumentPropertiesW(None, h, &name, None, None, 0) };
        let result = if size <= 0 {
            Err(PrintError::Open(printer.to_owned()))
        } else {
            let mut buf = vec![0u8; size as usize];
            let r = unsafe {
                DocumentPropertiesW(
                    None,
                    h,
                    &name,
                    Some(buf.as_mut_ptr() as *mut DEVMODEW),
                    None,
                    2,
                )
            };
            if r < 0 {
                Err(PrintError::Open(printer.to_owned()))
            } else {
                Ok(DevMode { buf })
            }
        };
        // SAFETY: opened above.
        let _ = unsafe { ClosePrinter(h) };
        result
    }

    fn get(&mut self) -> &mut DEVMODEW {
        // SAFETY: the buffer holds a DEVMODEW (and the driver's extra
        // bytes after it), as DocumentPropertiesW wrote it.
        unsafe { &mut *(self.buf.as_mut_ptr() as *mut DEVMODEW) }
    }

    /// Sets the orientation and paper for `page`.
    fn set_page(&mut self, page: &Page) {
        let dm = self.get();
        let (w, h) = match page.orientation {
            Orientation::Portrait => (page.width, page.height),
            Orientation::Landscape => (page.height, page.width),
        };
        // The printer members of the DEVMODE unions.
        {
            dm.Anonymous1.Anonymous1.dmOrientation = match page.orientation {
                Orientation::Portrait => DMORIENT_PORTRAIT as i16,
                Orientation::Landscape => DMORIENT_LANDSCAPE as i16,
            };
            match standard_paper(w, h) {
                Some(code) => {
                    dm.Anonymous1.Anonymous1.dmPaperSize = code;
                    dm.dmFields |= DM_ORIENTATION | DM_PAPERSIZE;
                }
                None => {
                    // Tenths of a millimetre.
                    let tenth_mm = |t: u32| (f64::from(t) * 254.0 / 1440.0).round() as i16;
                    dm.Anonymous1.Anonymous1.dmPaperSize = 0;
                    dm.Anonymous1.Anonymous1.dmPaperWidth = tenth_mm(w);
                    dm.Anonymous1.Anonymous1.dmPaperLength = tenth_mm(h);
                    dm.dmFields |= DM_ORIENTATION | DM_PAPERSIZE | DM_PAPERWIDTH | DM_PAPERLENGTH;
                }
            }
        }
    }
}

/// The Windows paper size code for a portrait page, if it is a standard
/// one (within a millimetre).
fn standard_paper(w: u32, h: u32) -> Option<i16> {
    let near = |a: u32, b: u32| a.abs_diff(b) <= 57;
    [(12240, 15840, 1), (12240, 20160, 5), (11906, 16838, 9)]
        .into_iter()
        .find(|&(pw, ph, _)| near(w, pw) && near(h, ph))
        .map(|(_, _, code)| code)
}

/// A printer device context, deleted when dropped.
struct Dc(HDC);

impl Drop for Dc {
    fn drop(&mut self) {
        // SAFETY: created by CreateDCW.
        let _ = unsafe { DeleteDC(self.0) };
    }
}

/// Prints `doc` on `printer` (the default printer when `None`) as one
/// spooler document titled `title`, keeping `margin` (1/1440 inch; `None`
/// for [`DEFAULT_MARGIN`]) clear. With `to_file`, the printer's output goes
/// to that file instead of the port ("Microsoft Print to PDF" writes the
/// PDF there without asking).
pub fn print(
    doc: &Document,
    printer: Option<&str>,
    title: &str,
    margin: Option<u32>,
    to_file: Option<&std::path::Path>,
) -> Result<(), PrintError> {
    load_fonts();
    let name = match printer.filter(|p| !p.trim().is_empty()) {
        Some(p) => p.to_owned(),
        None => default_printer().ok_or(PrintError::NoPrinter)?,
    };
    let mut dm = DevMode::of(&name)?;
    if let Some(first) = doc.pages.first() {
        dm.set_page(first);
    }
    let wname = HSTRING::from(name.as_str());
    // SAFETY: valid strings and DEVMODE; the DC is deleted by `Dc`.
    let hdc = unsafe { CreateDCW(w!("WINSPOOL"), &wname, PCWSTR::null(), Some(dm.get())) };
    if hdc.is_invalid() {
        return Err(PrintError::Open(name));
    }
    let dc = Dc(hdc);
    let wtitle = wide(title);
    let wfile = to_file.map(|p| wide(&p.to_string_lossy()));
    let info = DOCINFOW {
        cbSize: std::mem::size_of::<DOCINFOW>() as i32,
        lpszDocName: PCWSTR(wtitle.as_ptr()),
        lpszOutput: wfile
            .as_ref()
            .map_or(PCWSTR::null(), |f| PCWSTR(f.as_ptr())),
        ..Default::default()
    };
    // SAFETY: a valid DC and DOCINFOW.
    if unsafe { StartDocW(dc.0, &info) } <= 0 {
        return Err(PrintError::StartDoc);
    }
    for (i, page) in doc.pages.iter().enumerate() {
        if i > 0 {
            dm.set_page(page);
            // SAFETY: between pages, as ResetDCW requires.
            let _ = unsafe { ResetDCW(dc.0, dm.get()) };
        }
        // SAFETY: inside the document.
        if unsafe { StartPage(dc.0) } <= 0 {
            // SAFETY: the document was started.
            unsafe { AbortDoc(dc.0) };
            return Err(PrintError::Page(i + 1));
        }
        draw_page(dc.0, page, margin.unwrap_or(DEFAULT_MARGIN));
        // SAFETY: the page was started.
        if unsafe { EndPage(dc.0) } <= 0 {
            unsafe { AbortDoc(dc.0) };
            return Err(PrintError::Page(i + 1));
        }
    }
    // SAFETY: the document was started.
    if unsafe { EndDoc(dc.0) } <= 0 {
        return Err(PrintError::StartDoc);
    }
    Ok(())
}

fn create_font(face: Face, pitch: Pitch, scale: f64, dpi_y: i32) -> HFONT {
    let height = -(font::em(pitch) * scale * f64::from(dpi_y) / 1440.0).round() as i32;
    let family = HSTRING::from(FAMILY);
    // SAFETY: plain values and a valid face name.
    unsafe {
        CreateFontW(
            height,
            0,
            0,
            0,
            if face == Face::Bold {
                FW_BOLD.0 as i32
            } else {
                FW_NORMAL.0 as i32
            },
            u32::from(face == Face::Italic),
            0,
            0,
            FONT_CHARSET(1),
            OUT_TT_ONLY_PRECIS,
            CLIP_DEFAULT_PRECIS,
            PROOF_QUALITY,
            (FIXED_PITCH.0 as u32) | (FF_MODERN.0 as u32),
            &family,
        )
    }
}

fn draw_page(hdc: HDC, page: &Page, margin: u32) {
    let fit = fit(page, margin);
    // SAFETY: a valid printer DC throughout.
    unsafe {
        let dpi_x = f64::from(GetDeviceCaps(Some(hdc), LOGPIXELSX));
        let dpi_y_i = GetDeviceCaps(Some(hdc), LOGPIXELSY);
        let dpi_y = f64::from(dpi_y_i);
        // The device's origin is the printable area's corner; positions
        // are the paper's.
        let off_x = GetDeviceCaps(Some(hdc), PHYSICALOFFSETX);
        let off_y = GetDeviceCaps(Some(hdc), PHYSICALOFFSETY);
        let px = |t: f64| (t * dpi_x / 1440.0).round() as i32 - off_x;
        let py = |t: f64| (t * dpi_y / 1440.0).round() as i32 - off_y;
        SetBkMode(hdc, TRANSPARENT);
        SetTextAlign(hdc, TA_BASELINE | TA_LEFT);
        let mut fonts: Vec<((Face, Pitch), HFONT)> = Vec::new();
        for run in &page.runs {
            let key = (Face::of(run), run.pitch);
            let hf = match fonts.iter().find(|(k, _)| *k == key) {
                Some((_, f)) => *f,
                None => {
                    let f = create_font(key.0, key.1, fit.scale, dpi_y_i);
                    fonts.push((key, f));
                    f
                }
            };
            let old = SelectObject(hdc, hf.into());
            let x0 = fit.x(f64::from(run.x));
            let base = fit.y(f64::from(run.y) + font::baseline(run.height, run.pitch));
            let text: Vec<u16> = run.text.encode_utf16().collect();
            // Each character's advance, so that rounding never builds up.
            let edges: Vec<i32> = (0..=text.len())
                .map(|i| px(x0 + i as f64 * run.pitch.advance() * fit.scale))
                .collect();
            let dx: Vec<i32> = edges.windows(2).map(|w| w[1] - w[0]).collect();
            let _ = ExtTextOutW(
                hdc,
                edges[0],
                py(base),
                ETO_OPTIONS(0),
                None,
                PCWSTR(text.as_ptr()),
                text.len() as u32,
                Some(dx.as_ptr()),
            );
            if run.underline {
                let (below, thick) = font::underline(run.pitch);
                let width = (thick * fit.scale * dpi_y / 1440.0).round() as i32;
                let pen = CreatePen(PS_SOLID, width.max(1), Default::default());
                let old_pen = SelectObject(hdc, pen.into());
                let y = py(base + below * fit.scale);
                let _ = MoveToEx(hdc, edges[0], y, None);
                let _ = LineTo(hdc, *edges.last().unwrap_or(&edges[0]), y);
                SelectObject(hdc, old_pen);
                let _ = DeleteObject(pen.into());
            }
            SelectObject(hdc, old);
        }
        for (_, f) in fonts {
            let _ = DeleteObject(f.into());
        }
    }
}
