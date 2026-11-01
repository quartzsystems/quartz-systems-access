# Console font and palette: source

The 5250 display draws exactly like the QS/400 service console (the local
console that shows the SRC display, the IPL menu, DST, the installer and the
`QSCONSOLE` display). Everything here comes from the console's renderer in
the QS/400 repository, `github.com/quartzsystems/quartz-systems-400`, crate
`lic/console`, at commit `79cbecf88927adf0c93b0c8f43feb0da5973688b`
(2026-11-01).

## Vendored file

| File | Source path | Last changed upstream | Git blob | SHA-256 |
|---|---|---|---|---|
| `qs400-9x16.txt` | `lic/console/font/qs400-9x16.txt` | `5f9b52de` (2026-10-21) | `a938fc3fe1675cc2dda8b4bcc51875166ba86998` | `f61c48b185e59fbd0b248241299e184ae6680e2c605a0980cb672e9469455650` |

Copied byte for byte from the git blob (LF line ends); `.gitattributes`
marks this directory `-text` so no checkout converts it. To check:
`git hash-object qsa-ui/assets/console/qs400-9x16.txt` prints the blob above.

The font is a bitmap font in a text format: one block per glyph, a
`# U+XXXX name` header then 16 rows of 9 characters, `#` lit and `.` dark.
Cells are 9 by 16 pixels; glyph bodies use columns 0 to 6, columns 7 and 8
are the gap, box drawing spans the cell. The glyph set is printable ASCII
(U+0020 to U+007E), the box-drawing characters U+2500, U+2502, U+250C,
U+2510, U+2514, U+2518, U+251C, U+2524, U+252C, U+2534, U+253C, and U+E000,
the 5250 column separator. Any other character draws as the console's hollow
box (`REPLACEMENT` in `lic/console/src/font.rs`).

## Not vendored, reproduced

The `console` crate itself is not used as a dependency: it links `rustix` and
drives the Linux framebuffer (`fb`, `render`, `service`), so decision 4's rule
for a git dependency does not hold. These values and rules are reproduced in
`qsa-ui/src/console.rs`, each with its source:

- Palette, `lic/console/src/palette.rs`: background `000000`, green
  `33CC33`, white (high intensity) `E6E6E6`, red `E03030`, blue `5080FF`,
  turquoise `20C8C8`, pink `E060C0`, yellow `E6E040` (defined, never used:
  see below). Green at high intensity is white; the `_HI` variants exist
  but the attribute mapping never selects them.
- Attribute bytes to colours, `lic/console/src/terminal.rs` (`fb_attr`):
  `0x20`-`0x27` green, white when bit `0x02` is set; `0x28`-`0x2F` red;
  `0x30`-`0x37` turquoise, column separators (the console's comment: its
  palette "has one of" turquoise and yellow, so `0x34`-`0x37` yellow
  attributes draw turquoise); `0x38`-`0x3F` pink, blue when bit `0x02` is
  set. Bit `0x01` reverse image, `0x04` underline, `x7`/`xF` nondisplay
  (no reverse, no underline). An attribute byte's own cell is blank.
- Cell drawing, `lic/console/src/scale.rs` (`cells_line`): background fill,
  lit glyph pixels in the foreground; reverse image swaps the two (the
  foreground becomes the background colour); underline lights scanline 14
  across the cell; column separators light the cell's first and last
  column on scanlines 0, 4, 8 and 12; the cursor lights scanline 15 across
  the cell in white, on top of everything, steady (no blink). Blink is
  recorded but not drawn.
- Scaling, `lic/console/src/scale.rs` (`Layout`): each glyph pixel becomes
  an s×s block, nearest neighbour, s the largest integer that fits (at
  least 1), the grid centred, the margins the background colour.
- Message line: the console has no separate status line. Its input-inhibited
  indicator is `X II` in white at row 24 column 1 (`terminal.rs`). The
  operator information area below the grid here is new; it uses the same
  font, the same palette, and `X II` in white for an inhibited keyboard.

Black on white (a per-session option) is derived from the same palette:
background `E6E6E6` (the console's white), green and white text `000000`
(the console's background), the other colours unchanged, cursor `000000`.

## Licence

The font and the console code are Quartz Systems' own, under the QS/400
repository's MIT licence (see `THIRD_PARTY.md`).
