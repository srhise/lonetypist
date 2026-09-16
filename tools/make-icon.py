#!/usr/bin/env python3
"""Render the app icon: a CRT showing the app's own screen.

Glyphs come from the same VGA ROM the app draws with, and the tube around
them is drawn rather than photographed. Everything is deliberately
restrained: an icon is seen at 32 pixels far more often than at 1024, so
the shapes stay bold and the effects stay faint. No libraries -- a PNG is
a header plus deflate, and zlib is in the standard library.
"""
import math
import struct
import sys
import zlib

SIZE = 1024

BLUE = (0x00, 0x00, 0xA8)      # VGA colour 1, the app's background
GREY = (0xC8, 0xC8, 0xC8)      # text, a little brighter than the VGA grey
WHITE = (0xFF, 0xFF, 0xFF)     # the cursor
BEZEL_LIT = (0x44, 0x44, 0x4A)
BEZEL_DIM = (0x18, 0x18, 0x1C)

font = open('assets/fonts/IBM_VGA_8x16.bin', 'rb').read()


def glyph_rows(ch):
    o = ord(ch) * 16
    return font[o:o + 16]


def rounded(px, py, x, y, w, h, r):
    """Signed distance to a rounded rect: negative inside, positive out."""
    qx = abs(px - (x + w / 2)) - (w / 2 - r)
    qy = abs(py - (y + h / 2)) - (h / 2 - r)
    outside = math.hypot(max(qx, 0.0), max(qy, 0.0))
    return outside + min(max(qx, qy), 0.0) - r


def mix(a, b, t):
    t = max(0.0, min(1.0, t))
    return tuple(a[i] + (b[i] - a[i]) * t for i in range(3))


# --- the screen and what is on it -------------------------------------------

BEZEL = SIZE * 0.055
SX = SY = BEZEL
SW = SH = SIZE - 2 * BEZEL

TEXT = "LT"
CELLS = len(TEXT) + 1                      # the cursor takes the third cell
scale = SW * 0.74 / (CELLS * 8)
cw, chh = 8 * scale, 16 * scale

# Lowercase would sit low in the cell; centre on the ink, not the box.
ink_rows = [r for c in TEXT for r, bits in enumerate(glyph_rows(c)) if bits]
top, bottom = min(ink_rows), max(ink_rows) + 1
ox = (SIZE - CELLS * cw) / 2
oy = (SIZE - (bottom - top) * scale) / 2 - top * scale

text_mask = [[0.0] * SIZE for _ in range(SIZE)]
cursor_mask = [[0.0] * SIZE for _ in range(SIZE)]

for i, c in enumerate(TEXT):
    for r, bits in enumerate(glyph_rows(c)):
        for col in range(8):
            if not bits & (1 << (7 - col)):
                continue
            x0, y0 = ox + i * cw + col * scale, oy + r * scale
            for y in range(int(y0), min(int(y0 + scale) + 1, SIZE)):
                for x in range(int(x0), min(int(x0 + scale) + 1, SIZE)):
                    if 0 <= x < SIZE and 0 <= y < SIZE:
                        text_mask[y][x] = 1.0

cx0 = ox + len(TEXT) * cw
for y in range(int(oy + top * scale), int(oy + bottom * scale)):
    for x in range(int(cx0), min(int(cx0 + cw), SIZE)):
        if 0 <= x < SIZE and 0 <= y < SIZE:
            cursor_mask[y][x] = 1.0


def blur(src, radius):
    """Two passes of a box blur: separable, and close enough to gaussian."""
    tmp = [[0.0] * SIZE for _ in range(SIZE)]
    out = [[0.0] * SIZE for _ in range(SIZE)]
    inv = 1.0 / (2 * radius + 1)
    for y in range(SIZE):
        row = src[y]
        acc = sum(row[0:radius + 1])
        for x in range(SIZE):
            tmp[y][x] = acc * inv
            acc += (row[x + radius + 1] if x + radius + 1 < SIZE else 0.0)
            acc -= (row[x - radius] if x - radius >= 0 else 0.0)
    for x in range(SIZE):
        acc = sum(tmp[y][x] for y in range(0, radius + 1))
        for y in range(SIZE):
            out[y][x] = acc * inv
            acc += (tmp[y + radius + 1][x] if y + radius + 1 < SIZE else 0.0)
            acc -= (tmp[y - radius][x] if y - radius >= 0 else 0.0)
    return out


lit = [[min(1.0, text_mask[y][x] + cursor_mask[y][x]) for x in range(SIZE)]
       for y in range(SIZE)]
glow = blur(lit, int(SIZE * 0.014))

# --- compose ----------------------------------------------------------------

px = bytearray()
half = SIZE / 2
for y in range(SIZE):
    for x in range(SIZE):
        d_icon = rounded(x, y, 0, 0, SIZE, SIZE, SIZE * 0.22)
        if d_icon > 0.0:
            # Outside the icon, with one pixel of feather so the corners
            # do not look chewed.
            a = int(255 * max(0.0, 1.0 - d_icon))
            if a <= 0:
                px += bytes((0, 0, 0, 0))
            else:
                px += bytes((BEZEL_DIM[0], BEZEL_DIM[1], BEZEL_DIM[2], a))
            continue

        d_screen = rounded(x, y, SX, SY, SW, SH, SW * 0.10)
        if d_screen > 0.0:
            col = mix(BEZEL_LIT, BEZEL_DIM, (x + y) / (2.0 * SIZE) * 1.3)
            px += bytes((int(col[0]), int(col[1]), int(col[2]), 255))
            continue

        nx, ny = (x - half) / half, (y - half) / half
        r2 = nx * nx + ny * ny

        # Glyphs are sampled straight, never warped: a character grid that
        # goes soft stops reading as one.
        col = BLUE
        if text_mask[y][x]:
            col = GREY
        if cursor_mask[y][x]:
            col = WHITE

        g = glow[y][x]
        col = (col[0] + 120 * g, col[1] + 120 * g, col[2] + 90 * g)

        if y % 2 == 1:                       # scanlines, barely there
            col = (col[0] * 0.93, col[1] * 0.93, col[2] * 0.95)

        vig = 1.0 - 0.28 * r2 * r2           # the tube falls off at the corners
        col = (col[0] * vig, col[1] * vig, col[2] * vig)

        # The glass itself catches a little light at the top.
        sheen = max(0.0, 0.5 - (ny + 0.6) ** 2 - 0.15 * nx * nx) * 0.5
        col = (col[0] + 70 * sheen, col[1] + 70 * sheen, col[2] + 80 * sheen)

        # Feather the very edge of the screen into the bezel.
        edge = min(1.0, -d_screen)
        if edge < 1.0:
            col = mix(BEZEL_DIM, col, edge)

        px += bytes((min(255, int(col[0])), min(255, int(col[1])),
                     min(255, int(col[2])), 255))

# --- write the PNG ----------------------------------------------------------

raw = bytearray()
stride = SIZE * 4
for y in range(SIZE):
    raw.append(0)                            # filter type: none
    raw += px[y * stride:(y + 1) * stride]


def chunk(tag, data):
    return (struct.pack('>I', len(data)) + tag + data +
            struct.pack('>I', zlib.crc32(tag + data) & 0xFFFFFFFF))


out = sys.argv[1] if len(sys.argv) > 1 else 'target/icon.png'
open(out, 'wb').write(
    b'\x89PNG\r\n\x1a\n' +
    chunk(b'IHDR', struct.pack('>IIBBBBB', SIZE, SIZE, 8, 6, 0, 0, 0)) +
    chunk(b'IDAT', zlib.compress(bytes(raw), 9)) +
    chunk(b'IEND', b''))
print(f"wrote {out}")
