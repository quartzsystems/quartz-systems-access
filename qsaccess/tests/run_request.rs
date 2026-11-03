// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! `qsaccess run`: the running instance hands its session on a system to
//! the run over the instance endpoint (a reply endpoint of the same user's
//! only), the run attaches its own connection with it, and the exit codes
//! for bad arguments and request files.

#[path = "../../qap-client/tests/common/mod.rs"]
mod common;

use std::process::Command;
use std::sync::mpsc;

use common::{Server, cfg, start};
use qap_client::{Attached, Secret};
use qsa_config::system::CaCertificate;
use qsa_config::{Settings, System, Systems};
use qsaccess::app::AppState;
use qsaccess::ipc::{self, Endpoint};
use qsaccess::qap::{self, lock};
use qsaccess::request::Request;
use qsaccess::transfer::cli;

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

fn endpoint() -> Endpoint {
    #[cfg(windows)]
    {
        Endpoint::named(&format!("quartz-access-test-run-{}", std::process::id()))
    }
    #[cfg(unix)]
    {
        let dir = std::env::temp_dir().join(format!("qsa-run-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Endpoint::at(dir.join("s.sock"))
    }
}

#[test]
fn the_running_instances_session_is_handed_to_a_run() {
    let s = start(cfg());
    let sys = system(&s);
    let mut systems = Systems::new();
    systems.upsert(sys.clone());
    let state = AppState::new(None, systems, Settings::default()).shared();
    let h = state.borrow_mut().qap.client(&sys);
    lock(&h).sign_on("BOB", &Secret::from("SECRET1")).unwrap();

    // The running instance: its endpoint, and its answers on this thread.
    let main = endpoint();
    ipc::remember(&main);
    let (tx, rx) = mpsc::channel();
    let _server = ipc::Server::bind(&main).unwrap().spawn(move |args| {
        let _ = tx.send(args);
    });
    let ask = |system: &'static str| {
        let m = main.clone();
        std::thread::spawn(move || cli::borrow_session(&m, system))
    };
    let answer = |state: &qsaccess::app::Shared| {
        let args = rx.recv_timeout(std::time::Duration::from_secs(10)).unwrap();
        let r = Request::parse(&args);
        let (sys, reply) = r.session_token.clone().expect("a session request");
        cli::answer(state, &sys, &reply);
    };

    let run = ask("127.0.0.1");
    answer(&state);
    let token = run.join().unwrap().expect("the session handed over");
    let a = Attached::open(&qap::target_for(&sys), &token).unwrap();
    assert_eq!(a.job().user, "BOB");
    assert_ne!(a.job().job.number, lock(&h).job_info().unwrap().job.number);

    // A system with no session: nothing handed over.
    let run = ask("other.example.net");
    answer(&state);
    assert!(run.join().unwrap().is_none());

    // Only a reply endpoint of the same instance's naming is answered.
    let name = ipc::reply_name(&main, 4242);
    assert!(ipc::reply_endpoint(&main, &name).is_some());
    assert!(ipc::reply_endpoint(&main, &format!("{name}x")).is_none());
    assert!(ipc::reply_endpoint(&main, r"\\.\pipe\someone-else").is_none());
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
