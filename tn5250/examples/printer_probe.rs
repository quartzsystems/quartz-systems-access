// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! Connects as a printer, prints the negotiation (debug log), the start-up
//! response and what print records arrive, answers each with print
//! complete, and writes each job's print data to a folder.
//!
//! `cargo run -p tn5250 --example printer_probe -- HOST [PORT] --device NAME
//!  [--msgq NAME --lib NAME] [--tls] [--seconds N] [--jobs DIR] [--record FILE]`

use std::time::{Duration, Instant};

use tn5250::printer::{Event, Options, PrintEvent, PrinterSession, Response};
use tn5250::session::Target;
use tn5250::telnet::tls::{TlsOptions, Trust};

fn arg(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .cloned()
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let host = args.first().cloned().unwrap_or_else(|| "127.0.0.1".into());
    let tls = args.iter().any(|a| a == "--tls");
    let port = args
        .get(1)
        .and_then(|p| p.parse().ok())
        .unwrap_or(if tls { 992 } else { 23 });
    let device = arg(&args, "--device").unwrap_or_else(|| "QSAPRT01".into());
    let seconds: u64 = arg(&args, "--seconds")
        .and_then(|s| s.parse().ok())
        .unwrap_or(30);
    tn5250::log::set_sink(tn5250::log::Level::Debug, |l, t| {
        eprintln!("{:5} {t}", l.name());
    });
    let mut options = Options::new(&device, 37);
    if let Some(q) = arg(&args, "--msgq") {
        options.message_queue = q;
    }
    if let Some(l) = arg(&args, "--lib") {
        options.message_queue_library = l;
    }
    let target = Target {
        host: host.clone(),
        port,
        tls: tls.then(|| TlsOptions {
            trust: Trust::default(),
            system_roots: true,
            timeout: tn5250::telnet::CONNECT_TIMEOUT,
        }),
    };
    let mut s = match PrinterSession::connect(&target, &options) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("connect: {e}");
            std::process::exit(1);
        }
    };
    if let Some(path) = arg(&args, "--record") {
        let mut r =
            tn5250::replay::Recorder::create_printer(std::path::Path::new(&path), &device, 37)
                .expect("recording file");
        s.set_recorder(Box::new(move |d, b| r.record(d, b)));
    }
    let jobs_dir = arg(&args, "--jobs").map(std::path::PathBuf::from);
    if let Some(d) = &jobs_dir {
        std::fs::create_dir_all(d).expect("jobs folder");
    }
    let mut job: Vec<u8> = Vec::new();
    let mut n = 0;
    let end = Instant::now() + Duration::from_secs(seconds);
    while Instant::now() < end {
        let events = match s.poll(Duration::from_millis(100)) {
            Ok(e) => e,
            Err(e) => {
                println!("session ended: {e}");
                break;
            }
        };
        for ev in events {
            match ev {
                Event::Ready(m) => println!("ready: {m:?}"),
                Event::Startup(r) => println!(
                    "start-up response {} ({}), system {:?}, device {:?}",
                    r.code,
                    r.meaning(),
                    r.system,
                    r.device
                ),
                Event::ProtocolError(e) => println!("protocol error: {e}"),
                Event::Rejected(why) => println!("record rejected: {why}"),
                Event::Print(events) => {
                    for e in &events {
                        match e {
                            PrintEvent::JobStarted {
                                job: j,
                                transformed,
                            } => {
                                println!("job {j} started (transformed: {transformed})")
                            }
                            PrintEvent::Data { bytes, .. } => {
                                println!("  {} bytes", bytes.len());
                                job.extend_from_slice(bytes);
                            }
                            PrintEvent::JobEnded { job: j, end } => {
                                println!("job {j} ended ({end:?}), {} bytes", job.len());
                                n += 1;
                                if let Some(d) = &jobs_dir {
                                    let p = d.join(format!("job{n}.scs"));
                                    std::fs::write(&p, &job).expect("job file");
                                    println!("  written to {}", p.display());
                                }
                                job.clear();
                            }
                            PrintEvent::Cleared { .. } => {
                                println!("print buffers cleared");
                                job.clear();
                            }
                        }
                    }
                    s.respond(Response::Complete).expect("respond");
                }
            }
        }
    }
    s.close();
}
