//! A screenplay printed as if it were typed: a PDF of US Letter pages in
//! Courier 10 Pitch, every character struck by hand onto the 10 cpi /
//! 6 lpi grid.
//!
//! What makes a page look typed is not the typeface. It is that no two
//! strikes are alike: a type bar lands a hair high, the ribbon is
//! fresher under one letter than the next, the ink spreads into the
//! paper fibres. Each strike here gets its own position, density and
//! spread, from a hash of where it falls on the page, so the variance
//! is the same every time the script prints. A worn ribbon is lighter
//! and patchier; a clean one switches all of it off.
//!
//! The emphasis is a typewriter's too: there was no italic, so italic
//! and underline both overstrike an underscore, and bold strikes twice.
//!
//! The PDF is written by hand. It needs one font, one resource
//! dictionary, and a content stream per page, and that is all.

use serde::{Deserialize, Serialize};

use crate::script::PrintLine;

/// The typeface: Bitstream's Courier 10 Pitch, IBM's typewriter Courier.
/// See `assets/fonts/COURIER10-COPYING`.
const FONT: &[u8] = include_bytes!("../assets/fonts/Courier10PitchBT-Roman.otf");
const FONT_NAME: &str = "Courier10PitchBT-Roman";
/// The font's own advance, in thousandths of an em. Strikes are placed
/// on the grid regardless; this only keeps text selection honest.
const FONT_ADVANCE: u32 = 602;

const PAGE_W: f32 = 612.0;
const PAGE_H: f32 = 792.0;
const POINT_SIZE: f32 = 12.0;
/// 10 characters to the inch.
const PITCH: f32 = 7.2;
/// 6 lines to the inch.
const LEADING: f32 = 12.0;
/// Where the baseline sits below the top of a line.
const BASELINE: f32 = 9.5;
/// Body text begins an inch down.
const TOP_ROWS: usize = 6;
/// Page numbers sit half an inch down, flush with the right margin.
const NUMBER_ROW: usize = 3;
const RIGHT_EDGE: usize = crate::script::RIGHT_EDGE;
const LEFT_EDGE: usize = crate::script::ACTION_COL;

/// Opacity steps for ink density. Each is one graphics state.
const DENSITY_LEVELS: usize = 16;

#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Ribbon {
    /// A new ribbon: dark, a little uneven.
    #[default]
    Fresh,
    /// A ribbon near the end of its life: grey, patchy.
    Worn,
    /// No typewriter at all: even black type on the grid, for contest
    /// and agency submissions.
    Clean,
}

impl Ribbon {
    pub fn name(self) -> &'static str {
        match self {
            Ribbon::Fresh => "Fresh",
            Ribbon::Worn => "Worn",
            Ribbon::Clean => "Clean",
        }
    }

    pub fn next(self) -> Self {
        match self {
            Ribbon::Fresh => Ribbon::Worn,
            Ribbon::Worn => Ribbon::Clean,
            Ribbon::Clean => Ribbon::Fresh,
        }
    }
}

/// How a ribbon strikes.
struct Strike {
    /// Ink density range for an ordinary strike.
    density: (f32, f32),
    /// Chance a strike comes out starved of ink, and how light it is.
    starved: (f32, f32),
    /// Largest sideways and vertical slop, in points.
    jitter: (f32, f32),
    /// Largest rotation, in degrees.
    tilt: f32,
    /// Stroke width round each glyph: ink spreading into the paper.
    spread: f32,
    /// Chance of a faint second impression as the type bar bounces.
    bounce: f32,
    /// How strongly the ribbon's weave shows through: the opacity of
    /// the voids where the fabric carried no ink.
    weave: f32,
}

impl Ribbon {
    fn strike(self) -> Option<Strike> {
        match self {
            Ribbon::Fresh => Some(Strike {
                density: (0.88, 1.0),
                starved: (0.01, 0.72),
                jitter: (0.10, 0.14),
                tilt: 0.25,
                spread: 0.24,
                bounce: 0.03,
                weave: 0.22,
            }),
            Ribbon::Worn => Some(Strike {
                density: (0.70, 0.92),
                starved: (0.03, 0.55),
                jitter: (0.13, 0.18),
                tilt: 0.35,
                spread: 0.16,
                bounce: 0.02,
                weave: 0.5,
            }),
            Ribbon::Clean => None,
        }
    }
}

/// A page to print and the number it carries. Page one carries none.
pub struct Sheet<'a> {
    pub number: usize,
    pub lines: &'a [PrintLine],
}

/// The whole job, as PDF bytes.
pub fn render(title: &[(String, String)], sheets: &[Sheet], ribbon: Ribbon) -> Vec<u8> {
    let mut contents: Vec<Vec<u8>> = Vec::new();
    if let Some(page) = title_sheet(title) {
        contents.push(type_page(&page, 0, ribbon));
    }
    for sheet in sheets {
        let mut page: Vec<(usize, usize, String)> = sheet
            .lines
            .iter()
            .map(|l| (TOP_ROWS + l.row, l.col, l.text.clone()))
            .collect();
        if sheet.number > 1 {
            let n = format!("{}.", sheet.number);
            page.push((NUMBER_ROW, RIGHT_EDGE - n.len(), n));
        }
        contents.push(type_page(&page, sheet.number, ribbon));
    }
    if contents.is_empty() {
        contents.push(Vec::new());
    }
    let name = title
        .iter()
        .find(|(k, _)| k == "title")
        .map(|(_, v)| v.replace('\n', " "))
        .unwrap_or_default();
    let weave = ribbon.strike().map_or(0.0, |s| s.weave);
    assemble(&contents, &name, weave)
}

/// The title page as (row, column, text) on the sheet: the title a third
/// of the way down, the credits under it, contact and draft date at the
/// foot. `None` when the script has no title page.
fn title_sheet(fields: &[(String, String)]) -> Option<Vec<(usize, usize, String)>> {
    if fields.is_empty() {
        return None;
    }
    let get = |key: &str| {
        fields
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
            .filter(|v| !v.trim().is_empty())
    };
    let mut out = Vec::new();
    let centre =
        |s: &str| LEFT_EDGE + (RIGHT_EDGE - LEFT_EDGE).saturating_sub(s.chars().count()) / 2;
    let mut row = 21;
    if let Some(title) = get("title") {
        for line in title.lines() {
            // Underlined, the typist's way of setting a title apart.
            let t = format!("_{}_", line.trim().to_uppercase());
            out.push((row, centre(line.trim()), t));
            row += 1;
        }
    }
    row += 3;
    for key in ["credit", "author", "authors", "source"] {
        if let Some(v) = get(key) {
            for line in v.lines() {
                out.push((row, centre(line.trim()), line.trim().to_string()));
                row += 1;
            }
            row += 1;
        }
    }
    let foot = 59;
    if let Some(contact) = get("contact") {
        let lines: Vec<&str> = contact.lines().collect();
        let top = foot + 1 - lines.len();
        for (i, line) in lines.iter().enumerate() {
            out.push((top + i, LEFT_EDGE, line.trim().to_string()));
        }
    }
    let mut right = Vec::new();
    for key in ["draft date", "date", "copyright"] {
        if let Some(v) = get(key) {
            right.extend(v.lines().map(|l| l.trim().to_string()));
        }
    }
    let top = foot + 1 - right.len().min(foot);
    for (i, line) in right.iter().enumerate() {
        let col = RIGHT_EDGE.saturating_sub(line.chars().count());
        out.push((top + i, col, line.clone()));
    }
    Some(out)
}

/// A character with the overstrikes it calls for.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
struct Typed {
    c: char,
    underline: bool,
    bold: bool,
}

/// Fountain's emphasis in a line, read the way a typist would set it:
/// `*italic*` and `_underline_` are underscored, `**bold**` is struck
/// twice. Markers that do not pair up are typed as they stand.
fn emphasis(line: &str) -> Vec<Typed> {
    let plain = || {
        line.chars()
            .map(|c| Typed {
                c,
                underline: false,
                bold: false,
            })
            .collect()
    };
    let chars: Vec<char> = line.chars().collect();
    let (mut italic, mut bold, mut under) = (false, false, false);
    let mut out = Vec::with_capacity(chars.len());
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '\\' && i + 1 < chars.len() && matches!(chars[i + 1], '*' | '_') {
            out.push(Typed {
                c: chars[i + 1],
                underline: italic || under,
                bold,
            });
            i += 2;
            continue;
        }
        if c == '*' {
            let run = chars[i..].iter().take_while(|&&x| x == '*').count();
            match run {
                1 => italic = !italic,
                2 => bold = !bold,
                _ => {
                    italic = !italic;
                    bold = !bold;
                }
            }
            i += run;
            continue;
        }
        if c == '_' {
            under = !under;
            i += 1;
            continue;
        }
        out.push(Typed {
            c,
            underline: italic || under,
            bold,
        });
        i += 1;
    }
    if italic || bold || under {
        return plain();
    }
    out
}

/// SplitMix64: a hash good enough to make strikes look independent.
fn mix(mut x: u64) -> u64 {
    x = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
    x = (x ^ (x >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    x ^ (x >> 31)
}

/// A repeatable random number in 0..1 for a strike.
fn unit(seed: u64, salt: u64) -> f32 {
    (mix(seed ^ mix(salt)) >> 40) as f32 / (1u64 << 24) as f32
}

/// The same in -1..1.
fn signed(seed: u64, salt: u64) -> f32 {
    unit(seed, salt) * 2.0 - 1.0
}

/// WinAnsiEncoding, the font's byte for a character. Anything it cannot
/// type comes out as a question mark, as a typist would improvise.
fn win_ansi(c: char) -> u8 {
    const HIGH: &[(char, u8)] = &[
        ('\u{20AC}', 0x80),
        ('\u{201A}', 0x82),
        ('\u{0192}', 0x83),
        ('\u{201E}', 0x84),
        ('\u{2026}', 0x85),
        ('\u{2020}', 0x86),
        ('\u{2021}', 0x87),
        ('\u{02C6}', 0x88),
        ('\u{2030}', 0x89),
        ('\u{0160}', 0x8A),
        ('\u{2039}', 0x8B),
        ('\u{0152}', 0x8C),
        ('\u{017D}', 0x8E),
        ('\u{2018}', 0x91),
        ('\u{2019}', 0x92),
        ('\u{201C}', 0x93),
        ('\u{201D}', 0x94),
        ('\u{2022}', 0x95),
        ('\u{2013}', 0x96),
        ('\u{2014}', 0x97),
        ('\u{02DC}', 0x98),
        ('\u{2122}', 0x99),
        ('\u{0161}', 0x9A),
        ('\u{203A}', 0x9B),
        ('\u{0153}', 0x9C),
        ('\u{017E}', 0x9E),
        ('\u{0178}', 0x9F),
    ];
    match c as u32 {
        0x20..=0x7E | 0xA0..=0xFF => c as u8,
        _ => HIGH
            .iter()
            .find(|(h, _)| *h == c)
            .map(|(_, b)| *b)
            .unwrap_or(b'?'),
    }
}

/// One glyph struck onto the page.
struct Hit {
    byte: u8,
    x: f32,
    y: f32,
    tilt: f32,
    density: f32,
}

/// Every strike on a page, as the content stream that draws them.
fn type_page(lines: &[(usize, usize, String)], page: usize, ribbon: Ribbon) -> Vec<u8> {
    let strike = ribbon.strike();
    let mut hits: Vec<Hit> = Vec::new();
    for (row, col, text) in lines {
        let line_seed = mix(((page as u64) << 32) ^ (*row as u64));
        // The platen does not advance perfectly: each line sits a touch
        // high or low as a whole.
        let line_drift = strike
            .as_ref()
            .map_or(0.0, |s| signed(line_seed, 1) * s.jitter.1 * 0.6);
        let baseline = PAGE_H - (*row as f32 * LEADING + BASELINE) + line_drift;
        for (i, t) in emphasis(text).iter().enumerate() {
            let x = (*col + i) as f32 * PITCH;
            if t.c != ' ' {
                strike_one(&mut hits, &strike, line_seed, i, t.c, x, baseline, 0);
                if t.bold {
                    strike_one(&mut hits, &strike, line_seed, i, t.c, x + 0.45, baseline, 1);
                }
            }
            if t.underline {
                strike_one(&mut hits, &strike, line_seed, i, '_', x, baseline, 2);
            }
        }
    }

    let mut out = String::new();
    out.push_str(&format!("BT\n/F1 {POINT_SIZE} Tf\n"));
    match &strike {
        // Fill and stroke: the stroke is the ink spreading.
        Some(s) => out.push_str(&format!("2 Tr {:.2} w 1 j\n", s.spread)),
        None => out.push_str("0 Tr\n"),
    }
    // Ink is a warm near-black, not printer black.
    let ink = if strike.is_some() {
        "0.10 0.09 0.10"
    } else {
        "0 0 0"
    };
    out.push_str(&format!("{ink} rg {ink} RG\n"));
    let mut level = usize::MAX;
    for h in &hits {
        let l =
            ((h.density * (DENSITY_LEVELS - 1) as f32).round() as usize).min(DENSITY_LEVELS - 1);
        if l != level {
            out.push_str(&format!("/G{l} gs\n"));
            level = l;
        }
        let (sin, cos) = h.tilt.to_radians().sin_cos();
        out.push_str(&format!(
            "{:.4} {:.4} {:.4} {:.4} {:.2} {:.2} Tm ({}) Tj\n",
            cos,
            sin,
            -sin,
            cos,
            h.x,
            h.y,
            escape(h.byte)
        ));
    }
    out.push_str("ET\n");

    // The ribbon's weave: clip to every glyph just struck, then lay the
    // void texture over the page in paper white. Ink only shows it where
    // there is ink.
    if strike.is_some() && !hits.is_empty() {
        {
            out.push_str(&format!("q\nBT\n/F1 {POINT_SIZE} Tf\n7 Tr\n"));
            for h in &hits {
                let (sin, cos) = h.tilt.to_radians().sin_cos();
                out.push_str(&format!(
                    "{:.4} {:.4} {:.4} {:.4} {:.2} {:.2} Tm ({}) Tj\n",
                    cos,
                    sin,
                    -sin,
                    cos,
                    h.x,
                    h.y,
                    escape(h.byte)
                ));
            }
            out.push_str("ET\n");
            // Shift the tile per page so pages do not share a texture.
            let (ox, oy) = (
                unit(page as u64, 1) * WEAVE_TILE,
                unit(page as u64, 2) * WEAVE_TILE,
            );
            out.push_str(&format!(
                "/Weave gs /CsP cs 1 1 1 /P1 scn\n{:.2} {:.2} {} {} re f\nQ\n",
                -ox,
                -oy,
                PAGE_W + WEAVE_TILE,
                PAGE_H + WEAVE_TILE
            ));
        }
    }
    out.into_bytes()
}

/// Bits along each side of the weave texture.
const WEAVE_BITS: usize = 256;
/// The tile's size on the page, in points: about 0.3pt to a bit, the
/// scale of the voids a fabric ribbon leaves.
const WEAVE_TILE: f32 = 76.8;

/// The ribbon's weave as a 1-bit mask, a set bit where no ink landed.
/// Fine specks everywhere, short threads along the weave's diagonal,
/// and broad patches where the ribbon is drier than elsewhere.
fn weave_mask() -> Vec<u8> {
    let n = WEAVE_BITS;
    let mut bits = vec![false; n * n];
    // A coarse field, smoothly interpolated: the dry patches.
    let grid = 8;
    let cell = n / grid;
    let corner = |gx: usize, gy: usize| unit(((gx % grid) * 131 + (gy % grid)) as u64, 77);
    for y in 0..n {
        for x in 0..n {
            let (gx, gy) = (x / cell, y / cell);
            let (fx, fy) = (
                (x % cell) as f32 / cell as f32,
                (y % cell) as f32 / cell as f32,
            );
            let (sx, sy) = (fx * fx * (3.0 - 2.0 * fx), fy * fy * (3.0 - 2.0 * fy));
            let top = corner(gx, gy) * (1.0 - sx) + corner(gx + 1, gy) * sx;
            let bottom = corner(gx, gy + 1) * (1.0 - sx) + corner(gx + 1, gy + 1) * sx;
            let dry = top * (1.0 - sy) + bottom * sy;
            let p = 0.04 + 0.10 * (dry - 0.45).max(0.0);
            if unit((y * n + x) as u64, 91) < p {
                bits[y * n + x] = true;
            }
        }
    }
    // Threads: short runs on the diagonal of the weave.
    for t in 0..(n * n / 160) {
        let seed = mix(t as u64 ^ 0xFAB);
        let (mut x, mut y) = (
            (seed % n as u64) as usize,
            ((seed >> 20) % n as u64) as usize,
        );
        let len = 2 + (seed >> 40) as usize % 4;
        for _ in 0..len {
            bits[y * n + x] = true;
            x = (x + 1) % n;
            y = (y + 1) % n;
        }
    }
    let mut out = Vec::with_capacity(n * n / 8);
    for row in bits.chunks(n) {
        for byte in row.chunks(8) {
            let b = byte
                .iter()
                .enumerate()
                .fold(0u8, |acc, (i, &on)| acc | (u8::from(on) << (7 - i)));
            out.push(b);
        }
    }
    out
}

#[allow(clippy::too_many_arguments)]
fn strike_one(
    hits: &mut Vec<Hit>,
    strike: &Option<Strike>,
    line_seed: u64,
    col: usize,
    c: char,
    x: f32,
    y: f32,
    pass: u64,
) {
    let byte = win_ansi(c);
    let Some(s) = strike else {
        hits.push(Hit {
            byte,
            x,
            y,
            tilt: 0.0,
            density: 1.0,
        });
        return;
    };
    let seed = mix(line_seed ^ ((col as u64) << 8) ^ (pass << 56));
    // Every key has its own habits: a type bar bent a little, a face
    // that hits harder. These follow the character, not the position,
    // so every "e" on the page leans the same way.
    let key = mix(0xC0FFEE ^ c as u64);
    let key_dy = signed(key, 1) * 0.10;
    let key_dx = signed(key, 2) * 0.06;
    let key_weight = signed(key, 3) * 0.03;

    let dx = signed(seed, 10) * s.jitter.0 + key_dx;
    let dy = signed(seed, 11) * s.jitter.1 + key_dy;
    let tilt = signed(seed, 12) * s.tilt;
    let (lo, hi) = s.density;
    // Skewed towards the dark end: most strikes land well.
    let mut density = lo + (hi - lo) * unit(seed, 13).sqrt() + key_weight;
    if matches!(c, '.' | ',' | ':' | ';' | '\'' | '-') {
        // Small faces concentrate the blow.
        density += 0.04;
    }
    if unit(seed, 14) < s.starved.0 {
        density = s.starved.1 + unit(seed, 15) * 0.15;
    }
    let density = density.clamp(0.2, 1.0);
    hits.push(Hit {
        byte,
        x: x + dx,
        y: y + dy,
        tilt,
        density,
    });
    if unit(seed, 16) < s.bounce {
        // The type bar bounces off the platen and kisses it again.
        hits.push(Hit {
            byte,
            x: x + dx + 0.35 + signed(seed, 17) * 0.15,
            y: y + dy - 0.25,
            tilt,
            density: 0.22,
        });
    }
}

/// A byte as it goes inside a PDF string.
fn escape(b: u8) -> String {
    match b {
        b'(' | b')' | b'\\' => format!("\\{}", b as char),
        0x20..=0x7E => (b as char).to_string(),
        _ => format!("\\{b:03o}"),
    }
}

/// The font's CFF outlines, cut out of the OpenType wrapper. A PDF can
/// embed them directly as a Type 1 font program.
fn cff_table() -> &'static [u8] {
    let be16 = |o: usize| u16::from_be_bytes([FONT[o], FONT[o + 1]]) as usize;
    let be32 =
        |o: usize| u32::from_be_bytes([FONT[o], FONT[o + 1], FONT[o + 2], FONT[o + 3]]) as usize;
    let tables = be16(4);
    for i in 0..tables {
        let rec = 12 + 16 * i;
        if &FONT[rec..rec + 4] == b"CFF " {
            let (off, len) = (be32(rec + 8), be32(rec + 12));
            return &FONT[off..off + len];
        }
    }
    panic!("the bundled font has no CFF table");
}

fn deflate(data: &[u8]) -> Vec<u8> {
    miniz_oxide::deflate::compress_to_vec_zlib(data, 6)
}

/// A PDF string literal for document info.
fn text_string(s: &str) -> String {
    let bytes: String = s.chars().map(win_ansi).map(escape).collect();
    format!("({bytes})")
}

/// Write the file: catalog, page tree, one font, shared resources, and
/// a page and content stream for each sheet.
fn assemble(contents: &[Vec<u8>], title: &str, weave: f32) -> Vec<u8> {
    let mut pdf: Vec<u8> = b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let mut offsets: Vec<usize> = Vec::new();
    let mut object = |pdf: &mut Vec<u8>, id: usize, body: &[u8]| {
        debug_assert_eq!(offsets.len() + 1, id);
        offsets.push(pdf.len());
        pdf.extend_from_slice(format!("{id} 0 obj\n").as_bytes());
        pdf.extend_from_slice(body);
        pdf.extend_from_slice(b"\nendobj\n");
    };
    let stream = |dict: &str, data: &[u8]| -> Vec<u8> {
        let mut v = format!("<< {dict} /Length {} >>\nstream\n", data.len()).into_bytes();
        v.extend_from_slice(data);
        v.extend_from_slice(b"\nendstream");
        v
    };

    // 1 catalog, 2 pages, 3 font, 4 descriptor, 5 font file,
    // 6 resources, 7 info, 8 weave pattern, 9 weave mask, then a page
    // and its contents per sheet.
    let first_page = 10;
    let kids: Vec<String> = (0..contents.len())
        .map(|i| format!("{} 0 R", first_page + 2 * i))
        .collect();

    object(&mut pdf, 1, b"<< /Type /Catalog /Pages 2 0 R >>");
    object(
        &mut pdf,
        2,
        format!(
            "<< /Type /Pages /Kids [{}] /Count {} >>",
            kids.join(" "),
            contents.len()
        )
        .as_bytes(),
    );
    let widths = vec![FONT_ADVANCE.to_string(); 224].join(" ");
    object(
        &mut pdf,
        3,
        format!(
            "<< /Type /Font /Subtype /Type1 /BaseFont /{FONT_NAME} /FirstChar 32 \
             /LastChar 255 /Widths [{widths}] /Encoding /WinAnsiEncoding \
             /FontDescriptor 4 0 R >>"
        )
        .as_bytes(),
    );
    object(
        &mut pdf,
        4,
        format!(
            "<< /Type /FontDescriptor /FontName /{FONT_NAME} /Flags 33 \
             /FontBBox [-44 -299 663 858] /ItalicAngle 0 /Ascent 800 \
             /Descent -200 /CapHeight 579 /XHeight 451 /StemV 90 \
             /FontFile3 5 0 R >>"
        )
        .as_bytes(),
    );
    object(
        &mut pdf,
        5,
        &stream(
            "/Subtype /Type1C /Filter /FlateDecode",
            &deflate(cff_table()),
        ),
    );
    let states: String = (0..DENSITY_LEVELS)
        .map(|l| {
            let a = l as f32 / (DENSITY_LEVELS - 1) as f32;
            format!("/G{l} << /ca {a:.3} /CA {a:.3} >> ")
        })
        .collect();
    object(
        &mut pdf,
        6,
        format!(
            "<< /Font << /F1 3 0 R >> /ExtGState << {states}/Weave << /ca {:.2} >> >> \
             /Pattern << /P1 8 0 R >> /ColorSpace << /CsP [/Pattern /DeviceRGB] >> >>",
            weave
        )
        .as_bytes(),
    );
    object(
        &mut pdf,
        7,
        format!(
            "<< /Title {} /Creator (Lone Typist) /Producer (Lone Typist) >>",
            text_string(title)
        )
        .as_bytes(),
    );
    let t = WEAVE_TILE;
    object(
        &mut pdf,
        8,
        &stream(
            &format!(
                "/Type /Pattern /PatternType 1 /PaintType 2 /TilingType 1 \
                 /BBox [0 0 {t} {t}] /XStep {t} /YStep {t} \
                 /Resources << /XObject << /N 9 0 R >> >>"
            ),
            format!("q {t} 0 0 {t} 0 0 cm /N Do Q").as_bytes(),
        ),
    );
    object(
        &mut pdf,
        9,
        &stream(
            &format!(
                "/Type /XObject /Subtype /Image /Width {WEAVE_BITS} /Height {WEAVE_BITS} \
                 /ImageMask true /BitsPerComponent 1 /Decode [1 0] /Filter /FlateDecode"
            ),
            &deflate(&weave_mask()),
        ),
    );
    for (i, content) in contents.iter().enumerate() {
        let page = first_page + 2 * i;
        object(
            &mut pdf,
            page,
            format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {PAGE_W} {PAGE_H}] \
                 /Resources 6 0 R /Contents {} 0 R >>",
                page + 1
            )
            .as_bytes(),
        );
        object(
            &mut pdf,
            page + 1,
            &stream("/Filter /FlateDecode", &deflate(content)),
        );
    }

    let xref = pdf.len();
    let mut table = format!("xref\n0 {}\n0000000000 65535 f \n", offsets.len() + 1);
    for o in &offsets {
        table.push_str(&format!("{o:010} 00000 n \n"));
    }
    table.push_str(&format!(
        "trailer\n<< /Size {} /Root 1 0 R /Info 7 0 R >>\nstartxref\n{xref}\n%%EOF\n",
        offsets.len() + 1
    ));
    pdf.extend_from_slice(table.as_bytes());
    pdf
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(row: usize, col: usize, text: &str) -> PrintLine {
        PrintLine {
            row,
            col,
            text: text.to_string(),
        }
    }

    #[test]
    fn emphasis_underlines_italic_and_doubles_bold() {
        let t = emphasis("a *b* **c** _d_");
        let s: String = t.iter().map(|x| x.c).collect();
        assert_eq!(s, "a b c d");
        assert!(t[2].underline && !t[2].bold);
        assert!(t[4].bold && !t[4].underline);
        assert!(t[6].underline);
        assert!(!t[0].underline && !t[0].bold);
    }

    #[test]
    fn unpaired_markers_are_typed_as_they_stand() {
        let s: String = emphasis("5 * 3").iter().map(|x| x.c).collect();
        assert_eq!(s, "5 * 3");
        let s: String = emphasis("snake_case").iter().map(|x| x.c).collect();
        assert_eq!(s, "snake_case");
    }

    #[test]
    fn the_font_program_is_found() {
        let cff = cff_table();
        assert_eq!(&cff[..2], &[1, 0], "CFF version 1.0");
    }

    #[test]
    fn a_pdf_is_well_formed() {
        let lines = vec![
            line(0, 15, "INT. DINER - NIGHT"),
            line(2, 15, "Rain (hard)."),
        ];
        let pdf = render(
            &[],
            &[Sheet {
                number: 1,
                lines: &lines,
            }],
            Ribbon::Fresh,
        );
        assert!(pdf.starts_with(b"%PDF-1.4"));
        assert!(pdf.ends_with(b"%%EOF\n"));
        let text = String::from_utf8_lossy(&pdf);
        // The xref offset points at the xref table.
        let at: usize = text
            .rsplit("startxref\n")
            .next()
            .and_then(|s| s.lines().next())
            .and_then(|s| s.parse().ok())
            .expect("startxref");
        assert!(pdf[at..].starts_with(b"xref"));
        assert!(text.contains("/Count 1"));
    }

    #[test]
    fn the_same_script_prints_the_same_way_twice() {
        let lines = [line(0, 15, "The same every time.")];
        let a = type_page(&[(6, 15, lines[0].text.clone())], 2, Ribbon::Worn);
        let b = type_page(&[(6, 15, lines[0].text.clone())], 2, Ribbon::Worn);
        assert_eq!(a, b);
    }

    #[test]
    fn a_clean_ribbon_strikes_on_the_grid() {
        let page = String::from_utf8(type_page(&[(6, 15, "ab".to_string())], 1, Ribbon::Clean))
            .expect("ascii");
        assert!(page.contains("1.0000 0.0000 -0.0000 1.0000 108.00 "));
        assert!(page.contains("1.0000 0.0000 -0.0000 1.0000 115.20 "));
    }

    #[test]
    fn a_page_number_heads_every_page_but_the_first() {
        let lines = vec![line(0, 15, "x")];
        let one = render(
            &[],
            &[Sheet {
                number: 1,
                lines: &lines,
            }],
            Ribbon::Clean,
        );
        let two = render(
            &[],
            &[Sheet {
                number: 2,
                lines: &lines,
            }],
            Ribbon::Clean,
        );
        assert!(one.len() < two.len(), "page 2 carries a number");
    }

    #[test]
    fn a_title_page_is_its_own_sheet() {
        let title = vec![
            ("title".to_string(), "Pilot".to_string()),
            ("author".to_string(), "A. Writer".to_string()),
        ];
        let lines = vec![line(0, 15, "x")];
        let pdf = render(
            &title,
            &[Sheet {
                number: 1,
                lines: &lines,
            }],
            Ribbon::Clean,
        );
        assert!(String::from_utf8_lossy(&pdf).contains("/Count 2"));
    }

    #[test]
    fn characters_map_into_win_ansi() {
        assert_eq!(win_ansi('A'), b'A');
        assert_eq!(win_ansi('é'), 0xE9);
        assert_eq!(win_ansi('\u{2014}'), 0x97);
        assert_eq!(win_ansi('\u{2588}'), b'?');
        assert_eq!(escape(b'('), "\\(");
        assert_eq!(escape(0xE9), "\\351");
    }
}

/// Writes `target/screenplay-<ribbon>.pdf` from `tools/sample.fountain`,
/// for looking at: `cargo test dump_sample_pdf -- --ignored`.
#[cfg(test)]
mod sample {
    use super::*;
    use crate::fountain::Caret;
    use crate::script::{self, Layout};

    #[test]
    #[ignore]
    fn dump_sample_pdf() {
        let source = std::fs::read_to_string("tools/sample.fountain").expect("sample");
        let text: Vec<char> = source.chars().collect();
        let layout = Layout::screenplay(&text, Caret::default());
        let pages = layout.print_pages(&text);
        let sheets: Vec<Sheet> = pages
            .iter()
            .enumerate()
            .map(|(i, lines)| Sheet {
                number: i + 1,
                lines,
            })
            .collect();
        let title = script::title_page(&source);
        for ribbon in [Ribbon::Fresh, Ribbon::Worn, Ribbon::Clean] {
            let pdf = render(&title, &sheets, ribbon);
            let name = format!("target/screenplay-{}.pdf", ribbon.name().to_lowercase());
            std::fs::write(&name, pdf).expect("write");
        }
    }
}
