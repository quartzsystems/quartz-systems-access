// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! A QS/400 that speaks QAP versions 1 and 2 only: QS/400's engine at
//! 8306ec41 (`qapd_v2`, before version 3) with its own `qapd::testing`
//! system, behind the same loopback TLS listener.

use std::sync::{Arc, Mutex};

use qapd_v2::testing::{MemHost, System};
use qapd_v2::{Closed, Config};

use super::{TLS13, TlsConn, listen};

impl qapd_v2::Transport for TlsConn {
    fn close(&mut self) {
        self.close_tls();
    }
}

/// A version-2 server on loopback; its port, its system and how its
/// connections ended.
pub fn start_v2() -> (u16, Arc<System>, Arc<Mutex<Vec<Closed>>>) {
    let sys = System::new();
    let s = Arc::clone(&sys);
    let cfg = Config {
        poll: std::time::Duration::from_millis(10),
        ..Config::default()
    };
    let (port, closed) = listen(TLS13, move |t, job| {
        let mut host = MemHost::new(Arc::clone(&s), job);
        qapd_v2::serve(t, &mut host, cfg)
    });
    (port, sys, closed)
}
