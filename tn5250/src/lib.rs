// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! 5250 display and printer support for QS/400 Access for Desktop.
//!
//! - [`telnet`]: transports (TCP, TLS), the Telnet codec and option
//!   negotiation.
//! - [`negotiate`]: TN5250E on top of Telnet, without I/O.
//! - [`printer`]: printer sessions: print records, responses, jobs.
//! - [`session`]: a connected session for one worker thread.
//! - [`record`]: the 5250 record header.
//! - [`stream`]: the data stream from the host, applied to a [`screen::Screen`];
//!   [`inbound`]: what the display sends back; [`aid`]: attention keys.
//! - [`input`]: the keyboard; [`terminal`]: screen, records and keys together.
//! - [`codepage`]: EBCDIC translation behind one trait.
//! - [`replay`]: recording sessions and replaying them against goldens.
//! - [`log`]: where the crate's log lines go.
//! - [`wire`]: the vendor-prefixed strings the protocol requires.
//!
//! The crate is deliberately free of any user-interface dependency: the UI
//! crates depend on it, never the other way round.

pub mod aid;
pub mod codepage;
pub mod inbound;
pub mod input;
pub mod log;
pub mod negotiate;
pub mod printer;
pub mod record;
pub mod replay;
pub mod screen;
pub mod session;
pub mod stream;
pub mod telnet;
pub mod terminal;
pub mod wire;
