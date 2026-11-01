// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! Printed output from the SCS page model ([`scs::Document`]).
//!
//! - [`pdf`]: a PDF with the bundled print font, subset to what is used.
//! - [`gdi`] (Windows): a printer through the Windows spooler.
//! - [`cups`] (Linux): a PDF to a CUPS printer with `lp`.
//! - [`font`]: the bundled face and how runs are sized and placed in it.
//!
//! Nothing is reflowed. Each page keeps a margin clear (a quarter inch, or
//! the session's margins override); text inside it stays where the data
//! put it ([`pdf::fit`]).

pub mod font;
pub mod pdf;

#[cfg(windows)]
pub mod gdi;

pub mod cups;
