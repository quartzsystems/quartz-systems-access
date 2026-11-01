// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! The host servers' data stream, without I/O: what a client and a server
//! send each other, as types that encode to bytes and decode from them.
//! QS/400 Access for Desktop uses it as a client (through `hostsvr`); the
//! QS/400 system uses it for its own servers. Both sides of every
//! exchange are here, so each type has `encode` and `decode`.
//!
//! - [`mapper`]: the server mapper (port 449): service name in, port out.
//! - [`frame`]: the common header and LL/CP parameters every other
//!   exchange uses.
//! - [`message`]: system messages that come back with a failure.
//! - [`rc`]: return codes and their meanings.
//! - [`wire`]: every constant, with its source.
//!
//! The sign-on exchanges are not in this version: `SIGNON-SPEC.md` beside
//! this crate describes what is to come. The central server's license
//! requests are not here, and never will be: this crate neither builds nor
//! reads them.
//!
//! Secret bytes are [`pwsub::Secret`]s, so they are overwritten when
//! dropped, and no type here shows a parameter's or template's bytes in
//! `Debug`.
//!
//! The public API is kept stable; changes are listed in `CHANGELOG.md` at
//! the repository root.

pub mod frame;
pub mod mapper;
pub mod message;
pub mod rc;
pub mod wire;

pub use frame::{Frame, Header, Param};
pub use pwsub::Secret;

/// Why bytes could not be read as the expected unit.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum DecodeError {
    #[error("{got} bytes, need at least {need}")]
    Short { need: usize, got: usize },
    #[error("length field says {field} bytes, but {got} are there")]
    Length { field: usize, got: usize },
    #[error("template of {len} bytes does not fit ({room})")]
    Template { len: usize, room: usize },
    #[error("parameter at offset {offset} is malformed")]
    Param { offset: usize },
    #[error("server ID {got:04X}, expected {want:04X}")]
    Server { want: u16, got: u16 },
    #[error("request or reply ID {got:04X}, expected {want:04X}")]
    Request { want: u16, got: u16 },
    #[error("required parameter {0:04X} is missing")]
    Missing(u16),
    #[error("parameter {cp:04X} has {len} bytes, which is not valid for it")]
    Value { cp: u16, len: usize },
    #[error("mapper data is not valid: {0}")]
    Mapper(&'static str),
}

/// The reply ID for a request ID.
pub fn reply_id(request_id: u16) -> u16 {
    request_id | wire::REPLY_BIT
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reply_ids_set_the_top_bit() {
        assert_eq!(reply_id(wire::REQ_SIGNON_EXCHANGE), 0xF003);
        assert_eq!(reply_id(wire::REQ_CENTRAL_EXCHANGE), 0x9301);
    }
}
