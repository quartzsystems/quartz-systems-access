// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! What Printer Output shows, without its widgets: the filter and the
//! request it makes, the list's rows and status words, sorting a page,
//! the properties of a file, and the summary of actions on several files.

use std::cmp::Ordering;

use qap_client::proto::ServerMessage;
use qap_client::proto::ops::spool::{
    DeviceType, ListSpooledFiles, OutputQueue, SpoolStatus, SpooledFile, SpooledFileId,
};
use qap_client::proto::types::{JobId, ObjectName};
use qap_client::spool::Action;
use qsa_ui::listtable::Cell;
use qsa_ui::statusbar::Status;

/// The list's columns: title and width.
pub const COLUMNS: [(&str, i32); 10] = [
    ("File", 92),
    ("User", 80),
    ("User data", 84),
    ("Status", 70),
    ("Pages", 50),
    ("Copies", 50),
    ("Form type", 72),
    ("Output queue", 120),
    ("Created", 112),
    ("Job", 180),
];

/// Every status, as the Status filter offers them.
pub const STATUSES: [SpoolStatus; 9] = SpoolStatus::ALL;

/// The word a status shows as.
pub fn status_word(s: SpoolStatus) -> &'static str {
    match s {
        SpoolStatus::Ready => "Ready",
        SpoolStatus::Held => "Held",
        SpoolStatus::Open => "Open",
        SpoolStatus::Closed => "Closed",
        SpoolStatus::Writing => "Writing",
        SpoolStatus::Printing => "Printing",
        SpoolStatus::Message => "Message",
        SpoolStatus::Pending => "Pending",
        SpoolStatus::Saved => "Saved",
    }
}

/// The status a word names, in any case.
pub fn status_named(word: &str) -> Option<SpoolStatus> {
    STATUSES
        .into_iter()
        .find(|s| status_word(*s).eq_ignore_ascii_case(word.trim()))
}

/// A status word's colour: green while a file waits or prints, red while
/// its writer waits for a reply, black otherwise.
pub fn status_colour(s: SpoolStatus) -> Status {
    match s {
        SpoolStatus::Ready | SpoolStatus::Printing => Status::Active,
        SpoolStatus::Message => Status::Error,
        _ => Status::Neutral,
    }
}

/// A device type's name.
pub fn device_word(d: DeviceType) -> &'static str {
    match d {
        DeviceType::Scs => "SCS",
        DeviceType::Afp => "AFP",
        DeviceType::Ascii => "ASCII",
        DeviceType::Line => "Line",
    }
}

/// `QGPL/QPRINT`.
pub fn object_text(o: &ObjectName) -> String {
    if o.library.is_empty() {
        o.name.clone()
    } else {
        format!("{}/{}", o.library, o.name)
    }
}

/// `QGPL/QPRINT` or `QPRINT` (`*LIBL`) as an object name; `None` for an
/// empty one.
pub fn object_named(text: &str) -> Option<ObjectName> {
    let t = text.trim().to_ascii_uppercase();
    if t.is_empty() {
        return None;
    }
    let (library, name) = t.split_once('/').unwrap_or(("*LIBL", &t));
    let (library, name) = (library.trim(), name.trim());
    (!name.is_empty()).then(|| ObjectName {
        library: if library.is_empty() { "*LIBL" } else { library }.to_owned(),
        name: name.to_owned(),
    })
}

/// A job as the system writes it: `001234/BOB/QPADEV0001`.
pub fn job_text(j: &JobId) -> String {
    format!("{}/{}/{}", j.number, j.user, j.name)
}

/// A file as messages name it: its name.
pub fn file_text(id: &SpooledFileId) -> String {
    id.file.clone()
}

/// What the filter row asks for. Empty fields and `*ALL` are no filter.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Filter {
    /// `*CURRENT`, `*ALL` or a profile.
    pub user: String,
    /// `*ALL` or `LIBRARY/QUEUE`.
    pub outq: String,
    pub status: Option<SpoolStatus>,
    pub form_type: String,
    pub user_data: String,
}

impl Default for Filter {
    fn default() -> Filter {
        Filter {
            user: "*CURRENT".into(),
            outq: "*ALL".into(),
            status: None,
            form_type: String::new(),
            user_data: String::new(),
        }
    }
}

/// `*ALL` or empty: no filter.
fn any(v: &str) -> bool {
    let v = v.trim();
    v.is_empty() || v.eq_ignore_ascii_case("*ALL")
}

impl Filter {
    /// The filter on one queue (from Output Queues).
    pub fn on_queue(outq: &ObjectName) -> Filter {
        Filter {
            user: "*ALL".into(),
            outq: object_text(outq),
            ..Filter::default()
        }
    }

    /// The request for entries `start` to `start + max - 1`.
    pub fn request(&self, start: u32, max: u32) -> ListSpooledFiles {
        let user = self.user.trim().to_ascii_uppercase();
        let opt = |v: &str| (!any(v)).then(|| v.trim().to_owned());
        ListSpooledFiles {
            user: if user.is_empty() {
                "*CURRENT".into()
            } else {
                user
            },
            outq: if any(&self.outq) {
                None
            } else {
                object_named(&self.outq)
            },
            status: self.status,
            form_type: opt(&self.form_type).map(|v| v.to_ascii_uppercase()),
            user_data: opt(&self.user_data),
            start,
            max,
        }
    }

    /// What the list's title says it shows: `on QGPL/QPRINT` and the like.
    pub fn describe(&self) -> String {
        if any(&self.outq) {
            String::new()
        } else {
            object_text(&object_named(&self.outq).unwrap_or_default())
        }
    }
}

/// One row of the list. `created` formats a creation time.
pub fn row(f: &SpooledFile, created: &dyn Fn(i64) -> String) -> Vec<Cell> {
    vec![
        f.id.file.as_str().into(),
        f.user.as_str().into(),
        f.user_data.as_str().into(),
        Cell {
            text: status_word(f.status).to_owned(),
            status: status_colour(f.status),
        },
        f.pages.to_string().into(),
        f.copies.to_string().into(),
        f.form_type.as_str().into(),
        object_text(&f.outq).into(),
        created(f.created).into(),
        job_text(&f.id.job).into(),
    ]
}

/// Creation time in this computer's time zone: `2026-11-02 09:14`.
pub fn local_time(ms: i64) -> String {
    crate::localtime::format(ms.div_euclid(1000))
}

/// Creation time in UTC, for screenshots and tests: `2026-11-02 09:14`.
pub fn utc_time(ms: i64) -> String {
    let secs = ms.div_euclid(1000);
    let (days, rest) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    // Days since 1970-01-01 to a civil date.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}-{m:02}-{d:02} {:02}:{:02}",
        rest / 3600,
        rest % 3600 / 60
    )
}

/// Sorts a page in place by column `col` of [`COLUMNS`]; numbers and times
/// by value, words in any case. Equal rows keep their order.
pub fn sort_page(files: &mut [SpooledFile], col: usize, ascending: bool) {
    let text = |a: &str, b: &str| a.to_ascii_uppercase().cmp(&b.to_ascii_uppercase());
    let key = |a: &SpooledFile, b: &SpooledFile| -> Ordering {
        match col {
            0 => text(&a.id.file, &b.id.file).then(a.id.number.cmp(&b.id.number)),
            1 => text(&a.user, &b.user),
            2 => text(&a.user_data, &b.user_data),
            3 => text(status_word(a.status), status_word(b.status)),
            4 => a.pages.cmp(&b.pages),
            5 => a.copies.cmp(&b.copies),
            6 => text(&a.form_type, &b.form_type),
            7 => text(&object_text(&a.outq), &object_text(&b.outq)),
            8 => a.created.cmp(&b.created),
            _ => text(&job_text(&a.id.job), &job_text(&b.id.job)),
        }
    };
    files.sort_by(|a, b| {
        let o = key(a, b);
        if ascending { o } else { o.reverse() }
    });
}

/// The status bar's line for a page: `QAD6001` and, when sorted, that the
/// sort is of the page shown.
pub fn list_line(total: u32, system: &str, sorted_by: Option<&str>) -> String {
    let m = qsa_msg::message(
        qsa_msg::id::SPOOLED_FILES_LISTED,
        &[&total.to_string(), system],
    );
    match sorted_by {
        Some(c) => format!("{m} Sorted by {c} within this page only."),
        None => m.to_string(),
    }
}

/// The paging line: `Files 1-25 of 60, page 1 of 3`.
pub fn paging_line(p: &qap_client::spool::Paging, on_page: usize) -> String {
    let (page, pages) = p.page();
    match p.shown(on_page) {
        Some((a, b)) => format!("Files {a}-{b} of {}, page {page} of {pages}", p.total),
        None => "No files".to_owned(),
    }
}

/// Every field of a file, labelled, for Properties.
pub fn properties(f: &SpooledFile, created: &dyn Fn(i64) -> String) -> Vec<(&'static str, String)> {
    vec![
        ("File", f.id.file.clone()),
        ("File number", f.id.number.to_string()),
        ("Job", f.id.job.name.clone()),
        ("Job user", f.id.job.user.clone()),
        ("Job number", f.id.job.number.clone()),
        ("Owner", f.user.clone()),
        ("Status", status_word(f.status).to_owned()),
        ("Output queue", object_text(&f.outq)),
        ("User data", f.user_data.clone()),
        ("Form type", f.form_type.clone()),
        ("Device type", device_word(f.device_type).to_owned()),
        ("Pages", f.pages.to_string()),
        ("Copies", f.copies.to_string()),
        ("Size", size_text(f.size)),
        ("Created", created(f.created)),
    ]
}

/// `12 KB` and the like, with the bytes.
pub fn size_text(bytes: u64) -> String {
    match bytes {
        0..1024 => format!("{bytes} bytes"),
        _ => format!("{} KB ({bytes} bytes)", bytes.div_ceil(1024)),
    }
}

/// One line per file, for the delete confirmation: name, number, job.
pub fn file_lines(files: &[SpooledFile]) -> Vec<String> {
    files
        .iter()
        .map(|f| {
            format!(
                "{}   number {}   job {}",
                f.id.file,
                f.id.number,
                job_text(&f.id.job)
            )
        })
        .collect()
}

/// An output queue in pickers: `QGPL/PRT01`.
pub fn queue_choices(queues: &[OutputQueue]) -> Vec<String> {
    queues.iter().map(|q| object_text(&q.outq)).collect()
}

/// What an action is called: in menus, the summary's title and its rows.
pub fn action_name(a: &Action) -> &'static str {
    match a {
        Action::Hold { .. } => "Hold",
        Action::Release(_) => "Release",
        Action::Delete(_) => "Delete",
        Action::Move { .. } => "Move",
    }
}

/// The message for an action done on one file.
pub fn done_message(a: &Action) -> qsa_msg::Message {
    use qsa_msg::id;
    let file = file_text(a.id());
    match a {
        Action::Hold { .. } => qsa_msg::message(id::SPOOLED_FILE_HELD, &[&file]),
        Action::Release(_) => qsa_msg::message(id::SPOOLED_FILE_RELEASED, &[&file]),
        Action::Delete(_) => qsa_msg::message(id::SPOOLED_FILE_DELETED, &[&file]),
        Action::Move { outq, .. } => {
            qsa_msg::message(id::SPOOLED_FILE_MOVED, &[&file, &object_text(outq)])
        }
    }
}

/// How an action on one file ended: done, or refused with the system's
/// message as it sent it (or this program's line when there was none).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    Done,
    Refused {
        server: Option<ServerMessage>,
        line: String,
    },
}

/// One file's action and how it ended.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Done {
    pub file: SpooledFile,
    pub action: Action,
    pub outcome: Outcome,
}

/// The summary's heading: `QAD6006` when any failed, else how many were
/// done.
pub fn summary_heading(done: &[Done]) -> (String, Status) {
    let failed = done.iter().filter(|d| d.outcome != Outcome::Done).count();
    if failed > 0 {
        let m = qsa_msg::message(
            qsa_msg::id::SPOOL_ACTIONS_FAILED,
            &[&failed.to_string(), &done.len().to_string()],
        );
        (m.to_string(), Status::Error)
    } else {
        (
            format!("All {} actions were done.", done.len()),
            Status::Neutral,
        )
    }
}

/// The summary's rows: file, job, and the result (the system's message ID
/// in red and its text as sent, for a refusal).
pub fn summary_rows(done: &[Done]) -> Vec<Vec<Cell>> {
    done.iter()
        .map(|d| {
            let (id, text) = match &d.outcome {
                Outcome::Done => (Cell::from(""), done_message(&d.action).text),
                Outcome::Refused {
                    server: Some(m), ..
                } => (
                    Cell {
                        text: m.id.clone(),
                        status: Status::Error,
                    },
                    m.text.clone(),
                ),
                Outcome::Refused { server: None, line } => (
                    Cell {
                        text: "Failed".into(),
                        status: Status::Error,
                    },
                    line.clone(),
                ),
            };
            vec![
                d.file.id.file.as_str().into(),
                job_text(&d.file.id.job).into(),
                id,
                text.into(),
            ]
        })
        .collect()
}

/// The summary's columns.
pub const SUMMARY_COLUMNS: [(&str, i32); 4] =
    [("File", 90), ("Job", 170), ("Message", 70), ("Result", 300)];

/// The page model's options for spooled files, as a 5250 printer session
/// with its defaults lays them out: the system's CCSID when this program
/// has its code page (else 37), the device's default font, `paper` when the
/// data sets no page size.
pub fn scs_options(system_ccsid: u16, paper: qsa_config::qsp::Paper) -> scs::Options {
    let ccsid = if tn5250::codepage::by_ccsid(system_ccsid).is_some() {
        system_ccsid
    } else {
        37
    };
    scs::Options {
        ccsid,
        paper: match paper {
            qsa_config::qsp::Paper::Letter => scs::Paper::Letter,
            qsa_config::qsp::Paper::A4 => scs::Paper::A4,
            qsa_config::qsp::Paper::Legal => scs::Paper::Legal,
        },
        font: scs::font::DEFAULT_FONT,
    }
}

/// A file name for saving a spooled file: `QSYSPRT-2-001234.pdf`.
pub fn save_name(id: &SpooledFileId, ext: &str) -> String {
    crate::files::file_name_for(&format!("{}-{}-{}", id.file, id.number, id.job.number), ext)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(name: &str, status: SpoolStatus, pages: u32, created: i64) -> SpooledFile {
        SpooledFile {
            id: SpooledFileId {
                job: JobId {
                    name: "QPADEV0001".into(),
                    user: "BOB".into(),
                    number: "001234".into(),
                },
                file: name.into(),
                number: pages,
            },
            status,
            outq: ObjectName {
                library: "QGPL".into(),
                name: "QPRINT".into(),
            },
            user: "BOB".into(),
            user_data: name.into(),
            form_type: "*STD".into(),
            device_type: DeviceType::Scs,
            pages,
            copies: 1,
            size: 2048,
            created,
        }
    }

    #[test]
    fn the_filter_and_its_request() {
        let r = Filter::default().request(25, 25);
        assert_eq!(r.user, "*CURRENT");
        assert_eq!(
            (r.outq, r.status, r.form_type, r.user_data),
            (None, None, None, None)
        );
        assert_eq!((r.start, r.max), (25, 25));
        let f = Filter {
            user: "ann".into(),
            outq: "qgpl/prt01".into(),
            status: Some(SpoolStatus::Held),
            form_type: "*std".into(),
            user_data: "Payroll".into(),
        };
        let r = f.request(0, 50);
        assert_eq!(r.user, "ANN");
        assert_eq!(
            r.outq.as_ref().map(object_text).as_deref(),
            Some("QGPL/PRT01")
        );
        assert_eq!(r.status, Some(SpoolStatus::Held));
        assert_eq!(r.form_type.as_deref(), Some("*STD"));
        assert_eq!(
            r.user_data.as_deref(),
            Some("Payroll"),
            "user data as typed"
        );
        assert_eq!(
            object_named("PRT01").map(|o| object_text(&o)).as_deref(),
            Some("*LIBL/PRT01")
        );
        assert_eq!(object_named(" "), None);
        let q = Filter::on_queue(&ObjectName {
            library: "QGPL".into(),
            name: "QPRINT".into(),
        });
        assert_eq!(
            (q.user.as_str(), q.describe().as_str()),
            ("*ALL", "QGPL/QPRINT")
        );
    }

    #[test]
    fn rows_and_status_words() {
        let r = row(
            &file("QSYSPRT", SpoolStatus::Ready, 30, 1_793_000_060_000),
            &utc_time,
        );
        let text: Vec<&str> = r.iter().map(|c| c.text.as_str()).collect();
        assert_eq!(
            text,
            [
                "QSYSPRT",
                "BOB",
                "QSYSPRT",
                "Ready",
                "30",
                "1",
                "*STD",
                "QGPL/QPRINT",
                "2026-10-26 07:34",
                "001234/BOB/QPADEV0001"
            ]
        );
        assert_eq!(r[3].status, Status::Active);
        for (s, colour) in [
            (SpoolStatus::Ready, Status::Active),
            (SpoolStatus::Printing, Status::Active),
            (SpoolStatus::Message, Status::Error),
            (SpoolStatus::Held, Status::Neutral),
            (SpoolStatus::Saved, Status::Neutral),
        ] {
            assert_eq!(status_colour(s), colour, "{s:?}");
        }
        for s in STATUSES {
            assert_eq!(status_named(&status_word(s).to_lowercase()), Some(s));
        }
        assert_eq!(utc_time(0), "1970-01-01 00:00");
        assert_eq!(utc_time(951_782_400_000), "2000-02-29 00:00");
    }

    #[test]
    fn sorting_a_page() {
        let mut v = vec![
            file("B", SpoolStatus::Held, 3, 30),
            file("a", SpoolStatus::Ready, 10, 10),
            file("C", SpoolStatus::Message, 2, 20),
        ];
        sort_page(&mut v, 0, true);
        assert_eq!(
            v.iter().map(|f| f.id.file.as_str()).collect::<Vec<_>>(),
            ["a", "B", "C"]
        );
        sort_page(&mut v, 4, false);
        assert_eq!(v.iter().map(|f| f.pages).collect::<Vec<_>>(), [10, 3, 2]);
        sort_page(&mut v, 8, true);
        assert_eq!(
            v.iter().map(|f| f.created).collect::<Vec<_>>(),
            [10, 20, 30]
        );
        sort_page(&mut v, 3, true);
        assert_eq!(v[0].status, SpoolStatus::Held);
        assert_eq!(
            list_line(60, "SYS", Some("Pages")),
            "QAD6001: 60 spooled files on SYS. Sorted by Pages within this page only."
        );
        let mut p = qap_client::spool::Paging::new(25);
        p.update(60);
        assert_eq!(paging_line(&p, 25), "Files 1-25 of 60, page 1 of 3");
        p.update(0);
        assert_eq!(paging_line(&p, 0), "No files");
    }

    #[test]
    fn properties_have_every_field() {
        let f = file("QSYSPRT", SpoolStatus::Held, 30, 0);
        let p = properties(&f, &utc_time);
        assert_eq!(p.len(), 15);
        assert!(p.contains(&("Device type", "SCS".into())));
        assert!(p.contains(&("Size", "2 KB (2048 bytes)".into())));
        assert_eq!(
            file_lines(&[f])[0],
            "QSYSPRT   number 30   job 001234/BOB/QPADEV0001"
        );
    }

    #[test]
    fn the_summary() {
        let f = file("RPT001", SpoolStatus::Ready, 1, 0);
        let ok = Done {
            file: f.clone(),
            action: Action::Release(f.id.clone()),
            outcome: Outcome::Done,
        };
        let no = Done {
            outcome: Outcome::Refused {
                server: Some(ServerMessage {
                    id: "CPF3303".into(),
                    severity: 40,
                    data: Vec::new(),
                    text: "File RPT001 not found in job.".into(),
                    help: String::new(),
                }),
                line: String::new(),
            },
            ..ok.clone()
        };
        assert_eq!(
            summary_heading(&[ok.clone(), no.clone()]),
            (
                "QAD6006: 1 of 2 actions failed; see the details.".into(),
                Status::Error
            )
        );
        assert_eq!(
            summary_heading(&[ok.clone(), ok.clone()]).0,
            "All 2 actions were done."
        );
        let rows = summary_rows(&[ok, no]);
        assert_eq!(rows[0][3].text, "Spooled file RPT001 released.");
        assert_eq!(
            (rows[1][2].text.as_str(), rows[1][2].status),
            ("CPF3303", Status::Error)
        );
        assert_eq!(rows[1][3].text, "File RPT001 not found in job.");
    }

    #[test]
    fn layout_as_a_printer_session() {
        let o = scs_options(37, qsa_config::qsp::Paper::A4);
        assert_eq!((o.ccsid, o.font, o.paper), (37, 11, scs::Paper::A4));
        assert_eq!(scs_options(65535, qsa_config::qsp::Paper::Letter).ccsid, 37);
        assert_eq!(
            save_name(&file("QSYSPRT", SpoolStatus::Ready, 2, 0).id, "pdf"),
            "QSYSPRT-2-001234.pdf"
        );
    }
}
