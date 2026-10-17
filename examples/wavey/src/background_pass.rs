use pill_engine::game::*;

use crate::game::GameUIStateComponent;

const VERTEX_SHADER: &str = include_str!("../res/shaders/background_quad_vertex.wgsl");
const FRAGMENT_SHADER: &str = include_str!("../res/shaders/background_quad_fragment.wgsl");

// --- Parameters (edited in the egui overlay) ---

/// Everything the "2D sky and water" background needs. Mirrored by `SkyParams` in
/// background_quad_fragment.hlsl (all colours are linear RGB).
#[derive(Clone, Debug)]
pub struct SkyParams {
    /// Sky color at the top of the window.
    pub sky_top: [f32; 3],
    /// Sky color at the water line.
    pub sky_horizon: [f32; 3],
    /// Water color just below the water line.
    pub water_horizon: [f32; 3],
    /// Water color at the bottom of the window.
    pub water_bottom: [f32; 3],
    /// Sun / moon glow color.
    pub sun_color: [f32; 3],
    /// Glow strength (0 = none).
    pub sun_intensity: f32,
    /// Glow center in window UV space (0..1, y grows downwards).
    pub sun_position: [f32; 2],
    /// Glow radius in UV units.
    pub sun_radius: f32,
    /// How soft the glow edge is (0 = hard disc, 1 = fully soft).
    pub sun_softness: f32,
    /// Vertical position of the water line (0 = top, 1 = bottom).
    pub horizon: f32,
    /// Blend width of the water line.
    pub horizon_softness: f32,
    /// Vertical amplitude of the water line ripple.
    pub wave_amplitude: f32,
    /// Horizontal frequency of the water line ripple.
    pub wave_frequency: f32,
    /// Oscillation speed of the water line ripple, in radians per second.
    pub wave_speed: f32,
}

impl Default for SkyParams {
    fn default() -> Self {
        Self {
            sky_top: [0.16, 0.35, 0.62],
            // Warm pale horizon (yellowish) against the blue sky
            sky_horizon: [0.75, 0.72, 0.45],
            water_horizon: [0.20, 0.55, 0.75],
            // Deep navy water at the bottom of the window
            water_bottom: [0.01, 0.04, 0.08],
            sun_color: [1.0, 0.85, 0.55],
            sun_intensity: 0.35,
            sun_position: [0.72, 0.28],
            sun_radius: 0.18,
            sun_softness: 0.55,
            horizon: 0.5,
            horizon_softness: 0.027,
            wave_amplitude: 0.005,
            wave_frequency: 13.0,
            wave_speed: 2.7,
        }
    }
}

/// GPU layout of `SkyParams`: eight `float4`s, which is already std140-friendly.
/// Same order as `SkyParams` in background_quad_fragment.hlsl.
type SkyUniform = [[f32; 4]; 8];

fn sky_uniform(params: &SkyParams, time: f32) -> SkyUniform {
    let rgb = |color: [f32; 3]| [color[0], color[1], color[2], 1.0];
    [
        rgb(params.sky_top),
        rgb(params.sky_horizon),
        rgb(params.water_horizon),
        rgb(params.water_bottom),
        [
            params.sun_color[0],
            params.sun_color[1],
            params.sun_color[2],
            params.sun_intensity,
        ],
        [
            params.sun_position[0],
            params.sun_position[1],
            params.sun_radius,
            params.sun_softness,
        ],
        [
            params.horizon,
            params.horizon_softness,
            params.wave_amplitude,
            params.wave_frequency,
        ],
        [time, params.wave_speed, 0.0, 0.0],
    ]
}

// --- Pass ---

struct BackgroundQuadPassState {
    pipeline: Pipeline,
    params_buffer: wgpu::Buffer,
    params_bind_group: wgpu::BindGroup,
}

/// Game-defined background pass: covers the whole window with a 2D sky/water gradient.
///
/// The quad is generated in the vertex shader from `SV_VertexID`, so it needs no vertex buffer
/// and always fills the window, whatever its size. It runs before the opaque PBR pass (see
/// `Engine::add_background_pass`) and is driven by `GameUIStateComponent::sky`.
pub struct BackgroundQuadPass {
    state: Option<BackgroundQuadPassState>,
    /// Seconds since startup, used for the animated ripple.
    time: f32,
}

impl BackgroundQuadPass {
    pub fn new() -> Self {
        Self {
            state: None,
            time: 0.0,
        }
    }
}

impl Default for BackgroundQuadPass {
    fn default() -> Self {
        Self::new()
    }
}

impl Pass for BackgroundQuadPass {
    fn get_label(&self) -> &str {
        "background_quad_pass"
    }

    fn init(&mut self, renderer: &mut dyn PillRenderer) -> Result<()> {
        let bind_groups = vec![vec![wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        }]];

        let pipeline = renderer.create_pipeline(PipelineDesc {
            label: Some("background_quad_pass"),
            vs: ShaderDesc {
                source: VERTEX_SHADER,
                entry_func: "vs_main",
            },
            ps: ShaderDesc {
                source: FRAGMENT_SHADER,
                entry_func: "fs_main",
            },
            // No vertex buffer: the six quad corners come from SV_VertexID.
            vertex_buffers: &[],
            bind_groups,
            targets: &[Some(wgpu::ColorTargetState {
                format: HDR_TARGET_FORMAT,
                // Opaque: the quad fills the whole background.
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

        let params_buffer = renderer.create_buffer(BufferDesc {
            label: Some("background_quad_pass_params"),
            byte_size: std::mem::size_of::<SkyUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        })?;

        let params_bind_group =
            renderer
                .get_device()
                .create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("background_quad_pass_params_bind_group"),
                    layout: &pipeline.bind_group_layouts[0],
                    entries: &[wgpu::BindGroupEntry {
                        binding: 0,
                        resource: params_buffer.as_entire_binding(),
                    }],
                });

        self.state = Some(BackgroundQuadPassState {
            pipeline,
            params_buffer,
            params_bind_group,
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
        // UI state: toggle + sky parameters. Missing component = draw with defaults.
        let mut enabled = true;
        let mut params = SkyParams::default();
        if let Ok(ui_state) = world.get_global::<GameUIStateComponent>() {
            enabled = *ui_state
                .draw_background
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            params = ui_state
                .sky
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .clone();
        }

        let state = self
            .state
            .as_ref()
            .expect("BackgroundQuadPass: init() must run before draw()");

        if enabled {
            // `WorldQuery::delta_time` is in milliseconds; TimeComponent stores seconds.
            let delta_time_seconds = world
                .get_global::<TimeComponent>()
                .map(|time| time.delta_time)
                .unwrap_or(world.delta_time * 0.001);
            self.time += delta_time_seconds.max(0.0);
            renderer.get_queue().write_buffer(
                &state.params_buffer,
                0,
                bytemuck::bytes_of(&sky_uniform(&params, self.time)),
            );
        }

        let render_state = world.get_global::<RenderStateComponent>()?;
        let color_view = renderer
            .get_render_target_view(render_state.hdr_target())
            .expect("BackgroundQuadPass: HDR target missing");

        // Always clear: the PBR pass loads this target instead of clearing it, so when the
        // background is disabled the frame still starts from the regular background color.
        let clear_color = render_state.background_color;
        let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("background_quad_pass_render_pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: color_view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color {
                        r: clear_color[0] as f64,
                        g: clear_color[1] as f64,
                        b: clear_color[2] as f64,
                        a: 1.0,
                    }),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });

        if enabled {
            render_pass.set_pipeline(&state.pipeline.pipeline);
            render_pass.set_bind_group(0, &state.params_bind_group, &[]);
            // Six vertices = one quad, one instance.
            render_pass.draw(0..6, 0..1);
        }

        Ok(())
    }
}
