// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! Data Transfer: rows between the system's tables and files on this
//! computer, over a connection attached to the session (QAP version 4).
//! No user interface: the Data Transfer windows and `qsaccess run` both use
//! it.
//!
//! - [`format`]: CSV, TSV, XLSX, ODS and fixed width, and their options.
//! - [`text`]: delimited and fixed-width records and text encodings, NULL
//!   kept apart from an empty string both ways.
//! - [`writer`]: the files written as rows come.
//! - [`download`]: a table's or a query's rows to a file, streamed in
//!   blocks, the file renamed into place only at the end.

pub mod download;
pub mod format;
pub mod text;
pub mod writer;

pub use download::{Download, Failure, Progress, Source, Summary};
pub use format::{Dates, Encoding, Existing, FileOptions, Format};
pub mod reader;
pub mod upload;

pub use upload::{Commit, Map, Mode, SourceColumn, Upload};
