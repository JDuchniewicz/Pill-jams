use pill_engine::game::*;

use crate::game::GameUIStateComponent;

const VERTEX_SHADER: &str = include_str!("../res/shaders/post_process_vertex.wgsl");
const FRAGMENT_SHADER: &str = include_str!("../res/shaders/post_process_fragment.wgsl");

// --- Parameters (edited in the egui overlay) ---

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
        }
    }
}

/// GPU layout of `PostProcessParams`: three `float4`s, same order as in the HLSL struct.
type PostProcessUniform = [[f32; 4]; 3];

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
    ]
}

// --- Pass ---

struct PostProcessPassState {
    pipeline: Pipeline,
    sampler: wgpu::Sampler,
    params_buffer: wgpu::Buffer,
}

/// Game-defined post-process pass: samples the composed HDR scene and applies contrast & gamma,
/// chromatic aberration and a vignette in one fullscreen pass (see
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
