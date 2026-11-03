// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! Transfer file formats and their options.

use serde::{Deserialize, Serialize};

/// A transfer file's format.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Format {
    /// Comma-separated values (RFC 4180, with the delimiter and quote the
    /// options give).
    #[default]
    Csv,
    /// Tab-separated values: CSV with a tab as the delimiter.
    Tsv,
    /// Office Open XML spreadsheet.
    Xlsx,
    /// OpenDocument spreadsheet.
    Ods,
    /// Fixed-width columns.
    Fixed,
}

impl Format {
    /// Every format, in menu order.
    pub const ALL: [Format; 5] = [
        Format::Csv,
        Format::Tsv,
        Format::Xlsx,
        Format::Ods,
        Format::Fixed,
    ];

    /// Its name where the user sees it.
    pub fn label(self) -> &'static str {
        match self {
            Format::Csv => "CSV",
            Format::Tsv => "TSV",
            Format::Xlsx => "Excel workbook (XLSX)",
            Format::Ods => "OpenDocument spreadsheet (ODS)",
            Format::Fixed => "Fixed width",
        }
    }

    /// Its usual file extension.
    pub fn extension(self) -> &'static str {
        match self {
            Format::Csv => "csv",
            Format::Tsv => "tsv",
            Format::Xlsx => "xlsx",
            Format::Ods => "ods",
            Format::Fixed => "txt",
        }
    }

    /// The format a file's extension says, if any.
    pub fn from_path(p: &std::path::Path) -> Option<Format> {
        let e = p.extension()?.to_string_lossy().to_ascii_lowercase();
        Format::ALL.into_iter().find(|f| f.extension() == e)
    }

    /// Whether rows can be added to an existing file of this format.
    pub fn appends(self) -> bool {
        matches!(self, Format::Csv | Format::Tsv | Format::Fixed)
    }

    /// Whether the file is text (with an encoding).
    pub fn is_text(self) -> bool {
        matches!(self, Format::Csv | Format::Tsv | Format::Fixed)
    }

    /// Whether the file has sheets.
    pub fn has_sheets(self) -> bool {
        matches!(self, Format::Xlsx | Format::Ods)
    }
}

/// A text file's encoding.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Encoding {
    /// UTF-8; read with or without a byte order mark.
    #[default]
    Utf8,
    /// UTF-8 written with a byte order mark.
    Utf8Bom,
    /// UTF-16, little or big endian by its byte order mark (read only).
    Utf16,
    /// Windows code page 1252 (read only).
    Windows1252,
}

impl Encoding {
    /// Every encoding a file may be read in.
    pub const READ: [Encoding; 3] = [Encoding::Utf8, Encoding::Utf16, Encoding::Windows1252];

    pub fn label(self) -> &'static str {
        match self {
            Encoding::Utf8 => "UTF-8",
            Encoding::Utf8Bom => "UTF-8 with BOM",
            Encoding::Utf16 => "UTF-16",
            Encoding::Windows1252 => "Windows-1252",
        }
    }
}

/// How dates, times and timestamps are written and read.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Dates {
    /// ISO 8601.
    #[default]
    Iso,
    /// The job's date format and separators.
    Job,
}

/// What to do with an output file that is there already.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Existing {
    /// Replace it.
    Replace,
    /// Ask first.
    #[default]
    Ask,
    /// Add the rows at its end (CSV, TSV and fixed width).
    Append,
}

/// The options of a transfer file.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct FileOptions {
    pub format: Format,
    /// CSV's field delimiter (TSV's is a tab).
    pub delimiter: char,
    /// The quote character of CSV and TSV.
    pub quote: char,
    /// A header row of column names.
    pub header: bool,
    /// The text encoding.
    pub encoding: Encoding,
    /// A spreadsheet's sheet: the name to write, or the one to read
    /// (empty: the first).
    pub sheet: String,
    /// Fixed width: each column's width in characters, in output order.
    pub widths: Vec<usize>,
    /// Fixed width: a `Y`/`N` null indicator after each nullable column, so
    /// NULL and blanks stay apart.
    pub null_indicators: bool,
    pub dates: Dates,
    /// The decimal point of decimals and floats.
    pub point: char,
    /// CCSID 65535 character data as text in the job's CCSID rather than
    /// hex.
    pub bit_data_as_text: bool,
}

impl Default for FileOptions {
    fn default() -> Self {
        FileOptions {
            format: Format::Csv,
            delimiter: ',',
            quote: '"',
            header: true,
            encoding: Encoding::Utf8,
            sheet: String::new(),
            widths: Vec::new(),
            null_indicators: true,
            dates: Dates::Iso,
            point: '.',
            bit_data_as_text: false,
        }
    }
}

impl FileOptions {
    /// The field delimiter the format uses.
    pub fn delimiter(&self) -> char {
        if self.format == Format::Tsv {
            '\t'
        } else {
            self.delimiter
        }
    }

    /// The value style these options give, with the job's date style and
    /// CCSID.
    pub fn style(&self, job_dates: qsa_sql::DateStyle, job_ccsid: u16) -> qsa_sql::Style {
        qsa_sql::Style {
            dates: match self.dates {
                Dates::Iso => qsa_sql::DateStyle::Iso,
                Dates::Job => job_dates,
            },
            point: self.point,
            bit_data: if self.bit_data_as_text {
                qsa_sql::BitData::Text { ccsid: job_ccsid }
            } else {
                qsa_sql::BitData::Hex
            },
        }
    }
}
