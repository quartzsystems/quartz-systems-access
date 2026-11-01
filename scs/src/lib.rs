// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! SCS (SNA character string), the data a 5250 printer session receives,
//! read into a page model.
//!
//! - [`parse`]: a print job's bytes to a [`Document`].
//! - [`model`]: pages with their physical size, orientation, margins and
//!   positioned runs of text, in 1/1440 inch.
//! - [`text`]: a plain-text rendering, one text page per page.
//! - [`codes`]: the control codes; [`font`]: font IDs.
//!
//! Unknown or unsupported controls are skipped by their length and logged
//! once per job at debug level. No input makes the parser panic or lose its
//! place.
//!
//! The crate has no user-interface dependency; renderers live elsewhere.

pub mod codes;
pub mod font;
pub mod model;
pub mod parse;
pub mod text;

pub use model::{Document, Margins, Orientation, Page, Paper, Pitch, Run};
pub use parse::{Options, parse};
pub use text::to_text;
