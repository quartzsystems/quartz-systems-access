// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! The untrusted-certificate dialog: the certificate a system presented,
//! and the choice to trust it for that system alone. There is no way to
//! ignore verification.

use fltk::app;
use fltk::enums::{Align, FrameType};
use fltk::frame::Frame;
use fltk::prelude::*;
use fltk::window::Window;
use qsa_ui::chan::{self, Emit};
use qsa_ui::controls;
use qsa_ui::glyph::{self, Glyph, Size};
use qsa_ui::propdialog::center_on;
use qsa_ui::theme::{self, colors};
use tn5250::telnet::x509::CertInfo;

pub const W: i32 = 520;
pub const H: i32 = 330;

/// Builds the dialog without showing it. `true` arrives on the receiver
/// for Trust for this system, `false` for Cancel.
pub fn build(system: &str, reason: &str, info: &CertInfo) -> (Window, chan::Receiver<bool>) {
    let (tx, rx) = chan::channel::<bool>();
    let m = 12;
    let mut win = Window::new(0, 0, W, H, None);
    win.set_label("Certificate Not Trusted");
    win.set_color(colors::FACE);
    win.make_modal(true);
    let mut icon = Frame::new(m, m, 32, 32, None);
    icon.set_image(Some(glyph::image(Glyph::Cross, Size::Large)));
    let tx0 = m + 44;
    let msg = qsa_msg::message(qsa_msg::id::CERT_UNTRUSTED, &[system, reason]);
    let mut head = Frame::new(tx0, m, W - tx0 - m, 20, None);
    head.set_label(&format!("{} ({})", msg.text, msg.id));
    head.set_align(Align::Left | Align::Inside);
    head.set_label_font(theme::UI_FONT_BOLD);
    head.set_label_size(theme::UI_SIZE);
    let mut why = Frame::new(tx0, m + 22, W - tx0 - m, 34, None);
    why.set_label(&format!("Reason: {reason}"));
    why.set_align(Align::Left | Align::Inside | Align::Top | Align::Wrap);
    why.set_label_size(theme::UI_SIZE);

    let mut y = m + 64;
    let lw = 120;
    let vw = W - 2 * m - lw;
    let fp = info.sha256.split(':').collect::<Vec<_>>();
    let fp_two = format!(
        "{}\n{}",
        fp[..16.min(fp.len())].join(":"),
        fp[16.min(fp.len())..].join(":")
    );
    for (label, value, lines) in [
        ("Subject:", info.subject.as_str(), 1),
        ("Issuer:", info.issuer.as_str(), 1),
        ("Valid from:", info.not_before.as_str(), 1),
        ("Valid to:", info.not_after.as_str(), 1),
        ("SHA-256:", fp_two.as_str(), 2),
    ] {
        let h = 18 * lines + 4;
        controls::label(m, y, lw, 20, label);
        let mut v = Frame::new(m + lw, y, vw, h, None);
        v.set_frame(FrameType::FlatBox);
        v.set_color(colors::FACE);
        v.set_label(value);
        v.set_align(Align::Left | Align::Inside | Align::Top | Align::Clip);
        v.set_label_size(theme::UI_SIZE);
        if label == "SHA-256:" {
            v.set_label_font(theme::MONO_FONT);
            v.set_label_size(theme::MONO_SIZE);
        }
        y += h + 6;
    }
    let mut note = Frame::new(m, y + 2, W - 2 * m, 34, None);
    note.set_label(
        "Compare the fingerprint with the one shown on the system. Trusting adds the \
         certificate to this system's CA list only.",
    );
    note.set_align(Align::Left | Align::Inside | Align::Top | Align::Wrap);
    note.set_label_size(theme::UI_SIZE);

    let bh = theme::BUTTON_H;
    let tw = 150;
    let mut trust = controls::button(
        W - 2 * m - tw - theme::BUTTON_W + m / 2,
        H - m - bh,
        tw,
        bh,
        "Trust for this system",
    );
    trust.emit_to(tx, true);
    let mut cancel = controls::default_button(
        W - m - theme::BUTTON_W,
        H - m - bh,
        theme::BUTTON_W,
        bh,
        "Cancel",
    );
    cancel.emit_to(tx, false);
    win.end();
    win.set_callback(move |_| tx.send(false));
    (win, rx)
}

/// Shows the dialog; `true` if the user chose to trust the certificate.
pub fn ask(parent: Option<&dyn WindowExt>, system: &str, reason: &str, info: &CertInfo) -> bool {
    let (mut win, rx) = build(system, reason, info);
    center_on(&mut win, parent);
    win.show();
    while win.shown() {
        app::wait();
        if let Some(answer) = rx.recv() {
            win.hide();
            return answer;
        }
    }
    false
}
