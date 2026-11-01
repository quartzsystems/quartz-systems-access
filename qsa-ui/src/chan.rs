// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! Channels from widget callbacks to the code that owns the window.
//!
//! FLTK's `app::channel` is one queue for the whole process, and a `recv` of
//! the wrong type takes the message and throws it away. With more than one
//! window open, whichever receiver runs first eats the others' messages.
//! Each channel here has its own queue, so a window only ever sees its own.

use std::any::Any;
use std::collections::{BTreeMap, VecDeque};
use std::marker::PhantomData;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use fltk::app;
use fltk::enums::Shortcut;
use fltk::menu::MenuFlag;
use fltk::prelude::*;

type Queue = VecDeque<Box<dyn Any + Send>>;

static QUEUES: Mutex<BTreeMap<u64, Queue>> = Mutex::new(BTreeMap::new());
static NEXT: AtomicU64 = AtomicU64::new(1);

fn queues() -> std::sync::MutexGuard<'static, BTreeMap<u64, Queue>> {
    QUEUES.lock().unwrap_or_else(|e| e.into_inner())
}

/// Sends to one [`Receiver`] and wakes the event loop. Copy, like FLTK's, so
/// each callback can take its own.
pub struct Sender<T> {
    id: u64,
    _t: PhantomData<fn(T)>,
}

impl<T> Clone for Sender<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for Sender<T> {}

impl<T: Send + 'static> Sender<T> {
    /// Queues `v`. Does nothing once the receiver is gone.
    pub fn send(&self, v: T) {
        if let Some(q) = queues().get_mut(&self.id) {
            q.push_back(Box::new(v));
        }
        app::awake();
    }
}

/// The receiving end. Dropping it drops anything still queued.
pub struct Receiver<T> {
    id: u64,
    _t: PhantomData<fn() -> T>,
}

impl<T: 'static> Receiver<T> {
    /// The oldest message waiting, if any.
    pub fn recv(&self) -> Option<T> {
        let m = queues().get_mut(&self.id)?.pop_front()?;
        Some(
            *m.downcast::<T>()
                .expect("a channel carries only its own type"),
        )
    }
}

impl<T> Drop for Receiver<T> {
    fn drop(&mut self) {
        queues().remove(&self.id);
    }
}

/// A new channel with its own queue.
pub fn channel<T: Send + 'static>() -> (Sender<T>, Receiver<T>) {
    let id = NEXT.fetch_add(1, Ordering::Relaxed);
    queues().insert(id, VecDeque::new());
    (
        Sender {
            id,
            _t: PhantomData,
        },
        Receiver {
            id,
            _t: PhantomData,
        },
    )
}

/// `emit` for these channels: the widget's callback sends `v`.
pub trait Emit {
    fn emit_to<T: Clone + Send + 'static>(&mut self, tx: Sender<T>, v: T);
}

impl<W: WidgetBase> Emit for W {
    fn emit_to<T: Clone + Send + 'static>(&mut self, tx: Sender<T>, v: T) {
        self.set_callback(move |_| tx.send(v.clone()));
    }
}

/// `add_emit` for these channels: the menu item sends `v`.
pub trait MenuEmit {
    fn add_emit_to<T: Clone + Send + 'static>(
        &mut self,
        label: &str,
        shortcut: Shortcut,
        flag: MenuFlag,
        tx: Sender<T>,
        v: T,
    ) -> i32;
}

impl<M: MenuExt> MenuEmit for M {
    fn add_emit_to<T: Clone + Send + 'static>(
        &mut self,
        label: &str,
        shortcut: Shortcut,
        flag: MenuFlag,
        tx: Sender<T>,
        v: T,
    ) -> i32 {
        self.add(label, shortcut, flag, move |_| tx.send(v.clone()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_channel_keeps_its_own_messages() {
        let (ta, ra) = channel::<u8>();
        let (tb, rb) = channel::<u8>();
        let (tc, rc) = channel::<bool>();
        ta.send(1);
        tc.send(true);
        tb.send(2);
        ta.send(3);
        // Reading one channel never takes another's messages, even of the
        // same type.
        assert_eq!(rb.recv(), Some(2));
        assert_eq!(rb.recv(), None);
        assert_eq!(ra.recv(), Some(1));
        assert_eq!(ra.recv(), Some(3));
        assert_eq!(rc.recv(), Some(true));
        assert_eq!(ra.recv(), None);
    }

    #[test]
    fn sending_after_the_receiver_is_gone_does_nothing() {
        let (t, r) = channel::<u8>();
        drop(r);
        t.send(1);
    }
}
