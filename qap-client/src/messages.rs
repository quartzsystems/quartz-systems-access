// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! Version 6's `Messages` service: message queues, replies and sends, on
//! the shared connection.
//!
//! - [`Client::list_messages`]: one page of a queue, strictly newest first
//!   or oldest first, both by arrival. Keys are unique on a queue while the
//!   message exists.
//! - [`Client::reply_message`]: an empty reply is the message's default;
//!   one the message does not take is `Error` with `CPF2420`; a message
//!   that is not an inquiry waiting, or one already answered, is
//!   `InvalidRequest` with `CPF2422`; a key not on the queue is `NotFound`
//!   with `CPF2410`.
//! - [`Client::remove_messages`]: all or none (`NotFound`, `CPF2410`, when
//!   any key is missing). An inquiry removed before its reply gets none.
//! - [`Client::send_message`]: to a profile's queue or a queue. A `User`
//!   that never signed on has no queue yet (`NotFound`, `CPF2403`); an
//!   unknown profile is `CPF2204`. An inquiry's reply comes to the sender's
//!   own queue as a `Reply` whose `answers` is the key this returns ([`is_reply_to`]):
//!   the key of the sender's copy when the inquiry went to another queue,
//!   the inquiry's own when it went to the sender's own queue.
//!
//! Watches (`WatchQueue`, `WatchSpool`) are [`crate::watch`]'s: the client
//! owns them, watches again after a resume or a sign-on, and catches up.
//!
//! Every refusal is an [`Error::Status`] with the system's message as it
//! sent it.

use qap_proto::Op;
use qap_proto::ops::messages::{
    Key, ListMessages, MessageType, Messages, Queue, QueuedMessage, Recipient, RemoveMessages,
    ReplyMessage, SendMessage, Sent,
};

use crate::{Client, Error};

/// Messages per page of the Messages window.
pub const PAGE: u32 = 100;

/// A key as the system shows it: eight hexadecimal digits.
pub fn key_text(k: &Key) -> String {
    format!("{:08X}", u32::from_be_bytes(*k))
}

/// Whether `m` is the reply to the inquiry `SendMessage` answered `key`
/// for: a `Reply` whose `answers` is that key. Nothing else matches.
pub fn is_reply_to(m: &QueuedMessage, key: &Key) -> bool {
    m.kind == MessageType::Reply && m.answers.as_ref() == Some(key)
}

/// Whether `m` is an inquiry still waiting for its reply.
pub fn needs_reply(m: &QueuedMessage) -> bool {
    m.kind == MessageType::Inquiry && m.needs_reply
}

impl Client {
    /// `ListMessages` (version 6): `max` messages of `queue` from `start`
    /// (0-based), strictly newest first or oldest first; `max` 0 for no
    /// limit.
    pub fn list_messages(
        &mut self,
        queue: &Queue,
        newest_first: bool,
        start: u32,
        max: u32,
    ) -> Result<Messages, Error> {
        self.call(
            Op::ListMessages,
            &ListMessages {
                queue: queue.clone(),
                newest_first,
                start,
                max,
            },
        )
    }

    /// `ReplyMessage` (version 6): `reply` to the inquiry `key` on
    /// `queue`, empty for its default. The watch manager stops counting it
    /// as waiting.
    pub fn reply_message(&mut self, queue: &Queue, key: Key, reply: &str) -> Result<(), Error> {
        self.call::<_, ()>(
            Op::ReplyMessage,
            &ReplyMessage {
                queue: queue.clone(),
                key,
                reply: reply.to_owned(),
            },
        )?;
        self.watches_mut().answered(queue, &[key]);
        Ok(())
    }

    /// `RemoveMessages` (version 6): every one of `keys` from `queue`, or
    /// none of them when one is not there.
    pub fn remove_messages(&mut self, queue: &Queue, keys: &[Key]) -> Result<(), Error> {
        self.call::<_, ()>(
            Op::RemoveMessages,
            &RemoveMessages {
                queue: queue.clone(),
                keys: keys.to_vec(),
            },
        )?;
        self.watches_mut().answered(queue, keys);
        Ok(())
    }

    /// `SendMessage` (version 6): `text` to `to`; for an inquiry, the key
    /// its reply's `answers` will name on the sender's own queue.
    pub fn send_message(
        &mut self,
        to: &Recipient,
        text: &str,
        inquiry: bool,
    ) -> Result<Option<Key>, Error> {
        let sent: Sent = self.call(
            Op::SendMessage,
            &SendMessage {
                to: to.clone(),
                text: text.to_owned(),
                inquiry,
            },
        )?;
        Ok(sent.key)
    }

    /// Whether the profile may read `queue`: `ListMessages` of at most one
    /// message. `Ok(false)` for `NotAuthorized`; any other failure as it
    /// came.
    pub fn may_read_queue(&mut self, queue: &Queue) -> Result<bool, Error> {
        match self.list_messages(queue, true, 0, 1) {
            Ok(_) => Ok(true),
            Err(e) if e.status() == Some(qap_proto::Status::NotAuthorized) => Ok(false),
            Err(e) => Err(e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn msg(kind: MessageType, answers: Option<Key>) -> QueuedMessage {
        QueuedMessage {
            kind,
            answers,
            ..QueuedMessage::default()
        }
    }

    #[test]
    fn a_reply_matches_only_by_answers() {
        let k = [0, 0, 0, 7];
        assert!(is_reply_to(&msg(MessageType::Reply, Some(k)), &k));
        assert!(!is_reply_to(
            &msg(MessageType::Reply, Some([0, 0, 0, 8])),
            &k
        ));
        assert!(!is_reply_to(&msg(MessageType::Reply, None), &k));
        // A sender's copy with that key is the question, not the answer.
        let copy = QueuedMessage {
            key: k,
            ..msg(MessageType::SenderCopy, None)
        };
        assert!(!is_reply_to(&copy, &k));
        assert_eq!(key_text(&[0, 0, 0x01, 0xA2]), "000001A2");
    }
}
