// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! Connects, negotiates, prints the debug log and the first records, and
//! disconnects without typing anything.
//!
//! `cargo run -p tn5250 --example probe -- HOST [PORT] [--tls] [--132] [--device NAME] [--hex] [--enter] [--record FILE]`

use std::time::{Duration, Instant};

use tn5250::negotiate::{Event, Request, ScreenSize};
use tn5250::screen::Screen;
use tn5250::session::{Session, Target};
use tn5250::telnet::tls::{TlsOptions, Trust};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let host = args.first().cloned().unwrap_or_else(|| "127.0.0.1".into());
    let tls = args.iter().any(|a| a == "--tls");
    let port = args
        .get(1)
        .and_then(|p| p.parse().ok())
        .unwrap_or(if tls { 992 } else { 23 });
    let size = if args.iter().any(|a| a == "--132") {
        ScreenSize::S27x132
    } else {
        ScreenSize::S24x80
    };
    let device = args
        .iter()
        .position(|a| a == "--device")
        .and_then(|i| args.get(i + 1))
        .cloned();
    tn5250::log::set_sink(tn5250::log::Level::Debug, |l, t| {
        eprintln!("{:5} {t}", l.name());
    });
    let target = Target {
        host: host.clone(),
        port,
        tls: tls.then(|| TlsOptions {
            trust: Trust::default(),
            system_roots: true,
            timeout: tn5250::telnet::CONNECT_TIMEOUT,
        }),
    };
    let mut s = match Session::connect(&target, Request::display(size, device.as_deref(), 37)) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("connect: {e}");
            std::process::exit(1);
        }
    };
    let hex = args.iter().any(|a| a == "--hex");
    let mut enter = args.iter().any(|a| a == "--enter");
    let mut term = tn5250::terminal::Terminal::new(size, 37);
    let mut recorder = args
        .iter()
        .position(|a| a == "--record")
        .and_then(|i| args.get(i + 1))
        .map(|p| {
            tn5250::replay::Recorder::create(std::path::Path::new(p), size, 37)
                .expect("recording file")
        });
    let mut screen = Screen::new(size, tn5250::codepage::default_page());
    let start = Instant::now();
    let mut records = 0;
    while start.elapsed() < Duration::from_secs(8) && records < 6 {
        match s.poll(Duration::from_millis(200)) {
            Ok(events) => {
                for ev in events {
                    match ev {
                        Event::Record(r) => {
                            records += 1;
                            if hex {
                                println!("hex {}", tn5250::log::hex(&r.encode()));
                            }
                            if let Some(rec) = recorder.as_mut() {
                                rec.inbound(&r);
                            }
                            let applied = term.receive(&r);
                            screen = term.screen.clone();
                            for reply in &applied.replies {
                                if let Some(rec) = recorder.as_mut() {
                                    rec.outbound(&term.screen, reply);
                                }
                                s.send(reply).ok();
                            }
                            if enter && term.screen.keyboard.is_open() {
                                enter = false;
                                let before = term.screen.clone();
                                if let Some(rec) = recorder.as_mut() {
                                    rec.key(&before, tn5250::input::Key::Enter);
                                }
                                for rec in term.key(tn5250::input::Key::Enter).send {
                                    println!("sent {}", tn5250::log::hex(&rec.data));
                                    if let Some(r) = recorder.as_mut() {
                                        r.outbound(&term.screen, &rec);
                                    }
                                    s.send(&rec).ok();
                                }
                            }
                            for e in &applied.errors {
                                println!("stream error: {e}");
                            }
                            for reply in &applied.replies {
                                println!(
                                    "reply opcode {} {}",
                                    reply.opcode,
                                    tn5250::log::hex(&reply.data[..reply.data.len().min(16)])
                                );
                            }
                            println!(
                                "record opcode {} flags {:04X}, {} bytes: {}",
                                r.opcode,
                                r.flags,
                                r.data.len(),
                                tn5250::log::hex(&r.data[..r.data.len().min(48)])
                            );
                        }
                        other => println!("{other:?}"),
                    }
                }
            }
            Err(e) => {
                println!("ended: {e}");
                break;
            }
        }
    }
    for (i, line) in screen.text_rows().iter().enumerate() {
        println!("{:2} |{line}|", i + 1);
    }
    println!(
        "cursor {:?} keyboard {:?} read {:?} fields {}",
        screen.rc(screen.cursor),
        screen.keyboard,
        screen.pending_read,
        screen.fields.len()
    );
    println!("security: {:?}", s.security());
    s.close();
}
