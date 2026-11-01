// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! The reply to the 5250 Query (`WSF D9 70`): what kind of display this
//! is. It claims a 3179 model 2 (24×80) or a 3477 model FC (24×80 and
//! 27×132), colour, a standard keyboard, 256 input fields, and no GUI
//! constructs, so hosts draw windows with characters.

use crate::aid;
use crate::record::{Record, opcode};
use crate::screen::Screen;

/// The query reply record.
pub fn reply(s: &Screen) -> Record {
    let page = crate::codepage::default_page();
    let (device, model, sizes) = if s.wide_ok() {
        ("3477", "FC ", 0x30)
    } else {
        ("3179", "002", 0x10)
    };
    let mut q = Vec::with_capacity(64);
    // Workstation control unit, then its code level.
    q.extend_from_slice(&[0x06, 0x00]);
    q.extend_from_slice(&[0x01, 0x01, 0x00]);
    q.extend_from_slice(&[0; 16]);
    // A display station, its type and model.
    q.push(0x01);
    q.extend(page.encode(device));
    q.extend(page.encode(model));
    // Standard keyboard; no extended keyboard; reserved.
    q.extend_from_slice(&[0x02, 0x00, 0x00]);
    // Serial number.
    q.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]);
    // Maximum input fields.
    q.extend_from_slice(&(crate::screen::MAX_FIELDS as u16).to_be_bytes());
    // Control unit customisation; reserved.
    q.extend_from_slice(&[0x00, 0x00, 0x00]);
    // Capabilities: data stream functions; screen sizes and colour; no
    // highlighting extensions; no GUI; reserved.
    q.extend_from_slice(&[0x23, sizes | 0x01, 0x00, 0x00, 0x00]);
    q.resize(63, 0x00);

    let len = (q.len() + 5) as u16;
    let mut data = vec![0x00, 0x00, aid::INBOUND_WSF];
    data.extend_from_slice(&len.to_be_bytes());
    data.extend_from_slice(&[0xD9, 0x70, 0x80]);
    data.extend(q);
    Record::new(opcode::PUT_GET, data)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codepage;
    use crate::negotiate::ScreenSize;

    #[test]
    fn reply_describes_the_display() {
        let s = Screen::new(ScreenSize::S24x80, codepage::default_page());
        let r = reply(&s);
        let d = &r.data;
        assert_eq!(&d[..3], &[0, 0, 0x88]);
        assert_eq!(u16::from_be_bytes([d[3], d[4]]) as usize, d.len() - 3);
        assert_eq!(&d[5..8], &[0xD9, 0x70, 0x80]);
        let page = codepage::default_page();
        assert_eq!(page.decode(&d[30..37]), "3179002");
        let wide = Screen::new(ScreenSize::S27x132, page);
        let w = reply(&wide);
        assert_eq!(page.decode(&w.data[30..37]), "3477FC ");
        assert_ne!(w.data[50], d[50], "screen sizes differ");
    }
}
