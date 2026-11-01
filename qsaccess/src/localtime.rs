// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! Local date and time for display, from the platform's own time zone
//! rules: `localtime_r` from the C library on Linux, and
//! `SystemTimeToTzSpecificLocalTime` on Windows.

use std::time::{SystemTime, UNIX_EPOCH};

/// Seconds since 1970 now.
pub fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

/// Local year, month, day, hour and minute of a Unix time.
pub fn local(unix: i64) -> Option<(i32, u32, u32, u32, u32)> {
    imp::local(unix)
}

/// `2026-11-01 14:05`, in local time.
pub fn format(unix: i64) -> String {
    match local(unix) {
        Some((y, mo, d, h, mi)) => format!("{y:04}-{mo:02}-{d:02} {h:02}:{mi:02}"),
        None => String::new(),
    }
}

#[cfg(unix)]
mod imp {
    use std::os::raw::{c_char, c_int, c_long};

    /// `struct tm` as glibc and musl lay it out.
    #[repr(C)]
    struct Tm {
        sec: c_int,
        min: c_int,
        hour: c_int,
        mday: c_int,
        mon: c_int,
        year: c_int,
        wday: c_int,
        yday: c_int,
        isdst: c_int,
        gmtoff: c_long,
        zone: *const c_char,
    }

    unsafe extern "C" {
        fn tzset();
        fn localtime_r(t: *const i64, out: *mut Tm) -> *mut Tm;
    }

    pub fn local(unix: i64) -> Option<(i32, u32, u32, u32, u32)> {
        let mut tm = Tm {
            sec: 0,
            min: 0,
            hour: 0,
            mday: 0,
            mon: 0,
            year: 0,
            wday: 0,
            yday: 0,
            isdst: 0,
            gmtoff: 0,
            zone: std::ptr::null(),
        };
        // SAFETY: `time_t` is 64-bit on the supported Linux targets; both
        // pointers are to live locals, and localtime_r writes only `tm`.
        let ok = unsafe {
            tzset();
            !localtime_r(&unix, &mut tm).is_null()
        };
        ok.then(|| {
            (
                tm.year + 1900,
                (tm.mon + 1) as u32,
                tm.mday as u32,
                tm.hour as u32,
                tm.min as u32,
            )
        })
    }
}

#[cfg(windows)]
mod imp {
    use windows_sys::Win32::Foundation::{FILETIME, SYSTEMTIME};
    use windows_sys::Win32::System::Time::{FileTimeToSystemTime, SystemTimeToTzSpecificLocalTime};

    /// Seconds from 1601 (the FILETIME epoch) to 1970.
    const EPOCH_DIFF: i64 = 11_644_473_600;

    pub fn local(unix: i64) -> Option<(i32, u32, u32, u32, u32)> {
        let ticks = (unix.checked_add(EPOCH_DIFF)?).checked_mul(10_000_000)? as u64;
        let ft = FILETIME {
            dwLowDateTime: ticks as u32,
            dwHighDateTime: (ticks >> 32) as u32,
        };
        let zero = SYSTEMTIME {
            wYear: 0,
            wMonth: 0,
            wDayOfWeek: 0,
            wDay: 0,
            wHour: 0,
            wMinute: 0,
            wSecond: 0,
            wMilliseconds: 0,
        };
        let (mut utc, mut loc) = (zero, zero);
        // SAFETY: plain conversions between structures on the stack; a null
        // time zone means the current one.
        let ok = unsafe {
            FileTimeToSystemTime(&ft, &mut utc) != 0
                && SystemTimeToTzSpecificLocalTime(std::ptr::null(), &utc, &mut loc) != 0
        };
        ok.then(|| {
            (
                i32::from(loc.wYear),
                u32::from(loc.wMonth),
                u32::from(loc.wDay),
                u32::from(loc.wHour),
                u32::from(loc.wMinute),
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_a_plausible_local_time() {
        // 2026-07-01 12:00 UTC is 2026-07-01 or 2026-07-02 in every zone
        // (UTC-12 to UTC+14).
        let s = format(1_782_907_200);
        assert_eq!(s.len(), 16, "{s}");
        assert!(
            s.starts_with("2026-07-01") || s.starts_with("2026-07-02"),
            "{s}"
        );
        assert!(!format(now()).is_empty());
    }
}
