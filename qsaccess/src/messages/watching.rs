// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! The message watches this program keeps on each system's shared
//! connection, and what their reports do.
//!
//! - While signed on to a system whose QAP version has watches (6), the
//!   profile's own queue (`*Own`) is watched; `QSYSOPR` too when chosen in
//!   Messages > Options. The client watches them again itself after a
//!   resume or a new sign-on and catches up ([`qap_client::watch`]).
//! - Reports: `QAD9010` watching (the log), `QAD9011` a watch that
//!   stopped, `QAD9012` `QSYSOPR` refused (its option turned off),
//!   `QAD9013` messages caught up after reconnecting.
//! - An arrival opens Message Arrived as its queue's option says
//!   ([`super::model::opens_arrived`]); a reply marks the inquiry it
//!   answers as answered there.
//! - The main window's status bar shows "Messages waiting: N" for the
//!   selected system while it has watches: the inquiries waiting for a
//!   reply on its watched queues.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;

use fltk::prelude::*;
use qap_client::proto::Op;
use qap_client::proto::ops::messages::Queue;
use qap_client::watch::{Failure, Watch, WatchEvent};
use qap_client::{Error, Status};
use qsa_config::System;
use qsa_config::settings::Arrival;
use qsa_msg::{Message, id};

use crate::app::Shared;
use crate::mainwin::Ui;
use crate::qap::{self, Handle, lock};

use super::{arrived, model};

thread_local! {
    /// The client and profile each system's watches were set up for, by
    /// system name in lower case.
    static SET_UP: RefCell<HashMap<String, (usize, String)>> = RefCell::new(HashMap::new());
}

/// A watch's name in messages: its queue's, `Own` as the profile; a spool
/// target's output queue, or whose spooled files.
pub fn watch_name(w: &Watch, profile: &str) -> String {
    use qap_client::proto::ops::spool::SpoolTarget;
    match w {
        Watch::Queue(q) => model::queue_name(q, profile),
        Watch::Spool(SpoolTarget::Queue { outq }) => {
            format!("{}/{}", outq.library.trim(), outq.name.trim())
        }
        Watch::Spool(SpoolTarget::User { profile: p }) => {
            let who = if p.trim().eq_ignore_ascii_case("*CURRENT") {
                profile.trim()
            } else {
                p.trim()
            };
            format!("{who}'s spooled files")
        }
    }
}

/// Logs `m` and shows it on the main window's status bar.
fn report(ui: &Ui, m: &Message) {
    crate::log!("{m}");
    ui.report(m);
}

/// Sets up `system`'s watches once per client and profile: the own queue,
/// and `QSYSOPR` when chosen. On the GUI thread, between other work; the
/// watch requests run off it.
pub fn upkeep(ui: &Ui, system: &System, h: &Handle) {
    let (profile, watches) = {
        let Ok(mut c) = h.try_lock() else { return };
        let Some(p) = c.profile().map(|p| p.profile.clone()) else {
            return;
        };
        (p, c.supports(Op::WatchQueue).is_ok())
    };
    let k = system.name.to_ascii_lowercase();
    let this = (Arc::as_ptr(h) as usize, profile.clone());
    let done = SET_UP.with(|s| s.borrow().get(&k) == Some(&this));
    if done {
        return;
    }
    SET_UP.with(|s| s.borrow_mut().insert(k, this));
    if !watches {
        return;
    }
    let o = ui.state.borrow().settings.messages.for_system(&system.name);
    let (own, operator) = qap::busy(|| {
        let mut c = lock(h);
        let own = c.watch(Watch::Queue(Queue::Own));
        let operator = o
            .operator
            .then(|| c.watch(Watch::Queue(Queue::SystemOperator)));
        (own, operator)
    });
    if let Err(e) = own {
        stopped(
            ui,
            system,
            &Watch::Queue(Queue::Own),
            &profile,
            &Failure::from(&e),
        );
    }
    if let Some(Err(e)) = operator {
        let w = Watch::Queue(Queue::SystemOperator);
        if e.status() == Some(Status::NotAuthorized) {
            not_authorized(&ui.state, system);
            report(ui, &operator_refused(&system.name));
        } else {
            stopped(ui, system, &w, &profile, &Failure::from(&e));
        }
    }
}

/// `QAD9012` for `QSYSOPR` on `system`.
fn operator_refused(system: &str) -> Message {
    qsa_msg::message(id::WATCH_NOT_AUTHORIZED, &["QSYSOPR", system])
}

/// The profile may not read `QSYSOPR` on `system`: its option is turned
/// off and kept.
fn not_authorized(state: &Shared, system: &System) {
    let mut st = state.borrow_mut();
    let mut o = st.settings.messages.for_system(&system.name);
    if !o.operator {
        return;
    }
    o.operator = false;
    st.settings.messages.set(&system.name, o);
    if let Err(e) = st.save_settings() {
        crate::log!("settings not saved: {e}");
    }
}

/// `QAD9011`: `w` stopped.
fn stopped(ui: &Ui, system: &System, w: &Watch, profile: &str, f: &Failure) {
    let why = match &f.message {
        Some(m) => format!("{}: {}", m.id, m.text.trim_end_matches('.')),
        None => f.text.trim_end_matches('.').to_owned(),
    };
    let m = qsa_msg::message(
        id::WATCH_STOPPED,
        &[&watch_name(w, profile), &system.name, &why],
    );
    report(ui, &m);
}

/// Messages > Options turned `QSYSOPR` on or off for `system`: watched or
/// no longer, at once. A refusal turns it off again with `QAD9012`.
pub fn operator(parent: Option<&dyn WindowExt>, state: &Shared, system: &str, on: bool) {
    let Some(h) = state.borrow_mut().qap.signed_on_client(system) else {
        return;
    };
    let w = Watch::Queue(Queue::SystemOperator);
    let r: Result<(), Error> = qap::busy(|| {
        let mut c = lock(&h);
        if on {
            c.watch(w.clone())
        } else {
            c.unwatch(&w)
        }
    });
    match r {
        Ok(()) => {}
        Err(e) if e.status() == Some(Status::NotAuthorized) => {
            let sys = state.borrow().systems.get(system).cloned();
            if let Some(sys) = sys {
                not_authorized(state, &sys);
            }
            let m = operator_refused(system);
            crate::log!("{m}");
            qsa_ui::msgdetails::show_message(parent, &m);
        }
        Err(e) => {
            let sys = state
                .borrow()
                .systems
                .get(system)
                .cloned()
                .unwrap_or_else(|| System::new(system));
            qap::show(parent, &qap::explain(&sys, &e));
        }
    }
}

/// What `system`'s watches reported, for the log, the status bar and
/// Message Arrived.
pub fn events(ui: &Ui, system: &System, profile: &str, events: &[WatchEvent]) {
    let o = ui.state.borrow().settings.messages.for_system(&system.name);
    for e in events {
        match e {
            WatchEvent::Watching(w) => {
                let m = qsa_msg::message(id::WATCHING, &[&watch_name(w, profile), &system.name]);
                crate::log!("{m}");
            }
            WatchEvent::Failed { watch, failure } => {
                if failure.not_authorized() && *watch == Watch::Queue(Queue::SystemOperator) {
                    not_authorized(&ui.state, system);
                    report(ui, &operator_refused(&system.name));
                } else {
                    stopped(ui, system, watch, profile, failure);
                }
            }
            WatchEvent::CaughtUp { queue, count } if *count > 0 => {
                let m = qsa_msg::message(
                    id::CAUGHT_UP,
                    &[&count.to_string(), &model::queue_name(queue, profile)],
                );
                report(ui, &m);
            }
            WatchEvent::Arrived { queue, message, .. } => {
                arrived::reply_arrived(&system.name, queue, message);
                let arrival = match queue {
                    Queue::Own => o.own_arrival,
                    Queue::SystemOperator if o.operator => o.operator_arrival,
                    _ => Arrival::None,
                };
                if model::opens_arrived(arrival, message) {
                    arrived::arrive(
                        &ui.state,
                        arrived::Item {
                            system: system.name.clone(),
                            queue: queue.clone(),
                            queue_name: model::queue_name(queue, profile),
                            message: message.clone(),
                        },
                    );
                }
            }
            _ => {}
        }
    }
}

/// The inquiries waiting on `system`'s watched queues, while it has
/// watches; `None` when it has none. A client busy elsewhere answers what
/// it last did.
pub fn waiting(state: &Shared, system: &str) -> Option<usize> {
    thread_local! {
        static LAST: RefCell<HashMap<String, Option<usize>>> = RefCell::new(HashMap::new());
    }
    let k = system.to_ascii_lowercase();
    let h = state.borrow().qap.get(system);
    let now = match h.as_ref().map(|h| h.try_lock()) {
        None => None,
        Some(Ok(c)) => (c.is_signed_on() && !c.watching().is_empty()).then(|| c.messages_waiting()),
        Some(Err(_)) => return LAST.with(|l| l.borrow().get(&k).copied().flatten()),
    };
    LAST.with(|l| l.borrow_mut().insert(k, now));
    now
}

/// The status bar's text for `n` inquiries waiting.
pub fn indicator_text(n: usize) -> String {
    format!("Messages waiting: {n}")
}
