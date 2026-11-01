// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! Runs a macro against a live system without the window, through the same
//! player the session window uses, then prints the final screen. Prompts
//! are read from standard input (or `--answer VALUE`, repeatable).
//!
//! `cargo run -p qsaccess --example run_macro -- HOST MACRO.qsm [--port N] [--tls] [--answer VALUE]...`

use std::io::BufRead;
use std::time::{Duration, Instant};

use qsa_config::qsm::MacroFile;
use qsa_config::settings::ScreenSize as CfgSize;
use qsaccess::macros::player::{Act, Player};
use tn5250::input::Key;
use tn5250::negotiate::{Event, Request, ScreenSize};
use tn5250::session::{Session, Target};
use tn5250::telnet::tls::{TlsOptions, Trust};
use tn5250::terminal::Terminal;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (Some(host), Some(path)) = (args.first(), args.get(1)) else {
        eprintln!("usage: run_macro HOST MACRO.qsm [--port N] [--tls] [--answer VALUE]...");
        std::process::exit(2);
    };
    let flag = |name: &str| args.iter().any(|a| a == name);
    let values = |name: &str| -> Vec<String> {
        args.windows(2)
            .filter(|w| w[0] == name)
            .map(|w| w[1].clone())
            .collect()
    };
    let tls = flag("--tls");
    let port = values("--port")
        .first()
        .and_then(|p| p.parse().ok())
        .unwrap_or(if tls { 992 } else { 23 });
    let mut answers = values("--answer");
    answers.reverse();
    let m = MacroFile::load(std::path::Path::new(path)).unwrap_or_else(|e| {
        eprintln!("{path}: {e}");
        std::process::exit(1);
    });
    let size = match m.info.screen_size {
        CfgSize::S24x80 => ScreenSize::S24x80,
        CfgSize::S27x132 => ScreenSize::S27x132,
    };
    let target = Target {
        host: host.clone(),
        port,
        tls: tls.then(|| TlsOptions {
            trust: Trust::default(),
            system_roots: true,
            timeout: tn5250::telnet::CONNECT_TIMEOUT,
        }),
    };
    let mut s = Session::connect(&target, Request::display(size, None, 37)).unwrap_or_else(|e| {
        eprintln!("connect: {e}");
        std::process::exit(1);
    });
    let mut term = Terminal::new(size, 37);
    let mut player = Player::new(&m, &m.display_name(None));
    let send = |s: &mut Session, recs: Vec<tn5250::record::Record>| {
        for r in recs {
            s.send(&r).ok();
        }
    };
    let deadline = Instant::now() + Duration::from_secs(300);
    let outcome = loop {
        if Instant::now() > deadline {
            break Err("took more than five minutes".to_owned());
        }
        match s.poll(Duration::from_millis(100)) {
            Ok(events) => {
                for ev in events {
                    if let Event::Record(r) = ev {
                        let applied = term.receive(&r);
                        send(&mut s, applied.replies);
                    }
                }
            }
            Err(e) => break Err(format!("session ended: {e}")),
        }
        // The keyboard opens only once the first screen is in.
        if term.screen.fields.is_empty()
            && term.screen.text_rows().iter().all(|l| l.trim().is_empty())
        {
            continue;
        }
        let result = loop {
            match player.poll(&term.screen, true, Instant::now()) {
                Act::Type(t) => {
                    for c in t.chars() {
                        let out = term.key(Key::Char(c));
                        send(&mut s, out.send);
                    }
                }
                Act::Key(k) => {
                    println!("step {}: {k:?}", player.step() - 1);
                    let out = term.key(k);
                    send(&mut s, out.send);
                }
                Act::Prompt { label, .. } => {
                    let v = answers.pop().unwrap_or_else(|| {
                        eprint!("{label}: ");
                        let mut line = String::new();
                        std::io::stdin().lock().read_line(&mut line).ok();
                        line.trim_end_matches(['\r', '\n']).to_owned()
                    });
                    player.answer(Some(v));
                }
                Act::Wait => break None,
                Act::Done => break Some(Ok(())),
                Act::Failed { step, reason } => {
                    break Some(Err(format!("stopped at step {step}: {reason}")));
                }
            }
        };
        if let Some(r) = result {
            break r;
        }
    };
    for (i, line) in term.screen.text_rows().iter().enumerate() {
        println!("{:2} |{line}|", i + 1);
    }
    s.close();
    match outcome {
        Ok(()) => println!("macro completed"),
        Err(e) => {
            println!("macro {e}");
            std::process::exit(1);
        }
    }
}
