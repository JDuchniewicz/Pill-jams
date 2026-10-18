use pill_engine::game::*;

use crate::game::GameUIStateComponent;

const VERTEX_SHADER: &str = include_str!("../res/shaders/post_process_vertex.wgsl");
const FRAGMENT_SHADER: &str = include_str!("../res/shaders/post_process_fragment.wgsl");

// --- Parameters (edited in the egui overlay) ---

/// Dither pattern used by the final quantization step. The order matches the indices consumed
/// by `post_process_fragment.hlsl`.
#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DitherPattern {
    /// Smooth interleaved gradient noise (screen-stable, low-contrast grain).
    InterleavedGradient = 0,
    /// Classic 4x4 Bayer ordered matrix.
    Bayer4 = 1,
    /// Classic 8x8 Bayer ordered matrix.
    Bayer8 = 2,
    /// Classic 16x16 Bayer ordered matrix.
    Bayer16 = 3,
    /// Per-pixel white noise hash.
    WhiteNoise = 4,
    /// Diagonal gradient stripes.
    Stripes = 5,
    /// Checkerboard cells.
    Checkerboard = 6,
    /// Halftone dot grid.
    Halftone = 7,
}

impl DitherPattern {
    /// All patterns, in shader index order.
    pub const ALL: [DitherPattern; 8] = [
        DitherPattern::InterleavedGradient,
        DitherPattern::Bayer4,
        DitherPattern::Bayer8,
        DitherPattern::Bayer16,
        DitherPattern::WhiteNoise,
        DitherPattern::Stripes,
        DitherPattern::Checkerboard,
        DitherPattern::Halftone,
    ];

    pub fn label(self) -> &'static str {
        match self {
            DitherPattern::InterleavedGradient => "Interleaved gradient",
            DitherPattern::Bayer4 => "Bayer 4x4",
            DitherPattern::Bayer8 => "Bayer 8x8",
            DitherPattern::Bayer16 => "Bayer 16x16",
            DitherPattern::WhiteNoise => "White noise",
            DitherPattern::Stripes => "Diagonal stripes",
            DitherPattern::Checkerboard => "Checkerboard",
            DitherPattern::Halftone => "Halftone dots",
        }
    }

    /// Whether the pattern uses the extra angle / width / softness controls.
    pub fn uses_extra_parameters(self) -> bool {
        matches!(self, DitherPattern::Stripes | DitherPattern::Halftone)
    }
}

/// Everything the post-process pass needs. Mirrored by `PostProcessParams` in
/// post_process_fragment.hlsl. The fields are grouped by shader section.
#[derive(Clone, Debug)]
pub struct PostProcessParams {
    // --- Section 1: contrast & gamma ---
    /// Contrast multiplier around mid grey (1 = unchanged).
    pub contrast: f32,
    /// Gamma exponent (1 = unchanged, >1 brightens mid tones).
    pub gamma: f32,
    // --- Section 2: chromatic aberration ---
    /// Radial colour separation in UV units (0 = off).
    pub aberration_strength: f32,
    // --- Section 3: vignette ---
    /// How much the corners are darkened (0 = off, 1 = black corners).
    pub vignette_strength: f32,
    /// Width of the vignette falloff.
    pub vignette_softness: f32,
    /// Normalized radius where the vignette starts (0 = centre, 1 = corners).
    pub vignette_radius: f32,
    // --- Section 4: dithering ---
    /// Amplitude of the ordered dither pattern, relative to one quantization step (0 = off).
    pub dither_strength: f32,
    /// Number of quantization levels per colour channel (256 = effectively unchanged).
    pub dither_levels: u32,
    /// Size of the dither pattern cells in pixels.
    pub dither_scale: f32,
    /// Which dither pattern to apply.
    pub dither_pattern: DitherPattern,
    /// Rotation of the stripe / halftone pattern in degrees.
    pub dither_angle: f32,
    /// Stripe band width or halftone dot size, in [0, 1] of a pattern cell.
    pub dither_width: f32,
    /// Softness of the stripe / halftone dot edges.
    pub dither_softness: f32,
}

impl Default for PostProcessParams {
    fn default() -> Self {
        Self {
            contrast: 1.0,
            gamma: 0.9,
            aberration_strength: 0.005,
            vignette_strength: 0.45,
            vignette_softness: 0.55,
            vignette_radius: 0.82,
            dither_strength: 1.0,
            dither_levels: 5,
            dither_scale: 1.5,
            dither_pattern: DitherPattern::Halftone,
            dither_angle: 45.0,
            dither_width: 1.0,
            dither_softness: 0.0,
        }
    }
}

/// GPU layout of `PostProcessParams`: five `float4`s, same order as in the HLSL struct.
type PostProcessUniform = [[f32; 4]; 5];

fn post_process_uniform(params: &PostProcessParams, screen: (u32, u32)) -> PostProcessUniform {
    [
        [
            params.contrast,
            params.gamma,
            params.aberration_strength,
            0.0,
        ],
        [
            params.vignette_strength,
            params.vignette_softness,
            params.vignette_radius,
            0.0,
        ],
        [screen.0 as f32, screen.1 as f32, 0.0, 0.0],
        [
            params.dither_strength,
            params.dither_levels as f32,
            params.dither_scale,
            params.dither_pattern as u32 as f32,
        ],
        [
            params.dither_angle.to_radians(),
            params.dither_width,
            params.dither_softness,
            0.0,
        ],
    ]
}

// --- Pass ---

struct PostProcessPassState {
    pipeline: Pipeline,
    sampler: wgpu::Sampler,
    params_buffer: wgpu::Buffer,
}

/// Game-defined post-process pass: samples the composed HDR scene and applies contrast & gamma,
/// chromatic aberration, a vignette and ordered dithering in one fullscreen pass (see
/// `Engine::add_post_process_pass`). It runs after every scene pass and before tonemapping.
pub struct PostProcessPass {
    state: Option<PostProcessPassState>,
}

impl PostProcessPass {
    pub fn new() -> Self {
        Self { state: None }
    }
}

impl Default for PostProcessPass {
    fn default() -> Self {
        Self::new()
    }
}

impl Pass for PostProcessPass {
    fn get_label(&self) -> &str {
        "post_process_pass"
    }

    fn init(&mut self, renderer: &mut dyn PillRenderer) -> Result<()> {
        let bind_groups = vec![vec![
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    multisampled: false,
                    view_dimension: wgpu::TextureViewDimension::D2,
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
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
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
        ]];

        let pipeline = renderer.create_pipeline(PipelineDesc {
            label: Some("post_process_pass"),
            vs: ShaderDesc {
                source: VERTEX_SHADER,
                entry_func: "vs_main",
            },
            ps: ShaderDesc {
                source: FRAGMENT_SHADER,
                entry_func: "fs_main",
            },
            // No vertex buffer: the fullscreen triangle comes from SV_VertexID.
            vertex_buffers: &[],
            bind_groups,
            targets: &[Some(wgpu::ColorTargetState {
                format: HDR_TARGET_FORMAT,
                blend: Some(wgpu::BlendState::REPLACE),
                write_mask: wgpu::ColorWrites::ALL,
            })],
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                polygon_mode: wgpu::PolygonMode::Fill,
                conservative: false,
                unclipped_depth: false,
            },
        })?;

        let sampler = renderer
            .get_device()
            .create_sampler(&wgpu::SamplerDescriptor {
                label: Some("post_process_pass_sampler"),
                address_mode_u: wgpu::AddressMode::ClampToEdge,
                address_mode_v: wgpu::AddressMode::ClampToEdge,
                address_mode_w: wgpu::AddressMode::ClampToEdge,
                mag_filter: wgpu::FilterMode::Linear,
                min_filter: wgpu::FilterMode::Linear,
                mipmap_filter: wgpu::FilterMode::Nearest,
                ..Default::default()
            });

        let params_buffer = renderer.create_buffer(BufferDesc {
            label: Some("post_process_pass_params"),
            byte_size: std::mem::size_of::<PostProcessUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        })?;

        self.state = Some(PostProcessPassState {
            pipeline,
            sampler,
            params_buffer,
        });
        Ok(())
    }

    fn draw(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        renderer: &mut dyn PillRenderer,
        _frame_target: &FrameTarget,
        world: &WorldQuery<'_>,
    ) -> Result<()> {
        // UI parameters; missing component = engine defaults (a no-op pass).
        let params = world
            .get_global::<GameUIStateComponent>()
            .map(|ui_state| {
                ui_state
                    .post_process
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .clone()
            })
            .unwrap_or_default();

        let render_state = world.get_global::<RenderStateComponent>()?;
        // No destination target means the engine did not register this pass; nothing to do.
        let Some(destination) = render_state.post_process_target() else {
            return Ok(());
        };

        let state = self
            .state
            .as_ref()
            .expect("PostProcessPass: init() must run before draw()");

        let screen = renderer.get_surface_size();
        renderer.get_queue().write_buffer(
            &state.params_buffer,
            0,
            bytemuck::bytes_of(&post_process_uniform(&params, screen)),
        );

        let source_view = renderer
            .get_render_target_view(render_state.hdr_target())
            .expect("PostProcessPass: HDR target missing");
        let destination_view = renderer
            .get_render_target_view(destination)
            .expect("PostProcessPass: post-process target missing");

        let bind_group = {
            let layout = &state.pipeline.bind_group_layouts[0];
            renderer
                .get_device()
                .create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("post_process_pass_bind_group"),
                    layout,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: wgpu::BindingResource::TextureView(source_view),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: wgpu::BindingResource::Sampler(&state.sampler),
                        },
                        wgpu::BindGroupEntry {
                            binding: 2,
                            resource: state.params_buffer.as_entire_binding(),
                        },
                    ],
                })
        };

        let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("post_process_pass_render_pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: destination_view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        render_pass.set_pipeline(&state.pipeline.pipeline);
        render_pass.set_bind_group(0, &bind_group, &[]);
        render_pass.draw(0..3, 0..1);
        Ok(())
    }
}
