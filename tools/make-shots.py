#!/usr/bin/env python3
"""Compose the App Store screenshots and marketing images.

    cargo test dump_store_set -- --ignored
    tools/make-shots.py            # -> target/store/*.png

The screens come from the app's own framebuffer dumps, shown the way the
app shows them: stretched to the 4:3 a VGA monitor displayed, with the
CRT pass from `shaders/crt.wgsl` (scanlines at the source row rate,
phosphor bloom, vignette) redone here in numpy. A raw 720x400 dump is
squashed and flat next to what is actually on a customer's display.

The paper comes from the PDFs the same test writes, rendered by PDFKit
through `tools/pdfpage.swift`.

Captions are set in the VGA ROM font the app draws with, stretched to
the same 4:3. The frame is a dark room, lit only by the screen: on the
app's own blue the screen has nothing to separate it from the page.

Needs numpy and Pillow, and swiftc for the page renderer.
"""
import os
import subprocess
import sys

import numpy as np
from PIL import Image, ImageDraw, ImageFilter

W, H = 2880, 1800                    # Apple's 16:10 Mac screenshot size
OUT = 'target/store'
ROOM = np.array([0x0B, 0x0B, 0x12], float) / 255
WHITE = np.array([1.0, 1.0, 1.0])
GREY = np.array([0xA8, 0xA8, 0xB4], float) / 255
AMBER = np.array([0xFC, 0xFC, 0x54], float) / 255   # VGA colour 14
PAPER = np.array([0xF7, 0xF3, 0xEA], float) / 255

FONT = open('assets/fonts/IBM_VGA_8x16.bin', 'rb').read()
PDFPAGE = 'target/pdfpage'


# --- the screen ------------------------------------------------------------

def load(name):
    return np.asarray(Image.open(f'target/store-{name}.bmp').convert('RGB'),
                      float) / 255


def crt(src, height):
    """The framebuffer as the app presents it, `height` pixels tall."""
    width = height * 4 // 3
    # Nearest up to a whole multiple, then area-average down: the same
    # flat texels with a one-pixel blend at their edges that `sharp_uv`
    # gives on the GPU.
    k = max(1, -(-height // src.shape[0]))
    big = Image.fromarray((src * 255).astype(np.uint8)).resize(
        (src.shape[1] * k * 2, src.shape[0] * k * 2), Image.NEAREST)
    img = np.asarray(big.resize((width, height), Image.BOX), float) / 255

    # Bloom: only light in excess of the pixel itself spills, so flat
    # areas keep their colour.
    radius = 0.0016 * width
    glow = np.asarray(Image.fromarray((img * 255).astype(np.uint8)).filter(
        ImageFilter.GaussianBlur(radius)), float) / 255
    img = img + np.maximum(glow - img, 0) * 0.38

    v = (np.arange(height) + 0.5) / height
    scan = 1 - 0.18 * np.sin(v * 400 * np.pi) ** 2
    img = img * scan[:, None, None]

    u = (np.arange(width) + 0.5) / width
    d2 = (u[None, :] - 0.5) ** 2 + (v[:, None] - 0.5) ** 2
    img = img * (1 - 0.12 * d2)[:, :, None]
    return np.clip(img, 0, 1)


def room(w=W, h=H):
    return np.tile(ROOM, (h, w, 1))


def spill(canvas, img, x, y, strength=0.55, reach=140):
    """Light from a lit screen falling on the dark around it."""
    h, w = img.shape[:2]
    pad = reach * 3
    halo = np.zeros((h + 2 * pad, w + 2 * pad, 3))
    halo[pad:pad + h, pad:pad + w] = img.mean(axis=(0, 1))
    halo = np.asarray(Image.fromarray((halo * 255).astype(np.uint8)).filter(
        ImageFilter.GaussianBlur(reach)), float) / 255
    paste_add(canvas, halo * strength, x - pad, y - pad)


def paste_add(canvas, img, x, y):
    region, src = clip(canvas, img, x, y)
    if region is not None:
        canvas[region] += src
        np.clip(canvas, 0, 1, out=canvas)


def paste(canvas, img, x, y, alpha=None):
    region, src = clip(canvas, img, x, y)
    if region is None:
        return
    if alpha is None:
        canvas[region] = src
    else:
        _, a = clip(canvas, alpha[:, :, None], x, y)
        canvas[region] = canvas[region] * (1 - a) + src * a


def clip(canvas, img, x, y):
    h, w = img.shape[:2]
    x0, y0 = max(x, 0), max(y, 0)
    x1, y1 = min(x + w, canvas.shape[1]), min(y + h, canvas.shape[0])
    if x0 >= x1 or y0 >= y1:
        return None, None
    return ((slice(y0, y1), slice(x0, x1)),
            img[y0 - y:y1 - y, x0 - x:x1 - x])


def tube(canvas, img, x, y):
    """The screen with a thin dark edge and its glow on the room."""
    h, w = img.shape[:2]
    spill(canvas, img, x, y)
    edge = 10
    canvas[y - edge:y + h + edge, x - edge:x + w + edge] = 0.03
    paste(canvas, img, x, y)


# --- type ------------------------------------------------------------------

def text_mask(text, scale):
    """CP437 glyphs at a whole-number scale, stretched 4:3 like the screen."""
    rows = np.zeros((16, 8 * len(text)), bool)
    for i, ch in enumerate(text):
        glyph = FONT[ord(ch) * 16:ord(ch) * 16 + 16]
        for r, bits in enumerate(glyph):
            for c in range(8):
                rows[r, i * 8 + c] = bool(bits & (0x80 >> c))
    sy = round(scale * 1.35)
    return np.kron(rows, np.ones((sy, scale), bool))


def write(canvas, text, x, y, scale, colour, align='left'):
    m = text_mask(text, scale)
    if align == 'center':
        x -= m.shape[1] // 2
    region, src = clip(canvas, m.astype(float), x, y)
    if region is not None:
        canvas[region] = np.where(src[:, :, None] > 0, colour, canvas[region])
    return m.shape[0]


def caption(canvas, head, sub, cx=W // 2, top=120, align='center'):
    y = top
    y += write(canvas, head.upper(), cx, y, 8, WHITE, align) + 46
    for line in sub:
        y += write(canvas, line, cx, y, 4, GREY, align) + 18
    return y


# --- paper -----------------------------------------------------------------

def page(pdf, index, scale, crop=None):
    if not os.path.exists(PDFPAGE):
        subprocess.run(['swiftc', '-O', 'tools/pdfpage.swift', '-o', PDFPAGE],
                       check=True)
    out = f'{OUT}/.page.png'
    args = [PDFPAGE, pdf, str(index), out, str(scale)]
    if crop:
        args += [str(v) for v in crop]
    subprocess.run(args, check=True)
    ink = np.asarray(Image.open(out).convert('RGB'), float) / 255
    os.remove(out)
    # Ink multiplies into paper; a little grain keeps the sheet from
    # reading as a screen.
    rng = np.random.default_rng(7)
    grain = 1 - rng.random(ink.shape[:2])[:, :, None] * 0.035
    return np.clip(ink * PAPER * grain, 0, 1)


def sheet(canvas, img, cx, cy, angle):
    """A page lying at an angle, with its shadow."""
    h, w = img.shape[:2]
    rgba = np.dstack([img, np.ones((h, w))])
    pil = Image.fromarray((rgba * 255).astype(np.uint8)).rotate(
        angle, resample=Image.BICUBIC, expand=True)
    rot = np.asarray(pil, float) / 255
    rh, rw = rot.shape[:2]
    x, y = cx - rw // 2, cy - rh // 2
    shadow = np.asarray(Image.fromarray((rot[:, :, 3] * 255).astype(np.uint8))
                        .filter(ImageFilter.GaussianBlur(28)), float) / 255
    region, s = clip(canvas, shadow[:, :, None], x + 18, y + 30)
    if region is not None:
        canvas[region] *= 1 - s * 0.75
    paste(canvas, rot[:, :, :3], x, y, rot[:, :, 3])


# --- the shots -------------------------------------------------------------

def screen_shot(name, head, sub):
    c = room()
    bottom = caption(c, head, sub)
    height = H - bottom - 110
    img = crt(load(name), height)
    tube(c, img, (W - img.shape[1]) // 2, bottom + 40)
    return c


def paper_shot():
    """The hero: the screen, and the page it prints."""
    c = room()
    bottom = caption(c, 'Screenplays, typewritten',
                     ['Format as you type. Print a manuscript that looks '
                      'like it came off a typewriter.'])
    img = crt(load('script'), 1150)
    tube(c, img, 150, bottom + 70)
    paper = page('target/store-fresh.pdf', 1, 1.75)
    sheet(c, paper, 2160, bottom + 70 + 640, -3)
    return c


def ribbon_shot():
    """The same speech on each ribbon, close enough to see the ink."""
    c = room()
    bottom = caption(c, 'Fresh, worn or clean ribbon',
                     ['Every strike lands a little differently. '
                      'Clean is crisp, for contests and agents.'])
    # THEO's first speech on page 1, in points from the top-left.
    crop = (172, 224, 238, 44)
    width, left, gap = 1900, 700, 56
    y = bottom + 70
    for ribbon in ('fresh', 'worn', 'clean'):
        img = page(f'target/store-{ribbon}.pdf', 1, width / crop[2], crop)
        sheet(c, img, left + width // 2, y + img.shape[0] // 2, 0)
        label = text_mask(ribbon.upper(), 6)
        write(c, ribbon.upper(), left - 70 - label.shape[1],
              y + (img.shape[0] - label.shape[0]) // 2, 6, AMBER)
        y += img.shape[0] + gap
    return c


def social_card():
    """The 1200x630 link preview, drawn at twice that and halved."""
    c = room(2400, 1260)
    write(c, 'LONE TYPIST', 110, 110, 9, WHITE)
    write(c, 'A distraction-free writer for the Mac.', 116, 300, 4, GREY)
    write(c, 'Prose and screenplays, typewritten.', 116, 380, 4, AMBER)
    img = crt(load('script'), 700)
    tube(c, img, 110, 500)
    paper = page('target/store-fresh.pdf', 1, 1.45)
    sheet(c, paper, 1850, 800, -3)
    return np.asarray(Image.fromarray((c * 255).astype(np.uint8)).resize(
        (1200, 630), Image.LANCZOS), float) / 255


SHOTS = [
    ('1-typewritten', paper_shot),
    ('2-screenplay', lambda: screen_shot(
        'script', 'Enter and Tab do the formatting',
        ['Headings, cues and dialogue fall into place. '
         'Names complete as you type.'])),
    ('3-prose', lambda: screen_shot(
        'prose', 'One document. One screen.',
        ['Nothing else. Except, if you like, the sound of a typewriter.'])),
    ('4-scenes', lambda: screen_shot(
        'scenes', 'Jump to any scene',
        ['Page numbers on screen match the printed page.'])),
    ('5-ribbon', ribbon_shot),
    ('social', social_card),
    ('6-menus', lambda: screen_shot(
        'menu', 'Every shortcut, on screen',
        ['Menus that teach you the keys as you use them.'])),
]


def main():
    os.makedirs(OUT, exist_ok=True)
    only = set(sys.argv[1:])
    for name, make in SHOTS:
        if only and name not in only:
            continue
        img = make()
        path = f'{OUT}/{name}.png'
        Image.fromarray((np.clip(img, 0, 1) * 255).astype(np.uint8)).save(path)
        print(path)


main()
