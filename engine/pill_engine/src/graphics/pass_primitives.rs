use crate::config::{
    DEPTH_TARGET_FORMAT, HDR_TARGET_FORMAT, MAX_PRIMITIVE_LINES, MAX_PRIMITIVE_POINTS,
    PRIMITIVE_FRAGMENT_SHADER, PRIMITIVE_POINT_SIZE_PIXELS, PRIMITIVE_POINT_VERTEX_SHADER,
    PRIMITIVE_VERTEX_SHADER,
};
use crate::ecs::PrimitiveDrawComponent;
use crate::graphics::{
    camera_view_projection, BufferDesc, FrameTarget, Pass, PassTextureHandle, PillRenderer,
    Pipeline, PipelineDesc, ShaderDesc, WorldQuery,
};

use pill_core::Result;

/// One vertex of a debug primitive: world-space position + linear RGBA color.
#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct PrimitiveVertex {
    position: [f32; 3],
    color: [f32; 4],
}

impl PrimitiveVertex {
    fn layout<'a>() -> wgpu::VertexBufferLayout<'a> {
        use std::mem;
        wgpu::VertexBufferLayout {
            array_stride: mem::size_of::<PrimitiveVertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &[
                wgpu::VertexAttribute {
                    offset: 0,
                    shader_location: 0,
                    format: wgpu::VertexFormat::Float32x3,
                },
                wgpu::VertexAttribute {
                    offset: mem::size_of::<[f32; 3]>() as wgpu::BufferAddress,
                    shader_location: 1,
                    format: wgpu::VertexFormat::Float32x4,
                },
            ],
        }
    }
}

#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct PrimitiveCameraUniform {
    view_projection: [[f32; 4]; 4],
    viewport: [f32; 2],
    point_size: f32,
    _padding: f32,
}

struct PrimitivesState {
    // Depth-tested and always-on-top variants for each topology.
    line_pipeline: Pipeline,
    line_pipeline_overlay: Pipeline,
    point_pipeline: Pipeline,
    point_pipeline_overlay: Pipeline,
    camera_buffer: wgpu::Buffer,
    camera_bind_group: wgpu::BindGroup,
    line_vertex_buffer: wgpu::Buffer,
    point_vertex_buffer: wgpu::Buffer,
}

/// Draws queued lines and points submitted through `Engine::draw_line` / `Engine::draw_point` /
/// `Engine::draw_aabb`.
///
/// Runs after the engine's opaque PBR pass and after game-defined passes, before tonemapping, so
/// the output stays in the HDR scene target and is tonemapped together with the rest of the scene.
///
/// Topologies are `LineList` (1-pixel lines, the wgpu/WebGPU limit) and triangle-based
/// screen-space quads for points (`PRIMITIVE_POINT_SIZE_PIXELS`). Both have depth-tested and
/// always-visible variants, selected per command via `PrimitiveStyle::depth_test`.
pub struct PassPrimitives {
    color_target: PassTextureHandle,
    depth_target: PassTextureHandle,
    state: Option<PrimitivesState>,
    line_vertices: Vec<PrimitiveVertex>,
    point_vertices: Vec<PrimitiveVertex>,
}

impl PassPrimitives {
    pub fn new(color_target: PassTextureHandle, depth_target: PassTextureHandle) -> Self {
        Self {
            color_target,
            depth_target,
            state: None,
            line_vertices: Vec::with_capacity(MAX_PRIMITIVE_LINES * 2),
            point_vertices: Vec::with_capacity(MAX_PRIMITIVE_POINTS),
        }
    }

    fn create_pipeline(
        renderer: &mut dyn PillRenderer,
        label: &str,
        vs_source: &'static str,
        topology: wgpu::PrimitiveTopology,
        depth_test: bool,
        instance_step: bool,
    ) -> Result<Pipeline> {
        let mut vertex_layout = PrimitiveVertex::layout();
        if instance_step {
            vertex_layout.step_mode = wgpu::VertexStepMode::Instance;
        }

        let bind_groups = vec![vec![wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::VERTEX,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        }]];

        renderer.create_pipeline(PipelineDesc {
            label: Some(label),
            vs: ShaderDesc {
                source: vs_source,
                entry_func: "vs_main",
            },
            ps: ShaderDesc {
                source: PRIMITIVE_FRAGMENT_SHADER,
                entry_func: "fs_main",
            },
            vertex_buffers: &[vertex_layout],
            bind_groups,
            targets: &[Some(wgpu::ColorTargetState {
                format: HDR_TARGET_FORMAT,
                blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                write_mask: wgpu::ColorWrites::ALL,
            })],
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_TARGET_FORMAT,
                // Primitives never occlude scene geometry; they only get occluded by it.
                depth_write_enabled: false,
                depth_compare: if depth_test {
                    wgpu::CompareFunction::LessEqual
                } else {
                    wgpu::CompareFunction::Always
                },
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            primitive: wgpu::PrimitiveState {
                topology,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                polygon_mode: wgpu::PolygonMode::Fill,
                conservative: false,
                unclipped_depth: false,
            },
        })
    }
}

impl Pass for PassPrimitives {
    fn get_label(&self) -> &str {
        "pass_primitives"
    }

    fn init(&mut self, renderer: &mut dyn PillRenderer) -> Result<()> {
        let line_pipeline = Self::create_pipeline(
            renderer,
            "pass_primitives_lines",
            PRIMITIVE_VERTEX_SHADER,
            wgpu::PrimitiveTopology::LineList,
            true,
            false,
        )?;
        let line_pipeline_overlay = Self::create_pipeline(
            renderer,
            "pass_primitives_lines_overlay",
            PRIMITIVE_VERTEX_SHADER,
            wgpu::PrimitiveTopology::LineList,
            false,
            false,
        )?;
        let point_pipeline = Self::create_pipeline(
            renderer,
            "pass_primitives_points",
            PRIMITIVE_POINT_VERTEX_SHADER,
            wgpu::PrimitiveTopology::TriangleList,
            true,
            true,
        )?;
        let point_pipeline_overlay = Self::create_pipeline(
            renderer,
            "pass_primitives_points_overlay",
            PRIMITIVE_POINT_VERTEX_SHADER,
            wgpu::PrimitiveTopology::TriangleList,
            false,
            true,
        )?;

        let camera_buffer = renderer.create_buffer(BufferDesc {
            label: Some("pass_primitives_camera"),
            byte_size: std::mem::size_of::<PrimitiveCameraUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        })?;

        let camera_bind_group =
            renderer
                .get_device()
                .create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("pass_primitives_camera_bind_group"),
                    layout: &line_pipeline.bind_group_layouts[0],
                    entries: &[wgpu::BindGroupEntry {
                        binding: 0,
                        resource: camera_buffer.as_entire_binding(),
                    }],
                });

        let line_vertex_buffer = renderer.create_buffer(BufferDesc {
            label: Some("pass_primitives_line_vertices"),
            byte_size: (MAX_PRIMITIVE_LINES * 2 * std::mem::size_of::<PrimitiveVertex>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        })?;
        let point_vertex_buffer = renderer.create_buffer(BufferDesc {
            label: Some("pass_primitives_point_vertices"),
            byte_size: (MAX_PRIMITIVE_POINTS * std::mem::size_of::<PrimitiveVertex>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        })?;

        self.state = Some(PrimitivesState {
            line_pipeline,
            line_pipeline_overlay,
            point_pipeline,
            point_pipeline_overlay,
            camera_buffer,
            camera_bind_group,
            line_vertex_buffer,
            point_vertex_buffer,
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
        let primitives = world.get_global::<PrimitiveDrawComponent>()?;
        let lines = primitives.take_lines();
        let points = primitives.take_points();
        if lines.is_empty() && points.is_empty() {
            return Ok(());
        }

        let (camera_component, transform_component) = world.active_camera()?;
        let view_projection = camera_view_projection(camera_component, transform_component);
        let (viewport_width, viewport_height) = renderer.get_surface_size();
        let camera_uniform = PrimitiveCameraUniform {
            view_projection: view_projection.to_cols_array_2d(),
            viewport: [viewport_width.max(1) as f32, viewport_height.max(1) as f32],
            point_size: PRIMITIVE_POINT_SIZE_PIXELS,
            _padding: 0.0,
        };

        // Depth-tested primitives first, always-visible ones last; each group is one draw call.
        self.line_vertices.clear();
        let mut tested_line_vertices = 0u32;
        for depth_test in [true, false] {
            for line in lines.iter().filter(|line| line.depth_test == depth_test) {
                self.line_vertices.push(PrimitiveVertex {
                    position: line.from,
                    color: line.color,
                });
                self.line_vertices.push(PrimitiveVertex {
                    position: line.to,
                    color: line.color,
                });
            }
            if depth_test {
                tested_line_vertices = self.line_vertices.len() as u32;
            }
        }

        self.point_vertices.clear();
        let mut tested_point_vertices = 0u32;
        for depth_test in [true, false] {
            for point in points.iter().filter(|point| point.depth_test == depth_test) {
                self.point_vertices.push(PrimitiveVertex {
                    position: point.position,
                    color: point.color,
                });
            }
            if depth_test {
                tested_point_vertices = self.point_vertices.len() as u32;
            }
        }

        let state = self
            .state
            .as_ref()
            .expect("PassPrimitives: state not initialized — call init() before draw()");

        let queue = renderer.get_queue();
        queue.write_buffer(&state.camera_buffer, 0, bytemuck::bytes_of(&camera_uniform));
        if !self.line_vertices.is_empty() {
            queue.write_buffer(
                &state.line_vertex_buffer,
                0,
                bytemuck::cast_slice(&self.line_vertices),
            );
        }
        if !self.point_vertices.is_empty() {
            queue.write_buffer(
                &state.point_vertex_buffer,
                0,
                bytemuck::cast_slice(&self.point_vertices),
            );
        }

        let color_view = renderer
            .get_render_target_view(self.color_target)
            .expect("PassPrimitives: color target missing");
        let depth_view = renderer
            .get_render_target_view(self.depth_target)
            .expect("PassPrimitives: depth target missing");

        let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("pass_primitives_render_pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: color_view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: depth_view,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
        });

        let total_line_vertices = self.line_vertices.len() as u32;
        if total_line_vertices > 0 {
            render_pass.set_vertex_buffer(0, state.line_vertex_buffer.slice(..));
            if tested_line_vertices > 0 {
                render_pass.set_pipeline(&state.line_pipeline.pipeline);
                render_pass.set_bind_group(0, &state.camera_bind_group, &[]);
                render_pass.draw(0..tested_line_vertices, 0..1);
            }
            if tested_line_vertices < total_line_vertices {
                render_pass.set_pipeline(&state.line_pipeline_overlay.pipeline);
                render_pass.set_bind_group(0, &state.camera_bind_group, &[]);
                render_pass.draw(tested_line_vertices..total_line_vertices, 0..1);
            }
        }

        let total_point_vertices = self.point_vertices.len() as u32;
        if total_point_vertices > 0 {
            render_pass.set_vertex_buffer(0, state.point_vertex_buffer.slice(..));
            if tested_point_vertices > 0 {
                render_pass.set_pipeline(&state.point_pipeline.pipeline);
                render_pass.set_bind_group(0, &state.camera_bind_group, &[]);
                render_pass.draw(0..6, 0..tested_point_vertices);
            }
            if tested_point_vertices < total_point_vertices {
                render_pass.set_pipeline(&state.point_pipeline_overlay.pipeline);
                render_pass.set_bind_group(0, &state.camera_bind_group, &[]);
                render_pass.draw(0..6, tested_point_vertices..total_point_vertices);
            }
        }

        Ok(())
    }
}
