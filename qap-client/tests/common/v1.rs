// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! A QS/400 that speaks only QAP version 1: QS/400's engine at 58a7e6d3
//! (`qapd_v1`, before version 2), behind the same loopback TLS listener.
//! `qapd::testing` is version 2, so the host here is this crate's own, as
//! small as the fallback checks need: `BOB` (`SECRET1`), sessions,
//! `SystemInfo`.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Mutex};

use qap_proto_v1::ops::signon::SignOnReply;
use qap_proto_v1::ops::system::SystemInfo;
use qap_proto_v1::{Secret, ServerMessage, Status, Token};
use qapd_v1::sessions::Sessions;
use qapd_v1::{Closed, Config, Ending, Host, Refusal, SystemId};

use super::{TLS13, TlsConn, listen};

impl qapd_v1::Transport for TlsConn {
    fn close(&mut self) {
        self.close_tls();
    }
}

/// What every connection of the version-1 system shares.
#[derive(Default)]
pub struct V1System {
    pub sessions: Mutex<Sessions>,
    running_as: Mutex<HashMap<u32, String>>,
    tokens: AtomicU8,
}

/// One connection job of it.
struct V1Host {
    sys: Arc<V1System>,
    job: u32,
}

impl V1Host {
    fn reply(&self, token: Token, user: &str) -> SignOnReply {
        SignOnReply {
            token,
            idle_timeout_secs: 600,
            profile: user.into(),
            ..SignOnReply::default()
        }
    }
}

impl Host for V1Host {
    fn system_id(&mut self) -> SystemId {
        SystemId {
            system_name: "QSV1".into(),
            release: "V1R0M0".into(),
            time_zone: "QN0000UTC".into(),
            ccsid: 37,
        }
    }

    fn now_ms(&mut self) -> i64 {
        1_793_000_000_000
    }

    fn message(&mut self, id: &str, data: &[&str]) -> ServerMessage {
        ServerMessage {
            id: id.into(),
            severity: 40,
            data: data.join(" ").into_bytes(),
            text: format!("{id} {}", data.join(" ")),
            help: format!("Help for {id}."),
        }
    }

    fn system_info(&mut self) -> SystemInfo {
        SystemInfo {
            system_name: "QSV1".into(),
            ..SystemInfo::default()
        }
    }

    fn ending(&mut self) -> Option<Ending> {
        None
    }

    fn sign_on(&mut self, user: &str, password: &Secret) -> Result<SignOnReply, Refusal> {
        let user = user.trim().to_ascii_uppercase();
        if user != "BOB" || password.expose() != "SECRET1" {
            return Err(Refusal {
                status: Status::SignOnFailed,
                message: self.message("CPF1107", &[]),
            });
        }
        let n = self.sys.tokens.fetch_add(1, Ordering::SeqCst) + 1;
        let token = Token([n; 32]);
        self.sys
            .sessions
            .lock()
            .unwrap()
            .create(&token, &user, 600, self.job);
        self.sys
            .running_as
            .lock()
            .unwrap()
            .insert(self.job, user.clone());
        Ok(self.reply(token, &user))
    }

    fn change_password(&mut self, _: &str, _: &Secret, _: &Secret) -> Result<SignOnReply, Refusal> {
        Err(Refusal {
            status: Status::Error,
            message: self.message("CPF2362", &[]),
        })
    }

    fn resume(&mut self, token: &Token) -> Result<SignOnReply, Refusal> {
        let r = self
            .sys
            .sessions
            .lock()
            .unwrap()
            .resume(token, self.job, 0, &|_| true);
        match r {
            Ok(s) => Ok(self.reply(token.clone(), &s.user)),
            Err(_) => Err(Refusal {
                status: Status::SessionExpired,
                message: self.message("TCP1A53", &[]),
            }),
        }
    }

    fn sign_off(&mut self, token: &Token) {
        self.sys.sessions.lock().unwrap().end(token);
        self.sys.running_as.lock().unwrap().remove(&self.job);
    }

    fn detach(&mut self, token: &Token) {
        self.sys.sessions.lock().unwrap().detach(token, self.job, 0);
        self.sys.running_as.lock().unwrap().remove(&self.job);
    }
}

/// A version-1 server on loopback; its port and how its connections ended.
pub fn start_v1() -> (u16, Arc<V1System>, Arc<Mutex<Vec<Closed>>>) {
    let sys = Arc::new(V1System::default());
    let s = Arc::clone(&sys);
    let cfg = Config {
        poll: std::time::Duration::from_millis(10),
        ..Config::default()
    };
    let (port, closed) = listen(TLS13, move |t, job| {
        let mut host = V1Host {
            sys: Arc::clone(&s),
            job,
        };
        qapd_v1::serve(t, &mut host, cfg)
    });
    (port, sys, closed)
}
