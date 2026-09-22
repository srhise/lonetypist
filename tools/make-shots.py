#!/usr/bin/env python3
"""Compose App Store screenshots from the app's own framebuffer dumps.

A store screenshot gets about a second, so each one carries a short line
above the screen it is showing. The text is set in the same VGA ROM font
the app draws with: a caption in Helvetica over a DOS screen would read
as an apology for the DOS screen.

The frame is dark rather than the app's blue. On blue the screen has
nothing to separate it from the page and the whole thing goes flat; on
near-black it reads as a lit monitor in an unlit room, which is the
point of the app.

Sources are the BMPs from `cargo test -- --ignored`. Scaling is always by
a whole number -- a character grid resampled to a fraction is mush.

    tools/make-shots.py target/app-preview.bmp "Nothing but the page" out.png
"""
import struct
import sys
import zlib

OW, OH = 2880, 1800            # Apple's 16:10 Mac screenshot size
BACK = (0x0E, 0x0E, 0x14)      # the dark room around the tube
BEZEL = (0x3A, 0x3A, 0x42)     # the tube's edge
WHITE = (0xFF, 0xFF, 0xFF)
AMBER = (0xFC, 0xFC, 0x54)     # VGA colour 14, for the odd accent

CAPTION_TOP = 150              # where the caption sits
BAND = 430                     # room reserved for it
BOTTOM = 150                   # breathing room under the screen

FONT = open('assets/fonts/IBM_VGA_8x16.bin', 'rb').read()


def read_bmp(path):
    """24-bit bottom-up BMP in, (width, height, top-down rows of RGB) out."""
    data = open(path, 'rb').read()
    off = struct.unpack_from('<I', data, 10)[0]
    w = struct.unpack_from('<i', data, 18)[0]
    h = struct.unpack_from('<i', data, 22)[0]
    bpp = struct.unpack_from('<H', data, 28)[0]
    if bpp != 24:
        raise SystemExit(f"{path}: expected 24-bit, got {bpp}")
    stride = (w * 3 + 3) // 4 * 4
    rows = []
    for y in range(h - 1, -1, -1):
        base = off + y * stride
        rows.append([(data[base + x * 3 + 2], data[base + x * 3 + 1],
                      data[base + x * 3]) for x in range(w)])
    return w, h, rows


def write_png(path, width, height, pixels):
    raw = bytearray()
    for row in pixels:
        raw.append(0)
        for px in row:
            raw += bytes(px)

    def chunk(tag, payload):
        return (struct.pack('>I', len(payload)) + tag + payload +
                struct.pack('>I', zlib.crc32(tag + payload) & 0xFFFFFFFF))

    open(path, 'wb').write(
        b'\x89PNG\r\n\x1a\n' +
        chunk(b'IHDR', struct.pack('>IIBBBBB', width, height, 8, 2, 0, 0, 0)) +
        chunk(b'IDAT', zlib.compress(bytes(raw), 9)) +
        chunk(b'IEND', b''))


def draw_text(canvas, text, cx, top, scale, colour=WHITE):
    """Blit CP437 glyphs at a whole-number scale, centred on cx."""
    x0 = cx - (len(text) * 8 * scale) // 2
    for i, ch in enumerate(text):
        glyph = FONT[ord(ch) * 16:ord(ch) * 16 + 16]
        for r, bits in enumerate(glyph):
            if not bits:
                continue
            for c in range(8):
                if not bits & (1 << (7 - c)):
                    continue
                px, py = x0 + (i * 8 + c) * scale, top + r * scale
                for dy in range(scale):
                    row = canvas[py + dy] if 0 <= py + dy < OH else None
                    if row is None:
                        continue
                    for dx in range(scale):
                        if 0 <= px + dx < OW:
                            row[px + dx] = colour


def wrap(caption, limit):
    lines, line = [], ''
    for word in caption.split():
        trial = (line + ' ' + word).strip()
        if len(trial) > limit and line:
            lines.append(line)
            line = word
        else:
            line = trial
    lines.append(line)
    return lines


def main():
    if len(sys.argv) < 4:
        raise SystemExit(__doc__)
    src, caption, dst = sys.argv[1], sys.argv[2].upper(), sys.argv[3]

    w, h, rows = read_bmp(src)
    canvas = [[BACK] * OW for _ in range(OH)]

    # Fill the frame with the app: take the largest whole-number scale the
    # space allows, so the screen dominates rather than floats.
    scale = max(1, min((OW - 240) // w, (OH - BAND - BOTTOM) // h))
    sw, sh = w * scale, h * scale
    ox, oy = (OW - sw) // 2, BAND + (OH - BAND - BOTTOM - sh) // 2

    lines = wrap(caption, 30)
    cap_scale = 9 if max(len(l) for l in lines) <= 22 else 7
    y = CAPTION_TOP
    for line in lines:
        draw_text(canvas, line, OW // 2, y, cap_scale)
        y += 16 * cap_scale + 24

    # The tube's edge: a band of grey, then black, so the lit screen has
    # something to sit inside.
    for pad, colour in ((14, BEZEL), (6, (0, 0, 0))):
        for x in range(ox - pad, ox + sw + pad):
            for yy in list(range(oy - pad, oy - pad + 8)) + \
                      list(range(oy + sh + pad - 8, oy + sh + pad)):
                if 0 <= x < OW and 0 <= yy < OH:
                    canvas[yy][x] = colour
        for yy in range(oy - pad, oy + sh + pad):
            for x in list(range(ox - pad, ox - pad + 8)) + \
                     list(range(ox + sw + pad - 8, ox + sw + pad)):
                if 0 <= x < OW and 0 <= yy < OH:
                    canvas[yy][x] = colour

    for yy in range(sh):
        srow = rows[yy // scale]
        crow = canvas[oy + yy]
        for x in range(sw):
            crow[ox + x] = srow[x // scale]

    write_png(dst, OW, OH, canvas)
    print(f"{dst}: {w}x{h} at {scale}x -> {sw}x{sh}, caption {lines}")


main()
