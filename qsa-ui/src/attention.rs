// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! Windows that come up without taking the keyboard, and ask to be looked
//! at instead: for news that arrives while someone types elsewhere, such
//! as a message arriving while they work in a 5250 session.
//!
//! - Windows: the window is made minimised without being activated
//!   (`SW_SHOWMINNOACTIVE`, as FLTK does for a window iconized before it
//!   is shown), then restored with `SW_SHOWNOACTIVATE`. Its taskbar entry
//!   flashes until it is brought forward (`FlashWindowEx`).
//! - X11: the window is made iconic, given `_NET_WM_USER_TIME` 0, which
//!   tells an EWMH window manager not to focus it, then mapped raised; its
//!   urgency hint is set.
//! - Wayland: there is no way to map a window without focus or to mark it
//!   urgent that every compositor honours; the window is shown and the
//!   compositor decides.

use fltk::prelude::*;
use fltk::window::Window;

/// Shows `win`, not yet shown, without taking the focus from the window
/// that has it. A window shown already is left where it is.
pub fn show_without_focus(win: &mut Window) {
    if win.shown() {
        return;
    }
    imp::show_without_focus(win);
}

/// Asks for `win` to be looked at without bringing it forward: its taskbar
/// entry flashes (Windows) or it is marked urgent (X11).
pub fn ask_attention(win: &Window) {
    if win.shown() {
        imp::ask_attention(win);
    }
}

#[cfg(windows)]
mod imp {
    use fltk::prelude::*;
    use fltk::window::Window;
    use windows_sys::Win32::Foundation::HWND;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        FLASHW_TIMERNOFG, FLASHW_TRAY, FLASHWINFO, FlashWindowEx, SW_SHOWNOACTIVATE, ShowWindow,
    };

    fn hwnd(win: &Window) -> HWND {
        win.raw_handle() as HWND
    }

    pub fn show_without_focus(win: &mut Window) {
        // Made minimised and not activated, then restored as it is.
        win.iconize();
        let h = hwnd(win);
        if h.is_null() {
            return;
        }
        // SAFETY: a window this process created and still owns.
        unsafe {
            ShowWindow(h, SW_SHOWNOACTIVATE);
        }
    }

    pub fn ask_attention(win: &Window) {
        let h = hwnd(win);
        if h.is_null() {
            return;
        }
        let info = FLASHWINFO {
            cbSize: std::mem::size_of::<FLASHWINFO>() as u32,
            hwnd: h,
            dwFlags: FLASHW_TRAY | FLASHW_TIMERNOFG,
            uCount: 0,
            dwTimeout: 0,
        };
        // SAFETY: a window this process owns; `info` outlives the call.
        unsafe {
            FlashWindowEx(&info);
        }
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
mod imp {
    use std::ffi::{c_char, c_int, c_long, c_uchar, c_ulong, c_void};

    use fltk::prelude::*;
    use fltk::window::Window;

    /// Xlib's `XWMHints`.
    #[repr(C)]
    struct XWmHints {
        flags: c_long,
        input: c_int,
        initial_state: c_int,
        icon_pixmap: c_ulong,
        icon_window: c_ulong,
        icon_x: c_int,
        icon_y: c_int,
        icon_mask: c_ulong,
        window_group: c_ulong,
    }

    const X_URGENCY_HINT: c_long = 1 << 8;
    const XA_CARDINAL: c_ulong = 6;
    const PROP_MODE_REPLACE: c_int = 0;

    // FLTK's own (cfltk, linked by fltk-sys) and Xlib's, which fltk-sys
    // links for FLTK's X11 backend.
    unsafe extern "C" {
        fn Fl_using_wayland() -> c_int;
        fn XInternAtom(d: *mut c_void, name: *const c_char, only_if_exists: c_int) -> c_ulong;
        fn XChangeProperty(
            d: *mut c_void,
            w: c_ulong,
            property: c_ulong,
            kind: c_ulong,
            format: c_int,
            mode: c_int,
            data: *const c_uchar,
            n: c_int,
        ) -> c_int;
        fn XMapRaised(d: *mut c_void, w: c_ulong) -> c_int;
        fn XGetWMHints(d: *mut c_void, w: c_ulong) -> *mut XWmHints;
        fn XAllocWMHints() -> *mut XWmHints;
        fn XSetWMHints(d: *mut c_void, w: c_ulong, hints: *mut XWmHints) -> c_int;
        fn XFree(p: *mut c_void) -> c_int;
        fn XFlush(d: *mut c_void) -> c_int;
    }

    fn wayland() -> bool {
        // SAFETY: a query of FLTK's backend, after the display is open.
        unsafe { Fl_using_wayland() != 0 }
    }

    /// The X display and the window's XID; `None` on Wayland.
    fn x11(win: &Window) -> Option<(*mut c_void, c_ulong)> {
        if wayland() {
            return None;
        }
        let d = fltk::app::display();
        let w = win.raw_handle() as c_ulong;
        (!d.is_null() && w != 0).then_some((d, w))
    }

    pub fn show_without_focus(win: &mut Window) {
        if wayland() {
            win.show();
            return;
        }
        // Made iconic, so the window manager neither shows nor focuses it.
        win.iconize();
        let Some((d, w)) = x11(win) else { return };
        let name = c"_NET_WM_USER_TIME";
        let zero: c_ulong = 0;
        // SAFETY: FLTK's open display and a window this process made; the
        // property data outlives the call.
        unsafe {
            let atom = XInternAtom(d, name.as_ptr(), 0);
            XChangeProperty(
                d,
                w,
                atom,
                XA_CARDINAL,
                32,
                PROP_MODE_REPLACE,
                (&raw const zero).cast(),
                1,
            );
            XMapRaised(d, w);
            XFlush(d);
        }
    }

    pub fn ask_attention(win: &Window) {
        let Some((d, w)) = x11(win) else { return };
        // SAFETY: FLTK's open display and a window this process made; the
        // hints Xlib returns are freed with XFree.
        unsafe {
            let mut hints = XGetWMHints(d, w);
            if hints.is_null() {
                hints = XAllocWMHints();
            }
            if hints.is_null() {
                return;
            }
            (*hints).flags |= X_URGENCY_HINT;
            XSetWMHints(d, w, hints);
            XFree(hints.cast());
            XFlush(d);
        }
    }
}

#[cfg(target_os = "macos")]
mod imp {
    use fltk::prelude::*;
    use fltk::window::Window;

    pub fn show_without_focus(win: &mut Window) {
        win.show();
    }

    pub fn ask_attention(_win: &Window) {}
}
