use pill_engine::{define_component, game::*};

use crate::water::{GameState, WATER_X_MAX, WATER_X_MIN};

const VERTEX_SHADER: &str = include_str!("../res/shaders/water_mesh_vertex.wgsl");
const FRAGMENT_SHADER: &str = include_str!("../res/shaders/water_mesh_fragment.wgsl");

/// Name of the custom-shader "material" that draws procedural meshes as UV rainbows.
pub const WATER_MESH_SHADER_NAME: &str = "water_mesh_shader";

const MAX_WATER_MESHES: usize = 16;

/// Maximum number of water surface samples uploaded to the shader (one per simulated point).
const MAX_SURFACE_SAMPLES: usize = 1024;
/// Depth that maps to white in the grayscale output (the water body is ~100 units deep).
const MAX_DEPTH: f32 = 100.0;

const BIND_GROUP_FRAME: usize = 0;

// Entities with this component are drawn by `WaterMeshPass` using `water_mesh_shader`.
define_component!(WaterMeshComponent {
    mesh: MeshHandle,
});

/// Mirrors `PerDraw` in water_mesh_vertex.hlsl: position, rotation (radians), scale
type PerDraw = [[f32; 4]; 3];

struct WaterMeshPassState {
    pipeline: Pipeline,
    camera_buffer: wgpu::Buffer,
    per_draw_buffer: wgpu::Buffer,
    /// `SurfaceParams`: count, x_min, x_max, max_depth (bit-packed u32 words).
    surface_params_buffer: wgpu::Buffer,
    /// Water surface heights (world Y), uniformly spaced between x_min and x_max.
    surface_heights_buffer: wgpu::Buffer,
    frame_bind_group: wgpu::BindGroup,
}

/// Game-defined scene pass: draws `WaterMeshComponent` entities into the engine's HDR and depth
/// targets between the opaque PBR pass and tonemapping.
pub struct WaterMeshPass {
    state: Option<WaterMeshPassState>,
    per_draw_data: Vec<PerDraw>,
    draw_list: Vec<MeshHandle>,
}

impl WaterMeshPass {
    pub fn new() -> Self {
        Self {
            state: None,
            per_draw_data: Vec::with_capacity(MAX_WATER_MESHES),
            draw_list: Vec::with_capacity(MAX_WATER_MESHES),
        }
    }
}

impl Default for WaterMeshPass {
    fn default() -> Self {
        Self::new()
    }
}

impl Pass for WaterMeshPass {
    fn get_label(&self) -> &str {
        WATER_MESH_SHADER_NAME
    }

    fn init(&mut self, renderer: &mut dyn PillRenderer) -> Result<()> {
        let bind_groups = vec![vec![
            // 0: camera uniform
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
            // 1: per-draw storage
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: true },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
            // 2: water surface params (fragment)
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
            // 3: water surface heights (fragment)
            wgpu::BindGroupLayoutEntry {
                binding: 3,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: true },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
        ]];

        let pipeline = renderer.create_pipeline(PipelineDesc {
            label: Some(WATER_MESH_SHADER_NAME),
            vs: ShaderDesc {
                source: VERTEX_SHADER,
                entry_func: "vs_main",
            },
            ps: ShaderDesc {
                source: FRAGMENT_SHADER,
                entry_func: "fs_main",
            },
            vertex_buffers: &[RendererMesh::data_layout_descriptor()],
            bind_groups,
            targets: &[Some(wgpu::ColorTargetState {
                format: HDR_TARGET_FORMAT,
                blend: Some(wgpu::BlendState::REPLACE),
                write_mask: wgpu::ColorWrites::ALL,
            })],
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_TARGET_FORMAT,
                depth_write_enabled: true,
                depth_compare: wgpu::CompareFunction::LessEqual,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            primitive: wgpu::PrimitiveState {
                // The procedural polygon can be mirrored by its transform; no culling.
                cull_mode: None,
                ..Default::default()
            },
        })?;

        let camera_buffer = renderer.create_buffer(BufferDesc {
            label: Some("water_mesh_pass_camera"),
            byte_size: std::mem::size_of::<[[f32; 4]; 4]>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        })?;
        let per_draw_buffer = renderer.create_buffer(BufferDesc {
            label: Some("water_mesh_pass_per_draw"),
            byte_size: (std::mem::size_of::<PerDraw>() * MAX_WATER_MESHES) as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        })?;
        let surface_params_buffer = renderer.create_buffer(BufferDesc {
            label: Some("water_mesh_pass_surface_params"),
            byte_size: 16, // four u32 words
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        })?;
        let surface_heights_buffer = renderer.create_buffer(BufferDesc {
            label: Some("water_mesh_pass_surface_heights"),
            byte_size: (std::mem::size_of::<f32>() * MAX_SURFACE_SAMPLES) as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        })?;
        let frame_bind_group = renderer
            .get_device()
            .create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("water_mesh_pass_frame_bind_group"),
                layout: &pipeline.bind_group_layouts[BIND_GROUP_FRAME],
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: camera_buffer.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: per_draw_buffer.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: surface_params_buffer.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: surface_heights_buffer.as_entire_binding(),
                    },
                ],
            });

        self.state = Some(WaterMeshPassState {
            pipeline,
            camera_buffer,
            per_draw_buffer,
            surface_params_buffer,
            surface_heights_buffer,
            frame_bind_group,
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
        let state = self
            .state
            .as_ref()
            .expect("WaterMeshPass: init() must run before draw()");

        // Collect draws (TransformComponent rotation is in degrees, the shader expects radians)
        self.per_draw_data.clear();
        self.draw_list.clear();
        let transforms = &world.query::<TransformComponent>()?.data;
        let water_meshes = &world.query::<WaterMeshComponent>()?.data;
        for (transform, water_mesh) in transforms.iter().zip(water_meshes.iter()) {
            let (Some(transform), Some(water_mesh)) = (transform, water_mesh) else {
                continue;
            };
            if self.per_draw_data.len() == MAX_WATER_MESHES {
                break;
            }
            let rotation = transform.rotation;
            self.per_draw_data.push([
                transform.position.extend(0.0).to_array(),
                [
                    rotation.x.to_radians(),
                    rotation.y.to_radians(),
                    rotation.z.to_radians(),
                    0.0,
                ],
                transform.scale.extend(0.0).to_array(),
            ]);
            self.draw_list.push(water_mesh.mesh);
        }
        if self.draw_list.is_empty() {
            return Ok(());
        }

        let (camera, camera_transform) = world.active_camera()?;
        let view_projection = camera_view_projection(camera, camera_transform).to_cols_array_2d();
        let queue = renderer.get_queue();
        queue.write_buffer(
            &state.camera_buffer,
            0,
            bytemuck::bytes_of(&view_projection),
        );
        queue.write_buffer(
            &state.per_draw_buffer,
            0,
            bytemuck::cast_slice(&self.per_draw_data),
        );

        // Water surface heights (world Y), uniformly spaced in world X
        let (mut heights, x_min, x_max) = match world.get_global::<GameState>() {
            Ok(game_state) => (
                game_state
                    .water_points
                    .iter()
                    .take(MAX_SURFACE_SAMPLES)
                    .map(|point| point.y)
                    .collect::<Vec<f32>>(),
                WATER_X_MIN,
                WATER_X_MAX,
            ),
            Err(_) => (vec![0.0, 0.0], WATER_X_MIN, WATER_X_MAX),
        };
        if heights.len() < 2 {
            heights = vec![0.0, 0.0];
        }
        queue.write_buffer(
            &state.surface_heights_buffer,
            0,
            bytemuck::cast_slice(&heights),
        );
        let surface_params: [u32; 4] = [
            heights.len() as u32,
            x_min.to_bits(),
            x_max.to_bits(),
            MAX_DEPTH.to_bits(),
        ];
        queue.write_buffer(
            &state.surface_params_buffer,
            0,
            bytemuck::bytes_of(&surface_params),
        );

        // Draw on top of the PBR pass output (load, not clear)
        let render_state = world.get_global::<RenderStateComponent>()?;
        let color_view = renderer
            .get_render_target_view(render_state.hdr_target())
            .expect("WaterMeshPass: HDR target missing");
        let depth_view = renderer
            .get_render_target_view(render_state.depth_target())
            .expect("WaterMeshPass: depth target missing");
        let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("water_mesh_pass_render_pass"),
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
        render_pass.set_pipeline(&state.pipeline.pipeline);
        render_pass.set_bind_group(BIND_GROUP_FRAME as u32, &state.frame_bind_group, &[]);

        for (instance, mesh_handle) in self.draw_list.iter().enumerate() {
            let mesh = world.renderer_mesh(mesh_handle)?;
            render_pass.set_vertex_buffer(0, mesh.vertex_buffer.slice(..));
            render_pass.set_index_buffer(mesh.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
            let instance = instance as u32;
            render_pass.draw_indexed(0..mesh.index_count, 0, instance..instance + 1);
        }

        Ok(())
    }
}
