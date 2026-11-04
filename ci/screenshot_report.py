#!/usr/bin/env python3
# Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
# licensed under the terms of the MIT license which can be found in the
# root directory of this project.
"""Side-by-side screenshot report: Windows against Linux.

Usage:
    screenshot_report.py OUT_DIR LABEL=DIR [LABEL=DIR ...]

The first LABEL=DIR is the reference. Every other directory is compared
with it, view by view (one PNG per view, matched by file name). The report
is OUT_DIR/index.html, with the images copied beside it and, for each pair
of the same size, a difference image in which differing pixels are red.

Standard library only: PNG is read and written here with zlib.
"""

import html
import os
import shutil
import struct
import sys
import zlib

TOLERANCE = int(os.environ.get("QSA_SCREENSHOT_TOLERANCE", "40"))


def read_png(path):
    """Returns (width, height, rows of RGB bytes) for an 8-bit RGB/RGBA PNG."""
    data = open(path, "rb").read()
    if data[:8] != b"\x89PNG\r\n\x1a\n":
        raise ValueError(f"{path}: not a PNG")
    pos, idat, width, height, ctype = 8, b"", 0, 0, 2
    while pos < len(data):
        (n,) = struct.unpack(">I", data[pos : pos + 4])
        kind = data[pos + 4 : pos + 8]
        body = data[pos + 8 : pos + 8 + n]
        if kind == b"IHDR":
            width, height, depth, ctype, _, _, interlace = struct.unpack(">IIBBBBB", body)
            if depth != 8 or ctype not in (2, 6) or interlace:
                raise ValueError(f"{path}: only 8-bit RGB/RGBA, not interlaced")
        elif kind == b"IDAT":
            idat += body
        pos += 12 + n
    bpp = 3 if ctype == 2 else 4
    raw = zlib.decompress(idat)
    stride = width * bpp
    rows, prev = [], bytearray(stride)
    for y in range(height):
        f = raw[y * (stride + 1)]
        line = bytearray(raw[y * (stride + 1) + 1 : (y + 1) * (stride + 1)])
        for i in range(stride):
            a = line[i - bpp] if i >= bpp else 0
            b = prev[i]
            c = prev[i - bpp] if i >= bpp else 0
            if f == 1:
                line[i] = (line[i] + a) & 0xFF
            elif f == 2:
                line[i] = (line[i] + b) & 0xFF
            elif f == 3:
                line[i] = (line[i] + (a + b) // 2) & 0xFF
            elif f == 4:
                p = a + b - c
                pa, pb, pc = abs(p - a), abs(p - b), abs(p - c)
                pred = a if pa <= pb and pa <= pc else (b if pb <= pc else c)
                line[i] = (line[i] + pred) & 0xFF
        prev = line
        if bpp == 4:
            line = bytearray(b for j, b in enumerate(line) if j % 4 != 3)
        rows.append(bytes(line))
    return width, height, rows


def write_png(path, width, height, rows):
    def chunk(kind, body):
        crc = zlib.crc32(kind + body) & 0xFFFFFFFF
        return struct.pack(">I", len(body)) + kind + body + struct.pack(">I", crc)

    raw = b"".join(b"\x00" + r for r in rows)
    ihdr = struct.pack(">IIBBBBB", width, height, 8, 2, 0, 0, 0)
    with open(path, "wb") as f:
        f.write(b"\x89PNG\r\n\x1a\n")
        f.write(chunk(b"IHDR", ihdr))
        f.write(chunk(b"IDAT", zlib.compress(raw, 9)))
        f.write(chunk(b"IEND", b""))


def compare(a, b):
    """Returns (differing pixels, difference image rows), or None if sizes differ."""
    (wa, ha, ra), (wb, hb, rb) = a, b
    if (wa, ha) != (wb, hb):
        return None
    count, out = 0, []
    for la, lb in zip(ra, rb):
        line = bytearray()
        for i in range(0, len(la), 3):
            pa, pb = la[i : i + 3], lb[i : i + 3]
            if any(abs(x - y) > TOLERANCE for x, y in zip(pa, pb)):
                count += 1
                line += b"\xff\x00\x00"
            else:
                line += bytes(0xC0 + v // 4 for v in pa)
        out.append(bytes(line))
    return count, out


def main(argv):
    if len(argv) < 3 or any("=" not in a for a in argv[2:]):
        print(__doc__)
        return 2
    out = argv[1]
    sets = [a.split("=", 1) for a in argv[2:]]
    os.makedirs(out, exist_ok=True)
    views = sorted(
        {
            f[:-4]
            for _, d in sets
            if os.path.isdir(d)
            for f in os.listdir(d)
            if f.endswith(".png") and not f.endswith(".diff.png")
        }
    )
    ref_label, ref_dir = sets[0]
    head = "".join(f"<th>{html.escape(label)}</th>" for label, _ in sets)
    head += "".join(
        f"<th>{html.escape(label)} vs {html.escape(ref_label)}</th>" for label, _ in sets[1:]
    )
    body = []
    worst = 0.0
    for view in views:
        cells, diffs = [], []
        ref_img = None
        ref_path = os.path.join(ref_dir, view + ".png")
        if os.path.exists(ref_path):
            ref_img = read_png(ref_path)
        for n, (label, d) in enumerate(sets):
            src = os.path.join(d, view + ".png")
            if not os.path.exists(src):
                cells.append("<td>missing</td>")
                if n:
                    diffs.append("<td></td>")
                continue
            name = f"{view}.{label}.png"
            shutil.copyfile(src, os.path.join(out, name))
            cells.append(f'<td><img src="{html.escape(name)}"></td>')
            if n == 0:
                continue
            if ref_img is None:
                diffs.append("<td>no reference</td>")
                continue
            result = compare(ref_img, read_png(src))
            if result is None:
                diffs.append("<td>sizes differ</td>")
                worst = 100.0
                continue
            count, rows = result
            w, h, _ = ref_img
            pct = 100.0 * count / (w * h)
            worst = max(worst, pct)
            dname = f"{view}.{label}.diff.png"
            write_png(os.path.join(out, dname), w, h, rows)
            diffs.append(
                f'<td><img src="{html.escape(dname)}"><br>{count} pixels ({pct:.2f}%) differ</td>'
            )
        body.append(f"<tr><th>{html.escape(view)}</th>{''.join(cells)}{''.join(diffs)}</tr>")
    page = f"""<!DOCTYPE html>
<html><head><meta charset="utf-8"><title>QS/400 Access for Desktop: screenshots</title>
<style>
body {{ font: 13px sans-serif; margin: 16px; }}
table {{ border-collapse: collapse; }}
th, td {{ border: 1px solid #ccc; padding: 6px; vertical-align: top; text-align: left; }}
img {{ max-width: 520px; display: block; }}
</style></head><body>
<h1>Screenshots: {html.escape(" / ".join(label for label, _ in sets))}</h1>
<p>Pixels differing by more than {TOLERANCE} in any channel are red in the
difference images. Largest difference: {worst:.2f}% of a view.</p>
<table><tr><th>View</th>{head}</tr>
{"".join(body)}
</table></body></html>
"""
    with open(os.path.join(out, "index.html"), "w", encoding="utf-8") as f:
        f.write(page)
    print(f"report: {os.path.join(out, 'index.html')} (largest difference {worst:.2f}%)")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
