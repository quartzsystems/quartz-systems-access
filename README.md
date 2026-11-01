# QS/400 Access for Desktop

QS/400 Access for Desktop is the native desktop client for QS/400, the operating system from Quartz Systems, on Windows and Linux. It is written in Rust on FLTK and keeps a single, fixed look on every platform. It will provide a 5250 display and printer emulator, saved session files, data transfer, Run SQL, Integrated File System browsing and printer output; this first phase provides the workspace, the look, the main window and System Configurations. Build with `cargo build --release`; the binary is `qsaccess`, and `qsaccess --ui-sampler` opens a window that shows every widget once.

## 5250 display

Select **5250 Display** in the task pane, choose the screen size, code page,
device name and colour map, and Start. The display draws exactly like the QS/400
service console: its 9×16 bitmap font and its palette, scaled by whole numbers
only. Sessions can be recorded for the replay tests with `qsaccess --record
FILE` (or `QSACCESS_RECORD=FILE`); see `fixtures/README.md`.

The `tn5250` crate holds the protocol (Telnet, TN5250E, the 5250 data stream,
the keyboard) and has no UI dependency.

## Tests and goldens

`cargo test --workspace` runs everything, including the screenshot tests
against the goldens for your platform. Goldens are made on CI's pinned runners:

```
gh workflow run ci.yml --ref <branch> -f bless=true
gh run download <run-id> -n goldens-windows -D qsaccess/tests/goldens/windows
gh run download <run-id> -n goldens-linux -D qsaccess/tests/goldens
```

The Linux job runs in `ubuntu:24.04` pinned by digest, under Xvfb, with only the
bundled fonts. `TESTING.md` has the details, the manual Wayland check and the
live acceptance list.
