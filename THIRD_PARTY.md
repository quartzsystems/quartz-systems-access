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

## Bundled fonts

The UI and monospace faces are open-licence fonts used until Quartz Systems'
own fonts exist. They are embedded in the binary (`qsa-ui/fonts/`) and
registered privately with the process at start-up, so layouts match on every
platform. Both are published by Microsoft under the SIL Open Font License 1.1
and are bundled unmodified; they are not sold on their own.

| File | Family | Version | Licence | Use |
|------|--------|---------|---------|-----|
| `Selawik-Regular.ttf` | Selawik | 1.01 | SIL OFL 1.1, Reserved Font Name "Selawik" | UI text, 12 px |
| `Selawik-Bold.ttf` | Selawik Bold | 1.01 | as above | Emphasis |
| `CascadiaMono-Regular.ttf` | Cascadia Mono | 2407.24 | SIL OFL 1.1, Reserved Font Name "Cascadia Code" | System output |
| `CascadiaMono-Bold.ttf` | Cascadia Mono Bold | 2407.24 | as above | System output, bold |

Sources: the `1.01` release of `microsoft/Selawik` (`selawk.ttf`,
`selawkb.ttf`, renamed on copy) and the `v2407.24` release of
`microsoft/cascadia-code` (`ttf/static`). Licence texts:
`qsa-ui/fonts/OFL-Selawik.txt` and `qsa-ui/fonts/OFL-Cascadia.txt`.
