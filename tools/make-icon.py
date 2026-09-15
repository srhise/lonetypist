#!/usr/bin/env python3
"""Render the app icon from the same VGA ROM font the app draws with."""
import struct, sys, os

SIZE = 1024
BLUE = (0x00, 0x00, 0xAA)
WHITE = (0xAA, 0xAA, 0xAA)
BRIGHT = (0xFF, 0xFF, 0xFF)

font = open('assets/fonts/IBM_VGA_8x16.bin', 'rb').read()

def glyph_rows(ch):
    o = ord(ch) * 16
    return font[o:o + 16]

px = [[BLUE] * SIZE for _ in range(SIZE)]

# "LT" plus a block cursor, three cells wide, scaled to fill the icon.
# The full name would be mush at 16x16, so the icon keeps the prompt short.
# Uppercase, because the ROM's lowercase l is indistinguishable from a 1.
text = "LT"
cells = len(text) + 1
scale = SIZE * 82 // 100 // (cells * 8)       # leave a margin
cw, chh = 8 * scale, 16 * scale
total_w = cells * cw

# Lowercase occupies only the middle rows of a 16-row cell, so centring
# the cell box leaves the letters visibly low. Centre the ink instead.
ink_rows = [r for ch in text for r, bits in enumerate(glyph_rows(ch)) if bits]
top, bottom = min(ink_rows), max(ink_rows) + 1
ink_h = (bottom - top) * scale

ox = (SIZE - total_w) // 2
oy = (SIZE - ink_h) // 2 - top * scale

for i, ch in enumerate(text):
    rows = glyph_rows(ch)
    for r, bits in enumerate(rows):
        for c in range(8):
            if bits & (1 << (7 - c)):
                for sy in range(scale):
                    for sx in range(scale):
                        y = oy + r * scale + sy
                        x = ox + i * cw + c * scale + sx
                        if 0 <= x < SIZE and 0 <= y < SIZE:
                            px[y][x] = WHITE

# The block cursor: a filled cell in bright white.
bx = ox + len(text) * cw
for y in range(oy + top * scale, oy + bottom * scale):
    for x in range(bx, bx + cw):
        if 0 <= x < SIZE and 0 <= y < SIZE:
            px[y][x] = BRIGHT

# 24-bit BMP, bottom-up, BGR.
row_bytes = SIZE * 3
pad = (4 - row_bytes % 4) % 4
body = bytearray()
for y in range(SIZE - 1, -1, -1):
    for x in range(SIZE):
        r, g, b = px[y][x]
        body += bytes((b, g, r))
    body += b'\x00' * pad

hdr = b'BM' + struct.pack('<IHHI', 54 + len(body), 0, 0, 54)
hdr += struct.pack('<IiiHHIIiiII', 40, SIZE, SIZE, 1, 24, 0, len(body), 2835, 2835, 0, 0)
out = sys.argv[1] if len(sys.argv) > 1 else 'target/icon.bmp'
os.makedirs(os.path.dirname(out) or '.', exist_ok=True)
open(out, 'wb').write(hdr + body)
print(f"wrote {out}")
