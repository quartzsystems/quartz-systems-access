// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! The QS/400 Access for Desktop look: scheme, colours, fonts, glyphs and widgets.
//!
//! Call [`theme::init`] once, after creating the FLTK app and before making
//! any widget.

pub mod chan;
pub mod console;
pub mod controls;
pub mod glyph;
pub mod goldens;
pub mod groupbox;
pub mod inputbox;
pub mod listtable;
pub mod msgbox;
pub mod msgdetails;
pub mod propdialog;
pub mod sampler;
pub mod snapshot;
pub mod statusbar;
pub mod tabs;
pub mod taskpane;
pub mod terminal;
pub mod theme;
pub mod toolbar;

pub use glyph::Glyph;
pub use inputbox::InputBox;
pub use listtable::ListTable;
pub use msgbox::MessageBox;
pub use propdialog::PropertyDialog;
pub use statusbar::{Status, StatusBar};
pub use tabs::Tabs;
pub use taskpane::TaskPane;
pub use toolbar::Toolbar;
