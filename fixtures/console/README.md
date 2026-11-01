# Console pixel fixtures

`cargo test -p qsa-ui --test console_pixels` renders a recorded screen with
the 5250 display's renderer at 1× and compares it, pixel for pixel, with the
QS/400 service console's picture of the same screen. Only the 24×80 grid is
compared (720×384 pixels): the console has no status line, and the window
border is not part of either.

For each fixture `NAME` (a recording `../replay/NAME.qsr`):

- `NAME.png` is the console's own rendering, made by `reference/`, which
  replays the recording through the console's terminal
  (`lic/console/src/terminal.rs`) and draws it with the console's scaler
  (`lic/console/src/scale.rs`) at scale 1. QS/400's QEMU suite checks the
  console's real framebuffer against that same scaler, cell by cell.
- `NAME.capture.png`, when present, is a capture from a running console. The
  test compares against it too.

## Regenerating `NAME.png`

```
git clone https://github.com/quartzsystems/quartz-systems-400 fixtures/console/qs400
git -C fixtures/console/qs400 checkout 79cbecf88927adf0c93b0c8f43feb0da5973688b
cd fixtures/console/reference
cargo run --release -- ../../replay/NAME.qsr ../NAME.png
```

The checkout is git-ignored. Use the commit in
`qsa-ui/assets/console/SOURCE.md`; when the console's look changes upstream,
vendor the new font and update the palette and rules first, then move the
commit here and regenerate.

## Capturing a real console

The console and the telnet session must show the same screen. The sign-on
display cannot be used: on the console it names the display `QSCONSOLE`,
which a network session can never be given. Use the `MAIN` menu after
signing on as the same test profile instead:

1. Record the telnet side as `../replay/main-menu.qsr`: `qsaccess --record
   fixtures/replay/main-menu.qsr`, sign on as `QSATEST`, stop at `MAIN`,
   close the window.
2. Boot the same system under QEMU with a `virtio-vga` display at 1024×768
   (as the QS/400 suite's step 73 does). At 1024×768 the console's grid is
   at scale 1 from (152, 192); the IPL log line
   `slic: display: ... 80x24 grid at scale 1 from (X, Y)` gives the origin
   for other modes. A mode that gives scale 2 or more cannot be compared at
   1×; pass `qs400.console.scale=1` on the kernel command line.
3. Sign on at the console as `QSATEST` and stop at `MAIN`, the cursor on the
   command line, nothing typed.
4. In the QEMU monitor: `screendump /tmp/main-menu.ppm`.
5. Cut the grid out and keep it:

   ```
   cd fixtures/console/reference
   cargo run --release -- crop /tmp/main-menu.ppm 152 192 ../main-menu.capture.png
   cargo run --release -- ../../replay/main-menu.qsr ../main-menu.png
   ```

6. Add `"main-menu"` to `FIXTURES` in `qsa-ui/tests/console_pixels.rs`.

Anything on the screen that changes between the two (a date or time) makes
the fixture useless; pick a screen without one.
