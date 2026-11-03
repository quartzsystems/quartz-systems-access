// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! Uploads against QS/400's test system: mapping, a failing row in each
//! commit mode with the resume row decision 4 gives, the unjournaled-table
//! fallback, Replace in one transaction, cancel, and the round trip of the
//! all-types table through every format, which must reproduce it exactly,
//! and `DECFLOAT` texts (`1.50`, `1E+2`, `-0`) kept as text through each.

#[path = "../../qap-client/tests/common/mod.rs"]
mod common;

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use common::{Server, cfg, start};
use qap_client::proto::ops::sql::{SetOptions, SqlValue};
use qap_client::{Client, FetchEnd, Secret, SqlConnection};
use qsa_sql::DateStyle;
use qsa_transfer::upload::{self, Failure, Stop};
use qsa_transfer::{
    Commit, Download, FileOptions, Format, Map, Mode, Source, SourceColumn, Upload, download,
};

fn connection(s: &Server) -> SqlConnection {
    let mut c = Client::new(s.target());
    c.sign_on("BOB", &Secret::from("SECRET1")).unwrap();
    c.sql_connection().unwrap()
}

fn dir(name: &str) -> PathBuf {
    let d = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join("upload")
        .join(name);
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn rows(q: &mut SqlConnection, sql: &str) -> Vec<Vec<SqlValue>> {
    let p = q.prepare(sql).unwrap();
    let c = q
        .execute_now(p.statement, Vec::new())
        .unwrap()
        .executed
        .cursor
        .unwrap();
    let mut out = Vec::new();
    let end = q
        .fetch_into(
            c,
            None,
            &mut |b| {
                out.extend(b);
                Ok(())
            },
            &AtomicBool::new(false),
        )
        .unwrap();
    assert!(matches!(end, FetchEnd::Done { .. }));
    q.close_cursor(c).unwrap();
    q.release(p.statement).unwrap();
    out
}

fn put(q: &mut SqlConnection, u: &Upload) -> Result<qsa_transfer::Summary, Failure> {
    upload::run(q, u, DateStyle::Iso, 37, |_| {}, &AtomicBool::new(false))
}

/// A CSV of `ID,NAME` rows, the IDs given.
fn ids_csv(d: &std::path::Path, name: &str, ids: &[i64]) -> PathBuf {
    let mut s = String::from("ID,NAME\r\n");
    for i in ids {
        s.push_str(&format!("{i},row {i}\r\n"));
    }
    let p = d.join(name);
    std::fs::write(&p, s).unwrap();
    p
}

fn ids(q: &mut SqlConnection, table: &str) -> Vec<i64> {
    let mut v: Vec<i64> = rows(q, &format!("SELECT ID FROM HLTEST.{table}"))
        .into_iter()
        .map(|r| match r[0] {
            SqlValue::Integer { value } => value,
            ref o => panic!("{o:?}"),
        })
        .collect();
    v.sort_unstable();
    v
}

fn jrn(file: PathBuf, commit: Commit) -> Upload {
    Upload {
        file,
        schema: "HLTEST".into(),
        table: "JRN".into(),
        commit,
        // About seven rows a request, so there are several batches.
        batch_bytes: 300,
        ..Upload::default()
    }
}

#[test]
fn a_csv_by_name_and_mapped_by_hand() {
    let s = start(cfg());
    let mut q = connection(&s);
    let d = dir("mapped");
    // Columns in another order: mapped by header name.
    let p = d.join("a.csv");
    std::fs::write(&p, "NAME,ID\r\none,1\r\n,2\r\n\"\",3\r\n").unwrap();
    let r = put(&mut q, &jrn(p.clone(), Commit::Once)).unwrap();
    assert_eq!(r.rows, 3);
    let got = rows(&mut q, "SELECT * FROM HLTEST.JRN WHERE ID = 2");
    assert_eq!(got[0][1], SqlValue::Null, "an empty unquoted field is NULL");
    let got = rows(&mut q, "SELECT * FROM HLTEST.JRN WHERE ID = 3");
    assert_eq!(
        got[0][1],
        SqlValue::Text {
            text: String::new()
        }
    );
    // By hand: ID from position 1, NAME left to its default.
    let p2 = d.join("b.csv");
    std::fs::write(&p2, "x,10\r\ny,11\r\n").unwrap();
    let mut u = jrn(p2, Commit::Once);
    u.options.header = false;
    u.mapping = vec![Map {
        target: "ID".into(),
        from: SourceColumn::Position(1),
    }];
    put(&mut q, &u).unwrap();
    assert_eq!(ids(&mut q, "JRN"), [1, 2, 3, 10, 11]);
}

#[test]
fn a_failing_row_in_one_transaction_keeps_nothing() {
    let s = start(cfg());
    let mut q = connection(&s);
    let d = dir("once");
    // Row 17 repeats row 3's key.
    let mut list: Vec<i64> = (1..=30).collect();
    list[16] = 3;
    let f = put(&mut q, &jrn(ids_csv(&d, "a.csv", &list), Commit::Once)).unwrap_err();
    assert_eq!(f.stop, Stop::System);
    assert_eq!(
        (f.row, f.line),
        (Some(17), Some(18)),
        "row 17, line 18 after the header"
    );
    assert_eq!(f.message.unwrap().id, "SQL0803");
    assert_eq!((f.kept, f.resume), (0, None));
    assert!(ids(&mut q, "JRN").is_empty());
}

#[test]
fn a_failing_row_with_commits_every_batch_resumes_from_the_first_row_not_committed() {
    let s = start(cfg());
    let mut q = connection(&s);
    let d = dir("every");
    let mut list: Vec<i64> = (1..=30).collect();
    list[16] = 3;
    let file = ids_csv(&d, "a.csv", &list);
    let f = put(&mut q, &jrn(file, Commit::Every { batches: 1 })).unwrap_err();
    assert_eq!(f.row, Some(17));
    // The batches before the failing one are committed; the failing
    // batch's earlier rows are rolled back with it.
    let kept = ids(&mut q, "JRN");
    assert_eq!(kept.len() as u64, f.kept);
    assert!(f.kept > 0 && f.kept < 16, "{}", f.kept);
    assert_eq!(
        f.resume,
        Some(f.kept + 1),
        "the first row not committed, not the failing row"
    );
    assert_eq!(kept, (1..=f.kept as i64).collect::<Vec<_>>());
    // Fixed and resumed from there: every row once.
    list[16] = 17;
    let mut u = jrn(ids_csv(&d, "a.csv", &list), Commit::Every { batches: 1 });
    u.resume_row = f.resume;
    let r = put(&mut q, &u).unwrap();
    assert_eq!(r.rows, 30 - f.kept);
    assert_eq!(ids(&mut q, "JRN"), (1..=30).collect::<Vec<_>>());
}

#[test]
fn an_unjournaled_table_falls_back_and_resumes_from_the_failing_row() {
    let s = start(cfg());
    let mut q = connection(&s);
    let d = dir("nojrn");
    let mut list: Vec<i64> = (1..=30).collect();
    list[16] = 3;
    let file = ids_csv(&d, "a.csv", &list);
    let mut u = jrn(file.clone(), Commit::Once);
    u.table = "NOJRN".into();
    // Under commitment control: SQL7008 at the first Execute that changes
    // data; nothing went in.
    let f = put(&mut q, &u).unwrap_err();
    assert_eq!(f.stop, Stop::NoCommitmentControl);
    assert_eq!(f.message.as_ref().unwrap().id, "SQL7008");
    assert!(ids(&mut q, "NOJRN").is_empty());
    // Without it: every row before the failing one stays, the failing
    // request's earlier rows included; resume from the failing row.
    u.commit = Commit::Without;
    let f = put(&mut q, &u).unwrap_err();
    assert_eq!(f.row, Some(17));
    assert_eq!((f.kept, f.resume), (16, Some(17)));
    assert_eq!(ids(&mut q, "NOJRN"), (1..=16).collect::<Vec<_>>());
    list[16] = 17;
    u.file = ids_csv(&d, "a.csv", &list);
    u.resume_row = f.resume;
    put(&mut q, &u).unwrap();
    assert_eq!(ids(&mut q, "NOJRN"), (1..=30).collect::<Vec<_>>());
}

#[test]
fn replace_deletes_in_the_same_transaction() {
    let s = start(cfg());
    let mut q = connection(&s);
    let d = dir("replace");
    put(&mut q, &jrn(ids_csv(&d, "a.csv", &[1, 2, 3]), Commit::Once)).unwrap();
    // A failing replace keeps the old rows.
    let mut u = jrn(ids_csv(&d, "b.csv", &[7, 7]), Commit::Once);
    u.mode = Mode::Replace;
    assert!(put(&mut q, &u).is_err());
    assert_eq!(ids(&mut q, "JRN"), [1, 2, 3]);
    u.file = ids_csv(&d, "b.csv", &[7, 8]);
    put(&mut q, &u).unwrap();
    assert_eq!(ids(&mut q, "JRN"), [7, 8]);
}

#[test]
fn a_misfit_and_a_cancel() {
    let s = start(cfg());
    let mut q = connection(&s);
    let d = dir("misfit");
    let p = d.join("a.csv");
    std::fs::write(&p, "ID,NAME\r\n1,ok\r\nx,bad id\r\n").unwrap();
    let f = put(&mut q, &jrn(p, Commit::Once)).unwrap_err();
    assert_eq!(f.stop, Stop::Misfit);
    assert_eq!((f.row, f.line), (Some(2), Some(3)));
    assert_eq!(f.reason, "Column ID: value x does not fit INTEGER");
    assert!(ids(&mut q, "JRN").is_empty());

    // A cancel is a failed run: nothing kept in one transaction.
    let cancel = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&cancel);
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(300));
        flag.store(true, Ordering::SeqCst);
    });
    let mut u = jrn(
        ids_csv(&d, "slow.csv", &(1..=200).collect::<Vec<_>>()),
        Commit::Once,
    );
    u.table = "SLOWLOG".into();
    u.batch_bytes = 0;
    let f = upload::run(&mut q, &u, DateStyle::Iso, 37, |_| {}, &cancel).unwrap_err();
    assert_eq!((f.stop, f.kept, f.resume), (Stop::Cancelled, 0, None));
    assert!(ids(&mut q, "SLOWLOG").is_empty());
}

#[test]
fn the_all_types_table_round_trips_through_every_format() {
    let s = start(cfg());
    let mut q = connection(&s);
    let before = rows(&mut q, "SELECT * FROM HLTEST.ALLTYPES");
    assert_eq!(before.len(), 7);
    let d = dir("roundtrip");
    for format in Format::ALL {
        for bits_as_text in [false, true] {
            let file = d.join(format!("all-{bits_as_text}.{}", format.extension()));
            let options = FileOptions {
                format,
                bit_data_as_text: bits_as_text,
                ..FileOptions::default()
            };
            let got = download::run(
                &mut q,
                &Download {
                    source: Source::Table {
                        schema: "HLTEST".into(),
                        table: "ALLTYPES".into(),
                        where_clause: String::new(),
                    },
                    file: file.clone(),
                    options: options.clone(),
                    existing: qsa_transfer::Existing::Replace,
                    ..Download::default()
                },
                DateStyle::Iso,
                37,
                |_| {},
                &AtomicBool::new(false),
            );
            // CCSID 65535 bytes as text in CCSID 37 include control
            // characters, which ODS (XML) cannot hold: refused, never
            // dropped, and no file is left.
            if format == Format::Ods && bits_as_text {
                match got {
                    Err(qsa_transfer::Failure::File(why)) => {
                        assert!(why.contains("C_BITS"), "{why}")
                    }
                    other => panic!("{other:?}"),
                }
                assert!(!file.exists());
                continue;
            }
            got.unwrap();
            let r = put(
                &mut q,
                &Upload {
                    file: file.clone(),
                    options,
                    schema: "HLTEST".into(),
                    table: "ALLTYPES".into(),
                    mode: Mode::Replace,
                    ..Upload::default()
                },
            )
            .unwrap_or_else(|f| panic!("{format:?} (CCSID 65535 as text: {bits_as_text}): {f:?}"));
            assert_eq!(r.rows, 7);
            let after = rows(&mut q, "SELECT * FROM HLTEST.ALLTYPES");
            for (i, (a, b)) in before.iter().zip(&after).enumerate() {
                assert_eq!(
                    a,
                    b,
                    "{format:?} (bits as text {bits_as_text}), row {}",
                    i + 1
                );
            }
            assert_eq!(after.len(), 7);
        }
    }
    let _ = q.set_options(SetOptions::default());
}

#[test]
fn decfloat_text_round_trips_exactly_through_every_format() {
    let s = start(cfg());
    let mut q = connection(&s);
    let d = dir("decfloat");
    // The engine's own texts, each kept as sent: never compared by value.
    for v in [
        "1.50", "1E+2", "100", "0E+3", "-0", "1E-7", "-12.50", "1.5E-30",
    ] {
        let set = q
            .prepare("UPDATE HLTEST.ALLTYPES SET C_DECFLOAT = ? WHERE ID = 7")
            .unwrap();
        q.execute_now(
            set.statement,
            vec![vec![SqlValue::Decimal { text: v.into() }]],
        )
        .unwrap();
        q.release(set.statement).unwrap();
        for format in Format::ALL {
            let file = d.join(format!("df.{}", format.extension()));
            let options = FileOptions {
                format,
                ..FileOptions::default()
            };
            download::run(
                &mut q,
                &Download {
                    source: Source::Table {
                        schema: "HLTEST".into(),
                        table: "ALLTYPES".into(),
                        where_clause: "ID = 7".into(),
                    },
                    file: file.clone(),
                    options: options.clone(),
                    existing: qsa_transfer::Existing::Replace,
                    ..Download::default()
                },
                DateStyle::Iso,
                37,
                |_| {},
                &AtomicBool::new(false),
            )
            .unwrap();
            let del = q
                .prepare("DELETE FROM HLTEST.ALLTYPES WHERE ID = 7")
                .unwrap();
            q.execute_now(del.statement, Vec::new()).unwrap();
            q.release(del.statement).unwrap();
            put(
                &mut q,
                &Upload {
                    file: file.clone(),
                    options,
                    schema: "HLTEST".into(),
                    table: "ALLTYPES".into(),
                    mode: Mode::Append,
                    ..Upload::default()
                },
            )
            .unwrap_or_else(|f| panic!("{v} through {format:?}: {f:?}"));
            let back = rows(
                &mut q,
                "SELECT C_DECFLOAT FROM HLTEST.ALLTYPES WHERE ID = 7",
            );
            assert_eq!(
                back,
                [vec![SqlValue::Decimal { text: v.into() }]],
                "{v} through {format:?}"
            );
        }
    }
    let _ = q.set_options(SetOptions::default());
}
