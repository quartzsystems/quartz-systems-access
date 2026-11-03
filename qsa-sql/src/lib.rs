// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! SQL values as QS/400 Access for Desktop shows them and writes them to
//! transfer files, and reads them back. No user interface, and no floating
//! point for decimals.
//!
//! - [`decimal`]: exact decimal text, `DECFLOAT`'s E notation and special
//!   values included; when a spreadsheet number keeps a value exactly.
//! - [`datetime`]: dates, times and timestamps in the job's date format
//!   and separators or ISO 8601, a timestamp to its column's precision
//!   from its picoseconds; and read back.
//! - [`binary`]: hex, and CCSID 65535 character data as text in the job's
//!   CCSID with this program's code-page tables.
//! - [`value`]: a value for a grid, for CSV, TSV and fixed width, for XLSX
//!   and ODS, and back from them for an upload, with what does not fit.

pub mod binary;
pub mod datetime;
pub mod decimal;
pub mod value;

pub use datetime::{DateOrder, DateStyle};
pub use value::{BitData, Cell, Input, Kind, Misfit, Style};
