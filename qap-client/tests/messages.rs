// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! Version 6 against QS/400's test system (`qapd::testing::messages` and
//! its spool watches): every `Messages` operation and status, watches and
//! their events, the watch manager's catch-up after a forced close, a
//! passive watch beside a queue held in break mode, and a reply matched to
//! its inquiry through the sender's copy and `answers`.

mod common;

use std::time::{Duration, Instant};

use common::{cfg, start, until};
use qap_client::messages::{is_reply_to, needs_reply};
use qap_client::proto::Op;
use qap_client::proto::ops::messages::{
    Key, MessageType, Queue, QueueRef, QueuedMessage, Recipient,
};
use qap_client::proto::ops::spool::{SpoolChange, SpoolStatus, SpoolTarget, WatchSpool};
use qap_client::proto::types::{JobId, ObjectName};
use qap_client::watch::{Watch, WatchEvent};
use qap_client::{Client, Notice, Secret, Status};
use qapd::Config;
use qapd::testing::messages::{BREAK_JOB, Msg, OPERATOR_FILLER};
use qapd::testing::spool::File;

/// Watches look every 20 ms here.
fn quick() -> Config {
    Config {
        watch_poll: Duration::from_millis(20),
        ..cfg()
    }
}

fn client(s: &common::Server, user: &str, password: &str) -> Client {
    let mut c = Client::new(s.target());
    c.sign_on(user, &Secret::from(password)).unwrap();
    c
}

fn bob(s: &common::Server) -> Client {
    client(s, "BOB", "SECRET1")
}

fn named(lib: &str, name: &str) -> Queue {
    Queue::Named {
        queue: ObjectName {
            library: lib.into(),
            name: name.into(),
        },
    }
}

fn refused(e: &qap_client::Error) -> (Option<Status>, String) {
    (
        e.status(),
        e.server_message().map(|m| m.id.clone()).unwrap_or_default(),
    )
}

/// A message from another job, `sent` at `at`.
fn from_elsewhere(text: &str, at: i64) -> Msg {
    Msg {
        kind: MessageType::Informational,
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

/// A time after every fixture's.
fn later(s: &common::Server, n: i64) -> i64 {
    i64::try_from(s.c.now()).unwrap() * 1000 + 10_000_000 + n
}

/// Every watch event within `wait`, until `done` says enough.
fn events_until(
    c: &mut Client,
    wait: Duration,
    done: impl Fn(&[WatchEvent]) -> bool,
) -> Vec<WatchEvent> {
    let end = Instant::now() + wait;
    let mut all = Vec::new();
    loop {
        c.poll();
        all.extend(c.watch_events());
        if done(&all) || Instant::now() >= end {
            return all;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn arrived(evs: &[WatchEvent]) -> Vec<(QueuedMessage, bool)> {
    evs.iter()
        .filter_map(|e| match e {
            WatchEvent::Arrived {
                message, caught_up, ..
            } => Some((message.clone(), *caught_up)),
            _ => None,
        })
        .collect()
}

fn texts(evs: &[WatchEvent]) -> Vec<String> {
    arrived(evs).into_iter().map(|(m, _)| m.text).collect()
}

#[test]
fn lists_are_strictly_newest_or_oldest_first_and_paged() {
    let s = start(quick());
    let mut c = bob(&s);
    let op = c
        .list_messages(&Queue::SystemOperator, true, 0, 100)
        .unwrap();
    assert_eq!(op.total as usize, OPERATOR_FILLER + 9);
    assert_eq!(op.messages.len(), 100);
    let rest = c
        .list_messages(&Queue::SystemOperator, true, 100, 100)
        .unwrap();
    assert_eq!(rest.messages.len(), op.total as usize - 100);
    let newest_first: Vec<i64> = op
        .messages
        .iter()
        .chain(&rest.messages)
        .map(|m| m.sent)
        .collect();
    assert!(
        newest_first.windows(2).all(|w| w[0] > w[1]),
        "strictly newest first"
    );
    let oldest = c
        .list_messages(&Queue::SystemOperator, false, 0, 0)
        .unwrap();
    let mut back = newest_first.clone();
    back.reverse();
    assert_eq!(
        oldest.messages.iter().map(|m| m.sent).collect::<Vec<_>>(),
        back
    );
    // Keys are unique on the queue.
    let mut keys: Vec<Key> = oldest.messages.iter().map(|m| m.key).collect();
    keys.sort_unstable();
    keys.dedup();
    assert_eq!(keys.len(), oldest.messages.len());

    // BOB's own: every type, a sender's copy answered, CPA5305 waiting.
    let own = c.list_messages(&Queue::Own, false, 0, 0).unwrap();
    let kinds: Vec<MessageType> = own.messages.iter().map(|m| m.kind).collect();
    for k in MessageType::ALL {
        assert!(kinds.contains(&k), "{k:?} on BOB's queue");
    }
    let copy = own
        .messages
        .iter()
        .find(|m| m.kind == MessageType::SenderCopy)
        .unwrap();
    assert_eq!(copy.reply.as_deref(), Some("OK"));
    let reply = own
        .messages
        .iter()
        .find(|m| is_reply_to(m, &copy.key))
        .unwrap();
    assert_eq!(reply.text, "OK");
    let waiting: Vec<&QueuedMessage> = own.messages.iter().filter(|m| needs_reply(m)).collect();
    assert_eq!(waiting.len(), 1);
    assert_eq!(waiting[0].id, "CPA5305");
    // Only replies have `answers`.
    assert!(
        own.messages
            .iter()
            .all(|m| m.answers.is_some() == (m.kind == MessageType::Reply))
    );

    // Refusals as sent.
    let e = c
        .list_messages(&named("QGPL", "SECRETQ"), true, 0, 1)
        .unwrap_err();
    assert_eq!(refused(&e), (Some(Status::NotAuthorized), "CPF9802".into()));
    let e = c
        .list_messages(&named("QGPL", "NOSUCHQ"), true, 0, 1)
        .unwrap_err();
    assert_eq!(refused(&e), (Some(Status::NotFound), "CPF2403".into()));
    assert!(!c.may_read_queue(&named("QGPL", "SECRETQ")).unwrap());
    assert!(c.may_read_queue(&Queue::SystemOperator).unwrap());
}

#[test]
fn replies_are_checked_and_answered_once() {
    let s = start(quick());
    let mut c = bob(&s);
    let op = c
        .list_messages(&Queue::SystemOperator, false, 0, 0)
        .unwrap();
    let cpa = op
        .messages
        .iter()
        .find(|m| m.id == "CPA0701")
        .unwrap()
        .clone();
    assert!(needs_reply(&cpa));
    // A reply the message does not take.
    let e = c
        .reply_message(&Queue::SystemOperator, cpa.key, "X")
        .unwrap_err();
    assert_eq!(refused(&e), (Some(Status::Error), "CPF2420".into()));
    // An empty reply is its default.
    c.reply_message(&Queue::SystemOperator, cpa.key, "")
        .unwrap();
    let after = c
        .list_messages(&Queue::SystemOperator, false, 0, 0)
        .unwrap();
    let answered = after.messages.iter().find(|m| m.key == cpa.key).unwrap();
    assert_eq!(
        (answered.reply.as_deref(), answered.needs_reply),
        (Some("C"), false)
    );
    let reply = after
        .messages
        .iter()
        .find(|m| is_reply_to(m, &cpa.key))
        .unwrap();
    assert_eq!(
        (reply.text.as_str(), reply.from_user.as_str()),
        ("C", "BOB")
    );
    // Already answered; not an inquiry; not there.
    let e = c
        .reply_message(&Queue::SystemOperator, cpa.key, "C")
        .unwrap_err();
    assert_eq!(
        refused(&e),
        (Some(Status::InvalidRequest), "CPF2422".into())
    );
    let info = after
        .messages
        .iter()
        .find(|m| m.kind == MessageType::Informational)
        .unwrap();
    let e = c
        .reply_message(&Queue::SystemOperator, info.key, "C")
        .unwrap_err();
    assert_eq!(
        refused(&e),
        (Some(Status::InvalidRequest), "CPF2422".into())
    );
    let e = c
        .reply_message(&Queue::SystemOperator, [0xFF; 4], "C")
        .unwrap_err();
    assert_eq!(refused(&e), (Some(Status::NotFound), "CPF2410".into()));
    // An impromptu inquiry takes any reply; its default is NO.
    let forms = after
        .messages
        .iter()
        .find(|m| m.text == "Is the forms printer loaded?")
        .unwrap();
    c.reply_message(&Queue::SystemOperator, forms.key, "")
        .unwrap();
    let again = c.list_messages(&Queue::SystemOperator, true, 0, 0).unwrap();
    assert_eq!(
        again
            .messages
            .iter()
            .find(|m| m.key == forms.key)
            .unwrap()
            .reply
            .as_deref(),
        Some("NO")
    );
    // BOB's CPA5305 takes Y or N.
    let own = c.list_messages(&Queue::Own, true, 0, 0).unwrap();
    let cpa5305 = own.messages.iter().find(|m| m.id == "CPA5305").unwrap();
    let e = c
        .reply_message(&Queue::Own, cpa5305.key, "MAYBE")
        .unwrap_err();
    assert_eq!(refused(&e).1, "CPF2420");
    c.reply_message(&Queue::Own, cpa5305.key, "y").unwrap();
}

#[test]
fn removes_are_all_or_none() {
    let s = start(quick());
    let mut c = bob(&s);
    let own = c.list_messages(&Queue::Own, true, 0, 0).unwrap();
    let two: Vec<Key> = own.messages.iter().take(2).map(|m| m.key).collect();
    let e = c
        .remove_messages(&Queue::Own, &[two[0], [0xEE; 4]])
        .unwrap_err();
    assert_eq!(refused(&e), (Some(Status::NotFound), "CPF2410".into()));
    assert_eq!(
        c.list_messages(&Queue::Own, true, 0, 0).unwrap().total,
        own.total
    );
    c.remove_messages(&Queue::Own, &two).unwrap();
    let after = c.list_messages(&Queue::Own, true, 0, 0).unwrap();
    assert_eq!(after.total, own.total - 2);
    assert!(after.messages.iter().all(|m| !two.contains(&m.key)));
    let e = c
        .remove_messages(&named("QGPL", "SECRETQ"), &two)
        .unwrap_err();
    assert_eq!(refused(&e).0, Some(Status::NotAuthorized));
}

#[test]
fn sends_name_the_signed_on_profile() {
    let s = start(quick());
    let mut c = bob(&s);
    let user = |p: &str| Recipient::User { profile: p.into() };
    assert_eq!(
        c.send_message(&user("WARN"), "Lunch at noon.", false)
            .unwrap(),
        None
    );
    let mut warn = client(&s, "WARN", "WARNPW1");
    let got = warn.list_messages(&Queue::Own, true, 0, 1).unwrap();
    let m = &got.messages[0];
    assert_eq!(
        (m.text.as_str(), m.from_user.as_str()),
        ("Lunch at noon.", "BOB")
    );
    assert_eq!(m.kind, MessageType::Informational);
    // An unknown profile; a profile that never signed on has no queue yet.
    let e = c.send_message(&user("NOBODY"), "Hello", false).unwrap_err();
    assert_eq!(refused(&e), (Some(Status::NotFound), "CPF2204".into()));
    s.c.messages.lock().unwrap().profiles.push("NEWBIE".into());
    let e = c.send_message(&user("NEWBIE"), "Hello", false).unwrap_err();
    assert_eq!(refused(&e), (Some(Status::NotFound), "CPF2403".into()));
    // No text.
    let e = c.send_message(&user("WARN"), "  ", false).unwrap_err();
    assert_eq!(
        refused(&e),
        (Some(Status::InvalidRequest), "CPD0071".into())
    );
    // A queue BOB may not use.
    let to = Recipient::Queue {
        queue: named("QGPL", "SECRETQ"),
    };
    let e = c.send_message(&to, "Hello", false).unwrap_err();
    assert_eq!(refused(&e).0, Some(Status::NotAuthorized));
}

#[test]
fn watch_and_unwatch_statuses() {
    let s = start(quick());
    let mut c = bob(&s);
    let conn = c.connection().unwrap();
    // Not watched on this connection.
    let e = conn
        .call::<_, ()>(Op::UnwatchQueue, &QueueRef { queue: Queue::Own })
        .unwrap_err();
    assert_eq!(refused(&e), (Some(Status::NotFound), "TCP1A5B".into()));
    let user = |p: &str| SpoolTarget::User { profile: p.into() };
    let e = conn
        .call::<_, ()>(
            Op::UnwatchSpool,
            &WatchSpool {
                target: user("*CURRENT"),
            },
        )
        .unwrap_err();
    assert_eq!(refused(&e), (Some(Status::NotFound), "TCP1A5B".into()));
    // *ALL is no target; an unknown profile is not there.
    let e = conn
        .call::<_, ()>(
            Op::WatchSpool,
            &WatchSpool {
                target: user("*ALL"),
            },
        )
        .unwrap_err();
    assert_eq!(e.status(), Some(Status::InvalidRequest));
    let e = conn
        .call::<_, ()>(
            Op::WatchSpool,
            &WatchSpool {
                target: user("NOBODY"),
            },
        )
        .unwrap_err();
    assert_eq!(refused(&e), (Some(Status::NotFound), "CPF2204".into()));
    // A queue BOB may not read: refused, and not kept.
    let secret = Watch::Queue(named("QGPL", "SECRETQ"));
    let e = c.watch(secret.clone()).unwrap_err();
    assert_eq!(refused(&e).0, Some(Status::NotAuthorized));
    assert!(!c.watching().contains(&secret));
    // Watched, then not.
    c.watch(Watch::Queue(Queue::Own)).unwrap();
    c.watch(Watch::Queue(Queue::Own)).unwrap();
    assert_eq!(c.watching(), vec![Watch::Queue(Queue::Own)]);
    c.unwatch(&Watch::Queue(Queue::Own)).unwrap();
    assert!(c.watching().is_empty());
    let e = c
        .connection()
        .unwrap()
        .call::<_, ()>(Op::UnwatchQueue, &QueueRef { queue: Queue::Own })
        .unwrap_err();
    assert_eq!(refused(&e).1, "TCP1A5B");
}

#[test]
fn arrivals_come_once_with_the_inquiries_counted() {
    let s = start(quick());
    let mut c = bob(&s);
    c.watch(Watch::Queue(Queue::Own)).unwrap();
    assert_eq!(c.messages_waiting(), 1, "CPA5305");
    c.watch(Watch::Queue(Queue::SystemOperator)).unwrap();
    assert_eq!(c.messages_waiting(), 3, "and CPA0701 and the forms printer");
    let evs = c.watch_events();
    assert!(
        evs.contains(&WatchEvent::Watching(Watch::Queue(Queue::Own))),
        "{evs:?}"
    );
    assert!(arrived(&evs).is_empty(), "what was there is not an arrival");

    let at = later(&s, 0);
    s.c.inject_message("QUSRSYS", "BOB", from_elsewhere("First.", at))
        .unwrap();
    s.c.inject_message("QUSRSYS", "BOB", from_elsewhere("Second.", at + 1))
        .unwrap();
    let mut inquiry = from_elsewhere("Restart the writer?", at + 2);
    inquiry.kind = MessageType::Inquiry;
    inquiry.severity = 99;
    let ikey = s.c.inject_message("QSYS", "QSYSOPR", inquiry).unwrap();
    let evs = events_until(&mut c, Duration::from_secs(5), |e| arrived(e).len() >= 3);
    assert_eq!(texts(&evs), ["First.", "Second.", "Restart the writer?"]);
    assert!(arrived(&evs).iter().all(|(_, caught_up)| !caught_up));
    assert_eq!(c.messages_waiting(), 4);
    // Answered here: the inquiry is no longer waiting, and its reply
    // arrives as a message of its own.
    c.reply_message(&Queue::SystemOperator, ikey, "G").unwrap();
    assert_eq!(c.messages_waiting(), 3);
    let evs = events_until(&mut c, Duration::from_secs(5), |e| !arrived(e).is_empty());
    let (reply, _) = &arrived(&evs)[0];
    assert!(is_reply_to(reply, &ikey));
    // Nothing more, and nothing twice.
    let quiet = events_until(&mut c, Duration::from_millis(200), |_| false);
    assert!(arrived(&quiet).is_empty(), "{quiet:?}");
}

#[test]
fn a_passive_watch_beside_a_break_mode_holder() {
    let s = start(quick());
    let mut c = bob(&s);
    let team = named("QGPL", "TEAMQ");
    assert_eq!(s.c.break_holder("QGPL", "TEAMQ"), Some(BREAK_JOB));
    c.watch(Watch::Queue(team.clone())).unwrap();
    assert_eq!(
        s.c.break_holder("QGPL", "TEAMQ"),
        Some(BREAK_JOB),
        "still held"
    );
    s.c.inject_message(
        "QGPL",
        "TEAMQ",
        from_elsewhere("Stand-up in five.", later(&s, 0)),
    )
    .unwrap();
    let evs = events_until(&mut c, Duration::from_secs(5), |e| !arrived(e).is_empty());
    assert_eq!(texts(&evs), ["Stand-up in five."]);
    assert_eq!(s.c.break_holder("QGPL", "TEAMQ"), Some(BREAK_JOB));
}

#[test]
fn a_forced_close_catches_up_exactly_once() {
    let s = start(quick());
    let mut c = bob(&s);
    c.watch(Watch::Queue(Queue::Own)).unwrap();
    c.watch(Watch::Queue(Queue::SystemOperator)).unwrap();
    c.watch_events();
    let at = later(&s, 0);
    s.c.inject_message("QUSRSYS", "BOB", from_elsewhere("Before the drop.", at))
        .unwrap();
    let evs = events_until(&mut c, Duration::from_secs(5), |e| !arrived(e).is_empty());
    assert_eq!(texts(&evs), ["Before the drop."]);

    // The connection drops; three arrive while nothing watches.
    let resumes = c.resumes();
    s.c.drop_all();
    until("the client to see the close", || {
        c.connection().is_none_or(|n| n.closed().is_some())
    });
    for (i, t) in ["Gap one.", "Gap two.", "Gap three."].iter().enumerate() {
        s.c.inject_message("QUSRSYS", "BOB", from_elsewhere(t, at + 1 + i as i64))
            .unwrap();
    }
    let mut inquiry = from_elsewhere("Gap inquiry?", at + 10);
    inquiry.kind = MessageType::Inquiry;
    s.c.inject_message("QSYS", "QSYSOPR", inquiry).unwrap();

    // The next look resumes, watches again and catches up.
    assert!(c.poll().contains(&Notice::Resumed));
    assert_eq!(c.resumes(), resumes + 1);
    let evs = c.watch_events();
    let got = arrived(&evs);
    assert_eq!(
        got.iter().map(|(m, _)| m.text.as_str()).collect::<Vec<_>>(),
        ["Gap one.", "Gap two.", "Gap three.", "Gap inquiry?"]
    );
    assert!(got.iter().all(|(_, caught_up)| *caught_up));
    assert!(evs.contains(&WatchEvent::CaughtUp {
        queue: Queue::Own,
        count: 3
    }));
    assert!(evs.contains(&WatchEvent::CaughtUp {
        queue: Queue::SystemOperator,
        count: 1
    }));
    assert_eq!(c.messages_waiting(), 4);

    // Live again; nothing from the gap twice.
    s.c.inject_message("QUSRSYS", "BOB", from_elsewhere("After.", at + 20))
        .unwrap();
    let evs = events_until(&mut c, Duration::from_secs(5), |e| !arrived(e).is_empty());
    let quiet = events_until(&mut c, Duration::from_millis(200), |_| false);
    assert_eq!(texts(&evs), ["After."]);
    assert!(arrived(&quiet).is_empty(), "{quiet:?}");
}

#[test]
fn an_inquiry_and_its_reply_match_by_answers() {
    let s = start(quick());
    let mut c = bob(&s);
    c.watch(Watch::Queue(Queue::Own)).unwrap();
    c.watch_events();
    // To another queue: the key is the sender's copy's, on BOB's own.
    let to_ann = Recipient::User {
        profile: "ANN".into(),
    };
    let key = c
        .send_message(&to_ann, "Can you swap shifts?", true)
        .unwrap()
        .unwrap();
    let evs = events_until(&mut c, Duration::from_secs(5), |e| !arrived(e).is_empty());
    let (copy, _) = &arrived(&evs)[0];
    assert_eq!((copy.kind, copy.key), (MessageType::SenderCopy, key));
    assert_eq!(
        c.messages_waiting(),
        1,
        "a sender's copy is not waiting on BOB"
    );

    let mut ann = client(&s, "ANN", "ANNPW01");
    let theirs = ann.list_messages(&Queue::Own, true, 0, 0).unwrap();
    let inq = theirs
        .messages
        .iter()
        .find(|m| m.text == "Can you swap shifts?")
        .unwrap();
    assert!(needs_reply(inq));
    assert_ne!(inq.key, key, "the inquiry has its own key on ANN's queue");
    ann.reply_message(&Queue::Own, inq.key, "Yes").unwrap();

    let evs = events_until(&mut c, Duration::from_secs(5), |e| !arrived(e).is_empty());
    let (reply, _) = &arrived(&evs)[0];
    assert!(is_reply_to(reply, &key));
    assert_eq!(
        (reply.text.as_str(), reply.from_user.as_str()),
        ("Yes", "ANN")
    );
    let own = c.list_messages(&Queue::Own, true, 0, 0).unwrap();
    assert_eq!(
        own.messages
            .iter()
            .find(|m| m.key == key)
            .unwrap()
            .reply
            .as_deref(),
        Some("Yes")
    );
    // ANN's queue keeps the inquiry answered, and a reply of its own.
    let theirs = ann.list_messages(&Queue::Own, true, 0, 0).unwrap();
    assert!(theirs.messages.iter().any(|m| is_reply_to(m, &inq.key)));

    // To BOB's own queue: the key is the inquiry's, answered there.
    let me = Recipient::Queue { queue: Queue::Own };
    let key = c.send_message(&me, "Note to self?", true).unwrap().unwrap();
    let evs = events_until(&mut c, Duration::from_secs(5), |e| !arrived(e).is_empty());
    assert_eq!(arrived(&evs)[0].0.kind, MessageType::Inquiry);
    assert_eq!(c.messages_waiting(), 2);
    c.reply_message(&Queue::Own, key, "Done").unwrap();
    let evs = events_until(&mut c, Duration::from_secs(5), |e| !arrived(e).is_empty());
    assert!(is_reply_to(&arrived(&evs)[0].0, &key));
    assert_eq!(c.messages_waiting(), 1);
}

fn bob_file(name: &str, number: u32, outq: &str) -> File {
    use qap_client::proto::ops::spool::{DeviceType, SpooledFileId};
    File {
        id: SpooledFileId {
            job: JobId {
                name: "QPADEV0001".into(),
                user: "BOB".into(),
                number: "001234".into(),
            },
            file: name.into(),
            number,
        },
        status: SpoolStatus::Ready,
        outq: ObjectName {
            library: "QGPL".into(),
            name: outq.into(),
        },
        device: DeviceType::Ascii,
        pages: 1,
        copies: 1,
        data: b"hello\n".to_vec(),
        ..File::default()
    }
}

fn spool_changes(evs: &[WatchEvent]) -> Vec<(SpoolTarget, SpoolChange, String)> {
    evs.iter()
        .filter_map(|e| match e {
            WatchEvent::SpoolChanged(c) => Some((c.target.clone(), c.change, c.id.file.clone())),
            _ => None,
        })
        .collect()
}

#[test]
fn spool_watches_report_adds_changes_moves_and_removes() {
    let s = start(quick());
    let mut c = bob(&s);
    let mine = SpoolTarget::User {
        profile: "*CURRENT".into(),
    };
    let queue = |n: &str| SpoolTarget::Queue {
        outq: ObjectName {
            library: "QGPL".into(),
            name: n.into(),
        },
    };
    for t in [mine.clone(), queue("QPRINT"), queue("PRT01")] {
        c.watch(Watch::Spool(t)).unwrap();
    }
    c.watch_events();
    let id = s.c.spool_add(bob_file("WATCHED", 1, "QPRINT"));
    let evs = events_until(&mut c, Duration::from_secs(5), |e| {
        spool_changes(e).len() >= 2
    });
    let mut got = spool_changes(&evs);
    got.sort_by_key(|(t, _, _)| format!("{t:?}"));
    assert_eq!(
        got,
        [
            (queue("QPRINT"), SpoolChange::Added, "WATCHED".into()),
            (mine.clone(), SpoolChange::Added, "WATCHED".into()),
        ]
    );
    // Held: one Changed each.
    assert!(s.c.spool_change(&id, |f| f.status = SpoolStatus::Held));
    let evs = events_until(&mut c, Duration::from_secs(5), |e| {
        spool_changes(e).len() >= 2
    });
    assert!(
        spool_changes(&evs)
            .iter()
            .all(|(_, ch, _)| *ch == SpoolChange::Changed)
    );
    let held = evs
        .iter()
        .find_map(|e| match e {
            WatchEvent::SpoolChanged(c) => c.file.clone(),
            _ => None,
        })
        .unwrap();
    assert_eq!(held.status, SpoolStatus::Held);
    // Moved between two watched queues: Removed from one, Added to the
    // other; the user's target sees it Changed.
    assert!(s.c.spool_change(&id, |f| f.outq.name = "PRT01".into()));
    let evs = events_until(&mut c, Duration::from_secs(5), |e| {
        spool_changes(e).len() >= 3
    });
    let got = spool_changes(&evs);
    assert!(
        got.contains(&(queue("QPRINT"), SpoolChange::Removed, "WATCHED".into())),
        "{got:?}"
    );
    assert!(
        got.contains(&(queue("PRT01"), SpoolChange::Added, "WATCHED".into())),
        "{got:?}"
    );
    assert!(
        got.contains(&(mine.clone(), SpoolChange::Changed, "WATCHED".into())),
        "{got:?}"
    );
    let removed_at = got
        .iter()
        .position(|(t, ch, _)| *t == queue("QPRINT") && *ch == SpoolChange::Removed);
    let added_at = got
        .iter()
        .position(|(t, ch, _)| *t == queue("PRT01") && *ch == SpoolChange::Added);
    assert!(removed_at < added_at, "Removed then Added: {got:?}");
    // Deleted.
    assert!(s.c.spool_remove(&id));
    let evs = events_until(&mut c, Duration::from_secs(5), |e| {
        spool_changes(e).len() >= 2
    });
    let got = spool_changes(&evs);
    assert!(
        got.iter().all(|(_, ch, _)| *ch == SpoolChange::Removed),
        "{got:?}"
    );
    let removed = evs.iter().find_map(|e| match e {
        WatchEvent::SpoolChanged(c) => Some(c.file.is_none()),
        _ => None,
    });
    assert_eq!(removed, Some(true), "no file for a removed one");
}

#[test]
fn spool_watches_are_watched_again_after_a_resume() {
    let s = start(quick());
    let mut c = bob(&s);
    let mine = SpoolTarget::User {
        profile: "*CURRENT".into(),
    };
    c.watch(Watch::Spool(mine.clone())).unwrap();
    c.watch_events();
    s.c.drop_all();
    until("the client to see the close", || {
        c.connection().is_none_or(|n| n.closed().is_some())
    });
    assert!(c.poll().contains(&Notice::Resumed));
    let evs = c.watch_events();
    assert!(
        evs.contains(&WatchEvent::SpoolRewatched(mine.clone())),
        "{evs:?}"
    );
    s.c.spool_add(bob_file("AFTER", 2, "QPRINT"));
    let evs = events_until(&mut c, Duration::from_secs(5), |e| {
        !spool_changes(e).is_empty()
    });
    assert_eq!(
        spool_changes(&evs),
        [(mine, SpoolChange::Added, "AFTER".into())]
    );
}

#[test]
fn a_new_sign_on_watches_again_and_catches_up() {
    let s = start(quick());
    let mut c = bob(&s);
    c.watch(Watch::Queue(Queue::Own)).unwrap();
    c.watch_events();
    // The session ends; the watches stay with the client.
    c.sign_off().unwrap();
    assert!(
        c.watching().contains(&Watch::Queue(Queue::Own)),
        "kept to sign on again"
    );
    s.c.inject_message(
        "QUSRSYS",
        "BOB",
        from_elsewhere("While away.", later(&s, 0)),
    )
    .unwrap();
    // A new client for the system takes the watches over.
    let mut again = Client::new(s.target());
    again.adopt_watches(c.take_watches());
    again.sign_on("BOB", &Secret::from("SECRET1")).unwrap();
    let evs = again.watch_events();
    assert_eq!(texts(&evs), ["While away."]);
    assert!(evs.contains(&WatchEvent::CaughtUp {
        queue: Queue::Own,
        count: 1
    }));
    // As another profile, `Own` is another queue: nothing to catch up.
    again.sign_off().unwrap();
    let mut ann = Client::new(s.target());
    ann.adopt_watches(again.take_watches());
    ann.sign_on("ANN", &Secret::from("ANNPW01")).unwrap();
    let evs = ann.watch_events();
    assert!(arrived(&evs).is_empty(), "{evs:?}");
    assert!(evs.contains(&WatchEvent::Watching(Watch::Queue(Queue::Own))));
}

#[test]
fn a_catch_up_longer_than_a_page() {
    let s = start(quick());
    let mut c = bob(&s);
    c.watch(Watch::Queue(Queue::Own)).unwrap();
    c.watch_events();
    s.c.drop_all();
    until("the client to see the close", || {
        c.connection().is_none_or(|n| n.closed().is_some())
    });
    let at = later(&s, 0);
    for i in 0..250 {
        s.c.inject_message(
            "QUSRSYS",
            "BOB",
            from_elsewhere(&format!("Gap {i:03}."), at + i),
        )
        .unwrap();
    }
    assert!(c.poll().contains(&Notice::Resumed));
    let evs = c.watch_events();
    let want: Vec<String> = (0..250).map(|i| format!("Gap {i:03}.")).collect();
    assert_eq!(texts(&evs), want);
    assert!(evs.contains(&WatchEvent::CaughtUp {
        queue: Queue::Own,
        count: 250
    }));
}

#[test]
fn a_catch_up_whose_newest_key_was_removed_stops_by_time() {
    let s = start(quick());
    let mut c = bob(&s);
    c.watch(Watch::Queue(Queue::Own)).unwrap();
    c.watch_events();
    let at = later(&s, 0);
    let last =
        s.c.inject_message("QUSRSYS", "BOB", from_elsewhere("Newest known.", at))
            .unwrap();
    let evs = events_until(&mut c, Duration::from_secs(5), |e| !arrived(e).is_empty());
    assert_eq!(texts(&evs), ["Newest known."]);
    s.c.drop_all();
    until("the client to see the close", || {
        c.connection().is_none_or(|n| n.closed().is_some())
    });
    // The newest message the client had goes, and two arrive.
    {
        let mut qs = s.c.messages.lock().unwrap();
        let i = qs.find("QUSRSYS", "BOB").unwrap();
        qs.queues[i]
            .msgs
            .retain(|m| m.key != u32::from_be_bytes(last));
    }
    s.c.inject_message("QUSRSYS", "BOB", from_elsewhere("Gap one.", at + 1))
        .unwrap();
    s.c.inject_message("QUSRSYS", "BOB", from_elsewhere("Gap two.", at + 2))
        .unwrap();
    assert!(c.poll().contains(&Notice::Resumed));
    let evs = c.watch_events();
    assert_eq!(texts(&evs), ["Gap one.", "Gap two."], "nothing older again");
}

#[test]
fn a_not_authorized_watch_is_reported_and_taken_away() {
    let s = start(quick());
    let mut c = bob(&s);
    c.watch(Watch::Queue(Queue::SystemOperator)).unwrap();
    c.watch_events();
    s.c.drop_all();
    until("the client to see the close", || {
        c.connection().is_none_or(|n| n.closed().is_some())
    });
    // While away, QSYSOPR became ANN's alone.
    {
        let mut qs = s.c.messages.lock().unwrap();
        let i = qs.find("QSYS", "QSYSOPR").unwrap();
        qs.queues[i].users = vec!["ANN".into()];
    }
    assert!(c.poll().contains(&Notice::Resumed));
    let evs = c.watch_events();
    let failed = evs.iter().find_map(|e| match e {
        WatchEvent::Failed { watch, failure } => Some((watch.clone(), failure.clone())),
        _ => None,
    });
    let (watch, failure) = failed.expect("reported");
    assert_eq!(watch, Watch::Queue(Queue::SystemOperator));
    assert!(failure.not_authorized());
    assert!(c.watching().is_empty());
    assert_eq!(c.messages_waiting(), 0);
}

#[test]
fn watches_are_refused_before_version_6() {
    let s = start(quick());
    let mut t = s.target();
    t.options.max_version = 5;
    let mut c = Client::new(t);
    c.sign_on("BOB", &Secret::from("SECRET1")).unwrap();
    assert_eq!(c.version(), Some(5));
    let e = c.watch(Watch::Queue(Queue::Own)).unwrap_err();
    assert!(matches!(
        e,
        qap_client::Error::Unsupported {
            op: Op::WatchQueue,
            needs: 6,
            has: 5
        }
    ));
    let e = c.list_messages(&Queue::Own, true, 0, 1).unwrap_err();
    assert!(matches!(e, qap_client::Error::Unsupported { needs: 6, .. }));
    assert!(c.watching().is_empty());
}
