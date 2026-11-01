// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! Wire-format strings that the 5250 Telnet RFCs define with a vendor
//! prefix: terminal types and `NEW-ENVIRON` user variable names.
//!
//! The protocol requires these exact bytes, so they cannot be renamed. This
//! module is the only file the clean-room check allows them in. Elsewhere,
//! code refers to them by these constants' names. The strings never appear
//! in UI text, file formats, documentation, or logs above debug level: they
//! are written only to the debug-level negotiation trace.
//!
//! References: RFC 1205 (the 5250 Telnet interface) and RFC 4777 (the
//! TN5250E enhancements, which obsoletes RFC 2877). Section numbers below
//! are RFC 4777's unless stated.

// Terminal types.

/// The 24 by 80 colour display's terminal type. RFC 1205, section 2.
pub const TT_DISPLAY_24X80: &str = "IBM-3179-2";

/// The 27 by 132 colour display's terminal type. RFC 1205, section 2.
pub const TT_DISPLAY_27X132: &str = "IBM-3477-FC";

/// The single-byte (SBCS) printer's terminal type. Section 9.
pub const TT_PRINTER_SBCS: &str = "IBM-3812-1";

/// The double-byte (DBCS) printer's terminal type. Section 9.
pub const TT_PRINTER_DBCS: &str = "IBM-5553-B01";

// Display USERVARs.

/// Asks for a display start-up response record (`YES` or `NO`). Section 4.
pub const SEND_CONFIRMATION_RECORD: &str = "IBMSENDCONFREC";

/// The printer associated with a display device. Section 4.
pub const ASSOCIATED_PRINTER: &str = "IBMASSOCPRT";

// Automatic sign-on USERVARs.

/// The server's eight-byte random seed for password substitutes, sent as
/// the name followed directly by the seed bytes. Section 5.
pub const RANDOM_SEED: &str = "IBMRSEED";

/// A password substitute. Section 5. Never sent by this client.
pub const PASSWORD_SUBSTITUTE: &str = "IBMSUBSPW";

/// Current library for automatic sign-on. Section 5.
pub const CURRENT_LIBRARY: &str = "IBMCURLIB";

/// Initial menu for automatic sign-on. Section 5.
pub const INITIAL_MENU: &str = "IBMIMENU";

/// Program to call at automatic sign-on. Section 5.
pub const PROGRAM: &str = "IBMPROGRAM";

// Printer USERVARs.

/// DBCS (IGC) feature. Section 8.
pub const IGC_FEATURE: &str = "IBMIGCFEAT";

/// Name of the message queue for printer messages. Section 8.
pub const MESSAGE_QUEUE_NAME: &str = "IBMMSGQNAME";

/// Library of the message queue for printer messages. Section 8.
pub const MESSAGE_QUEUE_LIBRARY: &str = "IBMMSGQLIB";

/// Font identifier. Section 8.
pub const FONT: &str = "IBMFONT";

/// Form feed: `C` (continuous), `U` (cut) or `A` (autocut). Section 8.
pub const FORM_FEED: &str = "IBMFORMFEED";

/// Host print transform: `1` (yes) or `0` (no). Section 8.
pub const TRANSFORM: &str = "IBMTRANSFORM";

/// Manufacturer type and model, used with host print transform. Section 8.
pub const MANUFACTURER_TYPE_MODEL: &str = "IBMMFRTYPMDL";

/// Paper source 1, one binary byte. Section 8.
pub const PAPER_SOURCE_1: &str = "IBMPPRSRC1";

/// Paper source 2, one binary byte. Section 8.
pub const PAPER_SOURCE_2: &str = "IBMPPRSRC2";

/// Envelope hopper, one binary byte. Section 8.
pub const ENVELOPE: &str = "IBMENVELOPE";

/// ASCII code page 899 support: `1` (yes) or `0` (no). Section 8.
pub const ASCII_899: &str = "IBMASCII899";

/// Workstation customising object name. Section 8.
pub const WSCST_NAME: &str = "IBMWSCSTNAME";

/// Workstation customising object library. Section 8.
pub const WSCST_LIBRARY: &str = "IBMWSCSTLIB";
