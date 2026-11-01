// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! 5250 display and printer data stream support for QS/400 Access for Desktop.
//!
//! This crate will hold the Telnet negotiation, the 5250 data stream parser and
//! the presentation-space model used by the display and printer emulators. It is
//! deliberately free of any user-interface dependency: the UI crates depend on it,
//! never the other way round. It is empty until Phase 1.
