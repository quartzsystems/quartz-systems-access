# Third-party components

QS/400 Access for Desktop is MIT-licensed (see `LICENSE`). It builds on the
components below. Each is used under its own licence.

## Linked libraries

| Component | Version | Licence | Use |
|-----------|---------|---------|-----|
| FLTK | 1.4 (via fltk-sys) | LGPL-2.0 with the FLTK static-linking exception | Widget toolkit |
| fltk-rs (`fltk`, `fltk-sys`) | 1.5 | MIT | Rust bindings to FLTK |
| `serde` | 1 | MIT OR Apache-2.0 | Serialisation |
| `toml` | 0.9 | MIT OR Apache-2.0 | Settings and message catalogue format |
| `keyring` | 3.6 | MIT OR Apache-2.0 | Windows Credential Manager / Secret Service |
| `directories` | 6 | MIT OR Apache-2.0 | Settings location |
| `thiserror` | 2 | MIT OR Apache-2.0 | Error types |
| `windows-sys` | 0.61 | MIT OR Apache-2.0 | Named pipe, SID lookup (Windows only) |

Transitive dependencies are listed in `Cargo.lock`.

## Bundled fonts (placeholders)

The UI and monospace faces are placeholders until Quartz Systems' own fonts
exist. They are embedded in the binary (`qsa-ui/fonts/`) and registered
privately with the process at start-up, so layouts match on every platform.

| File | Family | Version | Licence | Use |
|------|--------|---------|---------|-----|
| `NotoSans-Regular.ttf` | Noto Sans | 2.015 (hinted) | SIL Open Font License 1.1 | UI text |
| `NotoSans-Bold.ttf` | Noto Sans Bold | 2.015 (hinted) | SIL Open Font License 1.1 | Group titles |
| `NotoSansMono-Regular.ttf` | Noto Sans Mono | 2.014 (hinted) | SIL Open Font License 1.1 | System output |
| `NotoSansMono-Bold.ttf` | Noto Sans Mono Bold | 2.014 (hinted) | SIL Open Font License 1.1 | System output, bold |

Source: the `NotoSans-v2.015` and `NotoSansMono-v2.014` releases of the
`notofonts/latin-greek-cyrillic` project, `hinted/ttf` builds. Copyright 2022
The Noto Project Authors. The licence text is in `qsa-ui/fonts/OFL-Noto.txt`.
The fonts are unmodified and are not sold on their own.
