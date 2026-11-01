// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! The host-server decoders: a frame and its messages, and both sides of
//! the server mapper.
//!
//! Input: any bytes, read as a frame, as a mapper request and as a mapper
//! reply.

#![no_main]

use hostsvr_proto::mapper::{MapperReply, MapperRequest};
use hostsvr_proto::{Frame, frame, message};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(f) = Frame::decode(data) {
        // What decodes, encodes back to the same bytes.
        assert_eq!(f.encode(), data);
        assert_eq!(f.len(), data.len());
        if let Ok(messages) = message::from_frame(&f) {
            for m in messages {
                let back = message::HostMessage::decode(&m.encode()).unwrap();
                assert_eq!(back, m);
            }
        }
    }
    if data.len() >= 4 {
        let _ = frame::frame_length(data[..4].try_into().unwrap());
    }
    if let Ok(Some(r)) = MapperRequest::decode(data) {
        assert_eq!(r.encode(), data);
    }
    if let Ok(r) = MapperReply::decode(data) {
        assert_eq!(MapperReply::decode(&r.encode()), Ok(r));
    }
});
