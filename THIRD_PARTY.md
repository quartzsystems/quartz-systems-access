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
| `DejaVuSansCondensed.ttf` | DejaVu Sans Condensed | 2.37 | Bitstream Vera licence, Arev additions; DejaVu changes public domain | UI text, 11 px |
| `DejaVuSansCondensed-Bold.ttf` | DejaVu Sans Condensed Bold | 2.37 | as above | Group titles |
| `DejaVuSansMono.ttf` | DejaVu Sans Mono | 2.37 | as above | System output |
| `DejaVuSansMono-Bold.ttf` | DejaVu Sans Mono Bold | 2.37 | as above | System output, bold |

Source: the `dejavu-fonts-ttf-2.37` release of the DejaVu project. The licence
text is in `qsa-ui/fonts/LICENSE-DejaVu.txt`; it carries the Bitstream Vera and
Arev sections, which are the ones that cover these four faces. The release's
licence file also has a section for the separate DejaVu Math font, which is
not bundled.
