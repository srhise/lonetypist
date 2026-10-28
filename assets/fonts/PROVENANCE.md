# Font provenance

Raw character-generator ROM dumps from IBM VGA hardware, obtained from
https://github.com/spacerace/romfont (`font-bin/`).

| File | Bytes | Layout | SHA-256 |
|---|---|---|---|
| `IBM_VGA_8x16.bin` | 4096 | 256 glyphs x 16 rows x 1 byte | `a8bad6fd78475a6bc2a05438c19207a9bb8c0f4f4099f60384e158cdc3eba580` |
| `IBM_VGA_8x8.bin`  | 2048 | 256 glyphs x  8 rows x 1 byte | `75c79a7e7fa423dda67ec6d6d76cec86b63f85677726368750c75b0920ddf319` |

Each byte is one row of one glyph, one bit per pixel, most significant
bit leftmost.

The upstream repository states no license, which is the usual state of
these files and not an obstacle. A bitmap font is a typeface design plus
the data that describes it, and US copyright law protects neither: 37 CFR
202.1(e) excludes typeface as such, and the raster itself carries no
authorship beyond the design. The protection that does attach to fonts
covers scalable font *programs* -- the hinting and outline code -- which a
1987 character generator has none of. These particular ROMs have been
redistributed for decades in emulators and terminal software.

## Courier 10 Pitch

| File | Bytes | SHA-256 |
|---|---|---|
| `Courier10PitchBT-Roman.otf` | 25388 | `d2e693ffb70a47a23456f386c60f6051ad6362b311efcb96c239a62f851e841a` |

Bitstream's Courier 10 Pitch, the IBM typewriter Courier, from the CTAN
`courierten` package (https://ctan.org/pkg/courierten), which converts
Bitstream's Type 1 release to OpenType. Screenplays print in it: the PDF
embeds the font's CFF outlines.

Bitstream's 1989-1992 notice, in `COURIER10-COPYING`, grants use,
copying, modification, sublicensing, sale and redistribution of the
Courier Type 1 fonts "for any purpose and without restriction",
provided the notice is left intact on all copies. It is kept here
beside the font and copied into the app bundle's Resources by both
packaging scripts.
