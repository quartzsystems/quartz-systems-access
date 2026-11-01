// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! 5250 display support for QS/400 Access for Desktop.
//!
//! - [`telnet`]: transports (TCP, TLS), the Telnet codec and option
//!   negotiation.
//! - [`log`]: where the crate's log lines go.
//!
//! The crate is deliberately free of any user-interface dependency: the UI
//! crates depend on it, never the other way round.

pub mod log;
pub mod telnet;
