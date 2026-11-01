// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! The System Configurations dialog.

use crate::mainwin::Ui;

/// Opens System Configurations.
pub fn open(ui: &Ui) {
    let mut s = ui.status.clone();
    s.set_message("System Configurations");
}
