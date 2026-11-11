// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! Certificate Management and the Security tab of System Properties edit
//! the same CA lists and stay in step while both are open: what one adds
//! or removes, the other shows at once; the dialog's OK saves its list and
//! its Cancel drops it, and the window follows both.
//!
//! The two are real windows (never shown), so this runs as a plain program
//! on FLTK's one UI thread (`harness = false`), not under the test harness.

use std::process::ExitCode;

use qsa_config::system::CaCertificate;
use qsa_config::{Settings, System, Systems};
use qsaccess::app::{AppState, Shared};
use qsaccess::certs::model::{Certificate, Entry};
use qsaccess::certs::{self, Certs, sample};
use qsaccess::{properties, trust};

/// 2026-11-01 12:00:00 UTC.
const NOW: i64 = 1_793_534_400;
const A: &str = "a.example.net";
const B: &str = "b.example.net";

fn cert(cn: &str) -> Certificate {
    Certificate::new(sample::der(
        cn,
        "Example Root CA",
        "260101000000Z",
        "360101000000Z",
    ))
}

/// What the window lists: each certificate's name and the systems that
/// trust it.
fn listed(c: &Certs) -> Vec<(String, Vec<String>)> {
    c.entries()
        .iter()
        .map(|e| (e.name(), e.systems.clone()))
        .collect()
}

fn want(rows: &[(&str, &[&str])]) -> Vec<(String, Vec<String>)> {
    rows.iter()
        .map(|(n, s)| ((*n).to_owned(), s.iter().map(|x| (*x).to_owned()).collect()))
        .collect()
}

fn entry(c: &Certs, cert: &Certificate) -> Entry {
    c.entries()
        .into_iter()
        .find(|e| e.key() == cert.info.sha256)
        .expect("the certificate is listed")
}

fn saved(state: &Shared, system: &str) -> Vec<CaCertificate> {
    state
        .borrow()
        .systems
        .get(system)
        .expect("a configured system")
        .ca_certificates
        .clone()
}

fn names(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| (*s).to_owned()).collect()
}

fn run() {
    let (x, y, z) = (cert("x"), cert("y"), cert("z"));
    let mut systems = Systems::new();
    systems.upsert(System {
        ca_certificates: vec![CaCertificate::Pem(x.pem())],
        ..System::new(A)
    });
    systems.upsert(System::new(B));
    let state = AppState::new(None, systems, Settings::default()).shared();

    let window = certs::build(&state, Some(NOW), Some(0));
    assert_eq!(listed(&window), want(&[("x", &[A])]));

    // System Properties opens on A: its Security tab and the window now
    // show and change one list.
    let a = state.borrow().systems.get(A).cloned().unwrap();
    let (_dialog, form) = properties::build("System Properties - a.example.net", &a);
    let editing = properties::share(&state, A, &form);
    assert_eq!(form.ca_rows(), ["Embedded PEM (1 certificate)"]);
    assert_eq!(listed(&window), want(&[("x", &[A])]));

    // Add on the tab: the window lists it at once. Nothing is saved yet.
    form.add_ca(CaCertificate::Pem(y.pem()));
    assert_eq!(listed(&window), want(&[("x", &[A]), ("y", &[A])]));
    assert_eq!(saved(&state, A), [CaCertificate::Pem(x.pem())]);

    // Import in the window for both systems: the tab lists it at once. B
    // has no dialog open, so its list is saved; A's waits for the dialog.
    assert_eq!(
        certs::import(&state, &z, &names(&[A, B])).unwrap(),
        names(&[A, B])
    );
    assert_eq!(form.ca_rows().len(), 3);
    assert_eq!(form.ca_list().borrow()[2], CaCertificate::Pem(z.pem()));
    assert_eq!(
        listed(&window),
        want(&[("x", &[A]), ("y", &[A]), ("z", &[A, B])])
    );
    assert_eq!(saved(&state, B), [CaCertificate::Pem(z.pem())]);
    assert_eq!(saved(&state, A), [CaCertificate::Pem(x.pem())]);

    // Remove in the window: gone from the tab.
    assert_eq!(
        certs::remove(&state, &entry(&window, &x)).unwrap(),
        names(&[A])
    );
    assert_eq!(
        *form.ca_list().borrow(),
        [CaCertificate::Pem(y.pem()), CaCertificate::Pem(z.pem())]
    );
    assert_eq!(form.ca_rows().len(), 2);
    assert_eq!(listed(&window), want(&[("y", &[A]), ("z", &[A, B])]));

    // Remove on the tab: gone from the window.
    form.remove_ca(0);
    assert_eq!(listed(&window), want(&[("z", &[A, B])]));

    // Trusted for... in the window, B alone: the tab's list is empty.
    let (added, removed) =
        certs::set_trusted_for(&state, &entry(&window, &z), &names(&[B])).unwrap();
    assert!(added.is_empty());
    assert_eq!(removed, names(&[A]));
    assert!(form.ca_rows().is_empty());
    assert_eq!(listed(&window), want(&[("z", &[B])]));

    // OK: the dialog's list is what is saved, as System Configurations
    // saves it, and the window still shows it.
    form.add_ca(CaCertificate::Pem(y.pem()));
    let edited = form.read().expect("the form is valid");
    drop(editing);
    // Until it is saved the window is back at the saved list.
    assert_eq!(listed(&window), want(&[("x", &[A]), ("z", &[B])]));
    state.borrow_mut().systems.change(A, edited).unwrap();
    certs::changed(&state);
    assert_eq!(listed(&window), want(&[("y", &[A]), ("z", &[B])]));
    assert_eq!(saved(&state, A), [CaCertificate::Pem(y.pem())]);

    // Cancel: what the tab changed and did not save goes with the dialog.
    let a = state.borrow().systems.get(A).cloned().unwrap();
    let (_dialog, form) = properties::build("System Properties - a.example.net", &a);
    let editing = properties::share(&state, A, &form);
    form.add_ca(CaCertificate::Pem(x.pem()));
    form.remove_ca(0);
    assert_eq!(listed(&window), want(&[("x", &[A]), ("z", &[B])]));
    drop(editing);
    assert_eq!(listed(&window), want(&[("y", &[A]), ("z", &[B])]));
    // With the dialog gone the window changes the saved list again.
    certs::import(&state, &x, &names(&[A])).unwrap();
    assert_eq!(saved(&state, A).len(), 2);
    assert_eq!(form.ca_rows().len(), 1, "the closed dialog is left alone");

    // Trust for this system, chosen when a connection presents a
    // certificate, shows in the window too.
    let w = cert("w");
    assert!(trust::add(&state, B, &w.der));
    assert_eq!(
        listed(&window),
        want(&[("w", &[B]), ("x", &[A]), ("y", &[A]), ("z", &[B])])
    );
    // And connections to B trust exactly what the window lists for it.
    let b = state.borrow().systems.get(B).cloned().unwrap();
    let trusted: Vec<Vec<u8>> = trust::for_system(&b)
        .certificates
        .iter()
        .map(|c| c.to_vec())
        .collect();
    assert_eq!(trusted, [z.der.clone(), w.der.clone()]);
}

fn main() -> ExitCode {
    if std::env::args().any(|a| a == "--list") {
        println!("certs_sync: test");
        return ExitCode::SUCCESS;
    }
    qsa_ui::theme::pin_font_rendering();
    let _app = fltk::app::App::default();
    if let Err(e) = qsa_ui::theme::init() {
        eprintln!("certs_sync: fonts not loaded: {e}");
        return ExitCode::FAILURE;
    }
    run();
    println!("ok       certs_sync");
    ExitCode::SUCCESS
}
