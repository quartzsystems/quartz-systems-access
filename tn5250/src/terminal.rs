// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! A display terminal: the screen, records from the host applied to it,
//! keys typed into it. The UI, the replay harness and the fuzz targets all
//! drive a session through this.

use crate::codepage;
use crate::inbound;
use crate::input::{self, Key, Outcome};
use crate::negotiate::ScreenSize;
use crate::record::{Record, opcode};
use crate::screen::{Keyboard, Screen};
use crate::stream::{self, Applied};

#[derive(Debug, Clone)]
pub struct Terminal {
    pub screen: Screen,
}

impl Terminal {
    /// A terminal of `size` in `ccsid` (37 when the CCSID is not built in).
    pub fn new(size: ScreenSize, ccsid: u16) -> Terminal {
        let page = codepage::by_ccsid(ccsid).unwrap_or_else(codepage::default_page);
        Terminal {
            screen: Screen::new(size, page),
        }
    }

    /// Applies a record from the host. An attention key pressed before the
    /// host's read goes out as soon as the read arrives.
    pub fn receive(&mut self, rec: &Record) -> Applied {
        let mut applied = stream::apply(&mut self.screen, rec);
        let s = &mut self.screen;
        if let (Some(aid), Some(read)) = (s.pending_aid, s.pending_read)
            && s.keyboard.is_open()
        {
            s.pending_aid = None;
            s.pending_read = None;
            s.keyboard = Keyboard::System;
            let data = inbound::read_reply(s, read, aid);
            applied.replies.push(Record::new(opcode::PUT_GET, data));
        }
        applied
    }

    /// Handles a key.
    pub fn key(&mut self, k: Key) -> Outcome {
        input::key(&mut self.screen, k)
    }

    /// Types text into the input fields.
    pub fn paste(&mut self, text: &str) -> Outcome {
        input::paste(&mut self.screen, text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::aid;
    use crate::stream::{ESC, cmd};

    #[test]
    fn a_key_pressed_before_the_read_goes_out_with_it() {
        let mut t = Terminal::new(ScreenSize::S24x80, 37);
        // Unlock without a read, press Enter, then the read arrives.
        t.receive(&Record::new(
            3,
            vec![ESC, cmd::WRITE_TO_DISPLAY, 0x00, 0x08],
        ));
        assert!(t.key(Key::Enter).send.is_empty());
        let a = t.receive(&Record::new(3, vec![ESC, cmd::READ_MDT_FIELDS, 0x00, 0x00]));
        assert_eq!(a.replies.len(), 1);
        assert_eq!(a.replies[0].data[2], aid::ENTER);
        assert_eq!(t.screen.keyboard, Keyboard::System);
    }
}
