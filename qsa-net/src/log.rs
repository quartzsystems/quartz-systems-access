// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! The protocol crates' log lines go to one process-wide sink that the
//! application installs. Without a sink they are dropped. `tn5250` and the
//! host-server crates log through this module too, so one sink takes them
//! all. Negotiation is logged at [`Level::Debug`]; record bytes at
//! [`Level::Trace`].

use std::sync::RwLock;

/// How much a line matters.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

impl Level {
    /// Short name for log files.
    pub fn name(self) -> &'static str {
        match self {
            Level::Trace => "trace",
            Level::Debug => "debug",
            Level::Info => "info",
            Level::Warn => "warn",
            Level::Error => "error",
        }
    }
}

type Sink = Box<dyn Fn(Level, &str) + Send + Sync>;

static SINK: RwLock<Option<(Level, Sink)>> = RwLock::new(None);

/// Sends lines at `min` and above to `sink` from now on.
pub fn set_sink(min: Level, sink: impl Fn(Level, &str) + Send + Sync + 'static) {
    if let Ok(mut s) = SINK.write() {
        *s = Some((min, Box::new(sink)));
    }
}

/// Whether a line at `level` would be written.
pub fn enabled(level: Level) -> bool {
    SINK.read()
        .ok()
        .and_then(|s| s.as_ref().map(|(min, _)| level >= *min))
        .unwrap_or(false)
}

/// Writes one line.
pub fn line(level: Level, text: &str) {
    if let Ok(s) = SINK.read()
        && let Some((min, sink)) = s.as_ref()
        && level >= *min
    {
        sink(level, text);
    }
}

/// Bytes as hex pairs, for traces.
pub fn hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 3);
    for (i, b) in bytes.iter().enumerate() {
        if i > 0 {
            s.push(' ');
        }
        s.push_str(&format!("{b:02X}"));
    }
    s
}

#[doc(hidden)]
#[macro_export]
macro_rules! log_at {
    ($level:expr, $($arg:tt)*) => {
        if $crate::log::enabled($level) {
            $crate::log::line($level, &format!($($arg)*));
        }
    };
}

/// Logs negotiation steps and other detail.
#[macro_export]
macro_rules! debug {
    ($($arg:tt)*) => { $crate::log_at!($crate::log::Level::Debug, $($arg)*) };
}

/// Logs record bytes.
#[macro_export]
macro_rules! trace {
    ($($arg:tt)*) => { $crate::log_at!($crate::log::Level::Trace, $($arg)*) };
}

/// Logs protocol errors and other things worth a look.
#[macro_export]
macro_rules! warn {
    ($($arg:tt)*) => { $crate::log_at!($crate::log::Level::Warn, $($arg)*) };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_pairs() {
        assert_eq!(hex(&[0xFF, 0x00, 0x1D]), "FF 00 1D");
        assert_eq!(hex(&[]), "");
    }
}
