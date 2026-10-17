use pill_engine::game::*;

use crate::game::GameUIStateComponent;

const VERTEX_SHADER: &str = include_str!("../res/shaders/uv_quad_vertex.wgsl");
const FRAGMENT_SHADER: &str = include_str!("../res/shaders/uv_quad_fragment.wgsl");

struct UvQuadPassState {
    pipeline: Pipeline,
}

/// Game-defined pass that covers the whole window with a UV-colored quad.
///
/// The quad is generated in the vertex shader from `SV_VertexID`, so it needs no vertex buffer
/// and always fills the window, whatever its size. Draws into the engine's HDR target between the
/// scene passes and tonemapping, and can be toggled with the "UV rainbow" checkbox in the Wavey
/// window.
pub struct UvQuadPass {
    state: Option<UvQuadPassState>,
}

impl UvQuadPass {
    pub fn new() -> Self {
        Self { state: None }
    }
}

impl Default for UvQuadPass {
    fn default() -> Self {
        Self::new()
    }
}

impl Pass for UvQuadPass {
    fn get_label(&self) -> &str {
        "uv_quad_pass"
    }

    fn init(&mut self, renderer: &mut dyn PillRenderer) -> Result<()> {
        let pipeline = renderer.create_pipeline(PipelineDesc {
            label: Some("uv_quad_pass"),
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
            bind_groups: Vec::new(),
            targets: &[Some(wgpu::ColorTargetState {
                format: HDR_TARGET_FORMAT,
                // Opaque: the quad replaces whatever the scene passes wrote.
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

        self.state = Some(UvQuadPassState { pipeline });
        Ok(())
    }

    fn draw(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        renderer: &mut dyn PillRenderer,
        _frame_target: &FrameTarget,
        world: &WorldQuery<'_>,
    ) -> Result<()> {
        // UI toggle; if the state component is missing, draw the quad.
        let mut enabled = true;
        if let Ok(ui_state) = world.get_global::<GameUIStateComponent>() {
            enabled = *ui_state
                .draw_uv_quad
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
        }

        let state = self
            .state
            .as_ref()
            .expect("UvQuadPass: init() must run before draw()");

        let render_state = world.get_global::<RenderStateComponent>()?;
        let color_view = renderer
            .get_render_target_view(render_state.hdr_target())
            .expect("UvQuadPass: HDR target missing");

        // Always clear: the PBR pass loads this target instead of clearing it, so when the quad
        // is disabled the frame still starts from the regular background color.
        let clear_color = render_state.background_color;
        let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("uv_quad_pass_render_pass"),
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
            // Six vertices = one quad, one instance.
            render_pass.draw(0..6, 0..1);
        }

        Ok(())
    }
}
