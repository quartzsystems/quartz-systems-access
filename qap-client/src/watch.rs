// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! Version 6's watches, owned by the [`Client`] on its shared connection:
//! message queues (`WatchQueue`) and spool targets (`WatchSpool`).
//!
//! Watches are passive on the server: never `InUse`, never in the way of
//! a 5250 session holding a queue in break mode. They report what happens
//! after their `Ok`, about every 250 ms, and end with their connection and
//! with a `SignOff` on it; the server never restores them, `Resume` does
//! not either. So the client keeps them:
//!
//! - [`Client::watch`] adds one and [`Client::unwatch`] takes one away.
//!   The first watch of a queue lists it whole once (newest first, 500 at
//!   a time) for its newest key and time and for the inquiries waiting.
//! - After every `Resume` and every new sign-on the client watches each
//!   again, on whatever thread brought the connection back. For a message
//!   queue it then lists newest first, [`CATCH_UP_PAGE`] at a time, until
//!   the newest key it had; when that key is no longer on the queue, until
//!   the first message older than the newest `sent` it had. What it finds
//!   it reports oldest first, as if each had arrived, and how many
//!   ([`WatchEvent::CaughtUp`]), then counts the inquiries waiting again.
//!   A spool watch is reported as watched again
//!   ([`WatchEvent::SpoolRewatched`]) for its window to refresh once.
//! - Every message reported is reported once: by key, per watch, whether
//!   it came as an event, in a catch-up, or both.
//! - A failure is an event: `NotAuthorized` takes the watch away (the
//!   caller says so and stops offering it); anything else leaves it to be
//!   tried at the next resume or sign-on.
//! - `Own` names the signed-on profile's queue: a sign-on as another
//!   profile starts its `Own` watches afresh.
//!
//! A watch is of the [`Queue`] or [`SpoolTarget`] value given, as the
//! server keeps them: `Own` and the same queue by name are two watches.

use std::collections::{BTreeSet, HashSet, VecDeque};

use qap_proto::ops::messages::{
    Key, ListMessages, MessageArrived, MessageType, Messages, Queue, QueueRef, QueuedMessage,
};
use qap_proto::ops::spool::{SpoolTarget, SpooledFileChanged, WatchSpool};
use qap_proto::{Op, ServerMessage, Status};

use crate::Error;
use crate::conn::Connection;

/// Messages per `ListMessages` of a catch-up.
pub const CATCH_UP_PAGE: u32 = 100;

/// Messages per `ListMessages` of a whole-queue look.
pub const SCAN_PAGE: u32 = 500;

/// Keys remembered per queue to drop what was reported already.
pub const SEEN_MAX: usize = 16_384;

/// Something watched.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Watch {
    /// A message queue.
    Queue(Queue),
    /// The spooled files of an output queue or of a user.
    Spool(SpoolTarget),
}

impl Watch {
    /// The operation that starts it.
    pub fn op(&self) -> Op {
        match self {
            Watch::Queue(_) => Op::WatchQueue,
            Watch::Spool(_) => Op::WatchSpool,
        }
    }
}

/// Why a watch failed, as the system said.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Failure {
    /// The status, when the system refused.
    pub status: Option<Status>,
    /// The system's message, when it sent one.
    pub message: Option<ServerMessage>,
    /// The failure in one line.
    pub text: String,
}

impl From<&Error> for Failure {
    fn from(e: &Error) -> Failure {
        Failure {
            status: e.status(),
            message: e.server_message().cloned(),
            text: e.to_string(),
        }
    }
}

impl Failure {
    /// The profile may not read what is watched.
    pub fn not_authorized(&self) -> bool {
        self.status == Some(Status::NotAuthorized)
    }
}

/// What the watches report, oldest first.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WatchEvent {
    /// The watch works on the current connection: the first time, or again
    /// after a resume or a sign-on.
    Watching(Watch),
    /// The watch failed. `NotAuthorized` has taken it away; anything else
    /// is tried again at the next resume or sign-on.
    Failed { watch: Watch, failure: Failure },
    /// A message, reported once: from an event, or found by a catch-up.
    Arrived {
        queue: Queue,
        message: QueuedMessage,
        caught_up: bool,
    },
    /// A catch-up after a resume or a sign-on found `count` messages.
    CaughtUp { queue: Queue, count: usize },
    /// A spooled file of a watched target changed.
    SpoolChanged(SpooledFileChanged),
    /// A spool watch was watched again after a resume or a sign-on; what
    /// changed in between was not reported.
    SpoolRewatched(SpoolTarget),
}

/// Keys reported, the newest [`SEEN_MAX`] of them.
#[derive(Clone, Debug, Default)]
struct Seen {
    set: HashSet<Key>,
    order: VecDeque<Key>,
}

impl Seen {
    /// True the first time `k` is seen.
    fn insert(&mut self, k: Key) -> bool {
        if !self.set.insert(k) {
            return false;
        }
        self.order.push_back(k);
        if self.order.len() > SEEN_MAX
            && let Some(old) = self.order.pop_front()
        {
            self.set.remove(&old);
        }
        true
    }
}

/// One watched queue.
#[derive(Clone, Debug)]
struct QueueWatch {
    queue: Queue,
    /// Watched on the current connection.
    live: bool,
    /// Listed whole once since it was added (or since `Own` changed
    /// profile): what came after is new.
    scanned: bool,
    /// The newest message known, by arrival: its key and `sent`.
    newest: Option<(Key, i64)>,
    seen: Seen,
    /// Inquiries waiting for a reply.
    waiting: BTreeSet<Key>,
}

impl QueueWatch {
    fn new(queue: Queue) -> QueueWatch {
        QueueWatch {
            queue,
            live: false,
            scanned: false,
            newest: None,
            seen: Seen::default(),
            waiting: BTreeSet::new(),
        }
    }

    /// Takes in `m`, arrived after what is known; true when it is new.
    fn take(&mut self, m: &QueuedMessage) -> bool {
        if !self.seen.insert(m.key) {
            return false;
        }
        self.newest = Some((m.key, m.sent));
        if m.kind == MessageType::Inquiry && m.needs_reply {
            self.waiting.insert(m.key);
        }
        if m.kind == MessageType::Reply
            && let Some(k) = m.answers
        {
            self.waiting.remove(&k);
        }
        true
    }
}

/// One watched spool target.
#[derive(Clone, Debug)]
struct SpoolWatch {
    target: SpoolTarget,
    live: bool,
    /// Watched on an earlier connection: watching it now is watching it
    /// again, with changes missed in between.
    watched_before: bool,
}

/// Every watch of a system, and what they reported not yet taken. Kept by
/// the [`crate::Client`]; handed to a new client for the same system
/// ([`crate::Client::adopt_watches`]) so that a new sign-on catches up.
#[derive(Clone, Debug, Default)]
pub struct Watches {
    queues: Vec<QueueWatch>,
    spools: Vec<SpoolWatch>,
    events: VecDeque<WatchEvent>,
    /// The profile `Own` was watched as.
    profile: Option<String>,
    /// Watch everything again once the connection is signed on.
    stale: bool,
}

impl Watches {
    /// Everything watched, live or not.
    pub fn all(&self) -> Vec<Watch> {
        let q = self.queues.iter().map(|q| Watch::Queue(q.queue.clone()));
        let s = self.spools.iter().map(|s| Watch::Spool(s.target.clone()));
        q.chain(s).collect()
    }

    /// Whether `w` is kept, live or not.
    pub fn has(&self, w: &Watch) -> bool {
        match w {
            Watch::Queue(q) => self.queues.iter().any(|x| x.queue == *q),
            Watch::Spool(t) => self.spools.iter().any(|x| x.target == *t),
        }
    }

    /// Whether `w` is watched on the current connection.
    pub fn is_live(&self, w: &Watch) -> bool {
        match w {
            Watch::Queue(q) => self.queues.iter().any(|x| x.queue == *q && x.live),
            Watch::Spool(t) => self.spools.iter().any(|x| x.target == *t && x.live),
        }
    }

    /// Inquiries waiting for a reply across every watched queue, each
    /// queue's counted once even when two watches name it.
    pub fn waiting(&self) -> usize {
        let mut counted: Vec<&Queue> = Vec::new();
        let mut n = 0;
        for q in &self.queues {
            if q.scanned && !counted.contains(&&q.queue) {
                counted.push(&q.queue);
                n += q.waiting.len();
            }
        }
        n
    }

    /// Inquiries waiting on `queue`, when it is watched.
    pub fn waiting_on(&self, queue: &Queue) -> Option<usize> {
        self.queues
            .iter()
            .find(|q| q.queue == *queue && q.scanned)
            .map(|q| q.waiting.len())
    }

    /// The keys of the inquiries waiting on `queue`, when it is watched.
    pub fn waiting_keys(&self, queue: &Queue) -> Option<Vec<Key>> {
        self.queues
            .iter()
            .find(|q| q.queue == *queue && q.scanned)
            .map(|q| q.waiting.iter().copied().collect())
    }

    /// What was reported since the last look, oldest first.
    pub fn take_events(&mut self) -> Vec<WatchEvent> {
        self.events.drain(..).collect()
    }

    /// Whether anything is waiting to be taken.
    pub fn has_events(&self) -> bool {
        !self.events.is_empty()
    }

    /// The connection closed: no watch is live on it.
    pub(crate) fn connection_gone(&mut self) {
        for q in &mut self.queues {
            q.live = false;
        }
        for s in &mut self.spools {
            s.live = false;
        }
    }

    /// A new connection signed on or resumed as `profile`: every watch is to
    /// be watched again. `Own` as another profile is another queue.
    pub(crate) fn signed_on(&mut self, profile: &str) {
        self.connection_gone();
        let same = self
            .profile
            .as_deref()
            .is_some_and(|p| p.trim().eq_ignore_ascii_case(profile.trim()));
        if !same {
            for q in self.queues.iter_mut().filter(|q| q.queue == Queue::Own) {
                *q = QueueWatch::new(Queue::Own);
            }
            self.profile = Some(profile.trim().to_owned());
        }
        self.stale = !self.queues.is_empty() || !self.spools.is_empty();
    }

    /// Whether the watches wait to be watched again.
    pub(crate) fn stale(&self) -> bool {
        self.stale
    }

    /// Inquiries answered or removed on `queue` by this client.
    pub(crate) fn answered(&mut self, queue: &Queue, keys: &[Key]) {
        for q in self.queues.iter_mut().filter(|q| q.queue == *queue) {
            for k in keys {
                q.waiting.remove(k);
            }
        }
    }

    /// `MessageArrived` from the connection.
    pub(crate) fn arrived(&mut self, ev: MessageArrived) {
        let Some(q) = self.queues.iter_mut().find(|q| q.queue == ev.queue) else {
            return;
        };
        // Before the first look the watch is not reported to anyone yet;
        // the look takes it in.
        if q.scanned && q.take(&ev.message) {
            self.events.push_back(WatchEvent::Arrived {
                queue: ev.queue,
                message: ev.message,
                caught_up: false,
            });
        }
    }

    /// `SpooledFileChanged` from the connection.
    pub(crate) fn spool_changed(&mut self, ev: SpooledFileChanged) {
        if self.spools.iter().any(|s| s.target == ev.target) {
            self.events.push_back(WatchEvent::SpoolChanged(ev));
        }
    }

    /// Adds `w` and watches it on `c`. Already live: nothing to do. A watch
    /// added now that fails is not kept: the caller has the error.
    pub(crate) fn add(&mut self, c: &Connection, w: Watch) -> Result<(), Error> {
        if self.is_live(&w) {
            return Ok(());
        }
        let new = !self.has(&w);
        if new {
            match &w {
                Watch::Queue(q) => self.queues.push(QueueWatch::new(q.clone())),
                Watch::Spool(t) => self.spools.push(SpoolWatch {
                    target: t.clone(),
                    live: false,
                    watched_before: false,
                }),
            }
        }
        let r = self.establish(c, &w);
        if let Err(e) = &r
            && (new || e.status() == Some(Status::NotAuthorized))
        {
            self.forget(&w);
        }
        r
    }

    /// Takes `w` away; true when it was live on the connection, which is
    /// then to be told.
    pub(crate) fn remove(&mut self, w: &Watch) -> bool {
        let live = self.is_live(w);
        self.forget(w);
        live
    }

    fn forget(&mut self, w: &Watch) {
        match w {
            Watch::Queue(q) => self.queues.retain(|x| x.queue != *q),
            Watch::Spool(t) => self.spools.retain(|x| x.target != *t),
        }
    }

    /// Watches everything again on `c`, catching up, and reports how each
    /// went. Stops at a connection that closed, to be tried at the next
    /// resume; true when everything was tried.
    pub(crate) fn rewatch(&mut self, c: &Connection) -> bool {
        self.stale = false;
        for w in self.all() {
            if self.is_live(&w) {
                continue;
            }
            match self.establish(c, &w) {
                Ok(()) => {}
                Err(e @ Error::Closed(_)) => {
                    qsa_net::debug!("qap: watching {w:?} again: {e}");
                    self.stale = true;
                    return false;
                }
                Err(e) => {
                    let failure = Failure::from(&e);
                    if failure.not_authorized() {
                        self.forget(&w);
                    }
                    self.events
                        .push_back(WatchEvent::Failed { watch: w, failure });
                }
            }
        }
        true
    }

    /// Sends `w`'s watch on `c` and, for a queue, looks at it: the whole
    /// queue the first time, a catch-up after that.
    fn establish(&mut self, c: &Connection, w: &Watch) -> Result<(), Error> {
        match w {
            Watch::Spool(t) => {
                c.call::<_, ()>(Op::WatchSpool, &WatchSpool { target: t.clone() })?;
                let s = self
                    .spools
                    .iter_mut()
                    .find(|s| s.target == *t)
                    .expect("added before it is watched");
                s.live = true;
                if std::mem::replace(&mut s.watched_before, true) {
                    self.events.push_back(WatchEvent::SpoolRewatched(t.clone()));
                }
                self.events.push_back(WatchEvent::Watching(w.clone()));
                Ok(())
            }
            Watch::Queue(q) => {
                c.call::<_, ()>(Op::WatchQueue, &QueueRef { queue: q.clone() })?;
                let i = self
                    .queues
                    .iter()
                    .position(|x| x.queue == *q)
                    .expect("added before it is watched");
                self.queues[i].live = true;
                if self.queues[i].scanned {
                    let count = self.catch_up(c, i)?;
                    self.events.push_back(WatchEvent::CaughtUp {
                        queue: q.clone(),
                        count,
                    });
                }
                self.scan(c, i)?;
                self.events.push_back(WatchEvent::Watching(w.clone()));
                Ok(())
            }
        }
    }

    /// What arrived on queue `i` while it was not watched: newest first
    /// until the newest key known, or, when that key is gone, until the
    /// first message older than the newest `sent` known. Reported oldest
    /// first, each once; how many.
    fn catch_up(&mut self, c: &Connection, i: usize) -> Result<usize, Error> {
        let queue = self.queues[i].queue.clone();
        let newest = self.queues[i].newest;
        let mut found = Vec::new();
        let mut start = 0u32;
        'pages: loop {
            let page: Messages = list(c, &queue, true, start, CATCH_UP_PAGE)?;
            let n = u32::try_from(page.messages.len()).unwrap_or(u32::MAX);
            for m in page.messages {
                if let Some((key, sent)) = newest
                    && (m.key == key || m.sent < sent)
                {
                    break 'pages;
                }
                found.push(m);
            }
            start = start.saturating_add(n);
            if n == 0 || start >= page.total {
                break;
            }
        }
        let q = &mut self.queues[i];
        let mut count = 0;
        for m in found.into_iter().rev() {
            if q.take(&m) {
                count += 1;
                self.events.push_back(WatchEvent::Arrived {
                    queue: queue.clone(),
                    message: m,
                    caught_up: true,
                });
            }
        }
        Ok(count)
    }

    /// Lists queue `i` whole, newest first, for its newest message and the
    /// inquiries waiting. Messages listed count as reported. The first look
    /// takes what is there as known; a look after a catch-up recounts.
    fn scan(&mut self, c: &Connection, i: usize) -> Result<(), Error> {
        let queue = self.queues[i].queue.clone();
        let mut waiting = BTreeSet::new();
        let mut newest = None;
        let mut keys = Vec::new();
        let mut start = 0u32;
        loop {
            let page: Messages = list(c, &queue, true, start, SCAN_PAGE)?;
            let n = u32::try_from(page.messages.len()).unwrap_or(u32::MAX);
            for m in &page.messages {
                if newest.is_none() {
                    newest = Some((m.key, m.sent));
                }
                if m.kind == MessageType::Inquiry && m.needs_reply {
                    waiting.insert(m.key);
                }
                keys.push(m.key);
            }
            start = start.saturating_add(n);
            if n == 0 || start >= page.total {
                break;
            }
        }
        let q = &mut self.queues[i];
        // Oldest first, so the newest stay remembered.
        for k in keys.into_iter().rev() {
            q.seen.insert(k);
        }
        // The newest by time: an event taken during the look may be newer
        // than what the look saw first. An empty queue keeps what was
        // known, so a catch-up stops at its time.
        if let Some((k, sent)) = newest
            && q.newest.is_none_or(|(_, known)| sent >= known)
        {
            q.newest = Some((k, sent));
        }
        q.waiting = waiting;
        q.scanned = true;
        Ok(())
    }
}

/// `ListMessages` on `c`.
fn list(
    c: &Connection,
    queue: &Queue,
    newest_first: bool,
    start: u32,
    max: u32,
) -> Result<Messages, Error> {
    c.call(
        Op::ListMessages,
        &ListMessages {
            queue: queue.clone(),
            newest_first,
            start,
            max,
        },
    )
}
