// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! An entry's Properties: every field the system gives for it (name,
//! folder, type, a link's target, size, last change, owner and CCSID). A
//! new name in the Name field renames the entry on OK, the system's refusal
//! shown in the dialog. Authorities are not shown or changed here in this
//! release, and the dialog says so.

use fltk::enums::Align;
use fltk::input::Input;
use fltk::prelude::*;
use qap_client::Files;
use qap_client::ifs::{join, parent};
use qsa_config::System;
use qsa_ui::{PropertyDialog, controls, theme};

use super::model::{self, Row, Target};
use crate::qap::{self, Handle};

/// Dialog width and height.
pub const W: i32 = 440;
pub const H: i32 = 380;

/// What the dialog says about authorities.
pub const AUTHORITIES: &str = "Authorities are not shown or changed here in this release. Use the system's Work with Authority (WRKAUT) command for them.";

/// The dialog's rows: label and value, the Name field aside.
pub fn fields(row: &Row) -> Vec<(&'static str, String)> {
    let e = &row.entry;
    let mut out = vec![
        ("Folder:", parent(&row.path).to_owned()),
        ("Type:", row.type_text()),
    ];
    match &row.link {
        Some(Target::To(t, _)) => out.push(("Target:", t.clone())),
        Some(Target::Broken(t)) => out.push(("Target:", format!("{t} (not there)"))),
        Some(Target::Unknown(t)) => out.push(("Target:", t.clone())),
        None => {}
    }
    let size = if row.is_real_folder() {
        String::new()
    } else if e.size < 1024 {
        model::bytes_text(e.size)
    } else {
        format!(
            "{} bytes ({})",
            model::grouped(e.size),
            model::bytes_text(e.size)
        )
    };
    out.push(("Size:", size));
    out.push(("Modified:", model::modified_text(e.modified)));
    out.push(("Owner:", e.owner.clone()));
    let ccsid = match e.ccsid {
        0 => String::new(),
        65535 => "65535 (binary)".into(),
        1208 => "1208 (UTF-8)".into(),
        c => c.to_string(),
    };
    out.push(("CCSID:", ccsid));
    out
}

/// The dialog for `row`, not shown, and its Name field.
pub fn build(row: &Row) -> (PropertyDialog, Input) {
    let mut d = PropertyDialog::new(&format!("Properties - {}", row.entry.name), W, H);
    let mut name = None;
    let rows = fields(row);
    let entry_name = row.entry.name.clone();
    d.tabs().add_page("General", |r| {
        let fh = theme::FIELD_H;
        let (lx, vx) = (r.x + 12, r.x + 100);
        let vw = r.w - 112;
        let mut y = r.y + 14;
        let mut l = controls::label(lx, y, 88, fh, "Name:");
        l.set_align(Align::Left | Align::Inside);
        let mut n = controls::field(vx, y, vw, fh);
        n.set_value(&entry_name);
        name = Some(n);
        y += fh + 12;
        for (label, value) in &rows {
            let mut l = controls::label(lx, y, 88, fh, label);
            l.set_align(Align::Left | Align::Inside);
            let mut v = controls::label(vx, y, vw, fh, value);
            v.set_align(Align::Left | Align::Inside | Align::Clip);
            v.set_tooltip(value);
            y += fh + 4;
        }
        let mut note = controls::label(lx, y + 10, r.w - 24, 48, AUTHORITIES);
        note.set_align(Align::Left | Align::Inside | Align::Top | Align::Wrap);
        note.set_label_color(theme::colors::DISABLED_TEXT);
    });
    (d, name.expect("the General page has the Name field"))
}

/// Shows `row`'s Properties over `parent`. A new name renames it on the
/// system when OK is chosen; the new path then, else `None`.
pub fn run(parent: Option<&dyn WindowExt>, h: &Handle, sys: &System, row: &Row) -> Option<String> {
    let (mut d, name) = build(row);
    let mut renamed = None;
    let ok = d.run(parent, || {
        let new = name.value().trim().to_owned();
        if new.is_empty() || new == row.entry.name {
            return Ok(());
        }
        if new.contains('/') {
            return Err("A name has no / in it.".into());
        }
        let to = join(qap_client::ifs::parent(&row.path), &new);
        let (from, t) = (row.path.clone(), to.clone());
        match qap::busy(|| qap::lock(h).rename(&from, &t, false)) {
            Ok(()) => {
                renamed = Some(to);
                Ok(())
            }
            Err(e) => Err(qap::explain(sys, &e).line()),
        }
    });
    if ok { renamed } else { None }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ifs::entry;
    use qap_client::proto::ops::ifs::EntryKind;

    #[test]
    fn every_field() {
        let mut r = Row {
            path: "/home/BOB/link1208".into(),
            entry: entry("link1208", EntryKind::Symlink, 0, 0),
            link: Some(Target::Broken("/home/BOB/gone.txt".into())),
        };
        let f = fields(&r);
        let get = |k: &str| f.iter().find(|(l, _)| *l == k).map(|(_, v)| v.clone());
        assert_eq!(get("Folder:").as_deref(), Some("/home/BOB"));
        assert_eq!(get("Type:").as_deref(), Some("Broken link"));
        assert_eq!(
            get("Target:").as_deref(),
            Some("/home/BOB/gone.txt (not there)")
        );
        assert_eq!(get("Owner:").as_deref(), Some("BOB"));
        r.link = None;
        r.entry = entry("big.bin", EntryKind::File, 200 << 20, 65535);
        let f = fields(&r);
        let get = |k: &str| f.iter().find(|(l, _)| *l == k).map(|(_, v)| v.clone());
        assert_eq!(
            get("Size:").as_deref(),
            Some("209,715,200 bytes (200.0 MiB)")
        );
        assert_eq!(get("CCSID:").as_deref(), Some("65535 (binary)"));
        assert!(get("Target:").is_none());
    }
}
