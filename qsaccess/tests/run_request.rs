// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! `qsaccess run`: a run hands the running instance only the request
//! file's path, the instance runs it on its own session and attached
//! connection and sends back progress, the summary and the exit code, no
//! token ever crossing the link; and the exit codes for bad arguments and
//! request files.

#[path = "../../qap-client/tests/common/mod.rs"]
mod common;

use std::process::Command;
use std::sync::{Mutex, MutexGuard, mpsc};
use std::time::Duration;

use common::{Server, cfg, start};
use qap_client::Secret;
use qsa_config::system::CaCertificate;
use qsa_config::{Settings, System, Systems};
use qsa_transfer::request;
use qsa_transfer::{Download, Source};
use qsaccess::app::AppState;
use qsaccess::ipc::{self, Endpoint};
use qsaccess::qap::lock;
use qsaccess::request::Request;
use qsaccess::transfer::cli;

/// `qap::busy` runs FLTK's event loop while it waits, and that loop is for
/// one thread at a time: the tests that serve a run take turns, or two of
/// them in it at once can hang.
static GUI: Mutex<()> = Mutex::new(());

fn gui() -> MutexGuard<'static, ()> {
    GUI.lock().unwrap_or_else(|e| e.into_inner())
}

fn system(s: &Server) -> System {
    let mut sys = System::new("127.0.0.1");
    sys.ports.qap = s.port;
    sys.ca_certificates = vec![CaCertificate::Path(
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../qsa-net/tests/data/test-ca.pem"
        )
        .into(),
    )];
    sys
}

fn endpoint(tag: &str) -> Endpoint {
    #[cfg(windows)]
    {
        Endpoint::named(&format!(
            "quartz-access-test-run-{tag}-{}",
            std::process::id()
        ))
    }
    #[cfg(unix)]
    {
        let dir = std::env::temp_dir().join(format!("qsa-run-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Endpoint::at(dir.join("s.sock"))
    }
}

/// The hexadecimal and plain forms of `bytes`.
fn forms(bytes: &[u8]) -> Vec<String> {
    let hex: String = bytes.iter().map(|b| format!("{b:02X}")).collect();
    let mut v = vec![hex.clone(), hex.to_lowercase()];
    if let Ok(s) = std::str::from_utf8(bytes) {
        v.push(s.to_owned());
    }
    v
}

#[test]
fn a_run_is_delegated_to_the_instance_and_no_token_crosses_the_link() {
    let _gui = gui();
    let s = start(cfg());
    let sys = system(&s);
    let mut systems = Systems::new();
    systems.upsert(sys.clone());
    let state = AppState::new(None, systems, Settings::default()).shared();
    let h = state.borrow_mut().qap.client(&sys);
    lock(&h).sign_on("BOB", &Secret::from("SECRET1")).unwrap();
    let jobs = s.jobs().len();

    // The saved request: HLINV.ASSETS to a CSV beside it.
    let dir = std::env::temp_dir().join(format!("qsa-run-req-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let qdx = dir.join("assets.qdx");
    let csv = dir.join("assets.csv");
    let r = request::Request::Download {
        system: sys.name.clone(),
        download: Download {
            source: Source::Table {
                schema: "HLINV".into(),
                table: "ASSETS".into(),
                where_clause: String::new(),
            },
            file: csv.clone(),
            ..Download::default()
        },
    };
    request::save(&r, &qdx).unwrap();

    // The running instance: its endpoint, every message it is sent kept,
    // and the run served here, on its own session.
    let main = endpoint("delegated");
    let (tx, rx) = mpsc::channel();
    let server = ipc::Server::bind(&main).unwrap().spawn(move |args, link| {
        let _ = tx.send((args, link));
    });
    // `qsaccess run`'s side, every frame it is sent kept.
    let m = main.clone();
    let path = qdx.clone();
    let run = std::thread::spawn(move || {
        let mut link = m.open().unwrap().expect("the instance listens");
        link.send(&["--run".to_owned(), path.display().to_string()])
            .unwrap();
        let mut got = Vec::new();
        while let Some(f) = link.recv().unwrap() {
            let last = f.first().is_some_and(|k| k == "exit");
            got.push(f);
            if last {
                break;
            }
        }
        got
    });
    let (asked, link) = rx.recv_timeout(Duration::from_secs(10)).unwrap();
    let parsed = Request::parse(&asked);
    assert_eq!(parsed.run.as_deref(), Some(qdx.as_path()));
    cli::serve(&state, parsed.run.as_ref().unwrap(), link, false);
    let replies = run.join().unwrap();

    // Only the path went to the instance; lines and the exit code came back.
    assert_eq!(asked, ["--run".to_owned(), qdx.display().to_string()]);
    assert!(
        replies
            .iter()
            .all(|f| f.len() == 2 && ["progress", "summary", "exit"].contains(&f[0].as_str())),
        "{replies:?}"
    );
    assert_eq!(
        replies.last().unwrap(),
        &["exit".to_owned(), "0".to_owned()]
    );
    assert!(
        replies
            .iter()
            .any(|f| f[0] == "summary" && f[1].starts_with("QAD7030")),
        "{replies:?}"
    );
    assert_eq!(std::fs::read_to_string(&csv).unwrap().lines().count(), 13);
    // The instance ran it on a connection of its own, attached to its
    // session.
    assert_eq!(s.jobs().len(), jobs + 1);
    // Nothing that crossed the link holds the session token: the test
    // system's tokens end in 28 bytes of 0x5A.
    let tail = [0x5Au8; 28];
    let all: Vec<String> = asked
        .iter()
        .chain(replies.iter().flatten())
        .cloned()
        .collect();
    for secret in forms(&tail) {
        assert!(
            all.iter().all(|a| !a.contains(&secret)),
            "{secret:?} crossed the link"
        );
    }
    assert_eq!(server.refused(), 0);

    // delegate() relays the same run, and says how it ended.
    let m = main.clone();
    let path = qdx.clone();
    let run = std::thread::spawn(move || cli::delegate(&m, &path, &cli::Out::Console));
    let (asked, link) = rx.recv_timeout(Duration::from_secs(10)).unwrap();
    cli::serve(
        &state,
        Request::parse(&asked).run.as_ref().unwrap(),
        link,
        false,
    );
    assert_eq!(run.join().unwrap(), Some(cli::Exit::Done));

    // A request file the instance cannot read: exit 1, from the instance.
    let bad = dir.join("bad.qdx");
    std::fs::write(
        &bad,
        "[qsaccess]
kind = \"display\"
version = 1
",
    )
    .unwrap();
    let m = main.clone();
    let path = bad.clone();
    let run = std::thread::spawn(move || cli::delegate(&m, &path, &cli::Out::Console));
    let (asked, link) = rx.recv_timeout(Duration::from_secs(10)).unwrap();
    cli::serve(
        &state,
        Request::parse(&asked).run.as_ref().unwrap(),
        link,
        false,
    );
    assert_eq!(run.join().unwrap(), Some(cli::Exit::BadInput));

    // No instance: nothing to delegate to.
    drop(server);
    #[cfg(unix)]
    assert_eq!(cli::delegate(&main, &qdx, &cli::Out::Console), None);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn bad_arguments_and_request_files_exit_1() {
    let dir = std::env::temp_dir().join(format!("qsa-run-cfg-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let exe = env!("CARGO_BIN_EXE_qsaccess");
    let code = |args: &[&str]| {
        Command::new(exe)
            .arg("--config-dir")
            .arg(&dir)
            .args(args)
            .output()
            .unwrap()
            .status
            .code()
    };
    assert_eq!(code(&["run"]), Some(1));
    assert_eq!(code(&["run", "a.qdx", "b.qdx"]), Some(1));
    assert_eq!(
        code(&["run", &dir.join("missing.qdx").display().to_string()]),
        Some(1)
    );
    let bad = dir.join("bad.qdx");
    std::fs::write(&bad, "[qsaccess]\nkind = \"display\"\nversion = 1\n").unwrap();
    assert_eq!(code(&["run", &bad.display().to_string()]), Some(1));
    // A system this computer has no configuration for.
    let unknown = dir.join("unknown.qdx");
    std::fs::write(
        &unknown,
        "[qsaccess]\nkind = \"transfer\"\nversion = 1\n[transfer]\ndirection = \"download\"\nsystem = \"nowhere.example.net\"\nfile = \"x.csv\"\n[transfer.source]\nkind = \"query\"\nsql = \"VALUES 1\"\n",
    )
    .unwrap();
    assert_eq!(code(&["run", &unknown.display().to_string()]), Some(1));
}

#[test]
fn a_files_request_runs_on_the_instance_with_its_exit_code() {
    let _gui = gui();
    use qsa_transfer::files::{Direction, Options};
    use qsa_transfer::request::FilesRequest;
    let s = start(cfg());
    let sys = system(&s);
    let mut systems = Systems::new();
    systems.upsert(sys.clone());
    let state = AppState::new(None, systems, Settings::default()).shared();
    let h = state.borrow_mut().qap.client(&sys);
    lock(&h).sign_on("BOB", &Secret::from("SECRET1")).unwrap();
    let dir = std::env::temp_dir().join(format!("qsa-run-files-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let write = |name: &str, remote: &str| {
        let qdx = dir.join(name);
        let r = request::Request::Files {
            system: sys.name.clone(),
            files: FilesRequest {
                direction: Direction::Download,
                local: vec![dir.join("out")],
                remote: remote.into(),
                recurse: true,
                options: Options {
                    text: true,
                    crlf: false,
                    ..Options::default()
                },
            },
        };
        request::save(&r, &qdx).unwrap();
        qdx
    };
    let main = endpoint("files");
    let (tx, rx) = mpsc::channel();
    let _server = ipc::Server::bind(&main).unwrap().spawn(move |args, link| {
        let _ = tx.send((args, link));
    });
    let run = |qdx: std::path::PathBuf| {
        let m = main.clone();
        let t = std::thread::spawn(move || cli::delegate(&m, &qdx, &cli::Out::Console));
        let (asked, link) = rx.recv_timeout(Duration::from_secs(10)).unwrap();
        cli::serve(
            &state,
            Request::parse(&asked).run.as_ref().unwrap(),
            link,
            false,
        );
        t.join().unwrap()
    };
    // The text files of a folder, converted by the system: exit 0.
    let ok = write("text.qdx", "/home/BOB/text/text*.txt");
    assert_eq!(run(ok), Some(cli::Exit::Done));
    assert_eq!(
        std::fs::read_to_string(dir.join("out/text37.txt")).unwrap(),
        qapd::testing::files::TEXT
    );
    // A folder the profile may not read: exit 3, the system's refusal said.
    let refused = write("private.qdx", "/home/ANN/private");
    assert_eq!(run(refused), Some(cli::Exit::Failed));
    let _ = std::fs::remove_dir_all(&dir);
}
