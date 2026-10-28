//! A software reproduction of the present pass.
//!
//! Framebuffer dumps show what the rasterizer produced, which has been
//! correct all along; what goes wrong happens when the GPU scales that
//! framebuffer to the window. This mirrors `shaders/crt.wgsl` exactly so
//! the on-screen result can be inspected without a screenshot.

#![cfg(test)]

use crate::app::App;
use crate::vga::{FB_HEIGHT, FB_WIDTH};

const TEX_W: f32 = FB_WIDTH as f32;
const TEX_H: f32 = FB_HEIGHT as f32;
const DISPLAY_ASPECT: f32 = 4.0 / 3.0;

// Must match shaders/crt.wgsl.
const CURVATURE: f32 = 0.0;
const SCANLINE_DEPTH: f32 = 0.18;
const BLOOM_RADIUS: f32 = 0.0016;
const BLOOM_STRENGTH: f32 = 0.38;
const VIGNETTE_STRENGTH: f32 = 0.12;

fn letterbox(w: f32, h: f32) -> [f32; 2] {
    let a = w / h;
    if a > DISPLAY_ASPECT {
        [DISPLAY_ASPECT / a, 1.0]
    } else {
        [1.0, a / DISPLAY_ASPECT]
    }
}

fn sample_bilinear(fb: &[u8], uv: [f32; 2]) -> [f32; 3] {
    let x = uv[0] * TEX_W - 0.5;
    let y = uv[1] * TEX_H - 0.5;
    let x0 = x.floor();
    let y0 = y.floor();
    let fx = x - x0;
    let fy = y - y0;

    let texel = |ix: f32, iy: f32| -> [f32; 3] {
        let ix = (ix as i32).clamp(0, FB_WIDTH as i32 - 1) as usize;
        let iy = (iy as i32).clamp(0, FB_HEIGHT as i32 - 1) as usize;
        let i = (iy * FB_WIDTH + ix) * 4;
        [
            fb[i] as f32 / 255.0,
            fb[i + 1] as f32 / 255.0,
            fb[i + 2] as f32 / 255.0,
        ]
    };

    let a = texel(x0, y0);
    let b = texel(x0 + 1.0, y0);
    let c = texel(x0, y0 + 1.0);
    let d = texel(x0 + 1.0, y0 + 1.0);
    let mut out = [0.0f32; 3];
    for k in 0..3 {
        let top = a[k] + (b[k] - a[k]) * fx;
        let bot = c[k] + (d[k] - c[k]) * fx;
        out[k] = top + (bot - top) * fy;
    }
    out
}

/// Mirrors `sharp_uv` in the shader.
fn sharp_uv(uv: [f32; 2], draw: [f32; 2]) -> [f32; 2] {
    let mut out = [0.0f32; 2];
    let tex = [TEX_W, TEX_H];
    for k in 0..2 {
        let texel = uv[k] * tex[k];
        let floored = texel.floor();
        let s = texel - floored;
        let scale = (draw[k] / tex[k]).max(1.0);
        let region = 0.5 - 0.5 / scale;
        let cd = s - 0.5;
        let f = (cd - cd.clamp(-region, region)) * scale + 0.5;
        out[k] = (floored + f) / tex[k];
    }
    out
}

fn barrel(uv: [f32; 2]) -> [f32; 2] {
    let cx = uv[0] * 2.0 - 1.0;
    let cy = uv[1] * 2.0 - 1.0;
    let r2 = cx * cx + cy * cy;
    [
        (cx * (1.0 + CURVATURE * r2)) * 0.5 + 0.5,
        (cy * (1.0 + CURVATURE * r2)) * 0.5 + 0.5,
    ]
}

/// Render the framebuffer the way the GPU would, into `w` x `h` RGBA.
pub fn present(fb: &[u8], w: usize, h: usize, effects: bool, time: f32) -> Vec<u8> {
    let scale = letterbox(w as f32, h as f32);
    let draw = [w as f32 * scale[0], h as f32 * scale[1]];
    let mut out = vec![0u8; w * h * 4];

    for py in 0..h {
        for px in 0..w {
            let ndc_x = (px as f32 + 0.5) / w as f32 * 2.0 - 1.0;
            let ndc_y = 1.0 - (py as f32 + 0.5) / h as f32 * 2.0;

            let i = (py * w + px) * 4;
            out[i + 3] = 0xFF;
            if ndc_x.abs() > scale[0] || ndc_y.abs() > scale[1] {
                continue; // the bezel
            }

            let uv = [
                (ndc_x / scale[0] + 1.0) * 0.5,
                (1.0 - ndc_y / scale[1]) * 0.5,
            ];

            let mut color;
            if !effects {
                color = sample_bilinear(fb, sharp_uv(uv, draw));
            } else {
                let b = if CURVATURE > 0.0 { barrel(uv) } else { uv };
                if b[0] < 0.0 || b[0] > 1.0 || b[1] < 0.0 || b[1] > 1.0 {
                    continue;
                }
                color = sample_bilinear(fb, sharp_uv(b, draw));

                let mut glow = [0.0f32; 3];
                let taps = [
                    [BLOOM_RADIUS, 0.0],
                    [-BLOOM_RADIUS, 0.0],
                    [0.0, BLOOM_RADIUS],
                    [0.0, -BLOOM_RADIUS],
                    [BLOOM_RADIUS, BLOOM_RADIUS],
                    [-BLOOM_RADIUS, -BLOOM_RADIUS],
                ];
                for t in taps {
                    let s = sample_bilinear(fb, [b[0] + t[0], b[1] + t[1]]);
                    for k in 0..3 {
                        glow[k] += s[k];
                    }
                }
                // Only light in excess of this pixel; see the shader.
                for k in 0..3 {
                    let excess = (glow[k] / 6.0 - color[k]).max(0.0);
                    color[k] += excess * BLOOM_STRENGTH;
                }

                let scan =
                    1.0 - SCANLINE_DEPTH * (b[1] * TEX_H * std::f32::consts::PI).sin().powi(2);
                let flicker = 1.0 + 0.012 * (time * 6.0).sin();
                let dx = b[0] - 0.5;
                let dy = b[1] - 0.5;
                let d = (dx * dx + dy * dy).sqrt();
                let vig = 1.0 - VIGNETTE_STRENGTH * d * d;
                for c in color.iter_mut() {
                    *c = *c * scan * flicker * vig;
                }
            }

            for (k, c) in color.iter().enumerate() {
                out[i + k] = (c.clamp(0.0, 1.0) * 255.0).round() as u8;
            }
        }
    }
    out
}

/// Average 2x2 blocks, the way a Retina screenshot is presented.
pub fn downsample2(src: &[u8], w: usize, h: usize) -> (Vec<u8>, usize, usize) {
    let (dw, dh) = (w / 2, h / 2);
    let mut out = vec![0u8; dw * dh * 4];
    for y in 0..dh {
        for x in 0..dw {
            for k in 0..4 {
                let mut sum = 0u32;
                for dy in 0..2 {
                    for dx in 0..2 {
                        sum += src[((y * 2 + dy) * w + (x * 2 + dx)) * 4 + k] as u32;
                    }
                }
                out[(y * dw + x) * 4 + k] = (sum / 4) as u8;
            }
        }
    }
    (out, dw, dh)
}

pub fn crop(src: &[u8], w: usize, x0: usize, y0: usize, cw: usize, ch: usize) -> Vec<u8> {
    let mut out = vec![0u8; cw * ch * 4];
    for y in 0..ch {
        for x in 0..cw {
            let s = ((y0 + y) * w + (x0 + x)) * 4;
            let d = (y * cw + x) * 4;
            out[d..d + 4].copy_from_slice(&src[s..s + 4]);
        }
    }
    out
}

fn scene() -> Vec<u8> {
    let mut a = App::new();
    a.load_text("It was a bright cold day in April, and the clocks were striking thirteen.");
    a.paint();
    let mut fb = vec![0u8; FB_WIDTH * FB_HEIGHT * 4];
    a.screen().render(&mut fb);
    fb
}

/// `cargo test simulate_window -- --ignored --nocapture`
#[test]
#[ignore]
fn simulate_window() {
    let fb = scene();
    // A 1080x810 logical window on a Retina display.
    let (w, h) = (2160, 1620);

    for (name, effects) in [("effects", true), ("clean", false)] {
        let full = present(&fb, w, h, effects, 0.0);
        let (small, sw, sh) = downsample2(&full, w, h);
        crate::vga::preview::write_bmp_rgba(&format!("target/sim-{name}.bmp"), &small, sw, sh);
        // A 1:1 crop of the first line, where the cursor sits.
        let ch = crop(&full, w, 150, 10, 900, 110);
        crate::vga::preview::write_bmp_rgba(
            &format!("target/sim-{name}-status.bmp"),
            &ch,
            900,
            110,
        );
    }
    println!("wrote target/sim-*.bmp");
}

/// `cargo test simulate_menu -- --ignored --nocapture`
#[test]
#[ignore]
fn simulate_menu() {
    use crate::input::Purpose;
    use crate::keymap::{Command, Motion};

    type Setup = fn(&mut App);
    let shots: [(&str, Setup); 2] = [
        ("menu", |a: &mut App| {
            a.apply(Command::MenuBar, 0);
            a.apply(
                Command::Move {
                    motion: Motion::Down,
                    extend: false,
                },
                0,
            );
        }),
        ("field", |a: &mut App| {
            a.open_field(
                Purpose::Retrieve,
                "New Document",
                "Document to be created:",
                "chapter-one.txt",
            );
        }),
    ];

    for (name, setup) in shots {
        let mut a = App::new();
        a.load_text("It was a bright cold day in April, and the clocks were striking thirteen.");
        setup(&mut a);
        a.paint();
        let mut fb = vec![0u8; FB_WIDTH * FB_HEIGHT * 4];
        a.screen().render(&mut fb);

        let (w, h) = (2160, 1620);
        let full = present(&fb, w, h, true, 0.0);
        let (small, sw, sh) = downsample2(&full, w, h);
        crate::vga::preview::write_bmp_rgba(&format!("target/sim-{name}.bmp"), &small, sw, sh);
    }
    println!("wrote target/sim-menu.bmp and target/sim-field.bmp");
}
