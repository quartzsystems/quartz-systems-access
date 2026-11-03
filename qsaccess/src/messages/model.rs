// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! What the Messages windows show of a message, without a window: rows,
//! type words, queue names, the detail pane's text, and which messages a
//! Remove may take without asking again.

use qap_client::messages::{is_reply_to, needs_reply};
use qap_client::proto::ops::messages::{Key, MessageType, Queue, QueuedMessage};
use qap_client::proto::types::ObjectName;
use qsa_ui::Status;
use qsa_ui::listtable::Cell;

/// The list's columns: title and width.
pub const COLUMNS: [(&str, i32); 7] = [
    ("Sent", 118),
    ("Type", 90),
    ("Severity", 58),
    ("ID", 68),
    ("Message", 352),
    ("From user", 80),
    ("From job", 170),
];

/// The column whose click opens the message's Details.
pub const ID_COLUMN: usize = 3;

/// A message's type as the list shows it.
pub fn type_word(k: MessageType) -> &'static str {
    match k {
        MessageType::Informational => "Informational",
        MessageType::Inquiry => "Inquiry",
        MessageType::Reply => "Reply",
        MessageType::Completion => "Completion",
        MessageType::Diagnostic => "Diagnostic",
        MessageType::Escape => "Escape",
        MessageType::Notify => "Notify",
        MessageType::SenderCopy => "Sender copy",
    }
}

/// A job as the system names it: `number/user/name`; empty for none.
pub fn job_text(m: &QueuedMessage) -> String {
    let j = &m.from_job;
    if j.name.is_empty() && j.number.is_empty() {
        String::new()
    } else {
        crate::qap::job_name(j)
    }
}

/// The first line of a message's text, for the list.
pub fn first_line(text: &str) -> &str {
    text.lines().next().unwrap_or("").trim_end()
}

/// One row: `sent` formats the time.
pub fn row(m: &QueuedMessage, sent: &dyn Fn(i64) -> String) -> Vec<Cell> {
    vec![
        sent(m.sent).into(),
        Cell {
            text: type_word(m.kind).into(),
            status: if needs_reply(m) {
                Status::Error
            } else {
                Status::Neutral
            },
        },
        format!("{:02}", m.severity).into(),
        m.id.as_str().into(),
        first_line(&m.text).into(),
        m.from_user.as_str().into(),
        job_text(m).into(),
    ]
}

/// Which rows are bold: inquiries waiting for a reply.
pub fn bold(msgs: &[QueuedMessage]) -> Vec<bool> {
    msgs.iter().map(needs_reply).collect()
}

/// Sorts a page by column `col`; ties keep the list's order (newest first).
pub fn sort_page(msgs: &mut [QueuedMessage], col: usize, ascending: bool) {
    let key = |m: &QueuedMessage| -> (i64, String) {
        match col {
            0 => (m.sent, String::new()),
            1 => (0, type_word(m.kind).to_owned()),
            2 => (i64::from(m.severity), String::new()),
            3 => (0, m.id.clone()),
            4 => (0, m.text.to_lowercase()),
            5 => (0, m.from_user.clone()),
            _ => (0, job_text(m)),
        }
    };
    msgs.sort_by(|a, b| {
        let o = key(a).cmp(&key(b));
        if ascending { o } else { o.reverse() }
    });
}

/// A queue's name in messages and titles: the profile for `Own`, `QSYSOPR`,
/// or `LIBRARY/NAME`.
pub fn queue_name(q: &Queue, profile: &str) -> String {
    match q {
        Queue::Own => profile.trim().to_owned(),
        Queue::SystemOperator => "QSYSOPR".into(),
        Queue::Named { queue } => format!("{}/{}", queue.library.trim(), queue.name.trim()),
    }
}

/// The queue selector's entry for a queue.
pub fn queue_choice(q: &Queue, profile: &str) -> String {
    match q {
        Queue::Own => format!("My messages ({})", profile.trim()),
        Queue::SystemOperator => "System operator (QSYSOPR)".into(),
        Queue::Named { .. } => queue_name(q, profile),
    }
}

/// `LIBRARY/NAME` (or `NAME`, in `*LIBL`) as a queue; `None` when it is
/// not a name.
pub fn named_queue(text: &str) -> Option<Queue> {
    let t = text.trim().to_ascii_uppercase();
    let (lib, name) = match t.split_once('/') {
        Some((l, n)) => (l.trim().to_owned(), n.trim().to_owned()),
        None => ("*LIBL".to_owned(), t.clone()),
    };
    let ok = |s: &str| {
        !s.is_empty()
            && s.len() <= 10
            && s.chars()
                .all(|c| c.is_ascii_alphanumeric() || "$#@_.*".contains(c))
    };
    if !ok(&lib) || !ok(&name) || name.starts_with('*') {
        return None;
    }
    Some(Queue::Named {
        queue: ObjectName { library: lib, name },
    })
}

/// Whether Remove may take `m` without the separate confirmation: anything
/// but an inquiry waiting for its reply, or a sender's copy still waiting
/// (its reply would have nothing to match).
pub fn removable(m: &QueuedMessage) -> bool {
    !needs_reply(m) && !(m.kind == MessageType::SenderCopy && m.reply.is_none())
}

/// What the detail pane shows of `m`: its text, its second-level text as
/// sent, who sent it, and its reply once it has one.
pub fn detail(m: &QueuedMessage, sent: &dyn Fn(i64) -> String) -> String {
    let mut out = m.text.trim_end().to_owned();
    let help = m.help.trim_end();
    if !help.is_empty() {
        out.push_str("\n\n");
        out.push_str(help);
    }
    out.push_str("\n\n");
    let ident = if m.id.is_empty() {
        type_word(m.kind).to_owned()
    } else {
        format!("{} {}, severity {:02}", type_word(m.kind), m.id, m.severity)
    };
    out.push_str(&ident);
    out.push_str(&format!(
        "\nSent {} by {}",
        sent(m.sent),
        m.from_user.trim()
    ));
    let job = job_text(m);
    if !job.is_empty() {
        out.push_str(&format!(", job {job}"));
    }
    out.push('.');
    match (&m.kind, &m.reply) {
        (MessageType::Inquiry | MessageType::SenderCopy, Some(r)) => {
            out.push_str(&format!("\nAnswered: {r}"));
        }
        (MessageType::Inquiry, None) if m.needs_reply => {
            out.push_str("\nWaiting for a reply.");
        }
        (MessageType::SenderCopy, None) => out.push_str("\nNo reply yet."),
        _ => {}
    }
    out
}

/// The message a detail pane's link leads to, if any: for a `Reply`, the
/// message its `answers` names; for an inquiry or a sender's copy, the
/// `Reply` that answers it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Link {
    /// To the inquiry or sender's copy this reply answers.
    Answers(Key),
    /// To the reply that answers this message.
    AnsweredBy(Key),
}

impl Link {
    /// The link of `m`, if it has one.
    pub fn of(m: &QueuedMessage) -> Option<Link> {
        match m.kind {
            MessageType::Reply => m.answers.map(Link::Answers),
            MessageType::Inquiry | MessageType::SenderCopy if m.reply.is_some() => {
                Some(Link::AnsweredBy(m.key))
            }
            _ => None,
        }
    }

    /// Its button's label.
    pub fn label(self) -> &'static str {
        match self {
            Link::Answers(_) => "Go to the Message It Answers",
            Link::AnsweredBy(_) => "Go to Its Reply",
        }
    }

    /// Whether `m` is where it leads.
    pub fn leads_to(self, m: &QueuedMessage) -> bool {
        match self {
            Link::Answers(k) => m.key == k,
            Link::AnsweredBy(k) => is_reply_to(m, &k),
        }
    }
}

/// Whether `m`, arrived on a queue whose option is `arrival`, opens Message
/// Arrived. A sender's copy and a reply never do.
pub fn opens_arrived(arrival: qsa_config::settings::Arrival, m: &QueuedMessage) -> bool {
    use qsa_config::settings::Arrival;
    if matches!(m.kind, MessageType::SenderCopy | MessageType::Reply) {
        return false;
    }
    match arrival {
        Arrival::InquiriesOnly => needs_reply(m),
        Arrival::All => true,
        Arrival::None => false,
    }
}

/// The status bar's line: `QAD9001`, with the page's sort when it has one.
pub fn list_line(total: u32, queue: &str, sorted: Option<&str>) -> String {
    let m = qsa_msg::message(qsa_msg::id::MESSAGES_LISTED, &[&total.to_string(), queue]);
    match sorted {
        Some(c) => format!("{m} Sorted by {c} within this page only."),
        None => m.to_string(),
    }
}

/// "Messages 1-100 of 149, page 1 of 2"; "No messages" when empty.
pub fn paging_line(p: &qap_client::spool::Paging, shown: usize) -> String {
    let (page, pages) = p.page();
    match p.shown(shown) {
        Some((a, b)) => format!("Messages {a}-{b} of {}, page {page} of {pages}", p.total),
        None => "No messages".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::splf::model::utc_time;
    use qap_client::proto::types::JobId;

    fn msg(kind: MessageType) -> QueuedMessage {
        QueuedMessage {
            key: [0, 0, 0, 9],
            id: "CPA0701".into(),
            kind,
            severity: 99,
            text: "Verify alignment on device QPADEV0003. (C D I R)\nMore".into(),
            help: "Cause . . . . . :   The forms may not be aligned.".into(),
            sent: 1_793_000_000_000,
            from_user: "QSYS".into(),
            from_job: JobId {
                name: "QSYSARB".into(),
                user: "QSYS".into(),
                number: "000012".into(),
            },
            needs_reply: kind == MessageType::Inquiry,
            reply: None,
            answers: None,
        }
    }

    #[test]
    fn rows_type_words_and_bold() {
        let m = msg(MessageType::Inquiry);
        let r = row(&m, &utc_time);
        let t: Vec<&str> = r.iter().map(|c| c.text.as_str()).collect();
        assert_eq!(
            t,
            [
                "2026-10-26 07:33",
                "Inquiry",
                "99",
                "CPA0701",
                "Verify alignment on device QPADEV0003. (C D I R)",
                "QSYS",
                "000012/QSYS/QSYSARB",
            ]
        );
        assert_eq!(r[1].status, Status::Error);
        assert_eq!(type_word(MessageType::SenderCopy), "Sender copy");
        let copy = msg(MessageType::SenderCopy);
        assert_eq!(bold(&[m.clone(), copy.clone()]), [true, false]);
        let mut answered = m.clone();
        answered.needs_reply = false;
        answered.reply = Some("C".into());
        assert_eq!(bold(&[answered]), [false]);
    }

    #[test]
    fn queues_by_name() {
        assert_eq!(queue_name(&Queue::Own, "BOB"), "BOB");
        assert_eq!(queue_name(&Queue::SystemOperator, "BOB"), "QSYSOPR");
        let q = named_queue("qgpl/teamq").unwrap();
        assert_eq!(queue_name(&q, "BOB"), "QGPL/TEAMQ");
        assert_eq!(queue_choice(&Queue::Own, "BOB"), "My messages (BOB)");
        assert_eq!(
            queue_name(&named_queue("TEAMQ").unwrap(), ""),
            "*LIBL/TEAMQ"
        );
        for bad in ["", "/", "QGPL/", "A B/C", "QGPL/*ALL", "TOOLONGNAME1/X"] {
            assert!(named_queue(bad).is_none(), "{bad}");
        }
    }

    #[test]
    fn what_remove_takes_without_asking_again() {
        assert!(!removable(&msg(MessageType::Inquiry)));
        assert!(!removable(&msg(MessageType::SenderCopy)));
        let mut copy = msg(MessageType::SenderCopy);
        copy.reply = Some("OK".into());
        assert!(removable(&copy));
        for k in [
            MessageType::Informational,
            MessageType::Reply,
            MessageType::Completion,
            MessageType::Diagnostic,
            MessageType::Escape,
            MessageType::Notify,
        ] {
            assert!(removable(&msg(k)), "{k:?}");
        }
    }

    #[test]
    fn the_detail_pane_and_its_link() {
        let m = msg(MessageType::Inquiry);
        let d = detail(&m, &utc_time);
        assert!(d.starts_with("Verify alignment on device QPADEV0003. (C D I R)\nMore\n\nCause"));
        assert!(d.contains("Inquiry CPA0701, severity 99"));
        assert!(d.contains("Sent 2026-10-26 07:33 by QSYS, job 000012/QSYS/QSYSARB."));
        assert!(d.ends_with("Waiting for a reply."));
        assert_eq!(Link::of(&m), None);
        let mut copy = msg(MessageType::SenderCopy);
        copy.reply = Some("Yes".into());
        assert!(detail(&copy, &utc_time).ends_with("Answered: Yes"));
        let link = Link::of(&copy).unwrap();
        assert_eq!(link, Link::AnsweredBy(copy.key));
        let mut reply = msg(MessageType::Reply);
        reply.key = [0, 0, 0, 10];
        reply.answers = Some(copy.key);
        assert!(link.leads_to(&reply));
        assert_eq!(Link::of(&reply), Some(Link::Answers(copy.key)));
        assert!(Link::Answers(copy.key).leads_to(&copy));
        assert!(!Link::Answers(copy.key).leads_to(&reply));
    }

    #[test]
    fn what_opens_message_arrived() {
        use qsa_config::settings::Arrival;
        let inquiry = msg(MessageType::Inquiry);
        let info = msg(MessageType::Informational);
        let mut answered = inquiry.clone();
        answered.needs_reply = false;
        let mut copy = msg(MessageType::SenderCopy);
        copy.needs_reply = true;
        let reply = msg(MessageType::Reply);
        assert!(opens_arrived(Arrival::InquiriesOnly, &inquiry));
        assert!(!opens_arrived(Arrival::InquiriesOnly, &info));
        assert!(!opens_arrived(Arrival::InquiriesOnly, &answered));
        assert!(opens_arrived(Arrival::All, &info));
        assert!(!opens_arrived(Arrival::None, &inquiry));
        for a in Arrival::ALL {
            assert!(!opens_arrived(a, &copy), "{a:?}");
            assert!(!opens_arrived(a, &reply), "{a:?}");
        }
    }

    #[test]
    fn sorting_a_page() {
        let mut a = msg(MessageType::Informational);
        a.severity = 0;
        a.sent = 3;
        let mut b = msg(MessageType::Escape);
        b.severity = 40;
        b.sent = 2;
        let mut page = vec![a.clone(), b.clone()];
        sort_page(&mut page, 2, false);
        assert_eq!(page[0].severity, 40);
        sort_page(&mut page, 0, false);
        assert_eq!(page[0].sent, 3);
        sort_page(&mut page, 1, true);
        assert_eq!(page[0].kind, MessageType::Escape);
    }

    #[test]
    fn lines() {
        assert_eq!(
            list_line(149, "QSYSOPR", None),
            "QAD9001: 149 messages on QSYSOPR."
        );
        let mut p = qap_client::spool::Paging::new(100);
        p.update(149);
        assert_eq!(paging_line(&p, 100), "Messages 1-100 of 149, page 1 of 2");
        p.update(0);
        assert_eq!(paging_line(&p, 0), "No messages");
    }
}
