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

None yet.
