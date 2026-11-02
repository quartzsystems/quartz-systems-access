// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! The QAP client (`docs/qap-protocol-spec.md`). The envelope, the
//! operations' bodies and the Wire codec come from QS/400's `qap-proto` and
//! `ipc` crates, pinned by commit in the workspace manifest.

pub use qap_proto as proto;
