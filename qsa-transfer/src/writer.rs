// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! Writers of transfer files: rows go out as they come, so a download
//! never holds a whole result.
//!
//! - CSV, TSV and fixed width write the exact text ([`qsa_sql::value`]).
//! - XLSX (rust_xlsxwriter's constant-memory sheet) and ODS (the
//!   spreadsheet XML zipped as it is written) write number cells only for
//!   values a spreadsheet keeps exactly, text cells otherwise, boolean
//!   cells for `BOOLEAN`, and nothing for NULL (XLSX writes an empty string
//!   as the formula `=""`, a spreadsheet's empty text). A decimal number cell gets
//!   its column's scale as its number format.

use std::fs::File;
use std::io::{self, BufWriter, Write};
use std::path::Path;

use qap_client::proto::ops::sql::{Column, SqlValue};
use qsa_sql::value::{self, Cell, Kind};
use qsa_sql::{DateStyle, Style};

use crate::format::{Encoding, FileOptions, Format};
use crate::text::{self, FixedColumn};

/// Why a file could not be written.
#[derive(Debug, thiserror::Error)]
pub enum WriteError {
    #[error(transparent)]
    Io(#[from] io::Error),
    #[error("{0}")]
    Sheet(String),
    /// A value longer than its fixed-width column.
    #[error("the value of {column} is longer than its width, {width}")]
    TooWide { column: String, width: usize },
    /// A value of this column has control characters ODS cannot hold
    /// (CCSID 65535 data shown as text): write that data as hex.
    #[error(
        "a value of {0} has control characters an ODS file cannot hold; write CCSID 65535 data as hex"
    )]
    Unwritable(String),
}

impl From<rust_xlsxwriter::XlsxError> for WriteError {
    fn from(e: rust_xlsxwriter::XlsxError) -> Self {
        WriteError::Sheet(e.to_string())
    }
}

impl From<zip::result::ZipError> for WriteError {
    fn from(e: zip::result::ZipError) -> Self {
        WriteError::Sheet(e.to_string())
    }
}

/// Where rows go.
pub trait Sink {
    /// One row, its values in the columns' order.
    fn row(&mut self, values: &[SqlValue]) -> Result<(), WriteError>;
    /// Writes what is left and closes the file.
    fn finish(self: Box<Self>) -> Result<(), WriteError>;
}

/// The default width of a fixed-width column, in characters.
pub fn default_width(c: &Column) -> usize {
    let len = c.length as usize;
    match value::kind(c) {
        Kind::Integer { min, .. } => min.to_string().len(),
        Kind::Decimal { precision, scale } => precision as usize + usize::from(scale > 0) + 1,
        Kind::DecFloat => 42,
        Kind::Float => 24,
        Kind::Character { bits: true, .. } | Kind::Binary => 2 * len.max(1),
        Kind::Character { .. } | Kind::Other => len.max(1),
        Kind::Date => 10,
        Kind::Time => 8,
        Kind::Timestamp => {
            19 + if c.scale > 0 {
                usize::from(c.scale) + 1
            } else {
                0
            }
        }
        Kind::Boolean => 5,
    }
}

/// Whether a fixed-width column is right-aligned.
fn right_aligned(c: &Column) -> bool {
    matches!(
        value::kind(c),
        Kind::Integer { .. } | Kind::Decimal { .. } | Kind::DecFloat | Kind::Float
    )
}

/// The fixed-width layout of `columns` with `opts`' widths (the defaults
/// where it has none).
pub fn fixed_columns(columns: &[Column], opts: &FileOptions) -> Vec<FixedColumn> {
    columns
        .iter()
        .enumerate()
        .map(|(i, c)| FixedColumn {
            width: opts
                .widths
                .get(i)
                .copied()
                .unwrap_or_else(|| default_width(c)),
            right: right_aligned(c),
            indicator: opts.null_indicators && c.nullable,
        })
        .collect()
}

/// A file for `columns` at `path`, as `opts` says. With `append` the rows
/// go after what the file has (CSV, TSV and fixed width), without a header.
pub fn open(
    path: &Path,
    columns: &[Column],
    opts: &FileOptions,
    job_dates: DateStyle,
    job_ccsid: u16,
    append: bool,
) -> Result<Box<dyn Sink>, WriteError> {
    let style = opts.style(job_dates, job_ccsid);
    Ok(match opts.format {
        Format::Csv | Format::Tsv | Format::Fixed => {
            let f = std::fs::OpenOptions::new()
                .create(true)
                .write(true)
                .append(append)
                .truncate(!append)
                .open(path)?;
            let mut w = BufWriter::new(f);
            let fresh = !append || std::fs::metadata(path).map_or(true, |m| m.len() == 0);
            if fresh {
                w.write_all(&text::encode("", opts.encoding, true))?;
            }
            let mut t = TextSink {
                w,
                columns: columns.to_vec(),
                style,
                format: opts.format,
                delimiter: opts.delimiter(),
                quote: opts.quote,
                fixed: fixed_columns(columns, opts),
                encoding: opts.encoding,
            };
            if opts.header && opts.format != Format::Fixed && fresh {
                let names: Vec<text::Field> =
                    columns.iter().map(|c| Some(c.name.clone())).collect();
                t.line(&text::delimited_record(&names, t.delimiter, t.quote))?;
            }
            Box::new(t)
        }
        Format::Xlsx => Box::new(XlsxSink::new(path, columns, opts, style)?),
        Format::Ods => Box::new(OdsSink::new(path, columns, opts, style)?),
    })
}

/// CSV, TSV and fixed width.
struct TextSink {
    w: BufWriter<File>,
    columns: Vec<Column>,
    style: Style,
    format: Format,
    delimiter: char,
    quote: char,
    fixed: Vec<FixedColumn>,
    encoding: Encoding,
}

impl TextSink {
    fn line(&mut self, s: &str) -> io::Result<()> {
        self.w.write_all(&text::encode(s, self.encoding, false))?;
        self.w.write_all(b"\r\n")
    }
}

impl Sink for TextSink {
    fn row(&mut self, values: &[SqlValue]) -> Result<(), WriteError> {
        let fields: Vec<text::Field> = values
            .iter()
            .zip(&self.columns)
            .map(|(v, c)| value::text_field(v, c, &self.style))
            .collect();
        let line = if self.format == Format::Fixed {
            text::fixed_record(&fields, &self.fixed).map_err(|i| WriteError::TooWide {
                column: self.columns[i].name.clone(),
                width: self.fixed[i].width,
            })?
        } else {
            text::delimited_record(&fields, self.delimiter, self.quote)
        };
        self.line(&line)?;
        Ok(())
    }

    fn finish(mut self: Box<Self>) -> Result<(), WriteError> {
        self.w.flush()?;
        self.w.get_ref().sync_all()?;
        Ok(())
    }
}

/// A sheet's name: what the options say, else `Sheet1`, without the
/// characters spreadsheets refuse and at most 31 characters.
pub fn sheet_name(opts: &FileOptions) -> String {
    let n: String = opts
        .sheet
        .chars()
        .filter(|c| !matches!(c, '[' | ']' | ':' | '*' | '?' | '/' | '\\'))
        .take(31)
        .collect();
    if n.trim().is_empty() {
        "Sheet1".into()
    } else {
        n
    }
}

/// The number format for a decimal column's cells: its scale's digits.
fn num_format(c: &Column) -> Option<String> {
    match value::kind(c) {
        Kind::Decimal { scale, .. } if scale > 0 => {
            Some(format!("0.{}", "0".repeat(usize::from(scale))))
        }
        _ => None,
    }
}

struct XlsxSink {
    book: rust_xlsxwriter::Workbook,
    path: std::path::PathBuf,
    columns: Vec<Column>,
    formats: Vec<Option<rust_xlsxwriter::Format>>,
    style: Style,
    row: u32,
}

impl XlsxSink {
    fn new(
        path: &Path,
        columns: &[Column],
        opts: &FileOptions,
        style: Style,
    ) -> Result<XlsxSink, WriteError> {
        let mut book = rust_xlsxwriter::Workbook::new();
        // Its temporary files next to the output, not in the system's.
        if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
            book.set_tempdir(dir)?;
        }
        let sheet = book.add_worksheet_with_constant_memory()?;
        sheet.set_name(sheet_name(opts))?;
        // An empty string is the formula `=""` (a spreadsheet has no empty
        // text cell otherwise, only a blank one, which is NULL here); its
        // result is written as empty text.
        sheet.set_formula_result_default("");
        let mut row = 0;
        if opts.header {
            for (i, c) in columns.iter().enumerate() {
                sheet.write_string(0, col(i), &c.name)?;
            }
            row = 1;
        }
        let formats = columns
            .iter()
            .map(|c| num_format(c).map(|f| rust_xlsxwriter::Format::new().set_num_format(f)))
            .collect();
        Ok(XlsxSink {
            book,
            path: path.to_owned(),
            columns: columns.to_vec(),
            formats,
            style,
            row,
        })
    }
}

fn col(i: usize) -> u16 {
    u16::try_from(i).unwrap_or(u16::MAX)
}

impl Sink for XlsxSink {
    fn row(&mut self, values: &[SqlValue]) -> Result<(), WriteError> {
        let r = self.row;
        let sheet = self.book.worksheet_from_index(0)?;
        for (i, (v, c)) in values.iter().zip(&self.columns).enumerate() {
            match value::sheet_cell(v, c, &self.style) {
                Cell::Empty => {}
                Cell::Text(t) if t.is_empty() => {
                    sheet.write_formula(r, col(i), rust_xlsxwriter::Formula::new("=\"\""))?;
                }
                Cell::Text(t) => {
                    sheet.write_string(r, col(i), t)?;
                }
                Cell::Number { value, .. } => match &self.formats[i] {
                    Some(f) => {
                        sheet.write_number_with_format(r, col(i), value, f)?;
                    }
                    None => {
                        sheet.write_number(r, col(i), value)?;
                    }
                },
                Cell::Bool(b) => {
                    sheet.write_boolean(r, col(i), b)?;
                }
            }
        }
        self.row += 1;
        Ok(())
    }

    fn finish(mut self: Box<Self>) -> Result<(), WriteError> {
        self.book.save(&self.path)?;
        Ok(())
    }
}

/// XML text with its markup characters escaped.
fn xml(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            // Characters XML 1.0 does not allow are left out.
            c if (c as u32) < 0x20 && !matches!(c, '\t' | '\n' | '\r') => {}
            c => out.push(c),
        }
    }
    out
}

/// A paragraph of cell text: tabs, line ends and runs of spaces as ODS
/// writes them, so the text reads back as it was.
fn ods_text(s: &str) -> String {
    let mut out = String::from("<text:p>");
    let mut spaces = 0usize;
    let flush = |out: &mut String, spaces: &mut usize, first: bool| {
        if *spaces == 0 {
            return;
        }
        // A space at the start, or more than one, needs `text:s`.
        if first || *spaces > 1 {
            let keep = if first { 0 } else { 1 };
            if keep == 1 {
                out.push(' ');
            }
            let n = *spaces - keep;
            if n > 0 {
                out.push_str(&format!("<text:s text:c=\"{n}\"/>"));
            }
        } else {
            out.push(' ');
        }
        *spaces = 0;
    };
    let mut first = true;
    for c in s.chars() {
        match c {
            ' ' => {
                spaces += 1;
                continue;
            }
            _ => flush(&mut out, &mut spaces, first),
        }
        first = false;
        match c {
            '\t' => out.push_str("<text:tab/>"),
            '\n' => out.push_str("</text:p><text:p>"),
            '\r' => {}
            c => out.push_str(&xml(&c.to_string())),
        }
    }
    // Trailing spaces.
    if spaces > 0 {
        out.push_str(&format!("<text:s text:c=\"{spaces}\"/>"));
    }
    out.push_str("</text:p>");
    out
}

const ODS_MIME: &str = "application/vnd.oasis.opendocument.spreadsheet";

const ODS_MANIFEST: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<manifest:manifest xmlns:manifest="urn:oasis:names:tc:opendocument:xmlns:manifest:1.0" manifest:version="1.2">
 <manifest:file-entry manifest:full-path="/" manifest:version="1.2" manifest:media-type="application/vnd.oasis.opendocument.spreadsheet"/>
 <manifest:file-entry manifest:full-path="content.xml" manifest:media-type="text/xml"/>
</manifest:manifest>
"#;

struct OdsSink {
    zip: zip::ZipWriter<BufWriter<File>>,
    columns: Vec<Column>,
    style: Style,
}

impl OdsSink {
    fn new(
        path: &Path,
        columns: &[Column],
        opts: &FileOptions,
        style: Style,
    ) -> Result<OdsSink, WriteError> {
        use zip::write::SimpleFileOptions;
        let f = BufWriter::new(File::create(path)?);
        let mut zip = zip::ZipWriter::new(f);
        // The MIME type first, stored, as OpenDocument requires.
        zip.start_file(
            "mimetype",
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored),
        )?;
        zip.write_all(ODS_MIME.as_bytes())?;
        let deflate =
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        zip.start_file("META-INF/manifest.xml", deflate)?;
        zip.write_all(ODS_MANIFEST.as_bytes())?;
        zip.start_file("content.xml", deflate)?;
        write!(
            zip,
            r#"<?xml version="1.0" encoding="UTF-8"?>
<office:document-content xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:table="urn:oasis:names:tc:opendocument:xmlns:table:1.0" xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0" office:version="1.2"><office:body><office:spreadsheet><table:table table:name="{}">"#,
            xml(&sheet_name(opts))
        )?;
        let mut s = OdsSink {
            zip,
            columns: columns.to_vec(),
            style,
        };
        if opts.header {
            let mut r = String::from("<table:table-row>");
            for c in columns {
                r.push_str(&format!(
                    "<table:table-cell office:value-type=\"string\">{}</table:table-cell>",
                    ods_text(&c.name)
                ));
            }
            r.push_str("</table:table-row>");
            s.zip.write_all(r.as_bytes())?;
        }
        Ok(s)
    }
}

impl Sink for OdsSink {
    fn row(&mut self, values: &[SqlValue]) -> Result<(), WriteError> {
        let mut r = String::from("<table:table-row>");
        for (v, c) in values.iter().zip(&self.columns) {
            match value::sheet_cell(v, c, &self.style) {
                Cell::Empty => r.push_str("<table:table-cell/>"),
                // XML cannot hold most control characters: refused, never
                // dropped (CCSID 65535 data shown as text can have them).
                Cell::Text(t)
                    if t.chars()
                        .any(|ch| (ch as u32) < 0x20 && !matches!(ch, '\t' | '\n' | '\r')) =>
                {
                    return Err(WriteError::Unwritable(c.name.clone()));
                }
                Cell::Text(t) => r.push_str(&format!(
                    "<table:table-cell office:value-type=\"string\">{}</table:table-cell>",
                    ods_text(&t)
                )),
                Cell::Number { text, .. } => {
                    // The exact text is the value: ODS keeps it as written.
                    let t = xml(&text);
                    r.push_str(&format!(
                        "<table:table-cell office:value-type=\"float\" office:value=\"{t}\"><text:p>{t}</text:p></table:table-cell>"
                    ));
                }
                Cell::Bool(b) => {
                    let (v, shown) = if b {
                        ("true", "TRUE")
                    } else {
                        ("false", "FALSE")
                    };
                    r.push_str(&format!(
                        "<table:table-cell office:value-type=\"boolean\" office:boolean-value=\"{v}\"><text:p>{shown}</text:p></table:table-cell>"
                    ));
                }
            }
        }
        r.push_str("</table:table-row>");
        self.zip.write_all(r.as_bytes())?;
        Ok(())
    }

    fn finish(mut self: Box<Self>) -> Result<(), WriteError> {
        self.zip.write_all(
            b"</table:table></office:spreadsheet></office:body></office:document-content>",
        )?;
        let mut f = self.zip.finish()?;
        f.flush()?;
        f.get_ref().sync_all()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ods_text_keeps_spaces_tabs_and_lines() {
        assert_eq!(ods_text("a b"), "<text:p>a b</text:p>");
        assert_eq!(
            ods_text(" lead"),
            "<text:p><text:s text:c=\"1\"/>lead</text:p>"
        );
        assert_eq!(
            ods_text("a  b"),
            "<text:p>a <text:s text:c=\"1\"/>b</text:p>"
        );
        assert_eq!(ods_text("a\tb"), "<text:p>a<text:tab/>b</text:p>");
        assert_eq!(ods_text("x\ny"), "<text:p>x</text:p><text:p>y</text:p>");
        assert_eq!(ods_text("t  "), "<text:p>t<text:s text:c=\"2\"/></text:p>");
        assert_eq!(ods_text("<&>"), "<text:p>&lt;&amp;&gt;</text:p>");
        assert_eq!(ods_text(""), "<text:p></text:p>");
    }

    #[test]
    fn sheet_names_and_widths() {
        let mut o = FileOptions::default();
        assert_eq!(sheet_name(&o), "Sheet1");
        o.sheet = "HLINV/ASSETS: [all]".into();
        assert_eq!(sheet_name(&o), "HLINVASSETS all");
        let c = |t: &str, length: u32, scale: u16| Column {
            name: "C".into(),
            sql_type: t.into(),
            length,
            scale,
            nullable: true,
            ..Column::default()
        };
        assert_eq!(default_width(&c("DECIMAL", 11, 2)), 13);
        assert_eq!(default_width(&c("SMALLINT", 2, 0)), 6);
        assert_eq!(default_width(&c("BIGINT", 8, 0)), 20);
        assert_eq!(default_width(&c("TIMESTAMP", 32, 12)), 32);
        assert_eq!(default_width(&c("VARBINARY", 16, 0)), 32);
    }
}
