// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! `.qdx` request files: a transfer saved to run again, from its window or
//! with `qsaccess run`. Version 2 has two types of transfer: a table's rows
//! (Data Transfer) and files of the integrated file system. A version 1
//! file has no type and is a table's.
//!
//! ```toml
//! [qsaccess]
//! kind = "transfer"
//! version = 2
//!
//! [transfer]
//! type = "table"
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
//! A table upload has `schema`, `table`, `mode`, `[[transfer.mapping]]`,
//! `[transfer.commit]` and `resume_row` instead.
//!
//! ```toml
//! [transfer]
//! type = "files"
//! direction = "upload"
//! system = "qs400.example.net"
//! local = ["reports", "data/*.csv"]   # files, folders or a glob
//! remote = "/home/BOB/incoming"        # the system's folder
//! recurse = true
//! data = "text"                        # or "binary"
//! ccsid = 1208                         # for files made on the system
//! line_endings = "crlf"                # or "lf": text only
//! existing = "replace-if-newer"        # ask, skip, replace
//! ```
//!
//! A files download has the folder written into as its one `local`, and as
//! `remote` the system's folder, file, or folder and a name pattern
//! (`/home/BOB/reports/*.txt`). A request never holds a password.

use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::download::{Download, Source};
use crate::files::{Options as FileTransferOptions, Policy};
use crate::format::{Existing, FileOptions};
use crate::upload::{Commit, Map, Mode, Upload};

/// The `[qsaccess]` kind of a request file.
pub const KIND: &str = "transfer";
/// The request file version this program writes, and the newest it reads.
pub const VERSION: u32 = 2;
/// The request file extension.
pub const EXTENSION: &str = "qdx";

/// Which way a request goes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Direction {
    /// From the system to this computer.
    #[default]
    Download,
    /// From this computer to the system.
    Upload,
}

/// What a request moves.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Type {
    /// A table's rows (version 1 has no other).
    #[default]
    Table,
    /// Files of the integrated file system (version 2).
    Files,
}

/// A files request.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FilesRequest {
    pub direction: crate::files::Direction,
    /// An upload's files, folders and globs; a download's folder written
    /// into, the first.
    pub local: Vec<PathBuf>,
    /// An upload's folder written into; a download's folder, file, or
    /// folder and a name pattern.
    pub remote: String,
    /// Folders with their folders, all the way down.
    pub recurse: bool,
    pub options: FileTransferOptions,
}

/// A request: the system, and the transfer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Request {
    Download { system: String, download: Download },
    Upload { system: String, upload: Upload },
    Files { system: String, files: FilesRequest },
}

impl Request {
    pub fn system(&self) -> &str {
        match self {
            Request::Download { system, .. }
            | Request::Upload { system, .. }
            | Request::Files { system, .. } => system,
        }
    }

    /// The file the transfer reads or writes; a files request's first
    /// local path.
    pub fn file(&self) -> &Path {
        match self {
            Request::Download { download, .. } => &download.file,
            Request::Upload { upload, .. } => &upload.file,
            Request::Files { files, .. } => files.local.first().map_or(Path::new(""), |p| p),
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
    #[serde(rename = "type")]
    kind: Type,
    direction: Direction,
    system: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    file: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    options: Option<FileOptions>,
    // A table download.
    #[serde(skip_serializing_if = "Option::is_none")]
    source: Option<Source>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    columns: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    limit: Option<u64>,
    /// `ask`, `replace` or `append` for a table; `ask`, `skip`, `replace` or
    /// `replace-if-newer` for files.
    #[serde(skip_serializing_if = "String::is_empty")]
    existing: String,
    // A table upload.
    #[serde(skip_serializing_if = "String::is_empty")]
    schema: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    table: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    mode: Option<Mode>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    mapping: Vec<Map>,
    #[serde(skip_serializing_if = "Option::is_none")]
    commit: Option<Commit>,
    #[serde(skip_serializing_if = "Option::is_none")]
    resume_row: Option<u64>,
    // Files.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    local: Vec<String>,
    #[serde(skip_serializing_if = "String::is_empty")]
    remote: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    recurse: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<Data>,
    #[serde(skip_serializing_if = "Option::is_none")]
    ccsid: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    line_endings: Option<LineEndings>,
}

/// How a files request moves data.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Data {
    Binary,
    Text,
}

/// A text files request's line endings on this computer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum LineEndings {
    Crlf,
    Lf,
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

fn table_existing(e: Existing) -> &'static str {
    match e {
        Existing::Replace => "replace",
        Existing::Ask => "ask",
        Existing::Append => "append",
    }
}

fn policy_name(p: Policy) -> &'static str {
    match p {
        Policy::Ask => "ask",
        Policy::Skip => "skip",
        Policy::Replace => "replace",
        Policy::ReplaceIfNewer => "replace-if-newer",
    }
}

fn parse_table_existing(s: &str) -> Result<Existing, RequestError> {
    match s {
        "" | "ask" => Ok(Existing::Ask),
        "replace" => Ok(Existing::Replace),
        "append" => Ok(Existing::Append),
        other => Err(RequestError::Parse(format!(
            "existing = \"{other}\" is not ask, replace or append"
        ))),
    }
}

fn parse_policy(s: &str) -> Result<Policy, RequestError> {
    match s {
        "" | "ask" => Ok(Policy::Ask),
        "skip" => Ok(Policy::Skip),
        "replace" => Ok(Policy::Replace),
        "replace-if-newer" => Ok(Policy::ReplaceIfNewer),
        other => Err(RequestError::Parse(format!(
            "existing = \"{other}\" is not ask, skip, replace or replace-if-newer"
        ))),
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
            kind: Type::Table,
            direction: Direction::Download,
            system: system.clone(),
            file: relative(&d.file, dir),
            options: Some(d.options.clone()),
            source: Some(d.source.clone()),
            columns: d.columns.clone(),
            limit: d.limit,
            existing: table_existing(d.existing).into(),
            mode: Some(Mode::default()),
            commit: Some(Commit::default()),
            ..Transfer::default()
        },
        Request::Upload { system, upload: u } => Transfer {
            kind: Type::Table,
            direction: Direction::Upload,
            system: system.clone(),
            file: relative(&u.file, dir),
            options: Some(u.options.clone()),
            existing: table_existing(Existing::default()).into(),
            schema: u.schema.clone(),
            table: u.table.clone(),
            mode: Some(u.mode),
            mapping: u.mapping.clone(),
            commit: Some(u.commit),
            resume_row: u.resume_row,
            ..Transfer::default()
        },
        Request::Files { system, files: f } => Transfer {
            kind: Type::Files,
            direction: match f.direction {
                crate::files::Direction::Download => Direction::Download,
                crate::files::Direction::Upload => Direction::Upload,
            },
            system: system.clone(),
            local: f.local.iter().map(|p| relative(p, dir)).collect(),
            remote: f.remote.clone(),
            recurse: Some(f.recurse),
            data: Some(if f.options.text {
                Data::Text
            } else {
                Data::Binary
            }),
            ccsid: Some(f.options.ccsid),
            line_endings: Some(if f.options.crlf {
                LineEndings::Crlf
            } else {
                LineEndings::Lf
            }),
            existing: policy_name(f.options.existing).into(),
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
    // Version 1 knew no other type; its `type` was never written.
    let kind = if f.qsaccess.version < 2 {
        Type::Table
    } else {
        t.kind
    };
    if kind == Type::Files {
        if !t.remote.starts_with('/') {
            return Err(RequestError::Missing("folder on the system (remote)"));
        }
        if t.local.is_empty() {
            return Err(RequestError::Missing("files on this computer (local)"));
        }
        let text = t.data == Some(Data::Text);
        let defaults = FileTransferOptions::default();
        return Ok(Request::Files {
            system: t.system,
            files: FilesRequest {
                direction: match t.direction {
                    Direction::Download => crate::files::Direction::Download,
                    Direction::Upload => crate::files::Direction::Upload,
                },
                local: t.local.iter().map(|l| resolve(l, dir)).collect(),
                remote: t.remote,
                recurse: t.recurse.unwrap_or(true),
                options: FileTransferOptions {
                    text,
                    ccsid: t.ccsid.filter(|c| *c != 0).unwrap_or(defaults.ccsid),
                    crlf: match t.line_endings {
                        Some(l) => l == LineEndings::Crlf,
                        None => defaults.crlf,
                    },
                    existing: parse_policy(&t.existing)?,
                },
            },
        });
    }
    let file = resolve(&t.file, dir);
    let options = t.options.unwrap_or_default();
    Ok(match t.direction {
        Direction::Download => Request::Download {
            system: t.system,
            download: Download {
                source: t.source.ok_or(RequestError::Missing("source"))?,
                columns: t.columns,
                limit: t.limit,
                file,
                options,
                existing: parse_table_existing(&t.existing)?,
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
                    options,
                    schema: t.schema,
                    table: t.table,
                    mode: t.mode.unwrap_or_default(),
                    mapping: t.mapping,
                    commit: t.commit.unwrap_or_default(),
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

    fn assets() -> Request {
        Request::Download {
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
        }
    }

    #[test]
    fn a_download_both_ways() {
        let qdx = Path::new("/home/bob/req/assets.qdx");
        let r = assets();
        let text = to_text(&r, qdx);
        assert!(
            text.starts_with(
                "[qsaccess]\nkind = \"transfer\"\nversion = 2\n\n[transfer]\ntype = \"table\"\n"
            ),
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
    fn a_version_1_file_is_a_table_request() {
        // As version 1 wrote it: what version 2 writes for a table, without
        // the type.
        let qdx = Path::new("/home/bob/req/assets.qdx");
        let v1 = to_text(&assets(), qdx)
            .replace("version = 2\n", "version = 1\n")
            .replace("type = \"table\"\n", "");
        assert!(!v1.contains("type"), "{v1}");
        assert_eq!(from_text(&v1, qdx).unwrap(), assets());
        let upload = to_text(
            &Request::Upload {
                system: "S".into(),
                upload: Upload {
                    file: PathBuf::from("/home/bob/req/in.csv"),
                    schema: "HLTEST".into(),
                    table: "JRN".into(),
                    ..Upload::default()
                },
            },
            qdx,
        )
        .replace("version = 2\n", "version = 1\n")
        .replace("type = \"table\"\n", "");
        assert!(matches!(
            from_text(&upload, qdx).unwrap(),
            Request::Upload { .. }
        ));
        // A type in a version 1 file is not looked at.
        let typed = v1.replace("[transfer]\n", "[transfer]\ntype = \"files\"\n");
        assert_eq!(from_text(&typed, qdx).unwrap(), assets());
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
    fn files_both_ways() {
        let qdx = Path::new("/home/bob/req/files.qdx");
        let up = Request::Files {
            system: "qs400.example.net".into(),
            files: FilesRequest {
                direction: crate::files::Direction::Upload,
                local: vec![
                    PathBuf::from("/home/bob/req/reports"),
                    PathBuf::from("/home/bob/req/data/*.csv"),
                    PathBuf::from("/srv/elsewhere.txt"),
                ],
                remote: "/home/BOB/incoming".into(),
                recurse: true,
                options: FileTransferOptions {
                    text: true,
                    ccsid: 37,
                    crlf: true,
                    existing: Policy::ReplaceIfNewer,
                },
            },
        };
        let text = to_text(&up, qdx);
        for want in [
            "version = 2",
            "type = \"files\"",
            "direction = \"upload\"",
            "local = [\"reports\", \"data/*.csv\", \"/srv/elsewhere.txt\"]",
            "remote = \"/home/BOB/incoming\"",
            "recurse = true",
            "data = \"text\"",
            "ccsid = 37",
            "line_endings = \"crlf\"",
            "existing = \"replace-if-newer\"",
        ] {
            assert!(text.contains(want), "{want} in\n{text}");
        }
        assert!(
            !text.contains("[transfer.options]") && !text.contains("mode ="),
            "{text}"
        );
        assert_eq!(from_text(&text, qdx).unwrap(), up);
        assert_eq!(up.file(), Path::new("/home/bob/req/reports"));
        let down = Request::Files {
            system: "S".into(),
            files: FilesRequest {
                direction: crate::files::Direction::Download,
                local: vec![PathBuf::from("/home/bob/req/in")],
                remote: "/home/BOB/reports/*.txt".into(),
                recurse: false,
                options: FileTransferOptions {
                    text: false,
                    ccsid: 1208,
                    crlf: false,
                    existing: Policy::Skip,
                },
            },
        };
        assert_eq!(from_text(&to_text(&down, qdx), qdx).unwrap(), down);
        // What a files request may leave out.
        let short = "[qsaccess]\nkind = \"transfer\"\nversion = 2\n[transfer]\ntype = \"files\"\nsystem = \"S\"\nlocal = [\"in\"]\nremote = \"/home/BOB\"\n";
        let Request::Files { files, .. } = from_text(short, qdx).unwrap() else {
            panic!("a files request")
        };
        assert_eq!(files.direction, crate::files::Direction::Download);
        assert!(files.recurse);
        assert_eq!(files.options, FileTransferOptions::default());
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
                "[qsaccess]\nkind = \"transfer\"\nversion = 3\n[transfer]\n",
                p
            ),
            Err(RequestError::Newer(3))
        ));
        assert!(matches!(
            from_text(
                "[qsaccess]\nkind = \"transfer\"\nversion = 1\n[transfer]\nsystem = \"S\"\n",
                p
            ),
            Err(RequestError::Missing("source"))
        ));
        let files = "[qsaccess]\nkind = \"transfer\"\nversion = 2\n[transfer]\ntype = \"files\"\nsystem = \"S\"\n";
        assert!(matches!(
            from_text(&format!("{files}local = [\"a\"]\nremote = \"home\"\n"), p),
            Err(RequestError::Missing(_))
        ));
        assert!(matches!(
            from_text(&format!("{files}remote = \"/home\"\n"), p),
            Err(RequestError::Missing(_))
        ));
        assert!(matches!(
            from_text(
                &format!("{files}local = [\"a\"]\nremote = \"/home\"\nexisting = \"append\"\n"),
                p
            ),
            Err(RequestError::Parse(_))
        ));
        assert!(matches!(
            from_text("not toml [", p),
            Err(RequestError::Parse(_))
        ));
    }
}
