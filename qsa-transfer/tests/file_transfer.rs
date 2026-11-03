// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! File transfers against QS/400's test file system: binary byte for byte
//! both ways and resumed after a stop, the partial download beside its
//! target, the existing-file policies (Ask with each answer, Skip, Replace,
//! Replace if newer, Keep both), text converted by the system with line
//! endings changed here, a text download resumed from its last confirmed
//! `next_offset`, and the jobs a folder makes.

#[path = "../../qap-client/tests/common/mod.rs"]
mod common;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime};

use common::{Server, cfg, start};
use qap_client::ifs::{self, WriteOptions};
use qap_client::proto::Op;
use qap_client::{Client, Files, Secret};
use qapd::testing::files::{JAPANESE, MIXED, TEXT, TREE, text_file};
use qsa_transfer::files::transfer::{
    self, download_job, keep_both, part_path, plan_download, plan_upload, upload_job,
};
use qsa_transfer::files::{Choice, Conflict, Direction, Job, Options, Outcome, Policy};

fn bob(s: &Server) -> Client {
    let mut c = Client::new(s.target());
    c.sign_on("BOB", &Secret::from("SECRET1")).unwrap();
    c
}

fn dir(name: &str) -> PathBuf {
    let d = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join("file-transfer")
        .join(name);
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// Runs `job`, answering any question with `answer`.
fn run(c: &mut Client, job: &mut Job, answer: Choice) -> Outcome {
    transfer::run(
        c,
        job,
        &mut |_| answer,
        &mut |_| {},
        &AtomicBool::new(false),
    )
    .unwrap()
}

fn pattern(n: usize) -> Vec<u8> {
    (0..n).map(|i| ((i * 7 + i / 251) % 256) as u8).collect()
}

fn binary() -> Options {
    Options {
        existing: Policy::Replace,
        ..Options::default()
    }
}

fn text(crlf: bool, ccsid: u16) -> Options {
    Options {
        text: true,
        crlf,
        ccsid,
        existing: Policy::Replace,
    }
}

fn read_remote(c: &mut Client, path: &str, as_ccsid: Option<u16>) -> Vec<u8> {
    let mut out = Vec::new();
    ifs::read(
        |r| c.request(Op::ReadFile, r),
        path,
        ifs::ReadOptions {
            as_ccsid,
            ..ifs::ReadOptions::default()
        },
        &mut out,
        |_| {},
        &AtomicBool::new(false),
    )
    .unwrap();
    out
}

#[test]
fn binary_both_ways_resumed_after_a_cancel() {
    let s = start(cfg());
    let mut c = bob(&s);
    let d = dir("binary");
    let data = pattern(5 << 20);
    let local = d.join("up.bin");
    std::fs::write(&local, &data).unwrap();
    // Up, cancelled after the first part, then retried: it goes on where
    // the system's file ends.
    let mut job = upload_job(&local, "/home/BOB", binary());
    assert_eq!(
        (job.remote.as_str(), job.size),
        ("/home/BOB/up.bin", 5 << 20)
    );
    let cancel = AtomicBool::new(false);
    let o = transfer::run(
        &mut c,
        &mut job,
        &mut |_| Choice::Replace,
        &mut |_| cancel.store(true, Ordering::SeqCst),
        &cancel,
    )
    .unwrap();
    let Outcome::Cancelled(Some(at)) = o else {
        panic!("{o:?}")
    };
    assert!(at.remote > 0 && at.remote < data.len() as u64);
    assert!(job.resumes());
    let mut first = None;
    let o = transfer::run(
        &mut c,
        &mut job,
        &mut |_| panic!("a resume asks nothing"),
        &mut |p| {
            first.get_or_insert(p.offset);
        },
        &AtomicBool::new(false),
    )
    .unwrap();
    assert!(matches!(o, Outcome::Done { .. }), "{o:?}");
    assert!(first.unwrap() > at.remote, "went on from {}", at.remote);
    assert_eq!(read_remote(&mut c, "/home/BOB/up.bin", None), data);
    assert_eq!(c.get_info("/home/BOB/up.bin").unwrap().ccsid, ifs::UTF8);
    // Down, cancelled, the part kept beside the target, then retried.
    let entry = c.get_info("/home/BOB/up.bin").unwrap();
    let mut job = download_job("/home/BOB", &entry, &d.join("down"), binary());
    std::fs::create_dir_all(d.join("down")).unwrap();
    let cancel = AtomicBool::new(false);
    let o = transfer::run(
        &mut c,
        &mut job,
        &mut |_| Choice::Replace,
        &mut |_| cancel.store(true, Ordering::SeqCst),
        &cancel,
    )
    .unwrap();
    assert!(matches!(o, Outcome::Cancelled(Some(_))), "{o:?}");
    let part = part_path(&job.local);
    assert!(part.exists() && !job.local.exists());
    assert_eq!(
        std::fs::metadata(&part).unwrap().len(),
        job.resume.unwrap().local
    );
    assert!(matches!(
        run(&mut c, &mut job, Choice::Replace),
        Outcome::Done { .. }
    ));
    assert!(!part.exists());
    assert_eq!(std::fs::read(&job.local).unwrap(), data);
}

#[test]
fn existing_targets_follow_the_policy() {
    let s = start(cfg());
    let mut c = bob(&s);
    let d = dir("policy");
    let remote = c.get_info(&text_file(1208)).unwrap();
    let target = d.join("text1208.txt");
    let job = |existing| {
        download_job(
            ifs::parent(&text_file(1208)),
            &remote,
            &d,
            Options {
                existing,
                ..Options::default()
            },
        )
    };
    std::fs::write(&target, b"mine").unwrap();
    // Skip keeps it; Ask with Skip too, and it was asked with Keep both
    // offered.
    assert_eq!(
        run(&mut c, &mut job(Policy::Skip), Choice::Replace),
        Outcome::Skipped
    );
    let mut asked = Vec::new();
    let o = transfer::run(
        &mut c,
        &mut job(Policy::Ask),
        &mut |q: &Conflict| {
            asked.push(q.clone());
            Choice::Skip
        },
        &mut |_| {},
        &AtomicBool::new(false),
    )
    .unwrap();
    assert_eq!(o, Outcome::Skipped);
    assert_eq!(asked.len(), 1);
    assert!(asked[0].keep_both && asked[0].name == "text1208.txt");
    assert_eq!(std::fs::read(&target).unwrap(), b"mine");
    // Keep both: the next free number.
    let mut j = job(Policy::Ask);
    assert!(matches!(
        run(&mut c, &mut j, Choice::KeepBoth),
        Outcome::Done { .. }
    ));
    assert_eq!(j.local, d.join("text1208 (2).txt"));
    assert_eq!(std::fs::read(&j.local).unwrap(), TEXT.as_bytes());
    assert_eq!(keep_both(&target, Path::exists), d.join("text1208 (3).txt"));
    // Replace if newer: a local file changed later than the system's is
    // kept; one changed earlier is replaced.
    let later = SystemTime::now() + Duration::from_secs(3600);
    std::fs::File::options()
        .write(true)
        .open(&target)
        .unwrap()
        .set_modified(later)
        .unwrap();
    assert_eq!(
        run(&mut c, &mut job(Policy::ReplaceIfNewer), Choice::Skip),
        Outcome::Skipped
    );
    std::fs::File::options()
        .write(true)
        .open(&target)
        .unwrap()
        .set_modified(SystemTime::UNIX_EPOCH + Duration::from_secs(86_400))
        .unwrap();
    assert!(matches!(
        run(&mut c, &mut job(Policy::ReplaceIfNewer), Choice::Skip),
        Outcome::Done { .. }
    ));
    assert_eq!(std::fs::read(&target).unwrap(), TEXT.as_bytes());
    // Up to a file on the system: asked without Keep both, and Replace
    // replaces it.
    let local = d.join("new.txt");
    std::fs::write(&local, b"from here").unwrap();
    let up = |existing| {
        upload_job(
            &local,
            "/home/BOB/text",
            Options {
                existing,
                ..Options::default()
            },
        )
    };
    assert!(matches!(
        run(&mut c, &mut up(Policy::Ask), Choice::Skip),
        Outcome::Done { .. }
    ));
    let mut asked = Vec::new();
    let o = transfer::run(
        &mut c,
        &mut up(Policy::Ask),
        &mut |q: &Conflict| {
            asked.push(q.clone());
            Choice::Skip
        },
        &mut |_| {},
        &AtomicBool::new(false),
    )
    .unwrap();
    assert_eq!(o, Outcome::Skipped);
    assert_eq!(
        asked,
        [Conflict {
            name: "new.txt".into(),
            folder: "/home/BOB/text".into(),
            keep_both: false
        }]
    );
    std::fs::write(&local, b"again").unwrap();
    assert!(matches!(
        run(&mut c, &mut up(Policy::Ask), Choice::Replace),
        Outcome::Done { .. }
    ));
    assert_eq!(
        read_remote(&mut c, "/home/BOB/text/new.txt", None),
        b"again"
    );
    // Replace if newer: the system's file was changed just now, after this
    // one.
    std::fs::File::options()
        .write(true)
        .open(&local)
        .unwrap()
        .set_modified(SystemTime::UNIX_EPOCH + Duration::from_secs(86_400))
        .unwrap();
    assert_eq!(
        run(&mut c, &mut up(Policy::ReplaceIfNewer), Choice::Replace),
        Outcome::Skipped
    );
}

#[test]
fn text_is_converted_by_the_system_and_line_endings_here() {
    let s = start(cfg());
    let mut c = bob(&s);
    let d = dir("text");
    let crlf = TEXT.replace('\n', "\r\n");
    for ccsid in [37u16, 273, 819, 1208] {
        let e = c.get_info(&text_file(ccsid)).unwrap();
        for (lines, want) in [(true, crlf.as_str()), (false, TEXT)] {
            let mut job = download_job("/home/BOB/text", &e, &d, text(lines, 1208));
            job.local = d.join(format!("{ccsid}-{lines}.txt"));
            assert!(matches!(
                run(&mut c, &mut job, Choice::Replace),
                Outcome::Done { .. }
            ));
            assert_eq!(
                std::fs::read_to_string(&job.local).unwrap(),
                want,
                "{ccsid} {lines}"
            );
        }
    }
    // Up: CR LF here, LF on the system, in the CCSID asked for, matching
    // the system's own whole-file conversion. (Japanese for the mixed
    // CCSID: the German letters are not in it.)
    for (ccsid, words) in [(37u16, TEXT), (1208, TEXT), (MIXED, JAPANESE)] {
        let local = d.join(format!("up{ccsid}.txt"));
        std::fs::write(&local, words.replace('\n', "\r\n")).unwrap();
        let mut job = upload_job(&local, "/home/BOB", text(true, ccsid));
        assert!(matches!(
            run(&mut c, &mut job, Choice::Replace),
            Outcome::Done { .. }
        ));
        assert_eq!(c.get_info(&job.remote).unwrap().ccsid, ccsid);
        let back = read_remote(&mut c, &job.remote, Some(ifs::UTF8));
        assert_eq!(String::from_utf8(back).unwrap(), words, "{ccsid}");
        // The same bytes as the system's conversion of the text in one go.
        let one = format!("/home/BOB/one{ccsid}.txt");
        ifs::write(
            |w| c.request(Op::WriteFile, w),
            &one,
            &mut words.as_bytes(),
            WriteOptions {
                data_ccsid: Some(ifs::UTF8),
                ccsid,
                ..WriteOptions::default()
            },
            |_| {},
            &AtomicBool::new(false),
        )
        .unwrap();
        assert_eq!(
            read_remote(&mut c, &job.remote, None),
            read_remote(&mut c, &one, None),
            "{ccsid}"
        );
    }
}

#[test]
fn a_text_download_resumes_from_its_last_confirmed_offset() {
    let s = start(cfg());
    let mut c = bob(&s);
    let d = dir("text-resume");
    // A mixed file of a few MiB, so the download takes several requests:
    // made by the system from UTF-8 sent in text mode.
    let mut big = String::new();
    while big.len() < 3 << 20 {
        big.push_str(JAPANESE);
    }
    let src = d.join("jp.txt");
    std::fs::write(&src, &big).unwrap();
    let mut up = upload_job(&src, "/home/BOB", text(false, MIXED));
    up.remote = "/home/BOB/jp939.txt".into();
    assert!(matches!(
        run(&mut c, &mut up, Choice::Replace),
        Outcome::Done { .. }
    ));
    let e = c.get_info("/home/BOB/jp939.txt").unwrap();
    assert_eq!(e.ccsid, MIXED);
    // Down with CR LF, cancelled after the first part, then on from the
    // offset kept, on a new connection.
    let mut job = download_job("/home/BOB", &e, &d.join("out"), text(true, 1208));
    std::fs::create_dir_all(d.join("out")).unwrap();
    let cancel = AtomicBool::new(false);
    let o = transfer::run(
        &mut c,
        &mut job,
        &mut |_| Choice::Replace,
        &mut |_| cancel.store(true, Ordering::SeqCst),
        &cancel,
    )
    .unwrap();
    let Outcome::Cancelled(Some(at)) = o else {
        panic!("{o:?}")
    };
    assert!(at.remote > 0 && at.remote < e.size && at.local > 0);
    drop(c);
    let mut c = bob(&s);
    assert!(job.resumes());
    assert!(matches!(
        run(&mut c, &mut job, Choice::Replace),
        Outcome::Done { .. }
    ));
    assert_eq!(
        std::fs::read_to_string(&job.local).unwrap(),
        big.replace('\n', "\r\n")
    );
    // A text upload that stops starts again from the beginning.
    let mut up = upload_job(&src, "/home/BOB", text(false, MIXED));
    up.resume = Some(transfer::Resume {
        remote: 100,
        local: 100,
    });
    assert!(!up.resumes());
}

#[test]
fn folders_make_a_job_per_file() {
    let s = start(cfg());
    let mut c = bob(&s);
    let d = dir("folders");
    let jobs = plan_download(&mut c, TREE, &d, Options::default()).unwrap();
    assert_eq!(jobs.len(), 7);
    assert!(jobs.iter().all(|j| j.direction == Direction::Download));
    assert!(d.join("tree/src/lib").is_dir() && d.join("tree/docs/old").is_dir());
    let mut jobs = jobs;
    for j in &mut jobs {
        assert!(matches!(
            run(&mut c, j, Choice::Replace),
            Outcome::Done { .. }
        ));
    }
    assert_eq!(
        std::fs::read_to_string(d.join("tree/readme.txt")).unwrap(),
        "/home/BOB/tree/readme.txt"
    );
    // And back up under another folder.
    c.create_directory("/home/BOB/back", false).unwrap();
    let mut ups = plan_upload(
        &mut c,
        &d.join("tree"),
        "/home/BOB/back",
        Options::default(),
    )
    .unwrap();
    assert_eq!(ups.len(), 7);
    for j in &mut ups {
        assert!(
            matches!(run(&mut c, j, Choice::Replace), Outcome::Done { .. }),
            "{}",
            j.remote
        );
    }
    assert_eq!(
        read_remote(&mut c, "/home/BOB/back/tree/docs/old/notes.txt", None),
        b"/home/BOB/tree/docs/old/notes.txt"
    );
}

#[test]
fn a_saved_request_plans_its_files() {
    use qsa_transfer::files::transfer::plan_request;
    use qsa_transfer::request::FilesRequest;
    let s = start(cfg());
    let mut c = bob(&s);
    let d = dir("request");
    let req = |direction, local: Vec<PathBuf>, remote: &str, recurse| FilesRequest {
        direction,
        local,
        remote: remote.into(),
        recurse,
        options: binary(),
    };
    let names = |jobs: &[Job]| -> Vec<String> { jobs.iter().map(Job::name).collect() };
    // Down: a pattern in the folder, a file, a folder with and without its
    // folders.
    let r = req(
        Direction::Download,
        vec![d.join("a")],
        "/home/BOB/text/text*.txt",
        true,
    );
    let jobs = plan_request(&mut c, &r).unwrap();
    assert_eq!(
        names(&jobs),
        [
            "text1208.txt",
            "text273.txt",
            "text37.txt",
            "text819.txt",
            "text939.txt"
        ]
    );
    assert!(jobs.iter().all(|j| j.local.starts_with(d.join("a"))));
    let r = req(
        Direction::Download,
        vec![d.join("b")],
        &text_file(37),
        false,
    );
    assert_eq!(names(&plan_request(&mut c, &r).unwrap()), ["text37.txt"]);
    let r = req(Direction::Download, vec![d.join("c")], TREE, false);
    assert_eq!(names(&plan_request(&mut c, &r).unwrap()), ["readme.txt"]);
    let r = req(Direction::Download, vec![d.join("c")], TREE, true);
    assert_eq!(plan_request(&mut c, &r).unwrap().len(), 7);
    // Up: a glob, a folder with and without its folders, into a folder made.
    let src = d.join("src");
    std::fs::create_dir_all(src.join("sub/deeper")).unwrap();
    for f in ["a.csv", "b.csv", "c.txt", "sub/d.csv", "sub/deeper/e.csv"] {
        std::fs::write(src.join(f), f).unwrap();
    }
    let r = req(
        Direction::Upload,
        vec![src.join("*.csv")],
        "/home/BOB/in/new",
        true,
    );
    let jobs = plan_request(&mut c, &r).unwrap();
    assert_eq!(names(&jobs), ["a.csv", "b.csv"]);
    assert_eq!(
        c.get_info("/home/BOB/in/new").unwrap().kind,
        qap_client::proto::ops::ifs::EntryKind::Directory
    );
    let r = req(
        Direction::Upload,
        vec![src.join("sub")],
        "/home/BOB/in",
        false,
    );
    let jobs = plan_request(&mut c, &r).unwrap();
    assert_eq!(names(&jobs), ["d.csv"]);
    assert_eq!(jobs[0].remote, "/home/BOB/in/sub/d.csv");
    let r = req(
        Direction::Upload,
        vec![src.join("sub")],
        "/home/BOB/in",
        true,
    );
    let mut jobs = plan_request(&mut c, &r).unwrap();
    assert_eq!(jobs.len(), 2);
    for j in &mut jobs {
        assert!(matches!(
            run(&mut c, j, Choice::Replace),
            Outcome::Done { .. }
        ));
    }
    assert_eq!(
        read_remote(&mut c, "/home/BOB/in/sub/deeper/e.csv", None),
        b"sub/deeper/e.csv"
    );
    // The system's refusal of a folder it may not read.
    let r = req(
        Direction::Download,
        vec![d.join("x")],
        "/home/ANN/private",
        true,
    );
    let e = plan_request(&mut c, &r).unwrap_err();
    assert_eq!(e.error.status(), Some(qap_client::Status::NotAuthorized));
}
