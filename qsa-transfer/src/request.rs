// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! `.qdx` request files: a Data Transfer saved to run again, from its
//! window or with `qsaccess run`.
//!
//! ```toml
//! [qsaccess]
//! kind = "transfer"
//! version = 1
//!
//! [transfer]
//! direction = "download"
//! system = "qs400.example.net"
//! file = "assets.csv"            # relative to the .qdx when inside its folder
//! columns = ["ASSET_TAG", "COST"]
//! limit = 1000
//! existing = "replace"
//!
//! [transfer.source]
//! kind = "table"
//! schema = "HLINV"
//! table = "ASSETS"
//! where = "STATUS = 'ACTIVE'"
//!
//! [transfer.options]
//! format = "csv"
//! ```
//!
//! An upload has `schema`, `table`, `mode`, `[[transfer.mapping]]`,
//! `[transfer.commit]` and `resume_row` instead. A request never holds a
//! password.

use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::download::{Download, Source};
use crate::format::{Existing, FileOptions};
use crate::upload::{Commit, Map, Mode, Upload};

/// The `[qsaccess]` kind of a request file.
pub const KIND: &str = "transfer";
/// The request file version this program writes and reads.
pub const VERSION: u32 = 1;
/// The request file extension.
pub const EXTENSION: &str = "qdx";

/// Which way a request goes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Direction {
    /// From the system to a file.
    #[default]
    Download,
    /// From a file to the system.
    Upload,
}

/// A request: the system, and the transfer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Request {
    Download { system: String, download: Download },
    Upload { system: String, upload: Upload },
}

impl Request {
    pub fn system(&self) -> &str {
        match self {
            Request::Download { system, .. } | Request::Upload { system, .. } => system,
        }
    }

    /// The file the transfer reads or writes.
    pub fn file(&self) -> &Path {
        match self {
            Request::Download { download, .. } => &download.file,
            Request::Upload { upload, .. } => &upload.file,
        }
    }
}

#[derive(Serialize, Deserialize)]
struct Header {
    kind: String,
    version: u32,
}

#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
struct Transfer {
    direction: Direction,
    system: String,
    file: String,
    options: FileOptions,
    // A download.
    #[serde(skip_serializing_if = "Option::is_none")]
    source: Option<Source>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    columns: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    limit: Option<u64>,
    existing: Existing,
    // An upload.
    #[serde(skip_serializing_if = "String::is_empty")]
    schema: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    table: String,
    mode: Mode,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    mapping: Vec<Map>,
    commit: Commit,
    #[serde(skip_serializing_if = "Option::is_none")]
    resume_row: Option<u64>,
}

#[derive(Serialize, Deserialize)]
struct File {
    qsaccess: Header,
    transfer: Transfer,
}

/// Why a request file could not be read or written.
#[derive(Debug, thiserror::Error)]
pub enum RequestError {
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("not a request file: {0}")]
    Parse(String),
    #[error("not a transfer request (kind {0})")]
    Kind(String),
    #[error("request file version {0} is newer than this program reads")]
    Newer(u32),
    #[error("the request names no {0}")]
    Missing(&'static str),
}

/// `file` as the request writes it: relative to `dir` (the request's
/// folder) when it is inside it, with `/` between parts.
pub fn relative(file: &Path, dir: &Path) -> String {
    let rel = file
        .strip_prefix(dir)
        .ok()
        .filter(|r| r.components().all(|c| matches!(c, Component::Normal(_))));
    match rel {
        Some(r) => r
            .components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join("/"),
        None => file.display().to_string(),
    }
}

/// A request's `file` as a path: relative ones from `dir`.
pub fn resolve(file: &str, dir: &Path) -> PathBuf {
    let p = PathBuf::from(file);
    if p.is_absolute() || file.is_empty() {
        p
    } else {
        dir.join(p)
    }
}

/// `r` as the text of a request file at `path`.
pub fn to_text(r: &Request, path: &Path) -> String {
    let dir = path.parent().unwrap_or(Path::new(""));
    let t = match r {
        Request::Download {
            system,
            download: d,
        } => Transfer {
            direction: Direction::Download,
            system: system.clone(),
            file: relative(&d.file, dir),
            options: d.options.clone(),
            source: Some(d.source.clone()),
            columns: d.columns.clone(),
            limit: d.limit,
            existing: d.existing,
            ..Transfer::default()
        },
        Request::Upload { system, upload: u } => Transfer {
            direction: Direction::Upload,
            system: system.clone(),
            file: relative(&u.file, dir),
            options: u.options.clone(),
            schema: u.schema.clone(),
            table: u.table.clone(),
            mode: u.mode,
            mapping: u.mapping.clone(),
            commit: u.commit,
            resume_row: u.resume_row,
            ..Transfer::default()
        },
    };
    let f = File {
        qsaccess: Header {
            kind: KIND.into(),
            version: VERSION,
        },
        transfer: t,
    };
    toml::to_string(&f).expect("a request serialises")
}

/// Writes `r` to `path`.
pub fn save(r: &Request, path: &Path) -> Result<(), RequestError> {
    std::fs::write(path, to_text(r, path))?;
    Ok(())
}

/// A request from its text, as read from `path`.
pub fn from_text(text: &str, path: &Path) -> Result<Request, RequestError> {
    let f: File = toml::from_str(text).map_err(|e| RequestError::Parse(e.message().to_owned()))?;
    if f.qsaccess.kind != KIND {
        return Err(RequestError::Kind(f.qsaccess.kind));
    }
    if f.qsaccess.version > VERSION {
        return Err(RequestError::Newer(f.qsaccess.version));
    }
    let t = f.transfer;
    if t.system.trim().is_empty() {
        return Err(RequestError::Missing("system"));
    }
    let dir = path.parent().unwrap_or(Path::new(""));
    let file = resolve(&t.file, dir);
    Ok(match t.direction {
        Direction::Download => Request::Download {
            system: t.system,
            download: Download {
                source: t.source.ok_or(RequestError::Missing("source"))?,
                columns: t.columns,
                limit: t.limit,
                file,
                options: t.options,
                existing: t.existing,
            },
        },
        Direction::Upload => {
            if t.schema.is_empty() || t.table.is_empty() {
                return Err(RequestError::Missing("target table"));
            }
            Request::Upload {
                system: t.system,
                upload: Upload {
                    file,
                    options: t.options,
                    schema: t.schema,
                    table: t.table,
                    mode: t.mode,
                    mapping: t.mapping,
                    commit: t.commit,
                    batch_bytes: 0,
                    resume_row: t.resume_row,
                },
            }
        }
    })
}

/// Reads the request at `path`.
pub fn load(path: &Path) -> Result<Request, RequestError> {
    from_text(&std::fs::read_to_string(path)?, path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format::{Dates, Format};
    use crate::upload::SourceColumn;

    #[test]
    fn a_download_both_ways() {
        let qdx = Path::new("/home/bob/req/assets.qdx");
        let r = Request::Download {
            system: "qs400.example.net".into(),
            download: Download {
                source: Source::Table {
                    schema: "HLINV".into(),
                    table: "ASSETS".into(),
                    where_clause: "STATUS = 'ACTIVE'".into(),
                },
                columns: vec!["ASSET_TAG".into(), "COST".into()],
                limit: Some(1000),
                file: PathBuf::from("/home/bob/req/out/assets.tsv"),
                options: FileOptions {
                    format: Format::Tsv,
                    dates: Dates::Job,
                    point: ',',
                    ..FileOptions::default()
                },
                existing: Existing::Replace,
            },
        };
        let text = to_text(&r, qdx);
        assert!(
            text.starts_with("[qsaccess]\nkind = \"transfer\"\nversion = 1\n"),
            "{text}"
        );
        assert!(text.contains("file = \"out/assets.tsv\""), "{text}");
        assert!(text.contains("where = \"STATUS = 'ACTIVE'\""), "{text}");
        assert!(!text.to_ascii_lowercase().contains("password"));
        assert_eq!(from_text(&text, qdx).unwrap(), r);
        // A file outside the request's folder stays absolute.
        assert_eq!(
            relative(Path::new("/tmp/x.csv"), Path::new("/home/bob")),
            "/tmp/x.csv"
        );
    }

    #[test]
    fn an_upload_both_ways() {
        let qdx = Path::new("/r/up.qdx");
        let r = Request::Upload {
            system: "S".into(),
            upload: Upload {
                file: PathBuf::from("/r/in.xlsx"),
                options: FileOptions {
                    format: Format::Xlsx,
                    sheet: "Assets".into(),
                    ..FileOptions::default()
                },
                schema: "HLTEST".into(),
                table: "JRN".into(),
                mode: Mode::Replace,
                mapping: vec![
                    Map {
                        target: "ID".into(),
                        from: SourceColumn::Position(1),
                    },
                    Map {
                        target: "NAME".into(),
                        from: SourceColumn::Name("Name".into()),
                    },
                ],
                commit: Commit::Every { batches: 5 },
                batch_bytes: 0,
                resume_row: Some(201),
            },
        };
        let text = to_text(&r, qdx);
        assert!(text.contains("resume_row = 201"), "{text}");
        assert_eq!(from_text(&text, qdx).unwrap(), r);
        for c in [Commit::Once, Commit::Without] {
            let mut r2 = r.clone();
            if let Request::Upload { upload, .. } = &mut r2 {
                upload.commit = c;
            }
            assert_eq!(from_text(&to_text(&r2, qdx), qdx).unwrap(), r2);
        }
    }

    #[test]
    fn what_is_refused() {
        let p = Path::new("/r/x.qdx");
        assert!(matches!(
            from_text(
                "[qsaccess]\nkind = \"display\"\nversion = 1\n[transfer]\n",
                p
            ),
            Err(RequestError::Kind(_))
        ));
        assert!(matches!(
            from_text(
                "[qsaccess]\nkind = \"transfer\"\nversion = 2\n[transfer]\n",
                p
            ),
            Err(RequestError::Newer(2))
        ));
        assert!(matches!(
            from_text(
                "[qsaccess]\nkind = \"transfer\"\nversion = 1\n[transfer]\nsystem = \"S\"\n",
                p
            ),
            Err(RequestError::Missing("source"))
        ));
        assert!(matches!(
            from_text("not toml [", p),
            Err(RequestError::Parse(_))
        ));
    }
}
