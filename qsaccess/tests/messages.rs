// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! Messages in the application against QS/400's test system: the watches
//! a system's client keeps go to its next client when the session ends, so
//! the next sign-on watches again and catches up; the Messages-waiting
//! count follows the watched queues across a drop, a resume and a new
//! sign-on; and an arrival opens Message Arrived as its queue's option
//! says.

#[path = "../../qap-client/tests/common/mod.rs"]
mod common;

use std::time::{Duration, Instant};

use qap_client::proto::ops::messages::{MessageType, Queue};
use qap_client::proto::types::JobId;
use qap_client::{Notice, Secret, Watch, WatchEvent};
use qapd::Config;
use qapd::testing::messages::Msg;
use qsa_config::settings::Arrival;
use qsa_config::system::CaCertificate;
use qsa_config::{Settings, System, Systems};
use qsaccess::app::AppState;
use qsaccess::messages::{model, watching};
use qsaccess::qap::{self, lock};

fn system(port: u16) -> System {
    let mut sys = System::new("127.0.0.1");
    sys.ports.qap = port;
    sys.ca_certificates = vec![CaCertificate::Path(
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../qsa-net/tests/data/test-ca.pem"
        )
        .into(),
    )];
    sys
}

fn state(sys: &System) -> qsaccess::app::Shared {
    let mut systems = Systems::new();
    systems.upsert(sys.clone());
    AppState::new(None, systems, Settings::default()).shared()
}

fn quick() -> Config {
    Config {
        watch_poll: Duration::from_millis(20),
        ..common::cfg()
    }
}

fn inquiry(text: &str, at: i64) -> Msg {
    Msg {
        kind: MessageType::Inquiry,
        severity: 99,
        text: text.into(),
        sent: at,
        from: JobId {
            name: "QPADEV0009".into(),
            user: "ANN".into(),
            number: "004711".into(),
        },
        ..Msg::default()
    }
}

fn later(s: &common::Server, n: i64) -> i64 {
    i64::try_from(s.c.now()).unwrap() * 1000 + 10_000_000 + n
}

/// Polls `h` until `f` of what came is true, or five seconds.
fn until_events(h: &qap::Handle, f: impl Fn(&[WatchEvent]) -> bool) -> Vec<WatchEvent> {
    let end = Instant::now() + Duration::from_secs(5);
    let mut all = Vec::new();
    while Instant::now() < end {
        let mut c = lock(h);
        c.poll();
        all.extend(c.watch_events());
        drop(c);
        if f(&all) {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    all
}

#[test]
fn the_count_follows_the_queues_across_a_drop_and_a_new_sign_on() {
    let s = common::start(quick());
    let sys = system(s.port);
    let st = state(&sys);
    assert_eq!(watching::waiting(&st, "127.0.0.1"), None, "no client yet");
    let h = qap::client(&st, &sys);
    lock(&h).sign_on("BOB", &Secret::from("SECRET1")).unwrap();
    assert_eq!(watching::waiting(&st, "127.0.0.1"), None, "nothing watched");
    lock(&h).watch(Watch::Queue(Queue::Own)).unwrap();
    assert_eq!(watching::waiting(&st, "127.0.0.1"), Some(1), "CPA5305");
    lock(&h).watch_events();

    // An inquiry arrives; the count goes up once.
    s.c.inject_message("QUSRSYS", "BOB", inquiry("Lunch?", later(&s, 0)))
        .unwrap();
    until_events(&h, |e| {
        e.iter().any(|e| matches!(e, WatchEvent::Arrived { .. }))
    });
    assert_eq!(watching::waiting(&st, "127.0.0.1"), Some(2));

    // A drop with one more in the gap: resumed, caught up, counted once.
    s.c.drop_all();
    common::until("the close", || {
        lock(&h).connection().is_none_or(|n| n.closed().is_some())
    });
    s.c.inject_message("QUSRSYS", "BOB", inquiry("Dinner?", later(&s, 1)))
        .unwrap();
    assert!(lock(&h).poll().contains(&Notice::Resumed));
    assert_eq!(watching::waiting(&st, "127.0.0.1"), Some(3));
    let evs = lock(&h).watch_events();
    assert!(evs.contains(&WatchEvent::CaughtUp {
        queue: Queue::Own,
        count: 1
    }));

    // Answered elsewhere while away: the catch-up's reply and the recount
    // take it off.
    s.c.drop_all();
    common::until("the close", || {
        lock(&h).connection().is_none_or(|n| n.closed().is_some())
    });
    let own = {
        let qs = s.c.messages.lock().unwrap();
        let i = qs.find("QUSRSYS", "BOB").unwrap();
        qs.queues[i]
            .msgs
            .iter()
            .find(|m| m.text == "Lunch?")
            .unwrap()
            .key
    };
    // Answered from a second session of BOB's.
    let mut bob2 = qap_client::Client::new(common::target(s.port));
    bob2.sign_on("BOB", &Secret::from("SECRET1")).unwrap();
    bob2.reply_message(&Queue::Own, own.to_be_bytes(), "Yes")
        .unwrap();
    assert!(lock(&h).poll().contains(&Notice::Resumed));
    assert_eq!(watching::waiting(&st, "127.0.0.1"), Some(2));

    // The session ends under a window's own connection: the watches wait
    // for the next client, whose sign-on watches again and catches up.
    st.borrow_mut().qap.forget_ended("127.0.0.1");
    lock(&h).sign_off().unwrap();
    s.c.inject_message("QUSRSYS", "BOB", inquiry("Breakfast?", later(&s, 2)))
        .unwrap();
    let h2 = qap::client(&st, &sys);
    assert!(!std::sync::Arc::ptr_eq(&h, &h2));
    assert!(lock(&h2).watching().contains(&Watch::Queue(Queue::Own)));
    lock(&h2).sign_on("BOB", &Secret::from("SECRET1")).unwrap();
    let evs = lock(&h2).watch_events();
    let caught: Vec<String> = evs
        .iter()
        .filter_map(|e| match e {
            WatchEvent::Arrived { message, .. } => Some(message.text.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(caught, ["Breakfast?"]);
    assert_eq!(watching::waiting(&st, "127.0.0.1"), Some(3));
}

#[test]
fn arrivals_open_message_arrived_as_their_queue_says() {
    use qap_client::proto::ops::messages::QueuedMessage;
    let ask = QueuedMessage {
        kind: MessageType::Inquiry,
        needs_reply: true,
        ..QueuedMessage::default()
    };
    let tell = QueuedMessage::default();
    // The defaults: the own queue's inquiries only; QSYSOPR not watched.
    let o = Settings::default().messages.for_system("sys");
    assert_eq!(o.own_arrival, Arrival::InquiriesOnly);
    assert!(model::opens_arrived(o.own_arrival, &ask));
    assert!(!model::opens_arrived(o.own_arrival, &tell));
    assert!(!o.operator);
    assert_eq!(
        watching::watch_name(&Watch::Queue(Queue::Own), "BOB"),
        "BOB"
    );
    assert_eq!(
        watching::watch_name(&Watch::Queue(Queue::SystemOperator), "BOB"),
        "QSYSOPR"
    );
}
