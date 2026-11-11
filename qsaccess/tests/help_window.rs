// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! The Help window: Home, Contents order, links followed through FLTK's
//! help view, Back and Forward, Search, and a topic that does not exist
//! (`QAD0050`).
//!
//! FLTK's help view follows a link to another document by loading a file.
//! Help writes each link as a path that names no file and reads the topic
//! back from where the view says it went, so this checks that path with the
//! view itself: every link of every topic, followed as a click follows it,
//! shows the topic the link was written to.
//!
//! The window is a real one (never shown), so this runs as a plain program
//! on FLTK's one UI thread (`harness = false`), not under the test harness.

use std::process::ExitCode;

use fltk::prelude::*;

use qsaccess::help::{self, TOPICS, topic};

fn run() {
    let v = help::build();
    let slug = |v: &help::Viewer| v.current().map(|t| t.slug);

    // It opens at Home with nowhere to go back to.
    assert_eq!(slug(&v), Some(help::HOME));
    assert_eq!(v.can_go(), (false, false));
    assert_eq!(v.window.label(), "Help - Getting started");

    // A topic by name, then Back and Forward.
    assert!(v.go(topic::RUN_SQL, None));
    assert_eq!(slug(&v), Some(topic::RUN_SQL));
    assert_eq!(v.can_go(), (true, false));
    v.go_back();
    assert_eq!(slug(&v), Some(help::HOME));
    assert_eq!(v.can_go(), (false, true));
    v.go_forward();
    assert_eq!(slug(&v), Some(topic::RUN_SQL));
    assert_eq!(v.can_go(), (true, false));
    // The same topic again is not a step to go back over.
    assert!(v.go(topic::RUN_SQL, None));
    v.go_back();
    assert_eq!(slug(&v), Some(help::HOME));
    assert_eq!(v.can_go(), (false, true));

    // Every link of every topic, followed as the view follows a click.
    let mut followed = 0;
    for t in TOPICS {
        assert!(v.go(t.slug, None));
        for i in 0..v.link_addresses().len() {
            // Each showing of a topic has addresses of its own.
            assert!(v.go(t.slug, None));
            let address = v.link_addresses().swap_remove(i);
            // The address names the topic it leads to.
            let want = address
                .split('/')
                .nth(3)
                .unwrap_or_else(|| panic!("{}: link {address}", t.slug))
                .to_owned();
            assert!(help::find(&want).is_some(), "{}: link to {want}", t.slug);
            v.click_link(&address);
            assert_eq!(slug(&v), Some(want.as_str()), "{}: link {address}", t.slug);
            assert_eq!(v.status_text(), "", "{}: link {address}", t.slug);
            followed += 1;
        }
    }
    assert!(followed > 50, "only {followed} links followed");

    // A link from a showing that is no longer on screen does nothing.
    assert!(v.go(topic::SCHEMAS, None));
    let stale = v.link_addresses();
    assert!(!stale.is_empty(), "Schemas links to other topics");
    assert!(v.go(topic::MESSAGES, None));
    v.click_link(&stale[0]);
    assert_eq!(slug(&v), Some(topic::MESSAGES));

    // A message's entry in the reference.
    assert!(v.go(topic::MESSAGES_REFERENCE, Some("QAD0043")));
    assert_eq!(slug(&v), Some(topic::MESSAGES_REFERENCE));

    // A topic that does not exist: QAD0050, and the topic on screen stays.
    assert!(!v.go("no-such-topic", None));
    assert_eq!(slug(&v), Some(topic::MESSAGES_REFERENCE));
    assert_eq!(
        v.status_text(),
        "QAD0050: Help topic no-such-topic not found."
    );
    // So does a link to one, were there ever one: the topic it was in is
    // shown again, with the message.
    v.follow("no-such-topic", None);
    assert_eq!(slug(&v), Some(topic::MESSAGES_REFERENCE));
    assert_eq!(
        v.status_text(),
        "QAD0050: Help topic no-such-topic not found."
    );

    // Search lists the topics that contain what is typed.
    v.search_for("client certificates are not used");
    let found: Vec<&str> = v.found().iter().map(|t| t.slug).collect();
    assert_eq!(found, [topic::CERTIFICATE_MANAGEMENT]);
    assert_eq!(v.status_text(), "1 topic found.");
    v.search_for("zzzz no topic says this");
    assert!(v.found().is_empty());
    assert_eq!(
        v.status_text(),
        "No topic contains \"zzzz no topic says this\"."
    );
    v.search_for("");
    assert!(v.found().is_empty());
    assert_eq!(v.status_text(), "");

    // Home.
    v.go_home();
    assert_eq!(slug(&v), Some(help::HOME));
}

fn main() -> ExitCode {
    if std::env::args().any(|a| a == "--list") {
        println!("help_window: test");
        return ExitCode::SUCCESS;
    }
    qsa_ui::theme::pin_font_rendering();
    let _app = fltk::app::App::default();
    if let Err(e) = qsa_ui::theme::init() {
        eprintln!("help_window: fonts not loaded: {e}");
        return ExitCode::FAILURE;
    }
    run();
    println!("ok       help_window");
    ExitCode::SUCCESS
}
