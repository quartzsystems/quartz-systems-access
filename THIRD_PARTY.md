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
| `directories` | 6 | MIT OR Apache-2.0 | Settings location |
| `thiserror` | 2 | MIT OR Apache-2.0 | Error types |
| `windows-sys` | 0.61 | MIT OR Apache-2.0 | Named pipe, SID lookup (Windows only) |
| `rustls` (ring provider) | 0.23 | Apache-2.0 OR ISC OR MIT | TLS for 5250 sessions |
| `ring` (via rustls) | 0.17 | Apache-2.0 AND ISC | Cryptography for TLS |
| `rustls-pki-types` | 1 | MIT OR Apache-2.0 | Certificate and PEM types |
| `rustls-native-certs` | 0.8 | Apache-2.0 OR ISC OR MIT | The platform's trusted roots |
| `sha2` | 0.10 | MIT OR Apache-2.0 | Certificate fingerprints |
| `printpdf` (`text_layout` feature, for font subsetting) | 0.12 | MIT | PDF output of printer sessions |
| `windows` | 0.61 | MIT OR Apache-2.0 | Windows printing: spooler, GDI, print setup dialog (Windows only) |
| `rust_xlsxwriter` (`constant_memory` feature) | 0.102 | MIT OR Apache-2.0 | Data Transfer: XLSX files written |
| `calamine` | 0.38 | MIT | Data Transfer: XLSX and ODS files read |
| `zip` (`deflate` feature) | 9.2 | MIT | Data Transfer: ODS files written |

Transitive dependencies are listed in `Cargo.lock`.

The spreadsheet crates bring in `quick-xml` (MIT), `tempfile`, `fastrand`,
`atoi_simd`, `fast-float2`, `codepage`, `typed-path`, `bumpalo`,
`debug_unsafe` and `rustversion` (MIT or Apache-2.0), `byteorder`
(Unlicense or MIT), `zopfli` (Apache-2.0) and `zlib-rs` (Zlib).

`printpdf` brings in, among others, `lopdf` (MIT), `allsorts-azul` (Apache-2.0,
the font subsetter) and the `azul-*` layout crates (MIT). One of those,
`azul-simplecss` 0.2, is under the Mozilla Public License 2.0: it is used
unmodified, and its source is published on crates.io, which MPL-2.0 asks for.
The rest of `printpdf`'s tree is under MIT, Apache-2.0, BSD-3-Clause, Zlib or
Unicode-3.0 terms.

## Build-time tools

Used when the program is built; nothing of them is in the program.

| Component | Version | Licence | Use |
|-----------|---------|---------|-----|
| `pulldown-cmark` (`html` feature) | 0.13 | MIT | Help: the topics' Markdown (`qsaccess/help/`) made into the HTML the program carries |

It brings in `pulldown-cmark-escape` (MIT), `unicase` and `bitflags` (MIT or
Apache-2.0) and `memchr` (Unlicense or MIT).

## Bundled fonts

The UI and monospace faces are open-licence fonts used until Quartz Systems'
own fonts exist. They are embedded in the binary (`qsa-ui/fonts/`) and
registered privately with the process at start-up. Both are published by
Microsoft under the SIL Open Font License 1.1 and are bundled unmodified; they
are not sold on their own. On Windows, UI text uses the system's own Segoe UI,
which is not bundled, and Selawik is left out of the binary.

| File | Family | Version | Licence | Use |
|------|--------|---------|---------|-----|
| `Selawik-Regular.ttf` | Selawik | 1.01 | SIL OFL 1.1, Reserved Font Name "Selawik" | UI text, 12 px (not on Windows) |
| `Selawik-Bold.ttf` | Selawik Bold | 1.01 | as above | Emphasis |
| `CascadiaMono-Regular.ttf` | Cascadia Mono | 2407.24 | SIL OFL 1.1, Reserved Font Name "Cascadia Code" | System output |
| `CascadiaMono-Bold.ttf` | Cascadia Mono Bold | 2407.24 | as above | System output, bold |

Sources: the `1.01` release of `microsoft/Selawik` (`selawk.ttf`,
`selawkb.ttf`, renamed on copy) and the `v2407.24` release of
`microsoft/cascadia-code` (`ttf/static`). Licence texts:
`qsa-ui/fonts/OFL-Selawik.txt` and `qsa-ui/fonts/OFL-Cascadia.txt`.

## QS/400 console font

| File | Licence | Use |
|------|---------|-----|
| `qsa-ui/assets/console/qs400-9x16.txt` | MIT, Copyright (C) 2026, Quartz Systems (the QS/400 repository's `LICENSE`) | The 5250 display, its status line and Print Screen |

The QS/400 service console's 9×16 bitmap font, drawn by Quartz Systems,
vendored byte for byte from `github.com/quartzsystems/quartz-systems-400`
(`lic/console/font/qs400-9x16.txt`, commit `79cbecf8`). Its palette and drawing
rules are reproduced in `qsa-ui/src/console.rs`. Provenance:
`qsa-ui/assets/console/SOURCE.md`.

## Print font

Printer sessions print every font the data selects in one monospace face,
bundled in `qsa-print/fonts/` and embedded, subset, in every PDF. On Windows it
is loaded privately for the process (`AddFontMemResourceEx`), never installed.

| File | Family | Version | Licence | Use |
|------|--------|---------|---------|-----|
| `CourierPrime-Regular.ttf` | Courier Prime | 3.018 | SIL OFL 1.1 | Printed output |
| `CourierPrime-Bold.ttf` | Courier Prime Bold | 3.018 | as above | Printed output, bold and overstruck |
| `CourierPrime-Italic.ttf` | Courier Prime Italic | 3.018 | as above | Printed output, italic font IDs |

Copyright 2015 The Courier Prime Project Authors. Source: `google/fonts`,
`ofl/courierprime/` (commit `6c1dbe27`), bundled unmodified. Licence text:
`qsa-print/fonts/OFL-CourierPrime.txt`.
