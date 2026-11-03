// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! QAP version 4's `Sql` service, `Attach` and `JobInfo` against QS/400's
//! engine and its test database (`qapd::testing::sql`): every operation
//! and status, `Attach`'s place and limit, `SessionEnded` after a sign-off
//! elsewhere, the three cancel bodies, rollback when a connection closes,
//! every type, streaming in blocks, a connection dropped under a fetch, and
//! a slow select (plain and under `RepeatableRead`) holding up no other
//! connection of the session: Run Command, Printer Output and another
//! window's query answer at once beside it.

mod common;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use common::{Server, cfg, start, until};
use qap_client::proto::Op;
use qap_client::proto::ops::signon::{Attach, SignOn, SignOnReply};
use qap_client::proto::ops::sql::{
    CommitLevel, CursorRef, Fetch, ListSchemas, ListTables, Naming, SetOptions, SqlValue, TableKind,
};
use qap_client::{Client, Connection, Error, FetchEnd, Secret, SqlConnection, Status};

fn signed_on(s: &Server, user: &str, password: &str) -> Client {
    let mut c = Client::new(s.target());
    c.sign_on(user, &Secret::from(password)).unwrap();
    c
}

fn bob(s: &Server) -> (Client, SqlConnection) {
    let mut c = signed_on(s, "BOB", "SECRET1");
    let q = c.sql_connection().unwrap();
    (c, q)
}

/// Runs a query and fetches every row.
fn query(q: &mut SqlConnection, sql: &str) -> Vec<Vec<SqlValue>> {
    let p = q.prepare(sql).unwrap();
    let o = q.execute_now(p.statement, Vec::new()).unwrap();
    let cursor = o.executed.cursor.expect("a query has a cursor");
    let mut rows = Vec::new();
    let end = q
        .fetch_into(
            cursor,
            None,
            &mut |b| {
                rows.extend(b);
                Ok(())
            },
            &AtomicBool::new(false),
        )
        .unwrap();
    assert!(matches!(end, FetchEnd::Done { .. }), "{end:?}");
    q.close_cursor(cursor).unwrap();
    q.release(p.statement).unwrap();
    rows
}

fn text(v: &SqlValue) -> &str {
    match v {
        SqlValue::Text { text } | SqlValue::Decimal { text } | SqlValue::Float { text } => text,
        other => panic!("{other:?}"),
    }
}

fn int(v: i64) -> SqlValue {
    SqlValue::Integer { value: v }
}

fn name(v: &str) -> SqlValue {
    SqlValue::Text { text: v.into() }
}

fn sql_state(e: &Error) -> (String, i32, Option<u32>, String) {
    match e {
        Error::Sql { failure, message } => (
            failure.sqlstate.clone(),
            failure.sqlcode,
            failure.row,
            message.as_ref().map(|m| m.id.clone()).unwrap_or_default(),
        ),
        other => panic!("expected SqlError, got {other:?}"),
    }
}

#[test]
fn query_the_assets_with_every_operation() {
    let s = start(cfg());
    let (mut c, mut q) = bob(&s);
    assert_eq!(q.attached().job().user, "BOB");
    let p = q.prepare("SELECT * FROM HLINV.ASSETS").unwrap();
    assert_eq!(p.columns.len(), 12);
    assert_eq!(p.columns[0].name, "ASSET_TAG");
    assert_eq!(p.columns[0].label.trim(), "Asset tag");
    assert!(p.parameters.is_empty());
    let o = q.execute_now(p.statement, Vec::new()).unwrap();
    let cursor = o.executed.cursor.unwrap();
    // A block of 5, then the rest.
    let first = q.fetch(cursor, 5).unwrap().wait_done();
    let first = q.fetched(cursor, first).unwrap();
    assert_eq!((first.fetched.rows.len(), first.fetched.end), (5, false));
    let rest = q.fetch(cursor, 0).unwrap().wait_done();
    let rest = q.fetched(cursor, rest).unwrap();
    assert_eq!((rest.fetched.rows.len(), rest.fetched.end), (7, true));
    assert_eq!(text(&first.fetched.rows[0][0]), "QS000101  ");
    assert_eq!(
        first.fetched.rows[0][8],
        SqlValue::Decimal {
            text: "1200.00".into()
        }
    );
    q.close_cursor(cursor).unwrap();
    q.release(p.statement).unwrap();
    assert_eq!((q.statements(), q.cursors()), (0, 0));

    // The catalog, on the shared connection, a page at a time.
    let page = c
        .list_schemas(&ListSchemas {
            pattern: "ARCH%".into(),
            start: 0,
            max: 50,
        })
        .unwrap();
    assert_eq!((page.total, page.schemas.len()), (140, 50));
    assert_eq!(c.all_schemas("%").unwrap().len(), 146);
    let t = c
        .list_tables(&ListTables {
            schema: "HLINV".into(),
            pattern: "%".into(),
            kinds: vec![TableKind::View],
            start: 0,
            max: 0,
        })
        .unwrap();
    assert_eq!(t.tables.len(), 1);
    assert_eq!(t.tables[0].name, "ASSETS_ACTIVE");
    assert_eq!(c.all_tables("HLMANY", "%", &[]).unwrap().len(), 150);
    let kinds: Vec<TableKind> = c
        .all_tables("HLINV", "%", &[])
        .unwrap()
        .iter()
        .map(|t| t.kind)
        .collect();
    for k in [
        TableKind::Table,
        TableKind::View,
        TableKind::Alias,
        TableKind::LogicalFile,
    ] {
        assert!(kinds.contains(&k), "{k:?}");
    }
    let cols = c.list_columns("HLINV", "ASSETS").unwrap();
    assert_eq!(cols.len(), 12);
    assert_eq!(cols[8].sql_type, "DECIMAL");
    assert_eq!((cols[8].length, cols[8].scale), (11, 2));
}

#[test]
fn statuses() {
    let s = start(cfg());
    let (mut c, mut q) = bob(&s);
    // A statement the database does not know: SqlError, as it reports it.
    let e = q.prepare("SELEKT * FROM HLINV.ASSETS").unwrap_err();
    assert_eq!(sql_state(&e).0, "42601");
    assert_eq!(sql_state(&e).3, "SQL0104");
    // A table that is not there.
    let e = q.prepare("SELECT * FROM HLINV.NOPE").unwrap_err();
    assert_eq!(sql_state(&e).0, "42704");
    // A table BOB may not read: NotAuthorized with SQL0551, at Prepare and
    // ListColumns; the catalog still lists it.
    let e = q.prepare("SELECT * FROM HLPAY.SALARIES").unwrap_err();
    assert_eq!(e.status(), Some(Status::NotAuthorized));
    assert_eq!(e.server_message().unwrap().id, "SQL0551");
    let e = c.list_columns("HLPAY", "SALARIES").unwrap_err();
    assert_eq!(e.status(), Some(Status::NotAuthorized));
    assert_eq!(e.server_message().unwrap().id, "SQL0551");
    assert_eq!(c.all_tables("HLPAY", "%", &[]).unwrap().len(), 1);
    // ANN may.
    let mut ann = signed_on(&s, "ANN", "ANNPW01");
    let mut qa = ann.sql_connection().unwrap();
    assert_eq!(query(&mut qa, "SELECT * FROM HLPAY.SALARIES").len(), 2);
    // A cursor that is not open: NotFound from the server.
    let p = q
        .attached_mut()
        .request_here(
            Op::Fetch,
            &Fetch {
                cursor: 99,
                max_rows: 1,
            },
        )
        .unwrap()
        .wait(Duration::from_secs(5))
        .unwrap();
    assert_eq!(p.status, Status::NotFound);
    let p = q
        .attached_mut()
        .request_here(Op::CloseCursor, &CursorRef { cursor: 99 })
        .unwrap()
        .wait(Duration::from_secs(5))
        .unwrap();
    assert_eq!(p.status, Status::NotFound);
    // SetOptions while changes are uncommitted: InUse with SQL0752, and the
    // options asked for stay as they were.
    q.set_options(SetOptions {
        commit: CommitLevel::ReadCommitted,
        ..SetOptions::default()
    })
    .unwrap();
    let ins = q.prepare("INSERT INTO HLTEST.JRN VALUES(?, ?)").unwrap();
    q.execute_now(ins.statement, vec![vec![int(1), name("one")]])
        .unwrap();
    assert!(q.uncommitted());
    let e = q
        .set_options(SetOptions {
            commit: CommitLevel::None,
            ..SetOptions::default()
        })
        .unwrap_err();
    assert_eq!(e.status(), Some(Status::InUse));
    assert_eq!(e.server_message().unwrap().id, "SQL0752");
    assert_eq!(q.options().commit, CommitLevel::ReadCommitted);
    q.commit().unwrap();
    assert!(!q.uncommitted());
    q.set_options(SetOptions {
        commit: CommitLevel::None,
        naming: Naming::System,
        default_schema: None,
    })
    .unwrap();
    // System naming: library/file.
    assert_eq!(query(&mut q, "SELECT * FROM HLINV/ASSETS").len(), 12);
    // Commit and Rollback without commitment control are Ok.
    q.commit().unwrap();
    q.rollback().unwrap();
}

#[test]
fn commit_rollback_and_cursors_that_survive_them() {
    let s = start(cfg());
    let (_c, mut q) = bob(&s);
    q.set_options(SetOptions {
        commit: CommitLevel::ReadCommitted,
        naming: Naming::Sql,
        default_schema: Some("HLTEST".into()),
    })
    .unwrap();
    let ins = q.prepare("INSERT INTO JRN VALUES(?, ?)").unwrap();
    q.execute_now(ins.statement, vec![vec![int(1), name("kept")]])
        .unwrap();
    q.commit().unwrap();
    q.execute_now(ins.statement, vec![vec![int(2), name("undone")]])
        .unwrap();
    q.rollback().unwrap();
    assert_eq!(query(&mut q, "SELECT * FROM JRN").len(), 1);

    // An open cursor stays open across Commit and Rollback.
    let sel = q.prepare("SELECT * FROM HLINV.ASSETS").unwrap();
    let cursor = q
        .execute_now(sel.statement, Vec::new())
        .unwrap()
        .executed
        .cursor
        .unwrap();
    let r = q.fetch(cursor, 4).unwrap().wait_done();
    assert_eq!(q.fetched(cursor, r).unwrap().fetched.rows.len(), 4);
    q.commit().unwrap();
    q.rollback().unwrap();
    let r = q.fetch(cursor, 0).unwrap().wait_done();
    let rest = q.fetched(cursor, r).unwrap();
    assert_eq!((rest.fetched.rows.len(), rest.fetched.end), (8, true));
}

#[test]
fn a_failing_row_and_an_unjournaled_table() {
    let s = start(cfg());
    let (_c, mut q) = bob(&s);
    q.set_options(SetOptions {
        commit: CommitLevel::ReadCommitted,
        ..SetOptions::default()
    })
    .unwrap();
    let ins = q.prepare("INSERT INTO HLTEST.JRN VALUES(?, ?)").unwrap();
    // The third row's key is the first's: the request stops there, its row
    // the 0-based index in this request, the two before it changed.
    let e = q
        .execute_now(
            ins.statement,
            vec![
                vec![int(10), name("a")],
                vec![int(11), name("b")],
                vec![int(10), name("again")],
                vec![int(12), name("c")],
            ],
        )
        .unwrap_err();
    assert_eq!(
        sql_state(&e),
        ("23505".into(), -803, Some(2), "SQL0803".into())
    );
    assert!(q.uncommitted());
    assert_eq!(query(&mut q, "SELECT * FROM HLTEST.JRN").len(), 2);
    q.rollback().unwrap();
    assert_eq!(query(&mut q, "SELECT * FROM HLTEST.JRN").len(), 0);

    // Not journaled: SQL7008 (55019) at the first Execute that changes data,
    // not at Prepare.
    let ins = q.prepare("INSERT INTO HLTEST.NOJRN VALUES(?, ?)").unwrap();
    let e = q
        .execute_now(ins.statement, vec![vec![int(1), name("x")]])
        .unwrap_err();
    let (state, code, _, id) = sql_state(&e);
    assert_eq!(
        (state.as_str(), code, id.as_str()),
        ("55019", -7008, "SQL7008")
    );
    q.rollback().unwrap();
    q.set_options(SetOptions::default()).unwrap();
    q.execute_now(ins.statement, vec![vec![int(1), name("x")]])
        .unwrap();
    assert_eq!(query(&mut q, "SELECT * FROM HLTEST.NOJRN").len(), 1);
}

#[test]
fn uncommitted_changes_roll_back_when_the_connection_closes() {
    let s = start(cfg());
    let (mut c, mut q) = bob(&s);
    q.set_options(SetOptions {
        commit: CommitLevel::ReadCommitted,
        ..SetOptions::default()
    })
    .unwrap();
    let ins = q.prepare("INSERT INTO HLTEST.JRN VALUES(?, ?)").unwrap();
    q.execute_now(ins.statement, vec![vec![int(5), name("gone")]])
        .unwrap();
    q.close();
    until("the close", || s.ended().len() == 1);
    let mut q2 = c.sql_connection().unwrap();
    assert!(query(&mut q2, "SELECT * FROM HLTEST.JRN").is_empty());
}

#[test]
fn every_type() {
    let s = start(cfg());
    let (_c, mut q) = bob(&s);
    let p = q.prepare("SELECT * FROM HLTEST.ALLTYPES").unwrap();
    let col = |n: &str| p.columns.iter().position(|c| c.name == n).unwrap();
    assert_eq!(p.columns[col("C_BITS")].ccsid, 65535);
    assert_eq!(p.columns[col("C_TS12")].scale, 12);
    assert_eq!(p.columns[col("C_TS")].scale, 6);
    let rows = query(&mut q, "SELECT * FROM HLTEST.ALLTYPES");
    assert_eq!(rows.len(), 7);
    let decfloats: Vec<&str> = rows[..6]
        .iter()
        .map(|r| text(&r[col("C_DECFLOAT")]))
        .collect();
    assert_eq!(
        decfloats,
        [
            "1234.5678",
            "1.5E-30",
            "Infinity",
            "-Infinity",
            "NaN",
            "sNaN"
        ]
    );
    assert_eq!(
        rows[0][col("C_BITS")],
        SqlValue::Binary {
            data: vec![0x00, 0x01, 0xFE, 0xFF, 0x40, 0x40, 0x40, 0x40]
        }
    );
    match &rows[0][col("C_TS12")] {
        // .123456789012: ms .123, then 456 789 012 picoseconds past it.
        SqlValue::Timestamp { picos, .. } => assert_eq!(*picos, 456_789_012),
        other => panic!("{other:?}"),
    }
    match &rows[1][col("C_TS12")] {
        // 1970-01-01-00.00.00.000000000001: one picosecond past ms 0.
        SqlValue::Timestamp { ms, picos } => assert_eq!((*ms, *picos), (0, 1)),
        other => panic!("{other:?}"),
    }
    assert_eq!(rows[0][col("C_BOOLEAN")], SqlValue::Boolean { value: true });
    assert_eq!(
        rows[1][col("C_BOOLEAN")],
        SqlValue::Boolean { value: false }
    );
    assert_eq!(text(&rows[0][col("C_VARCHAR")]), "Grüße");
    assert_eq!(text(&rows[2][col("C_CLOB")]), "line\nbreak");
    assert!(matches!(rows[0][col("C_BIGINT")], SqlValue::Integer { .. }));
    assert!(matches!(rows[0][col("C_DATE")], SqlValue::Date { .. }));
    assert!(matches!(rows[0][col("C_TIME")], SqlValue::Time { .. }));
    assert!(matches!(rows[0][col("C_DOUBLE")], SqlValue::Float { .. }));
    // The seventh row is null but for its ID.
    assert!(rows[6][1..].iter().all(|v| *v == SqlValue::Null));
}

#[test]
fn a_result_over_a_mebibyte_streams_in_blocks() {
    let s = start(cfg());
    let (_c, mut q) = bob(&s);
    let p = q.prepare("SELECT * FROM HLTEST.BIG").unwrap();
    let cursor = q
        .execute_now(p.statement, Vec::new())
        .unwrap()
        .executed
        .cursor
        .unwrap();
    let mut blocks = Vec::new();
    let end = q
        .fetch_into(
            cursor,
            Some(1200),
            &mut |b| {
                blocks.push(b.len());
                Ok(())
            },
            &AtomicBool::new(false),
        )
        .unwrap();
    assert_eq!(end, FetchEnd::Limit { rows: 1200 });
    assert_eq!(blocks, [500, 500, 200]);
    // The rest in one Fetch with no limit: over 1 MiB, joined from More.
    let r = q.fetch(cursor, 0).unwrap().wait_done();
    let rest = q.fetched(cursor, r).unwrap();
    assert_eq!((rest.fetched.rows.len(), rest.fetched.end), (1800, true));
}

#[test]
fn cancel_bodies() {
    let s = start(cfg());
    let (_c, mut q) = bob(&s);
    // A started Fetch: the rows so far, end 0, the cursor closed.
    let p = q.prepare("SELECT * FROM HLTEST.SLOW").unwrap();
    let cursor = q
        .execute_now(p.statement, Vec::new())
        .unwrap()
        .executed
        .cursor
        .unwrap();
    let cancel = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&cancel);
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(300));
        flag.store(true, Ordering::SeqCst);
    });
    let mut got = 0usize;
    let end = q
        .fetch_into(
            cursor,
            None,
            &mut |b| {
                got += b.len();
                Ok(())
            },
            &cancel,
        )
        .unwrap();
    match &end {
        FetchEnd::Cancelled { rows, message } => {
            assert!(*rows > 0 && *rows < 300, "{rows}");
            assert_eq!(message.as_ref().unwrap().id, "SQL0952");
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(got as u64, end.rows());
    assert_eq!(q.cursors(), 0, "the cursor is closed");

    // A started Execute: Cancelled with its updated count, no cursor.
    let ins = q
        .prepare("INSERT INTO HLTEST.SLOWLOG VALUES(?, ?)")
        .unwrap();
    let rows: Vec<Vec<SqlValue>> = (1..=300).map(|n| vec![int(n), name("r")]).collect();
    let pending = q.execute(ins.statement, rows).unwrap();
    std::thread::sleep(Duration::from_millis(200));
    pending.cancel();
    let o = q.executed(ins.statement, pending.wait_done()).unwrap();
    assert!(o.cancelled);
    assert!(o.executed.updated > 0 && o.executed.updated < 300, "{o:?}");
    assert_eq!(o.executed.cursor, None);
    assert_eq!(o.message.unwrap().id, "SQL0952");

    // Cancelled before it started (waiting in the lane behind a slow
    // fetch): Cancelled with an empty body, nothing changed.
    let sel = q.prepare("SELECT * FROM HLTEST.SLOW").unwrap();
    let cursor = q
        .execute_now(sel.statement, Vec::new())
        .unwrap()
        .executed
        .cursor
        .unwrap();
    let slow = q.fetch(cursor, 0).unwrap();
    let queued = q
        .execute(ins.statement, vec![vec![int(1000), name("never")]])
        .unwrap();
    queued.cancel();
    let o = q.executed(ins.statement, queued.wait_done()).unwrap();
    assert!(o.cancelled);
    assert_eq!(o.executed.updated, 0);
    slow.cancel();
    let _ = q.fetched(cursor, slow.wait_done());
    q.rollback().unwrap();
    let mut seen = query(&mut q, "SELECT * FROM HLTEST.SLOWLOG WHERE ID = 1000");
    assert!(seen.pop().is_none());
}

#[test]
fn attach_must_come_first_and_is_not_retried_on_its_connection() {
    let s = start(cfg());
    let c0 = Connection::open(&s.target()).unwrap();
    let reply: SignOnReply = c0
        .call(
            Op::SignOn,
            &SignOn {
                user: "BOB".into(),
                password: Secret::from("SECRET1"),
            },
        )
        .unwrap();
    let token = reply.token.clone();
    // A Ping first: Attach is then InvalidRequest, and stays so on that
    // connection.
    let c1 = Connection::open(&s.target()).unwrap();
    let _: qap_client::proto::ops::connection::PingReply = c1.call(Op::Ping, &()).unwrap();
    for _ in 0..2 {
        let r = c1
            .request(
                Op::Attach,
                &Attach {
                    token: token.clone(),
                },
            )
            .unwrap()
            .wait(Duration::from_secs(5))
            .unwrap();
        assert_eq!(r.status, Status::InvalidRequest);
    }
    // A new connection with Attach first works.
    let c2 = Connection::open(&s.target()).unwrap();
    let a: SignOnReply = c2.call(Op::Attach, &Attach { token }).unwrap();
    assert_eq!(a.profile, "BOB");
    // JobInfo is answered before sign-on too.
    let c3 = Connection::open(&s.target()).unwrap();
    let j: qap_client::proto::ops::connection::JobInfoReply = c3.call(Op::JobInfo, &()).unwrap();
    assert!(!j.job.number.is_empty());
}

#[test]
fn the_session_has_sixteen_connections() {
    let s = start(cfg());
    let mut c = signed_on(&s, "BOB", "SECRET1");
    let held: Vec<SqlConnection> = (0..15).map(|_| c.sql_connection().unwrap()).collect();
    let e = c.sql_connection().unwrap_err();
    assert_eq!(e.status(), Some(Status::Busy));
    assert_eq!(e.server_message().unwrap().id, "TCP1A59");
    assert_eq!(held.len(), 15);
}

#[test]
fn a_sign_off_elsewhere_is_session_ended_in_the_window() {
    let s = start(cfg());
    let (mut c, mut q) = bob(&s);
    let p = q.prepare("SELECT * FROM HLINV.ASSETS").unwrap();
    c.sign_off().unwrap();
    until("SessionEnded", || q.attached_mut().poll().is_some());
    let why = q.attached().ended().unwrap();
    assert_eq!(why.message().unwrap().id, "TCP1A58");
    // Nothing more goes on it, and nothing attaches to the ended session.
    let e = q.execute_now(p.statement, Vec::new()).unwrap_err();
    assert!(
        matches!(e, Error::Closed(_)) || e.status() == Some(Status::SessionExpired),
        "{e:?}"
    );
    let e = q.prepare("SELECT * FROM HLINV.ASSETS").unwrap_err();
    assert_eq!(e.status(), Some(Status::SessionExpired));
    assert_eq!(q.attached().attaches(), 1);
}

#[test]
fn a_drop_mid_fetch_loses_the_cursor_and_the_next_run_attaches_again() {
    let s = start(cfg());
    let (_c, mut q) = bob(&s);
    q.set_options(SetOptions {
        commit: CommitLevel::None,
        naming: Naming::Sql,
        default_schema: Some("HLTEST".into()),
    })
    .unwrap();
    let job: u32 = q.attached().job().job.number.parse().unwrap();
    let p = q.prepare("SELECT * FROM SLOW").unwrap();
    let cursor = q
        .execute_now(p.statement, Vec::new())
        .unwrap()
        .executed
        .cursor
        .unwrap();
    let sys = Arc::clone(&s.c);
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(200));
        sys.drop_connection(job);
    });
    let mut got = 0usize;
    let e = q
        .fetch_into(
            cursor,
            None,
            &mut |b| {
                got += b.len();
                Ok(())
            },
            &AtomicBool::new(false),
        )
        .unwrap_err();
    assert!(matches!(e, Error::Closed(_)), "{e:?}");
    // Said once; the statement and cursor are gone, never sent again.
    assert!(q.lost());
    assert!(!q.lost());
    assert_eq!((q.statements(), q.cursors()), (0, 0));
    assert!(matches!(q.fetch(cursor, 1), Err(Error::Closed(_))));
    // The next run attaches a new connection with the options set again.
    assert_eq!(query(&mut q, "SELECT * FROM JRN").len(), 0);
    assert_eq!(q.attached().attaches(), 2);
}

/// While a slow query fetches on one window's connection, under
/// `commit`: Run Command, Printer Output's list and another window's query
/// on the same session each answer at once, and the slow fetch is still
/// going when they have. No statement in one job holds up another.
fn independent_of_a_slow_query(commit: CommitLevel) {
    use qap_client::proto::ops::spool::ListSpooledFiles;
    let s = start(cfg());
    let (mut c, mut slow) = bob(&s);
    slow.set_options(SetOptions {
        commit,
        ..SetOptions::default()
    })
    .unwrap();
    let mut other = c.sql_connection().unwrap();
    let p = slow.prepare("SELECT * FROM HLTEST.SLOW").unwrap();
    let cursor = slow
        .execute_now(p.statement, Vec::new())
        .unwrap()
        .executed
        .cursor
        .unwrap();
    let fetching = std::thread::spawn(move || {
        let mut got = 0usize;
        let end = slow
            .fetch_into(
                cursor,
                None,
                &mut |b| {
                    got += b.len();
                    Ok(())
                },
                &AtomicBool::new(false),
            )
            .unwrap();
        (got, end)
    });
    // The fetch is under way: 300 rows at 10 ms each.
    std::thread::sleep(Duration::from_millis(200));
    let quick = Duration::from_millis(1000);
    let at = std::time::Instant::now();
    let o = qap_client::CommandOutcome::from_reply(
        c.run_command("CRTLIB LIB(QATESTLIB)")
            .unwrap()
            .wait_done()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(o.status, Status::Ok);
    assert!(
        at.elapsed() < quick,
        "Run Command waited {:?}",
        at.elapsed()
    );
    let at = std::time::Instant::now();
    let files = c
        .list_spooled_files(&ListSpooledFiles {
            user: "*CURRENT".into(),
            ..ListSpooledFiles::default()
        })
        .unwrap();
    assert!(files.total > 0);
    assert!(
        at.elapsed() < quick,
        "Printer Output waited {:?}",
        at.elapsed()
    );
    let at = std::time::Instant::now();
    assert_eq!(query(&mut other, "SELECT * FROM HLINV.ASSETS").len(), 12);
    assert!(at.elapsed() < quick, "Run SQL waited {:?}", at.elapsed());
    assert!(!fetching.is_finished(), "the slow query is still fetching");
    let (got, end) = fetching.join().unwrap();
    assert_eq!((got, end.rows()), (300, 300));
}

#[test]
fn a_slow_select_holds_up_no_other_connection() {
    independent_of_a_slow_query(CommitLevel::None);
}

#[test]
fn a_slow_select_under_repeatable_read_holds_up_no_other_connection() {
    independent_of_a_slow_query(CommitLevel::RepeatableRead);
}

/// Row 7's `C_DECFLOAT` set to `v` by a parameter, then read back.
fn decfloat_through(q: &mut SqlConnection, v: &str) -> String {
    let set = q
        .prepare("UPDATE HLTEST.ALLTYPES SET C_DECFLOAT = ? WHERE ID = 7")
        .unwrap();
    let o = q
        .execute_now(
            set.statement,
            vec![vec![SqlValue::Decimal { text: v.into() }]],
        )
        .unwrap();
    assert_eq!(o.executed.updated, 1, "{v}");
    q.release(set.statement).unwrap();
    let rows = query(q, "SELECT C_DECFLOAT FROM HLTEST.ALLTYPES WHERE ID = 7");
    text(&rows[0][0]).to_owned()
}

#[test]
fn decfloat_comes_back_as_the_engines_text_exactly() {
    let s = start(cfg());
    let (_c, mut q) = bob(&s);
    // The engine's decimal floating-point string: coefficient and
    // exponent kept, compared here as text, never by value.
    for (sent, back) in [
        ("1.50", "1.50"),
        ("1E+2", "1E+2"),
        ("1.00E+2", "100"),
        ("1e2", "1E+2"),
        ("+1.5", "1.5"),
        ("0.0000001", "1E-7"),
        ("-0", "-0"),
        ("0E+3", "0E+3"),
        ("-12.50", "-12.50"),
        ("1.5E-30", "1.5E-30"),
        ("Infinity", "Infinity"),
        ("sNaN", "sNaN"),
    ] {
        assert_eq!(decfloat_through(&mut q, sent), back, "{sent}");
    }
}

#[test]
fn decfloat_equals_by_value_in_a_where_clause() {
    let s = start(cfg());
    let (_c, mut q) = bob(&s);
    let find = |q: &mut SqlConnection, v: &str| -> Vec<i64> {
        let p = q
            .prepare("SELECT ID FROM HLTEST.ALLTYPES WHERE C_DECFLOAT = ?")
            .unwrap();
        let o = q
            .execute_now(
                p.statement,
                vec![vec![SqlValue::Decimal { text: v.into() }]],
            )
            .unwrap();
        let cursor = o.executed.cursor.unwrap();
        let mut ids = Vec::new();
        q.fetch_into(
            cursor,
            None,
            &mut |b| {
                ids.extend(b.into_iter().map(|r| match r[0] {
                    SqlValue::Integer { value } => value,
                    ref other => panic!("{other:?}"),
                }));
                Ok(())
            },
            &AtomicBool::new(false),
        )
        .unwrap();
        q.close_cursor(cursor).unwrap();
        q.release(p.statement).unwrap();
        ids
    };
    assert_eq!(decfloat_through(&mut q, "1.50"), "1.50");
    assert_eq!(find(&mut q, "1.5"), [7]);
    assert_eq!(find(&mut q, "15E-1"), [7]);
    assert_eq!(decfloat_through(&mut q, "-0"), "-0");
    assert_eq!(find(&mut q, "0"), [7]);
    assert_eq!(find(&mut q, "1234.56780"), [1]);
}
