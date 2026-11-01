// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! Macros for 5250 display sessions.

use std::path::Path;

use fltk::prelude::*;

use crate::app::Shared;

/// Opens a `.qsm` file in the Macro editor.
pub fn open_file(_state: &Shared, path: &Path, parent: Option<&dyn WindowExt>) {
    crate::log!(
        "macro {} not opened: the Macro editor is not yet available",
        path.display()
    );
    let m = qsa_msg::message(qsa_msg::id::NOT_AVAILABLE, &[]);
    qsa_ui::msgdetails::show_message(parent, &m);
}
