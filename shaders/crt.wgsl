// Presents the 720x400 VGA framebuffer at a 4:3 aspect ratio.
//
// The quad covers the whole surface. Where the surface is wider or
// taller than 4:3, the picture is letterboxed inside it and the slack
// continues whatever the picture's edge holds on that row, with the same
// scanlines and vignette running across it, so the field reads as one
// continuous surface rather than a picture inside a bezel.
//
// Continuing the edge rather than filling with one flat colour is what
// lets a full-width element run to the edges of the display: the menu
// bar's grey reaches the sides in fullscreen instead of stopping short.
// Every other row ends in the background colour anyway, so they look
// exactly as they did.

struct Uniforms {
    // Clip-space scale that letterboxes the 4:3 image inside the surface.
    scale: vec2<f32>,
    // Size of the drawn rectangle in device pixels, for sharp sampling.
    draw_size: vec2<f32>,
    time: f32,
    // 0.0 = plain, 1.0 = CRT effects.
    effects: f32,
    _pad: vec2<f32>,
};

const TEX_SIZE: vec2<f32> = vec2<f32>(720.0, 400.0);

// Sharp bilinear sampling.
//
// The framebuffer is almost never scaled by a whole number, and plain
// nearest-neighbour at a fractional scale gives identical glyph strokes
// different widths -- some source columns land on two output pixels, some
// on one. This keeps each texel's interior flat and confines the blend to
// a one-device-pixel band at texel edges: as crisp as nearest, but every
// stroke the same weight at any zoom.
fn sharp_uv(uv: vec2<f32>) -> vec2<f32> {
    let texel = uv * TEX_SIZE;
    let texel_floored = floor(texel);
    let s = fract(texel);

    let scale = max(u.draw_size / TEX_SIZE, vec2<f32>(1.0, 1.0));
    let region_range = 0.5 - 0.5 / scale;

    let center_dist = s - 0.5;
    let f = (center_dist - clamp(center_dist, -region_range, region_range)) * scale + 0.5;
    return (texel_floored + f) / TEX_SIZE;
}

@group(0) @binding(0) var tex: texture_2d<f32>;
@group(0) @binding(1) var samp: sampler;
@group(0) @binding(2) var<uniform> u: Uniforms;

struct VertexOutput {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> VertexOutput {
    // A triangle strip covering the whole surface. The 4:3 picture is
    // the [0,1] square of uv space; the letterbox scale pushes the
    // surface edges out past it, and those pixels get the border.
    var positions = array<vec2<f32>, 4>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>( 1.0, -1.0),
        vec2<f32>(-1.0,  1.0),
        vec2<f32>( 1.0,  1.0),
    );

    var out: VertexOutput;
    let p = positions[index];
    out.clip = vec4<f32>(p, 0.0, 1.0);
    let q = p / u.scale;
    out.uv = vec2<f32>((q.x + 1.0) * 0.5, (1.0 - q.y) * 0.5);
    return out;
}

// Whether a uv lands on the picture rather than the slack around it.
fn on_picture(uv: vec2<f32>) -> bool {
    return uv.x >= 0.0 && uv.x <= 1.0 && uv.y >= 0.0 && uv.y <= 1.0;
}

// Pull a uv back onto the picture, to the centre of the nearest edge
// texel. Slack sampled through this continues the row it abuts.
fn edge_clamped(uv: vec2<f32>) -> vec2<f32> {
    let half_texel = vec2<f32>(0.5, 0.5) / TEX_SIZE;
    return clamp(uv, half_texel, vec2<f32>(1.0, 1.0) - half_texel);
}

// Source framebuffer is 720x400; scanlines run at that row frequency.
const SOURCE_HEIGHT: f32 = 400.0;
// Tube curvature. 0.0 is a flat screen with square 90-degree corners;
// raise it (0.015 to 0.03) to bow the edges like a real tube.
const CURVATURE: f32 = 0.0;
const SCANLINE_DEPTH: f32 = 0.18;
const BLOOM_RADIUS: f32 = 0.0016;
const BLOOM_STRENGTH: f32 = 0.38;
// Kept gentle: on a flat square screen a strong radial falloff reads
// as a smudge rather than as a tube.
const VIGNETTE_STRENGTH: f32 = 0.12;

// Pull the corners in slightly, as a curved tube does.
fn barrel(uv: vec2<f32>) -> vec2<f32> {
    let centered = uv * 2.0 - 1.0;
    let r2 = dot(centered, centered);
    return (centered * (1.0 + CURVATURE * r2)) * 0.5 + 0.5;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    // Sampling stays in uniform control flow (it needs derivatives), so
    // the slack is always sampled at the clamped edge and then replaced.
    let inside = on_picture(in.uv);

    if (u.effects < 0.5) {
        return vec4<f32>(textureSample(tex, samp, sharp_uv(edge_clamped(in.uv))).rgb, 1.0);
    }

    var uv = in.uv;
    if (CURVATURE > 0.0) {
        uv = barrel(in.uv);
    }
    // Past the edge of a curved tube there is no picture either.
    let lit = inside && on_picture(uv);

    var color = textureSample(tex, samp, sharp_uv(edge_clamped(uv))).rgb;

    // Phosphor bloom: bright text spills into the dark around it.
    //
    // Only the light in EXCESS of this pixel counts. Adding the raw
    // neighbour average would reduce, on any flat area, to multiplying
    // every pixel by (1 + strength) -- a global brightness boost that
    // washes the background out and fattens every glyph.
    var glow = vec3<f32>(0.0);
    glow = glow + textureSample(tex, samp, uv + vec2<f32>( BLOOM_RADIUS, 0.0)).rgb;
    glow = glow + textureSample(tex, samp, uv + vec2<f32>(-BLOOM_RADIUS, 0.0)).rgb;
    glow = glow + textureSample(tex, samp, uv + vec2<f32>(0.0,  BLOOM_RADIUS)).rgb;
    glow = glow + textureSample(tex, samp, uv + vec2<f32>(0.0, -BLOOM_RADIUS)).rgb;
    glow = glow + textureSample(tex, samp, uv + vec2<f32>( BLOOM_RADIUS,  BLOOM_RADIUS)).rgb;
    glow = glow + textureSample(tex, samp, uv + vec2<f32>(-BLOOM_RADIUS, -BLOOM_RADIUS)).rgb;
    let excess = max(glow / 6.0 - color, vec3<f32>(0.0));
    // The slack has no glyphs to glow, and must not borrow the picture's
    // edge column.
    color = color + excess * BLOOM_STRENGTH * select(0.0, 1.0, lit);

    // Scanlines at the source row frequency.
    let scan = 1.0 - SCANLINE_DEPTH * pow(sin(uv.y * SOURCE_HEIGHT * 3.14159265), 2.0);
    color = color * scan;

    // A very slight mains-frequency wobble in brightness.
    color = color * (1.0 + 0.012 * sin(u.time * 6.0));

    // Vignette.
    let d = distance(uv, vec2<f32>(0.5, 0.5));
    color = color * (1.0 - VIGNETTE_STRENGTH * d * d);

    return vec4<f32>(color, 1.0);
}
