//! The GPU pass that puts the VGA framebuffer on screen.

use pixels::wgpu::{self, util::DeviceExt};
use pixels::{Pixels, PixelsContext};

/// VGA text mode was displayed on a 4:3 monitor regardless of its 720x400
/// pixel count, which is why the glyphs need stretching vertically.
const DISPLAY_ASPECT: f32 = 4.0 / 3.0;

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct Uniforms {
    scale: [f32; 2],
    /// Drawn rectangle in device pixels, so the shader knows the zoom.
    /// Named `draw_size` because `target` is reserved in WGSL.
    draw_size: [f32; 2],
    time: f32,
    effects: f32,
    _pad: [f32; 2],
    /// What the slack around the 4:3 picture is painted, in the
    /// framebuffer texture's colour space.
    border: [f32; 4],
}

pub struct Params {
    pub surface: (u32, u32),
    pub time: f32,
    pub effects: bool,
    /// The screen's background, as a palette entry. The letterbox slack
    /// takes this colour so the field runs edge to edge.
    pub border: [u8; 3],
}

pub struct Present {
    pipeline: wgpu::RenderPipeline,
    bind_group: wgpu::BindGroup,
    uniform_buffer: wgpu::Buffer,
    /// Whether the framebuffer is sampled through an sRGB decode, in
    /// which case the border has to be handed over decoded too.
    texture_is_srgb: bool,
}

/// The sRGB transfer function, inverted: an 8-bit palette value to the
/// linear intensity the GPU sees after sampling an sRGB texture.
fn srgb_to_linear(c: u8) -> f32 {
    let v = c as f32 / 255.0;
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

impl Present {
    pub fn new(pixels: &Pixels) -> Self {
        let device = pixels.device();
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("crt"),
            source: wgpu::ShaderSource::Wgsl(include_str!("../shaders/crt.wgsl").into()),
        });

        let view = pixels
            .texture()
            .create_view(&wgpu::TextureViewDescriptor::default());

        // Linear, but the shader pre-shapes the UVs so interpolation only
        // happens in a one-pixel band at texel edges (see `sharp_uv`).
        // Plain nearest gave uneven stroke weights at fractional zoom.
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("crt-sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            ..Default::default()
        });

        let uniform_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("crt-uniforms"),
            contents: bytemuck::bytes_of(&Uniforms {
                scale: [1.0, 1.0],
                draw_size: [720.0, 400.0],
                time: 0.0,
                effects: 0.0,
                _pad: [0.0, 0.0],
                border: [0.0, 0.0, 0.0, 1.0],
            }),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("crt-layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("crt-bind-group"),
            layout: &layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: uniform_buffer.as_entire_binding(),
                },
            ],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("crt-pipeline-layout"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("crt-pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: pixels.render_texture_format(),
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleStrip,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });

        Self {
            pipeline,
            bind_group,
            uniform_buffer,
            texture_is_srgb: pixels.texture().format().is_srgb(),
        }
    }

    /// The border colour as the shader must see it: the same value the
    /// framebuffer's own background produces when sampled.
    fn border_value(&self, rgb: [u8; 3]) -> [f32; 4] {
        let convert = |c: u8| {
            if self.texture_is_srgb {
                srgb_to_linear(c)
            } else {
                c as f32 / 255.0
            }
        };
        [convert(rgb[0]), convert(rgb[1]), convert(rgb[2]), 1.0]
    }

    /// Clip-space scale that fits a 4:3 image inside the surface without
    /// distorting it, leaving slack on whichever axis is longer. The
    /// shader paints that slack in the screen's background colour.
    fn letterbox(surface: (u32, u32)) -> [f32; 2] {
        let (w, h) = (surface.0.max(1) as f32, surface.1.max(1) as f32);
        let surface_aspect = w / h;
        if surface_aspect > DISPLAY_ASPECT {
            [DISPLAY_ASPECT / surface_aspect, 1.0]
        } else {
            [1.0, surface_aspect / DISPLAY_ASPECT]
        }
    }

    pub fn render(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        context: &PixelsContext,
        params: &Params,
    ) {
        context.queue.write_buffer(
            &self.uniform_buffer,
            0,
            bytemuck::bytes_of(&{
                let scale = Self::letterbox(params.surface);
                Uniforms {
                    scale,
                    draw_size: [
                        params.surface.0 as f32 * scale[0],
                        params.surface.1 as f32 * scale[1],
                    ],
                    time: params.time,
                    effects: if params.effects { 1.0 } else { 0.0 },
                    _pad: [0.0, 0.0],
                    border: self.border_value(params.border),
                }
            }),
        );

        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("crt-pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations {
                    // The quad covers the whole surface, so this is never
                    // seen; it only has to be something.
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });

        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.bind_group, &[]);
        pass.draw(0..4, 0..1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The scale must never exceed 1.0 on either axis (that would crop)
    /// and must reproduce 4:3 in the drawn rectangle.
    fn drawn_aspect(surface: (u32, u32)) -> f32 {
        let s = Present::letterbox(surface);
        let w = surface.0 as f32 * s[0];
        let h = surface.1 as f32 * s[1];
        w / h
    }

    #[test]
    fn a_four_by_three_surface_is_filled_exactly() {
        let s = Present::letterbox((1080, 810));
        assert!((s[0] - 1.0).abs() < 1e-5);
        assert!((s[1] - 1.0).abs() < 1e-5);
    }

    #[test]
    fn a_wide_surface_is_pillarboxed() {
        let s = Present::letterbox((1920, 1080));
        assert!(s[0] < 1.0, "horizontal is inset");
        assert!((s[1] - 1.0).abs() < 1e-5, "vertical fills");
    }

    #[test]
    fn a_tall_surface_is_letterboxed() {
        let s = Present::letterbox((800, 1200));
        assert!((s[0] - 1.0).abs() < 1e-5, "horizontal fills");
        assert!(s[1] < 1.0, "vertical is inset");
    }

    #[test]
    fn the_drawn_rectangle_is_always_four_by_three() {
        for surface in [
            (1080, 810),
            (1920, 1080),
            (800, 1200),
            (640, 480),
            (2560, 1080),
        ] {
            let a = drawn_aspect(surface);
            assert!(
                (a - 4.0 / 3.0).abs() < 1e-4,
                "{surface:?} drew at aspect {a}"
            );
        }
    }

    #[test]
    fn the_scale_never_crops() {
        for surface in [(1, 1), (10000, 1), (1, 10000), (1920, 1080)] {
            let s = Present::letterbox(surface);
            assert!(
                s[0] <= 1.0 + 1e-6 && s[1] <= 1.0 + 1e-6,
                "{surface:?} -> {s:?}"
            );
        }
    }

    #[test]
    fn the_srgb_curve_maps_the_ends_and_the_vga_blue() {
        assert_eq!(srgb_to_linear(0), 0.0);
        assert!((srgb_to_linear(255) - 1.0).abs() < 1e-6);
        // Palette blue is 0xAA; linear it is about 0.402.
        let blue = srgb_to_linear(0xAA);
        assert!((blue - 0.402).abs() < 0.002, "got {blue}");
    }

    #[test]
    fn a_degenerate_surface_does_not_divide_by_zero() {
        let s = Present::letterbox((0, 0));
        assert!(s[0].is_finite() && s[1].is_finite());
    }
}
