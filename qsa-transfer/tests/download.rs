// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! Downloads against QS/400's test system: a table to CSV with its exact
//! text, columns chosen and ordered with a `WHERE` clause and a row limit,
//! the all-types table to every format (spreadsheet cells as decision 5
//! says), appending, an existing file, and cancel and failure leaving no
//! file behind.

#[path = "../../qap-client/tests/common/mod.rs"]
mod common;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use calamine::{Data, Reader};
use common::{Server, cfg, start};
use qap_client::{Client, Secret, SqlConnection};
use qsa_sql::DateStyle;
use qsa_transfer::{Download, Existing, Failure, FileOptions, Format, Source, download};

fn connection(s: &Server) -> SqlConnection {
    let mut c = Client::new(s.target());
    c.sign_on("BOB", &Secret::from("SECRET1")).unwrap();
    c.sql_connection().unwrap()
}

fn dir(name: &str) -> PathBuf {
    let d = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join("download")
        .join(name);
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn table(schema: &str, t: &str) -> Source {
    Source::Table {
        schema: schema.into(),
        table: t.into(),
        where_clause: String::new(),
    }
}

fn get(q: &mut SqlConnection, d: &Download) -> Result<qsa_transfer::Summary, Failure> {
    download::run(q, d, DateStyle::Iso, 37, |_| {}, &AtomicBool::new(false))
}

/// Only the file itself is in `d`: no temporary file left behind.
fn only(d: &Path, names: &[&str]) {
    let mut got: Vec<String> = std::fs::read_dir(d)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    got.sort();
    assert_eq!(got, names);
}

#[test]
fn a_table_to_csv_with_its_exact_text() {
    let s = start(cfg());
    let mut q = connection(&s);
    let d = dir("assets");
    let file = d.join("assets.csv");
    let r = get(
        &mut q,
        &Download {
            source: table("HLINV", "ASSETS"),
            file: file.clone(),
            ..Download::default()
        },
    )
    .unwrap();
    assert_eq!(r.rows, 12);
    let text = std::fs::read_to_string(&file).unwrap();
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 13);
    assert_eq!(
        lines[0],
        "ASSET_TAG,LOCATION,MANUFACTURER,MODEL,SERIAL,ASSET_TYPE,STATUS,ACQUIRED,COST,RACK_U,NOTES,CHANGED_TS"
    );
    // CHAR padding is quoted to keep it; NULL is an empty field; decimals
    // and timestamps exact.
    assert_eq!(
        lines[1],
        "\"QS000101  \",\"QS-HQ-R1  \",QUANTA,T4048-IX8,QTFCEV2340021,\"SWITCH      \",ACTIVE  ,2025-03-11,1200.00,10,,2026-10-29T13:58:02.000000"
            .replace("ACTIVE  ,", "\"ACTIVE  \",")
    );
    assert!(text.contains("\r\n"));
    only(&d, &["assets.csv"]);
}

#[test]
fn columns_chosen_a_where_clause_and_a_limit() {
    let s = start(cfg());
    let mut q = connection(&s);
    let d = dir("chosen");
    let file = d.join("a.tsv");
    let r = get(
        &mut q,
        &Download {
            source: Source::Table {
                schema: "HLINV".into(),
                table: "ASSETS".into(),
                where_clause: "STATUS = 'ACTIVE'".into(),
            },
            columns: vec!["COST".into(), "ASSET_TAG".into()],
            limit: Some(3),
            file: file.clone(),
            options: FileOptions {
                format: Format::Tsv,
                header: false,
                point: ',',
                ..FileOptions::default()
            },
            ..Download::default()
        },
    )
    .unwrap();
    assert_eq!(r.rows, 3);
    let text = std::fs::read_to_string(&file).unwrap();
    assert_eq!(
        text,
        "1200,00\t\"QS000101  \"\r\n1350,00\t\"QS000102  \"\r\n1500,00\t\"QS000110  \"\r\n"
    );
    // A query's columns, chosen by name.
    let file = d.join("q.csv");
    get(
        &mut q,
        &Download {
            source: Source::Query {
                sql: "SELECT * FROM HLINV.LOCATIONS;".into(),
            },
            columns: vec!["SITE".into()],
            file: file.clone(),
            ..Download::default()
        },
    )
    .unwrap();
    let text = std::fs::read_to_string(&file).unwrap();
    assert!(text.starts_with("SITE\r\nHeadquarters\r\n"));
    match get(
        &mut q,
        &Download {
            source: Source::Query {
                sql: "SELECT * FROM HLINV.LOCATIONS".into(),
            },
            columns: vec!["NOPE".into()],
            file: d.join("x.csv"),
            ..Download::default()
        },
    ) {
        Err(Failure::NoColumn(c)) => assert_eq!(c, "NOPE"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn every_format_of_the_all_types_table() {
    let s = start(cfg());
    let mut q = connection(&s);
    let d = dir("alltypes");
    for f in Format::ALL {
        let file = d.join(format!("all.{}", f.extension()));
        let r = get(
            &mut q,
            &Download {
                source: table("HLTEST", "ALLTYPES"),
                file: file.clone(),
                options: FileOptions {
                    format: f,
                    ..FileOptions::default()
                },
                ..Download::default()
            },
        )
        .unwrap();
        assert_eq!(r.rows, 7, "{f:?}");
        assert!(std::fs::metadata(&file).unwrap().len() > 0);
    }
    let csv = std::fs::read_to_string(d.join("all.csv")).unwrap();
    for exact in [
        "1.5E-30",
        "sNaN",
        "-Infinity",
        "2026-11-02T09:14:03.123456789012",
        "0001FEFF40404040",
        "true",
        "false",
        "Grüße",
        "\"line\nbreak\"",
    ] {
        assert!(csv.contains(exact), "{exact}");
    }
    // The seventh row: its ID, then every field NULL (empty, unquoted).
    let last = csv.lines().last().unwrap();
    assert_eq!(last, format!("7{}", ",".repeat(22)));

    // Spreadsheets: number cells only where exact, text otherwise.
    for f in [Format::Xlsx, Format::Ods] {
        let path = d.join(format!("all.{}", f.extension()));
        let mut book = calamine::open_workbook_auto(&path).unwrap();
        let sheet = book.worksheet_range_at(0).unwrap().unwrap();
        let header: Vec<String> = sheet
            .rows()
            .next()
            .unwrap()
            .iter()
            .map(ToString::to_string)
            .collect();
        let at = |name: &str| header.iter().position(|h| h == name).unwrap();
        let rows: Vec<&[Data]> = sheet.rows().skip(1).collect();
        assert_eq!(rows.len(), 7, "{f:?}");
        assert!(matches!(rows[0][at("C_DECIMAL")], Data::Float(_)), "{f:?}");
        assert!(
            matches!(rows[0][at("C_BOOLEAN")], Data::Bool(true)),
            "{f:?}"
        );
        assert_eq!(
            rows[2][at("C_DECFLOAT")],
            Data::String("Infinity".into()),
            "{f:?}"
        );
        // BIGINT past 15 digits is text.
        assert!(matches!(rows[0][at("C_BIGINT")], Data::String(_)), "{f:?}");
        assert_eq!(
            rows[0][at("C_BITS")],
            Data::String("0001FEFF40404040".into()),
            "{f:?}"
        );
        assert_eq!(
            rows[6][at("C_VARCHAR")],
            Data::Empty,
            "{f:?}: NULL is no value"
        );
        assert_eq!(
            rows[3][at("C_VARCHAR")],
            Data::String(String::new()),
            "{f:?}: an empty string"
        );
        assert_eq!(
            rows[4][at("C_VARCHAR")],
            Data::String(" lead".into()),
            "{f:?}"
        );
    }
    // Fixed width: one record per row and the null indicators.
    let fixed = std::fs::read_to_string(d.join("all.txt")).unwrap();
    assert!(fixed.contains("1.5E-30"));
}

#[test]
fn appending_and_an_existing_file() {
    let s = start(cfg());
    let mut q = connection(&s);
    let d = dir("append");
    let file = d.join("loc.csv");
    let mut req = Download {
        source: table("HLINV", "LOCATIONS"),
        file: file.clone(),
        ..Download::default()
    };
    get(&mut q, &req).unwrap();
    assert!(matches!(get(&mut q, &req), Err(Failure::Exists(_))));
    req.existing = Existing::Append;
    get(&mut q, &req).unwrap();
    let text = std::fs::read_to_string(&file).unwrap();
    assert_eq!(text.lines().count(), 1 + 4 + 4, "one header, then both");
    req.existing = Existing::Replace;
    get(&mut q, &req).unwrap();
    assert_eq!(std::fs::read_to_string(&file).unwrap().lines().count(), 5);
    only(&d, &["loc.csv"]);
}

#[test]
fn cancel_and_failure_leave_nothing_behind() {
    let s = start(cfg());
    let mut q = connection(&s);
    let d = dir("cancel");
    let cancel = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&cancel);
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(300));
        flag.store(true, Ordering::SeqCst);
    });
    let req = Download {
        source: table("HLTEST", "SLOW"),
        file: d.join("slow.csv"),
        ..Download::default()
    };
    match download::run(&mut q, &req, DateStyle::Iso, 37, |_| {}, &cancel) {
        Err(Failure::Cancelled { message, .. }) => {
            assert_eq!(message.unwrap().id, "SQL0952");
        }
        other => panic!("{other:?}"),
    }
    only(&d, &[]);
    // A statement that fails: nothing written.
    let bad = Download {
        source: Source::Query {
            sql: "SELEKT 1".into(),
        },
        file: d.join("bad.csv"),
        ..Download::default()
    };
    match get(&mut q, &bad) {
        Err(Failure::System(e)) => assert_eq!(e.server_message().unwrap().id, "SQL0104"),
        other => panic!("{other:?}"),
    }
    only(&d, &[]);
    // A value too wide for its fixed-width column: nothing written.
    let narrow = Download {
        source: table("HLINV", "LOCATIONS"),
        file: d.join("narrow.txt"),
        options: FileOptions {
            format: Format::Fixed,
            widths: vec![10, 3],
            ..FileOptions::default()
        },
        ..Download::default()
    };
    assert!(matches!(get(&mut q, &narrow), Err(Failure::File(_))));
    only(&d, &[]);
}
